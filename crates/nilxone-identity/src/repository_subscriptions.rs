// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

// Issuer-side persistence only. The caller must first authenticate to
// 0xda-market's issuer API and validate a *processing* order, its snapshot,
// linked recipient and SKU. No browser/user endpoint calls this operation.
//
// The purchase debits authoritative seeds and grants Special in one SQLite
// BEGIN IMMEDIATE transaction. Core decides eligibility, the calendar-month
// term and time priority (`special_seeds_term_months`, `grant_special_months`).
use nilxone_contracts::{SpecialSeedsOrder, SubscriptionClock, special_seeds_term_months};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuerSpecialOrderSnapshot {
    pub order_id: String,
    /// What lets this order spend the Bond's seeds. The issuer worker resolves
    /// it server-side; the debit re-checks it under the wallet lock.
    pub authorization: SpendAuthorization,
    pub issuer: String,
    pub denomination: String,
    pub status: String,
    pub recipient_pub_dress: String,
    pub sku: String,
    pub price_id: u64,
    pub amount_minor: u64,
    /// Calendar months the verified order bought. It must equal the term Core
    /// derives from the SKU; a client's notion of a month is never authoritative.
    pub term_months: u32,
}

/// Who authorized a Special seeds order on the buyer's side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpendAuthorization {
    /// A browser visit: SHA-256 of the authenticated 0x1 session active when
    /// checkout started. That session must still be the active one.
    Session(Vec<u8>),
    /// A channel account, such as Telegram, through which the person ordered.
    /// It must still be bound to the paying Bond when the debit commits, so an
    /// account unlinked (or moved to another Bond) after ordering cannot spend.
    ProviderLink { provider: String, subject: String },
}

impl SpendAuthorization {
    fn well_formed(&self) -> bool {
        match self {
            Self::Session(hash) => hash.len() == 32,
            Self::ProviderLink { provider, subject } => {
                provider.parse::<nilxone_contracts::ProviderType>().is_ok()
                    && subject.parse::<nilxone_contracts::ProviderSubject>().is_ok()
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, sqlx::FromRow)]
pub struct SpecialItemView {
    pub order_id: String,
    pub sku: String,
    pub state: String,
    pub slot_x: Option<u8>,
    pub slot_y: Option<u8>,
}

/// A Special that still has at least this long to run is only extended on
/// purpose. Below it the leftover is not worth a question: the new term starts
/// from now and replaces it.
pub const SPECIAL_REPLACE_CONFIRM_MS: u64 = 60 * 60 * 1000;

/// What resolving a paid item did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialActivation {
    /// The term was granted and the item is spent (also on an exact replay).
    Activated,
    /// Arrival: the item is in the backpack and has not been spent.
    Stored,
    /// Refused until confirmed: the Bond still holds this much Special.
    /// Activating again with the confirmation extends it by the item's term.
    NeedsConfirmation { special_remaining_ms: u64 },
    /// No such paid item, not collected yet, or the active client changed.
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialSeedsOutcome {
    /// The debit and Special grant were committed together.
    Granted,
    /// Committed a paid item without starting the subscription clock.
    IssuedItem,
    /// Exact replay of an already committed issuer operation.
    AlreadyGranted,
    /// No credited authoritative seeds exist (or not enough).
    InsufficientSeeds,
    /// Wrong product, recipient, invalid duration, or amount.
    InvalidTerms,
    /// Order id was committed under different immutable terms.
    ConflictingReplay,
    /// The buyer's active session changed or another activation is pending.
    SessionChanged,
    /// The channel account that ordered is no longer bound to this Bond.
    AccountUnlinked,
    /// The Bond already holds Special (running or paused behind Exclusive);
    /// Core's `special_purchase_open` refuses a second purchase.
    SpecialActive,
}

/// Called while the same SQLite writer transaction owns the wallet. This
/// checks the Single Active Client snapshot and blocks spending during a real,
/// open activation request from another client. Mere login does not interrupt.
pub(crate) async fn spend_session_active_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    session_hash: &[u8],
    now_seconds: u64,
) -> Result<bool, RepositoryError> {
    let Ok(now) = i64::try_from(now_seconds) else { return Ok(false); };
    if session_hash.len() != 32 { return Ok(false); }
    let current = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM native_sessions
         WHERE pub_dress = ? AND token_hash = ? AND active = 1
         AND revoked_at IS NULL AND expires_at > ?)",
    )
    .bind(owner.as_str())
    .bind(session_hash)
    .bind(now)
    .fetch_one(&mut **transaction)
    .await? != 0;
    if !current { return Ok(false); }
    let takeover_in_progress = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM session_activation_requests
         WHERE pub_dress = ? AND requester_token_hash != ?
         AND ((status = 'pending' AND responds_by > ?)
              OR (status = 'objectable' AND objection_deadline > ?)))",
    )
    .bind(owner.as_str())
    .bind(session_hash)
    .bind(now)
    .bind(now)
    .fetch_one(&mut **transaction)
    .await? != 0;
    Ok(!takeover_in_progress)
}

/// Test-only: a file-backed store holding the legacy tables, the product
/// tables and the migration store, so a Bond can be attached to an AIS profile
/// whose live product session is the only way to act for it.
#[cfg(test)]
pub(crate) mod product_session_testing {
    use super::IdentityRepository;

    /// The digest bytes of the product session; its row stores the hex.
    pub(crate) const TOKEN: [u8; 32] = [7; 32];

    pub(crate) async fn file_repository() -> (IdentityRepository, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("directory");
        let url = format!("sqlite://{}", dir.path().join("actor.sqlite").display());
        let repository = IdentityRepository::connect(&url).await.expect("legacy store");
        let product = crate::public_info::PublicInfoRepository::connect(&url)
            .await
            .expect("product store");
        crate::public_info::LegacyBondMigrationRepository::prepare(product)
            .await
            .expect("migration store");
        (repository, dir)
    }

    /// Attaches `owner` (already registered) to a fresh AIS profile whose only
    /// live session is [`TOKEN`], `active` or not. The Bond has no native
    /// session unless the caller creates one.
    pub(crate) async fn attach(repository: &IdentityRepository, owner: &str, active: bool) {
        let hex: String = TOKEN.iter().map(|byte| format!("{byte:02x}")).collect();
        for (sql, binds) in [
            ("INSERT INTO public_profiles (issuer, subject, last_auth_time) VALUES ('https://auth.aiaiaiai.org', 'subject-a', 1)", vec![]),
            ("INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES (?)", vec![owner.to_owned()]),
        ] {
            let mut query = sqlx::query(sql);
            for bind in &binds {
                query = query.bind(bind);
            }
            query.execute(&repository.pool).await.expect(sql);
        }
        sqlx::query(
            "INSERT INTO product_sessions (token_hash, issuer, subject, profile_id, created_at, expires_at, active) \
             SELECT ?, issuer, subject, profile_id, 1, 10000000000, ? FROM public_profiles",
        )
        .bind(&hex)
        .bind(i64::from(active))
        .execute(&repository.pool)
        .await
        .expect("product session");
        sqlx::query(
            "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) \
             SELECT k.bond_id, p.profile_id, 2 FROM legacy_bond_keys k CROSS JOIN public_profiles p",
        )
        .execute(&repository.pool)
        .await
        .expect("binding");
    }
}

/// [`spend_session_active_in`] for an actor that may be an AIS product session
/// of an attached legacy Bond (#115). `session_hash` is the digest bytes of
/// either kind of session; the two kinds hash under different domains, and
/// each lookup also requires the session to belong to `owner`, so one cannot
/// stand in for the other.
///
/// A native session is decided exactly as before. Only when no native session
/// matches does a product session count, and only through the durable
/// `legacy_bond_bindings` link to `owner`'s current address: never through a
/// public name. It must be live and the profile's active client, and, as for
/// native sessions, an open activation request by another client of the same
/// profile blocks spending. When the migration or product tables are absent
/// the product path is simply unavailable. This is a write-boundary check; it
/// never activates anything.
pub(crate) async fn spend_actor_session_active_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    session_hash: &[u8],
    now_seconds: u64,
) -> Result<bool, RepositoryError> {
    if spend_session_active_in(transaction, owner, session_hash, now_seconds).await? {
        return Ok(true);
    }
    let Ok(now) = i64::try_from(now_seconds) else { return Ok(false); };
    if session_hash.len() != 32 { return Ok(false); }
    let present: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN \
         ('legacy_bond_keys', 'legacy_bond_bindings', 'product_sessions', \
          'public_profiles', 'product_activation_requests')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    if present != 5 { return Ok(false); }
    // Product sessions store the lowercase hex of their digest.
    let token: String = session_hash.iter().map(|byte| format!("{byte:02x}")).collect();
    let Some(profile) = sqlx::query(
        "SELECT s.issuer, s.subject FROM product_sessions s \
         JOIN public_profiles p \
           ON p.issuer = s.issuer AND p.subject = s.subject AND p.profile_id = s.profile_id \
         JOIN legacy_bond_bindings b ON b.profile_id = p.profile_id \
         JOIN legacy_bond_keys k ON k.bond_id = b.bond_id \
         JOIN identities i ON i.pub_dress = k.legacy_pub_dress AND i.identity_kind = 'human' \
         WHERE k.legacy_pub_dress = ? AND s.token_hash = ? AND s.active = 1 AND s.expires_at > ?",
    )
    .bind(owner.as_str())
    .bind(&token)
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await? else {
        return Ok(false);
    };
    let issuer: String = profile.try_get("issuer")?;
    let subject: String = profile.try_get("subject")?;
    let takeover_in_progress = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM product_activation_requests \
         WHERE issuer = ? AND subject = ? AND requester_token_hash != ? \
         AND status IN ('pending', 'approved', 'syncing') AND deadline > ?)",
    )
    .bind(&issuer)
    .bind(&subject)
    .bind(&token)
    .bind(now)
    .fetch_one(&mut **transaction)
    .await? != 0;
    Ok(!takeover_in_progress)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StarterSeedsCredit {
    Granted,
    AlreadyGranted,
}

/// Credits Core's `STARTER_SEEDS` once per Bond, inside the caller's write
/// transaction, so a Bond's first purchase or repair already sees them.
///
/// Owner decision (2026-10-30): until verified gameplay credits (jobs) exist,
/// every Bond gets this once: a new Bond on its first wallet use, an existing
/// Bond the same way. The once-only receipt in `bond_seed_credits` follows a
/// rename, so the credit is never issued again, even after it is spent. A
/// browser `inventory.state` snapshot is still never credited.
pub(crate) async fn credit_starter_seeds_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    now_ms: u64,
) -> Result<StarterSeedsCredit, RepositoryError> {
    let at = i64::try_from(now_ms).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
    let amount = i64::try_from(nilxone_contracts::STARTER_SEEDS)
        .map_err(|_| RepositoryError::CorruptSeedsBalance)?;
    let already_issued = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM bond_seed_credits
         WHERE owner_pub_dress = ? AND source_kind = 'starter' AND source_id = 'once')",
    )
    .bind(owner.as_str())
    .fetch_one(&mut **transaction)
    .await? != 0;
    if already_issued {
        return Ok(StarterSeedsCredit::AlreadyGranted);
    }

    // The balance is a read projection of authority-controlled economic
    // mutations; the credit and the once-only receipt share one commit.
    let balance_update = sqlx::query(
        "INSERT INTO bond_seeds(owner_pub_dress, balance) VALUES (?, ?)
         ON CONFLICT(owner_pub_dress) DO UPDATE
         SET balance = bond_seeds.balance + excluded.balance
         WHERE bond_seeds.balance <= ?",
    )
    .bind(owner.as_str())
    .bind(amount)
    .bind(i64::MAX - amount)
    .execute(&mut **transaction)
    .await?;
    if balance_update.rows_affected() != 1 {
        return Err(RepositoryError::CorruptSeedsBalance);
    }
    sqlx::query(
        "INSERT INTO bond_seed_credits
         (owner_pub_dress, source_kind, source_id, amount_minor, committed_at_ms)
         VALUES (?, 'starter', 'once', ?, ?)",
    )
    .bind(owner.as_str())
    .bind(amount)
    .bind(at)
    .execute(&mut **transaction)
    .await?;
    Ok(StarterSeedsCredit::Granted)
}

impl IdentityRepository {
    /// Server-owned, once-only starter credit based on Core's fixed rule
    /// (see `credit_starter_seeds_in`). There is no HTTP endpoint for it.
    pub async fn grant_starter_seeds_once(
        &self,
        owner: &PubDress,
        now_ms: u64,
    ) -> Result<StarterSeedsCredit, RepositoryError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let credit = credit_starter_seeds_in(&mut tx, owner, now_ms).await?;
        tx.commit().await?;
        Ok(credit)
    }

    /// The wallet as the Bond first sees it: its starter credit included.
    pub async fn read_seeds_balance_with_starter(
        &self,
        owner: &PubDress,
        now_ms: u64,
    ) -> Result<u64, RepositoryError> {
        self.grant_starter_seeds_once(owner, now_ms).await?;
        self.read_seeds_balance(owner).await
    }

    /// One authoritative seeds balance, owned exclusively by this service.
    /// Missing wallet means zero credited seeds; clients cannot supply a carry.
    pub async fn read_seeds_balance(&self, owner: &PubDress) -> Result<u64, RepositoryError> {
        let stored = sqlx::query_scalar::<_, i64>(
            "SELECT balance FROM bond_seeds WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(0);
        u64::try_from(stored).map_err(|_| RepositoryError::CorruptSeedsBalance)
    }

    /// Atomic debit and Special grant for a verified 0xda-market issuer order.
    ///
    /// This is **not** proof that the caller actually authenticated the issuer;
    /// the future private issuer worker must validate the processing snapshot
    /// using its issuer bearer credentials and enforce the SKU's term duration
    /// policy before invoking this method. It is deliberately not exposed as
    /// an HTTP endpoint or user action.
    pub async fn settle_special_seeds(
        &self,
        owner: &PubDress,
        order: &IssuerSpecialOrderSnapshot,
        now_ms: u64,
    ) -> Result<SpecialSeedsOutcome, RepositoryError> {
        self.settle_special_order(owner, order, now_ms, false).await
    }

    pub(crate) async fn settle_special_order(
        &self,
        owner: &PubDress,
        order: &IssuerSpecialOrderSnapshot,
        now_ms: u64,
        issue_item: bool,
    ) -> Result<SpecialSeedsOutcome, RepositoryError> {
        let valid_order_id = (8..=128).contains(&order.order_id.len())
            && order.order_id.bytes().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':' | b'.')
            });
        let core_term = special_seeds_term_months(&SpecialSeedsOrder {
            sku: &order.sku,
            issuer: &order.issuer,
            denomination: &order.denomination,
            status: &order.status,
            price_id: order.price_id,
            amount_minor: order.amount_minor,
        });
        if !valid_order_id
            || core_term != Some(order.term_months)
            || !order.authorization.well_formed()
            || order.recipient_pub_dress != owner.as_str()
        {
            return Ok(SpecialSeedsOutcome::InvalidTerms);
        }
        let (Ok(now), Ok(amount), Ok(price_id)) = (
            i64::try_from(now_ms),
            i64::try_from(order.amount_minor),
            i64::try_from(order.price_id),
        ) else {
            return Ok(SpecialSeedsOutcome::InvalidTerms);
        };

        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        // Replay is checked first, before inspecting the current wallet or
        // clock: later legitimate operations must not invalidate this receipt.
        // The SKU fixes the term; the stored duration is what that term came
        // to on the clock at the original grant, so a replay never recomputes it.
        let prior = sqlx::query(
            "SELECT owner_pub_dress, sku, price_id, amount_minor \
             FROM bond_seed_subscription_debits WHERE order_id = ?1
             UNION ALL SELECT owner_pub_dress, sku, price_id, amount_minor
             FROM paid_special_items WHERE order_id = ?1",
        )
        .bind(&order.order_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(prior) = prior {
            let identical =
                prior.try_get::<String, _>("owner_pub_dress")? == owner.as_str()
                && prior.try_get::<String, _>("sku")? == order.sku
                && prior.try_get::<i64, _>("price_id")? == price_id
                && prior.try_get::<i64, _>("amount_minor")? == amount;
            return Ok(if identical {
                SpecialSeedsOutcome::AlreadyGranted
            } else {
                SpecialSeedsOutcome::ConflictingReplay
            });
        }

        // Existing committed receipts can be confirmed after a device handoff.
        // Only a *new* debit requires the authorization to still hold.
        match &order.authorization {
            SpendAuthorization::Session(hash) => {
                if !spend_actor_session_active_in(&mut transaction, owner, hash, now_ms / 1000).await? {
                    return Ok(SpecialSeedsOutcome::SessionChanged);
                }
            }
            SpendAuthorization::ProviderLink { provider, subject } => {
                let bound = sqlx::query_scalar::<_, i64>(
                    "SELECT EXISTS(SELECT 1 FROM identity_providers \
                     WHERE provider = ? AND provider_subject = ? AND pub_dress = ?)",
                )
                .bind(provider)
                .bind(subject)
                .bind(owner.as_str())
                .fetch_one(&mut *transaction)
                .await? != 0;
                if !bound {
                    return Ok(SpecialSeedsOutcome::AccountUnlinked);
                }
            }
        }
        credit_starter_seeds_in(&mut transaction, owner, now_ms).await?;

        let balance = sqlx::query_scalar::<_, i64>(
            "SELECT balance FROM bond_seeds WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or(0);
        if balance < amount {
            return Ok(SpecialSeedsOutcome::InsufficientSeeds);
        }

        let previous: Option<String> = sqlx::query_scalar(
            "SELECT clock_json FROM bond_subscription_clocks \
             WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        let clock = match previous {
            Some(json) => serde_json::from_str::<SubscriptionClock>(&json)
                .map_err(|_| RepositoryError::CorruptSubscriptionClock)?,
            None => SubscriptionClock::new(owner.as_str(), now_ms)
                .map_err(|_| RepositoryError::CorruptSubscriptionClock)?,
        };
        if clock.bond_pub_dress != owner.as_str() {
            return Err(RepositoryError::CorruptSubscriptionClock);
        }
        // Core extends the projected end of existing Special (after queued
        // Exclusive) by calendar months in UTC. A refusal (rollback, overflow,
        // corruption) must never debit the buyer.
        let current = clock.advance(now_ms)
            .map_err(|_| RepositoryError::CorruptSubscriptionClock)?;
        // A paid item starts no time, so it can be bought while Special runs:
        // activation decides what happens to the time left. Only a purchase
        // that grants time at once must not stack.
        if !issue_item
            && !current.special_purchase_open(now_ms)
                .map_err(|_| RepositoryError::CorruptSubscriptionClock)?
        {
            return Ok(SpecialSeedsOutcome::SpecialActive);
        }
        let debit = sqlx::query(
            "UPDATE bond_seeds SET balance = balance - ? \
             WHERE owner_pub_dress = ? AND balance >= ?",
        )
        .bind(amount)
        .bind(owner.as_str())
        .bind(amount)
        .execute(&mut *transaction)
        .await?;
        if debit.rows_affected() != 1 {
            return Ok(SpecialSeedsOutcome::InsufficientSeeds);
        }

        if issue_item {
            sqlx::query(
                "INSERT INTO paid_special_items
                 (order_id, owner_pub_dress, sku, price_id, amount_minor, paid_at_ms, state)
                 VALUES (?, ?, ?, ?, ?, ?, 'paid')",
            )
            .bind(&order.order_id)
            .bind(owner.as_str())
            .bind(&order.sku)
            .bind(price_id)
            .bind(amount)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(SpecialSeedsOutcome::IssuedItem);
        }
        let next = current.grant_special_months(order.term_months, now_ms)
            .map_err(|_| RepositoryError::CorruptSubscriptionClock)?;
        let granted_ms = next.special_remaining_ms.get()
            .checked_sub(current.special_remaining_ms.get())
            .filter(|granted| *granted > 0)
            .ok_or(RepositoryError::CorruptSubscriptionClock)?;
        let duration = i64::try_from(granted_ms)
            .map_err(|_| RepositoryError::CorruptSubscriptionClock)?;
        let serialized = serde_json::to_string(&next)
            .map_err(|_| RepositoryError::CorruptSubscriptionClock)?;

        sqlx::query(
            "INSERT INTO bond_subscription_clocks (owner_pub_dress, clock_json) \
             VALUES (?, ?) \
             ON CONFLICT(owner_pub_dress) DO UPDATE SET clock_json = excluded.clock_json",
        )
        .bind(owner.as_str())
        .bind(serialized)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO bond_seed_subscription_debits \
             (order_id, owner_pub_dress, sku, price_id, amount_minor, duration_ms, committed_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&order.order_id)
        .bind(owner.as_str())
        .bind(&order.sku)
        .bind(price_id)
        .bind(amount)
        .bind(duration)
        .bind(now)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(SpecialSeedsOutcome::Granted)
    }

    /// Paid inventory and transit projection for the authenticated owner.
    pub async fn special_items(&self, owner: &PubDress) -> Result<Vec<SpecialItemView>, RepositoryError> {
        Ok(sqlx::query_as("SELECT order_id, sku, state, slot_x, slot_y FROM paid_special_items
            WHERE owner_pub_dress = ? AND state != 'activated' ORDER BY paid_at_ms, order_id")
            .bind(owner.as_str()).fetch_all(&self.pool).await?)
    }

    /// Collect at an authorized Market visit. The item stays paid if the caller
    /// leaves the flight before pickup, or if the active client changes.
    pub async fn collect_special_item(
        &self, owner: &PubDress, order_id: &str, visit_id: &str, session_hash: &[u8], now_ms: u64,
    ) -> Result<bool, RepositoryError> {
        let now = i64::try_from(now_ms).map_err(|_| RepositoryError::CorruptSeedsBalance)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if !spend_actor_session_active_in(&mut tx, owner, session_hash, now_ms / 1000).await? {
            return Ok(false);
        }
        let changed = sqlx::query(
            "UPDATE paid_special_items SET state = 'in_transit'
             WHERE owner_pub_dress = ?1 AND order_id = ?2 AND state IN ('paid','in_transit')
             AND paid_at_ms <= ?3 AND EXISTS (
               SELECT 1 FROM market_visits WHERE visit_id = ?4 AND owner_pub_dress = ?1
               AND session_token_hash = ?5 AND created_at_ms <= ?3 AND spend_until_ms > ?3)",
        ).bind(owner.as_str()).bind(order_id).bind(now).bind(visit_id).bind(session_hash)
            .execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        Ok(changed == 1)
    }

    /// Activate in flight or from inventory without silently confirming
    /// an extension of an existing Special. The legacy boolean API reports
    /// false when explicit confirmation is required; the confirmation-aware
    /// method returns the remaining duration. The paid order supplies the term.
    /// Retrying a successful activation never adds the same term again.
    pub async fn activate_special_item(
        &self, owner: &PubDress, order_id: &str, session_hash: &[u8], now_ms: u64,
    ) -> Result<bool, RepositoryError> {
        Ok(self
            .activate_special_item_with(owner, order_id, session_hash, now_ms, false)
            .await?
            == SpecialActivation::Activated)
    }

    /// Activate a paid item, telling the caller when the Bond still holds
    /// Special that a term would be added to.
    ///
    /// - Under [`SPECIAL_REPLACE_CONFIRM_MS`] of Special left, the new term
    ///   starts from now and replaces what was left.
    /// - At or over it, nothing changes until `confirm_active` is true; the
    ///   answer carries the time left so the caller can say so. Confirmed, the
    ///   term is added to the end of the Special running.
    pub async fn activate_special_item_with(
        &self, owner: &PubDress, order_id: &str, session_hash: &[u8], now_ms: u64,
        confirm_active: bool,
    ) -> Result<SpecialActivation, RepositoryError> {
        self.resolve_special_item(owner, order_id, session_hash, now_ms, None, confirm_active)
            .await
    }

    /// Resolve arrival against Avaia's ordinary items and the paid items already
    /// occupying her grid. The host supplies the current normal-inventory view.
    /// If no slot exists, never silently extend active Special; an explicit
    /// confirmation via the confirmation-aware method is still required.
    pub async fn deliver_special_item(
        &self, owner: &PubDress, order_id: &str, session_hash: &[u8], now_ms: u64,
        avaia: &nilxone_contracts::Backpack,
    ) -> Result<bool, RepositoryError> {
        Ok(matches!(
            self.resolve_special_item(owner, order_id, session_hash, now_ms, Some(avaia), false)
                .await?,
            SpecialActivation::Activated | SpecialActivation::Stored
        ))
    }

    async fn resolve_special_item(
        &self, owner: &PubDress, order_id: &str, session_hash: &[u8], now_ms: u64,
        arrival: Option<&nilxone_contracts::Backpack>, confirm_active: bool,
    ) -> Result<SpecialActivation, RepositoryError> {
        let now = i64::try_from(now_ms).map_err(|_| RepositoryError::CorruptSubscriptionClock)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if !spend_actor_session_active_in(&mut tx, owner, session_hash, now_ms / 1000).await? {
            return Ok(SpecialActivation::Unavailable);
        }
        let item = sqlx::query("SELECT state, sku FROM paid_special_items
            WHERE owner_pub_dress = ? AND order_id = ? AND paid_at_ms <= ?")
            .bind(owner.as_str()).bind(order_id).bind(now).fetch_optional(&mut *tx).await?;
        let Some(item) = item else { return Ok(SpecialActivation::Unavailable); };
        match item.try_get::<String, _>("state")?.as_str() {
            "activated" => return Ok(SpecialActivation::Activated),
            "stored" if arrival.is_some() => return Ok(SpecialActivation::Stored),
            "in_transit" | "stored" => {}
            _ => return Ok(SpecialActivation::Unavailable),
        }
        if let Some(bag) = arrival {
            let reserved: Vec<(u8, u8)> = sqlx::query_as(
                "SELECT slot_x, slot_y FROM paid_special_items WHERE owner_pub_dress = ? AND state = 'stored'",
            ).bind(owner.as_str()).fetch_all(&mut *tx).await?;
            if let Some((x, y)) = bag.subscription_slot(&reserved) {
                sqlx::query("UPDATE paid_special_items SET state = 'stored', slot_x = ?, slot_y = ? WHERE order_id = ?")
                    .bind(x).bind(y).bind(order_id).execute(&mut *tx).await?;
                tx.commit().await?;
                return Ok(SpecialActivation::Stored);
            }
        }
        let sku: String = item.try_get("sku")?;
        let months = sku.strip_prefix("0x1_special_").and_then(|s| s.strip_suffix('m'))
            .and_then(|s| s.parse().ok()).ok_or(RepositoryError::CorruptSubscriptionClock)?;
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT clock_json FROM bond_subscription_clocks WHERE owner_pub_dress = ?",
        ).bind(owner.as_str()).fetch_optional(&mut *tx).await?;
        let clock: SubscriptionClock = match previous {
            Some(json) => serde_json::from_str(&json).map_err(|_| RepositoryError::CorruptSubscriptionClock)?,
            None => SubscriptionClock::new(owner.as_str(), now_ms).map_err(|_| RepositoryError::CorruptSubscriptionClock)?,
        };
        if clock.bond_pub_dress != owner.as_str() { return Err(RepositoryError::CorruptSubscriptionClock); }
        let current = clock.advance(now_ms).map_err(|_| RepositoryError::CorruptSubscriptionClock)?;
        let left = current.special_remaining_ms.get();
        let replaces = left < SPECIAL_REPLACE_CONFIRM_MS;
        if !replaces && !confirm_active {
            // Dropping the transaction rolls it back: nothing was changed.
            return Ok(SpecialActivation::NeedsConfirmation { special_remaining_ms: left });
        }
        let base = if replaces {
            // Under an hour left: the new term starts from now, and the leftover
            // does not carry over. An Exclusive that is running still goes first.
            SubscriptionClock { special_remaining_ms: nilxone_contracts::DecimalU64::new(0), ..current }
        } else {
            current
        };
        let next = base.grant_special_months(months, now_ms).map_err(|_| RepositoryError::CorruptSubscriptionClock)?;
        sqlx::query("INSERT INTO bond_subscription_clocks(owner_pub_dress, clock_json)
            VALUES (?, ?) ON CONFLICT(owner_pub_dress) DO UPDATE SET clock_json = excluded.clock_json")
            .bind(owner.as_str()).bind(serde_json::to_string(&next).map_err(|_| RepositoryError::CorruptSubscriptionClock)?)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE paid_special_items SET state = 'activated', slot_x = NULL, slot_y = NULL, activated_at_ms = ? WHERE order_id = ?")
            .bind(now).bind(order_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(SpecialActivation::Activated)
    }

    /// Read the authority's effective Core clock. This does not grant access
    /// by itself; authenticated snapshots still belong to the service layer.
    pub async fn read_subscription_clock(
        &self,
        owner: &PubDress,
        now_ms: u64,
    ) -> Result<SubscriptionClock, RepositoryError> {
        let json = sqlx::query_scalar::<_, String>(
            "SELECT clock_json FROM bond_subscription_clocks WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_optional(&self.pool)
        .await?;
        match json {
            Some(json) => {
                let clock = serde_json::from_str::<SubscriptionClock>(&json)
                    .map_err(|_| RepositoryError::CorruptSubscriptionClock)?;
                if clock.bond_pub_dress == owner.as_str() {
                    Ok(clock)
                } else {
                    Err(RepositoryError::CorruptSubscriptionClock)
                }
            }
            None => SubscriptionClock::new(owner.as_str(), now_ms)
                .map_err(|_| RepositoryError::CorruptSubscriptionClock),
        }
    }
}

#[cfg(test)]
mod special_seeds_tests {
    use super::*;
    use nilxone_contracts::SubscriptionTier;

    /// 1970-01-01 plus one calendar month: January has 31 days.
    const JANUARY_MS: u64 = 31 * 24 * 60 * 60 * 1000;

    async fn setup() -> (IdentityRepository, PubDress) {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("database");
        let owner: PubDress = "0x0sky".parse().expect("owner");
        sqlx::query(
            "INSERT INTO identities(pub_dress, identity_kind, created_at) \
             VALUES (?, 'human', 1)",
        )
        .bind(owner.as_str())
        .execute(&repository.pool)
        .await
        .expect("identity");
        sqlx::query(
            "INSERT INTO native_sessions (token_hash, pub_dress, expires_at, created_at, active) \
             VALUES (?, ?, 100000, 1, 1)",
        )
        .bind(vec![9; 32])
        .bind(owner.as_str())
        .execute(&repository.pool)
        .await
        .expect("active session");
        (repository, owner)
    }

    fn order(id: &str) -> IssuerSpecialOrderSnapshot {
        IssuerSpecialOrderSnapshot {
            order_id: id.to_owned(),
            authorization: SpendAuthorization::Session(vec![9; 32]),
            issuer: "0x1".to_owned(),
            denomination: "seeds".to_owned(),
            status: "processing".to_owned(),
            recipient_pub_dress: "0x0sky".to_owned(),
            sku: "0x1_special_1m".to_owned(),
            price_id: 42,
            amount_minor: 640,
            term_months: 1,
        }
    }

    async fn fund_fixture(repository: &IdentityRepository, owner: &PubDress, amount: i64) {
        // Test-only setup, NOT a funding or import API. A live issuer must
        // receive credits exclusively from verified server-owned gameplay.
        sqlx::query("INSERT INTO bond_seeds(owner_pub_dress,balance) VALUES (?, ?)")
            .bind(owner.as_str())
            .bind(amount)
            .execute(&repository.pool)
            .await
            .expect("fixture balance");
        repository.mark_starter_seeds_issued_for_test(owner.as_str()).await.expect("spent starter");
    }

    #[tokio::test]
    async fn starter_credit_uses_core_constant_and_commits_once() {
        let (repository, owner) = setup().await;
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 0);
        assert_eq!(
            repository.grant_starter_seeds_once(&owner, 100).await.unwrap(),
            StarterSeedsCredit::Granted
        );
        assert_eq!(
            repository.grant_starter_seeds_once(&owner, 200).await.unwrap(),
            StarterSeedsCredit::AlreadyGranted
        );
        assert_eq!(
            repository.read_seeds_balance(&owner).await.unwrap(),
            nilxone_contracts::STARTER_SEEDS
        );
        let (count, amount, at): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*), sum(amount_minor), min(committed_at_ms)
             FROM bond_seed_credits WHERE owner_pub_dress = ?",
        )
        .bind(owner.as_str())
        .fetch_one(&repository.pool)
        .await
        .unwrap();
        assert_eq!((count, amount, at), (1, 10_000, 100));
    }

    #[tokio::test]
    async fn starter_credit_does_not_accept_client_balance_or_reissue_after_spend() {
        let (repository, owner) = setup().await;
        assert_eq!(
            repository.grant_starter_seeds_once(&owner, 100).await.unwrap(),
            StarterSeedsCredit::Granted
        );
        sqlx::query("UPDATE bond_seeds SET balance = balance - 640 WHERE owner_pub_dress = ?")
            .bind(owner.as_str())
            .execute(&repository.pool)
            .await
            .unwrap();
        assert_eq!(
            repository.grant_starter_seeds_once(&owner, 101).await.unwrap(),
            StarterSeedsCredit::AlreadyGranted
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 9_360);
    }

    #[tokio::test]
    async fn no_authoritative_seeds_means_no_purchase_or_grant() {
        let (repository, owner) = setup().await;
        // A Bond whose starter credit is spent and that has no other credit.
        repository.mark_starter_seeds_issued_for_test(owner.as_str()).await.unwrap();
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-no-funds"), 100).await.unwrap(),
            SpecialSeedsOutcome::InsufficientSeeds
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM bond_seed_subscription_debits")
            .fetch_one(&repository.pool).await.unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            repository.read_subscription_clock(&owner, 100).await.unwrap().special_remaining_ms.get(),
            0
        );
    }

    #[tokio::test]
    async fn paid_item_is_collectible_once_and_starts_time_only_at_activation() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 1000).await;
        let id = "paid-order-01";
        let verified = order(id);
        let session = [9_u8; 32];
        assert_eq!(repository.settle_special_order(&owner, &verified, 100, true).await.unwrap(), SpecialSeedsOutcome::IssuedItem);
        assert_eq!(repository.settle_special_seeds(&owner, &verified, 200).await.unwrap(), SpecialSeedsOutcome::AlreadyGranted);
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 360);
        assert_eq!(repository.read_subscription_clock(&owner, 200).await.unwrap().special_remaining_ms.get(), 0);
        assert!(!repository.activate_special_item(&owner, id, &session, 200).await.unwrap());
        let visit = "v".repeat(43);
        repository.create_market_visit(&visit, &owner, &session, 200, 1000).await.unwrap();
        assert!(!repository.collect_special_item(&owner, id, &visit, &[7; 32], 250).await.unwrap());
        assert!(repository.collect_special_item(&owner, id, &visit, &session, 250).await.unwrap());
        assert!(repository.activate_special_item(&owner, id, &session, 300).await.unwrap());
        assert!(repository.activate_special_item(&owner, id, &session, 400).await.unwrap());
        let clock = repository.read_subscription_clock(&owner, 400).await.unwrap();
        assert_eq!(clock.settled_at_ms.get(), 300);
        assert_eq!(clock.special_remaining_ms.get(), JANUARY_MS);
    }

    #[tokio::test]
    async fn arrival_stores_once_or_activates_if_the_grid_is_too_small() {
        use nilxone_contracts::{Backpack, Carry};
        for carry in [Carry::Pocket, Carry::Backpack] {
            let (repository, owner) = setup().await;
            fund_fixture(&repository, &owner, 1000).await;
            let id = "arrival-item-01";
            let session = [9; 32];
            repository.settle_special_order(&owner, &order(id), 100, true).await.unwrap();
            let visit = "v".repeat(43);
            repository.create_market_visit(&visit, &owner, &session, 100, 1000).await.unwrap();
            assert!(repository.collect_special_item(&owner, id, &visit, &session, 200).await.unwrap());
            let bag = Backpack::new(carry);
            for now in [300, 400] {
                assert!(repository.deliver_special_item(&owner, id, &session, now, &bag).await.unwrap());
            }
            let state: String = sqlx::query_scalar("SELECT state FROM paid_special_items WHERE order_id = ?")
                .bind(id).fetch_one(&repository.pool).await.unwrap();
            assert_eq!(state, if carry == Carry::Pocket { "activated" } else { "stored" });
            let clock = repository.read_subscription_clock(&owner, 400).await.unwrap();
            assert_eq!(clock.special_remaining_ms.get(), if carry == Carry::Pocket { JANUARY_MS } else { 0 });
            assert!(repository.activate_special_item(&owner, id, &session, 500).await.unwrap());
            assert_eq!(repository.read_subscription_clock(&owner, 500).await.unwrap().special_remaining_ms.get(), JANUARY_MS);
        }
    }

    #[tokio::test]
    async fn debit_grant_and_replay_are_atomic_and_once_only() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 1000).await;
        let verified = order("order-verified-1");
        assert_eq!(
            repository.settle_special_seeds(&owner, &verified, 100).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        assert_eq!(
            repository.settle_special_seeds(&owner, &verified, 200).await.unwrap(),
            SpecialSeedsOutcome::AlreadyGranted
        );
        let balance: i64 = sqlx::query_scalar(
            "SELECT balance FROM bond_seeds WHERE owner_pub_dress = ?",
        ).bind(owner.as_str()).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(balance, 360);
        let clock = repository.read_subscription_clock(&owner, 200).await.unwrap();
        assert_eq!(clock.special_remaining_ms.get(), JANUARY_MS);
        assert_eq!(clock.settled_at_ms.get(), 100);
        let receipt = repository.find_special_seeds_receipt("order-verified-1").await.unwrap().unwrap();
        assert_eq!(receipt.duration_ms, JANUARY_MS);

        let mut conflict = verified.clone();
        conflict.amount_minor += 1;
        assert_eq!(
            repository.settle_special_seeds(&owner, &conflict, 200).await.unwrap(),
            SpecialSeedsOutcome::ConflictingReplay
        );
    }

    #[tokio::test]
    async fn activation_or_session_switch_cancels_purchase_without_debit() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 1000).await;
        sqlx::query(
            "INSERT INTO native_sessions(token_hash, pub_dress, expires_at, created_at, active)
             VALUES (?, ?, 100000, 2, 0)",
        )
        .bind(vec![7; 32]).bind(owner.as_str())
        .execute(&repository.pool).await.unwrap();
        sqlx::query(
            "INSERT INTO session_activation_requests
             (id, pub_dress, requester_token_hash, client_label, status, created_at, responds_by)
             VALUES (?, ?, ?, 'Second device', 'pending', 1, 500)",
        )
        .bind(vec![1; 16]).bind(owner.as_str()).bind(vec![7; 32])
        .execute(&repository.pool).await.unwrap();

        let snapshot = order("order-switch-1");
        assert_eq!(repository.settle_special_seeds(&owner, &snapshot, 100_000).await.unwrap(),
                   SpecialSeedsOutcome::SessionChanged);
        sqlx::query("UPDATE session_activation_requests SET status='declined', resolved_at=101")
            .execute(&repository.pool).await.unwrap();
        // The active session still holds authority after the request was declined.
        assert_eq!(repository.settle_special_seeds(&owner, &snapshot, 100_000).await.unwrap(),
                   SpecialSeedsOutcome::Granted);
        // A verified switch invalidates the previous session even for replays.
        sqlx::query("UPDATE native_sessions SET active=0 WHERE token_hash = ?")
            .bind(vec![9;32]).execute(&repository.pool).await.unwrap();
        sqlx::query("UPDATE native_sessions SET active=1 WHERE token_hash = ?")
            .bind(vec![7;32]).execute(&repository.pool).await.unwrap();
        assert_eq!(repository.settle_special_seeds(&owner, &snapshot, 100_000).await.unwrap(),
                   SpecialSeedsOutcome::AlreadyGranted);
        let balance: i64 = sqlx::query_scalar(
            "SELECT balance FROM bond_seeds WHERE owner_pub_dress=?",
        ).bind(owner.as_str()).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(balance, 360);
    }

    #[tokio::test]
    async fn excludes_exclusive_and_rejects_recipient_mismatch() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 5000).await;
        let mut invalid = order("order-invalid-1");
        invalid.sku = "0x1_exclusive_1m".to_owned();
        assert_eq!(
            repository.settle_special_seeds(&owner, &invalid, 100).await.unwrap(),
            SpecialSeedsOutcome::InvalidTerms
        );
        invalid.sku = "0x1_special_1m".to_owned();
        invalid.recipient_pub_dress = "0x0other".to_owned();
        assert_eq!(
            repository.settle_special_seeds(&owner, &invalid, 100).await.unwrap(),
            SpecialSeedsOutcome::InvalidTerms
        );
        invalid.recipient_pub_dress = owner.as_str().to_owned();
        for (issuer, denomination, status) in [
            ("another-issuer", "seeds", "processing"),
            ("0x1", "USDT", "processing"),
            ("0x1", "seeds", "pending"),
        ] {
            invalid.issuer = issuer.to_owned();
            invalid.denomination = denomination.to_owned();
            invalid.status = status.to_owned();
            assert_eq!(
                repository.settle_special_seeds(&owner, &invalid, 100).await.unwrap(),
                SpecialSeedsOutcome::InvalidTerms
            );
        }
        assert_eq!(
            repository.read_subscription_clock(&owner, 100).await.unwrap().special_remaining_ms.get(),
            0
        );
    }

    #[tokio::test]
    async fn grant_during_exclusive_does_not_consume_paused_special() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 1000).await;
        let exclusive = SubscriptionClock::new(owner.as_str(), 100)
            .unwrap().grant(SubscriptionTier::Exclusive, 5_000, 100).unwrap();
        sqlx::query(
            "INSERT INTO bond_subscription_clocks(owner_pub_dress,clock_json) VALUES (?, ?)",
        ).bind(owner.as_str())
         .bind(serde_json::to_string(&exclusive).unwrap())
         .execute(&repository.pool).await.unwrap();

        let purchase = order("order-paused-1");
        assert_eq!(
            repository.settle_special_seeds(&owner, &purchase, 200).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        let clock = repository.read_subscription_clock(&owner, 200).await.unwrap();
        assert_eq!(clock.exclusive_remaining_ms.get(), 4900);
        // The calendar month starts where queued Exclusive ends.
        assert_eq!(clock.special_remaining_ms.get(), JANUARY_MS);
        assert_eq!(
            clock.snapshot(200, 500).unwrap().entitlement.unwrap().tier,
            SubscriptionTier::Exclusive
        );
    }

    #[tokio::test]
    async fn term_must_be_the_one_core_derives_from_the_sku() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 5000).await;
        let mut wrong = order("order-term-1");
        for months in [0, 3, 12] {
            wrong.term_months = months;
            assert_eq!(
                repository.settle_special_seeds(&owner, &wrong, 100).await.unwrap(),
                SpecialSeedsOutcome::InvalidTerms
            );
        }
        let mut three = order("order-term-3");
        three.sku = "0x1_special_3m".to_owned();
        three.term_months = 3;
        assert_eq!(
            repository.settle_special_seeds(&owner, &three, 100).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        // 1970-01-01 + 3 calendar months = 1970-04-01: 31 + 28 + 31 days.
        let clock = repository.read_subscription_clock(&owner, 100).await.unwrap();
        assert_eq!(clock.special_remaining_ms.get(), 90 * 24 * 60 * 60 * 1000);
    }

    const HOUR_MS: u64 = 60 * 60 * 1000;
    const DAY_MS: u64 = 24 * HOUR_MS;

    /// A Special that runs one month from 100 ms, and a collected one-month
    /// item ready to activate. The legacy grant is the cheap way to give the
    /// Bond a running Special; the item is what these tests are about.
    async fn running_special_and_a_collected_item() -> (IdentityRepository, PubDress) {
        let (repository, owner) = setup().await;
        sqlx::query("UPDATE native_sessions SET expires_at = 10000000000")
            .execute(&repository.pool).await.unwrap();
        fund_fixture(&repository, &owner, 5000).await;
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-running"), 100).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        assert_eq!(
            repository.settle_special_order(&owner, &order("order-item1"), 150, true).await.unwrap(),
            SpecialSeedsOutcome::IssuedItem
        );
        let visit = "i".repeat(43);
        repository.create_market_visit(&visit, &owner, &[9; 32], 160, 10_000_000_000_000).await.unwrap();
        assert!(repository.collect_special_item(&owner, "order-item1", &visit, &[9; 32], 200).await.unwrap());
        (repository, owner)
    }

    async fn special_left(repository: &IdentityRepository, owner: &PubDress, now_ms: u64) -> u64 {
        repository.read_subscription_clock(owner, now_ms).await.unwrap()
            .advance(now_ms).unwrap().special_remaining_ms.get()
    }

    #[tokio::test]
    async fn an_item_can_be_bought_while_special_runs_but_a_direct_grant_cannot() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 5000).await;
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-running"), 100).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        assert_eq!(
            repository.settle_special_order(&owner, &order("order-item1"), 150, true).await.unwrap(),
            SpecialSeedsOutcome::IssuedItem
        );
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-again"), 160).await.unwrap(),
            SpecialSeedsOutcome::SpecialActive
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 5000 - 640 - 640);
    }

    #[tokio::test]
    async fn activating_with_under_an_hour_left_starts_from_now_and_drops_the_leftover() {
        let (repository, owner) = running_special_and_a_collected_item().await;
        // Half an hour of the running month is left: 31 January, 23:30.
        let now = JANUARY_MS + 100 - HOUR_MS / 2;
        assert_eq!(special_left(&repository, &owner, now).await, HOUR_MS / 2);
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], now, false).await.unwrap(),
            SpecialActivation::Activated
        );
        // One month from 31 January is 28 February, the same time of day.
        assert_eq!(special_left(&repository, &owner, now).await, 28 * DAY_MS);
        // A retry never adds the term again.
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], now + 1, false).await.unwrap(),
            SpecialActivation::Activated
        );
        assert_eq!(special_left(&repository, &owner, now).await, 28 * DAY_MS);
    }

    #[tokio::test]
    async fn activating_with_an_hour_or_more_left_asks_first_and_then_adds_the_term() {
        let (repository, owner) = running_special_and_a_collected_item().await;
        let now = JANUARY_MS + 100 - 2 * HOUR_MS;
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], now, false).await.unwrap(),
            SpecialActivation::NeedsConfirmation { special_remaining_ms: 2 * HOUR_MS }
        );
        // Nothing changed: the item is not spent and the clock is as it was.
        assert_eq!(special_left(&repository, &owner, now).await, 2 * HOUR_MS);
        let state: String = sqlx::query_scalar("SELECT state FROM paid_special_items WHERE order_id = 'order-item1'")
            .fetch_one(&repository.pool).await.unwrap();
        assert_eq!(state, "in_transit");
        // Confirmed: the month is added to the end of the one that runs.
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], now, true).await.unwrap(),
            SpecialActivation::Activated
        );
        assert_eq!(special_left(&repository, &owner, now).await, 28 * DAY_MS + 2 * HOUR_MS);
    }

    #[tokio::test]
    async fn exactly_an_hour_left_still_asks() {
        let (repository, owner) = running_special_and_a_collected_item().await;
        let hour_left = JANUARY_MS + 100 - HOUR_MS;
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], hour_left, false).await.unwrap(),
            SpecialActivation::NeedsConfirmation { special_remaining_ms: HOUR_MS }
        );
        // One millisecond less is under the hour.
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], hour_left + 1, false).await.unwrap(),
            SpecialActivation::Activated
        );
    }

    #[tokio::test]
    async fn legacy_activation_never_implicitly_confirms_an_extension() {
        let (repository, owner) = running_special_and_a_collected_item().await;
        let now = JANUARY_MS + 100 - 2 * HOUR_MS;

        // Existing HTTP adapters only know the boolean API. They must not
        // turn a click into implicit consent to extend active Special.
        assert!(!repository.activate_special_item(&owner, "order-item1", &[9; 32], now).await.unwrap());
        assert_eq!(special_left(&repository, &owner, now).await, 2 * HOUR_MS);
        let state: String = sqlx::query_scalar(
            "SELECT state FROM paid_special_items WHERE order_id = 'order-item1'",
        ).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(state, "in_transit");

        // The caller may confirm only through the explicit confirmation-aware
        // method; a legacy replay after success cannot extend the term again.
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], now, true).await.unwrap(),
            SpecialActivation::Activated
        );
        assert!(repository.activate_special_item(&owner, "order-item1", &[9; 32], now + 1).await.unwrap());
        assert_eq!(special_left(&repository, &owner, now).await, 28 * DAY_MS + 2 * HOUR_MS);
    }

    #[tokio::test]
    async fn full_backpack_arrival_never_implicitly_confirms_an_extension() {
        use nilxone_contracts::{Backpack, Carry};

        let (repository, owner) = running_special_and_a_collected_item().await;
        let now = JANUARY_MS + 100 - 2 * HOUR_MS;
        // A 1x3 pocket has no 2x2 paid subscription slot.
        let pocket = Backpack::new(Carry::Pocket);
        assert!(!repository.deliver_special_item(&owner, "order-item1", &[9; 32], now, &pocket).await.unwrap());
        assert_eq!(special_left(&repository, &owner, now).await, 2 * HOUR_MS);
        let state: String = sqlx::query_scalar(
            "SELECT state FROM paid_special_items WHERE order_id = 'order-item1'",
        ).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(state, "in_transit");

        // Even after a failed arrival the user can explicitly confirm the
        // extension; the issuer receipt was not lost or consumed.
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], now, true).await.unwrap(),
            SpecialActivation::Activated
        );
        assert_eq!(special_left(&repository, &owner, now).await, 28 * DAY_MS + 2 * HOUR_MS);
    }

    #[tokio::test]
    async fn available_backpack_slot_stores_paid_item_without_touching_active_special() {
        use nilxone_contracts::{Backpack, Carry};

        let (repository, owner) = running_special_and_a_collected_item().await;
        let now = JANUARY_MS + 100 - 2 * HOUR_MS;
        let backpack = Backpack::new(Carry::Backpack);
        assert!(repository.deliver_special_item(&owner, "order-item1", &[9; 32], now, &backpack).await.unwrap());
        assert_eq!(special_left(&repository, &owner, now).await, 2 * HOUR_MS);
        let state: String = sqlx::query_scalar(
            "SELECT state FROM paid_special_items WHERE order_id = 'order-item1'",
        ).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(state, "stored");
        assert_eq!(
            repository.activate_special_item_with(&owner, "order-item1", &[9; 32], now, false).await.unwrap(),
            SpecialActivation::NeedsConfirmation { special_remaining_ms: 2 * HOUR_MS }
        );
    }

    #[tokio::test]
    async fn a_bond_that_holds_special_cannot_buy_it_again_until_it_ends() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 5000).await;
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-first"), 100).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        // While the first term runs, a second order is refused and pays nothing.
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-second"), 200).await.unwrap(),
            SpecialSeedsOutcome::SpecialActive
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 4360);
        assert!(repository.find_special_seeds_receipt("order-second").await.unwrap().is_none());
        // The refusal never touches the first order's replay.
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-first"), 300).await.unwrap(),
            SpecialSeedsOutcome::AlreadyGranted
        );
        // Once Special has run out, Special can be bought again.
        let ended = 100 + JANUARY_MS;
        sqlx::query("UPDATE native_sessions SET expires_at = ?")
            .bind(i64::try_from(ended / 1000 + 60).unwrap())
            .execute(&repository.pool)
            .await
            .unwrap();
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-after-end"), ended).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 3720);
    }

    #[tokio::test]
    async fn special_paused_behind_exclusive_still_blocks_a_purchase() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 5000).await;
        let held = SubscriptionClock::new(owner.as_str(), 100)
            .unwrap()
            .grant(SubscriptionTier::Special, 1_000, 100)
            .unwrap()
            .grant(SubscriptionTier::Exclusive, 5_000, 100)
            .unwrap();
        sqlx::query("INSERT INTO bond_subscription_clocks(owner_pub_dress,clock_json) VALUES (?, ?)")
            .bind(owner.as_str())
            .bind(serde_json::to_string(&held).unwrap())
            .execute(&repository.pool)
            .await
            .unwrap();
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-paused"), 200).await.unwrap(),
            SpecialSeedsOutcome::SpecialActive
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 5000);
    }

    fn telegram_order(id: &str) -> IssuerSpecialOrderSnapshot {
        IssuerSpecialOrderSnapshot {
            authorization: SpendAuthorization::ProviderLink {
                provider: "telegram".to_owned(),
                subject: "123456789".to_owned(),
            },
            ..order(id)
        }
    }

    async fn link_telegram(repository: &IdentityRepository, owner: &str) {
        sqlx::query(
            "INSERT INTO identity_providers(provider, provider_subject, pub_dress) \
             VALUES ('telegram', '123456789', ?)",
        )
        .bind(owner)
        .execute(&repository.pool)
        .await
        .expect("telegram link");
    }

    #[tokio::test]
    async fn a_telegram_order_spends_only_while_its_account_is_bound_to_the_bond() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 1000).await;
        // Not bound: nothing is debited, whatever the session state.
        assert_eq!(
            repository.settle_special_seeds(&owner, &telegram_order("order-tg-1"), 100).await.unwrap(),
            SpecialSeedsOutcome::AccountUnlinked
        );
        link_telegram(&repository, owner.as_str()).await;
        // A Telegram order needs no browser session.
        sqlx::query("UPDATE native_sessions SET active = 0").execute(&repository.pool).await.unwrap();
        assert_eq!(
            repository.settle_special_seeds(&owner, &telegram_order("order-tg-1"), 100).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 360);
        // A committed receipt is confirmed again even after the account is unlinked.
        sqlx::query("DELETE FROM identity_providers").execute(&repository.pool).await.unwrap();
        assert_eq!(
            repository.settle_special_seeds(&owner, &telegram_order("order-tg-1"), 200).await.unwrap(),
            SpecialSeedsOutcome::AlreadyGranted
        );
    }

    #[tokio::test]
    async fn a_telegram_account_bound_to_another_bond_cannot_spend_this_one() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 1000).await;
        sqlx::query("INSERT INTO identities(pub_dress, identity_kind, created_at) VALUES ('0x0other', 'human', 1)")
            .execute(&repository.pool)
            .await
            .unwrap();
        link_telegram(&repository, "0x0other").await;
        assert_eq!(
            repository.settle_special_seeds(&owner, &telegram_order("order-tg-other"), 100).await.unwrap(),
            SpecialSeedsOutcome::AccountUnlinked
        );
        let mut malformed = telegram_order("order-tg-bad");
        malformed.authorization = SpendAuthorization::ProviderLink {
            provider: "telegram".to_owned(),
            subject: String::new(),
        };
        assert_eq!(
            repository.settle_special_seeds(&owner, &malformed, 100).await.unwrap(),
            SpecialSeedsOutcome::InvalidTerms
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn the_first_purchase_of_a_new_bond_spends_its_starter_once() {
        let (repository, owner) = setup().await;
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-starter-1"), 100).await.unwrap(),
            SpecialSeedsOutcome::Granted
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 9_360);
        // Spending never earns a second starter credit.
        assert_eq!(
            repository.read_seeds_balance_with_starter(&owner, 200).await.unwrap(),
            9_360
        );
        assert_eq!(
            repository.grant_starter_seeds_once(&owner, 300).await.unwrap(),
            StarterSeedsCredit::AlreadyGranted
        );
    }

    #[tokio::test]
    async fn a_refused_purchase_does_not_commit_the_starter_credit() {
        let (repository, owner) = setup().await;
        let mut wrong = order("order-starter-refused");
        wrong.recipient_pub_dress = "0x0other".to_owned();
        assert_eq!(
            repository.settle_special_seeds(&owner, &wrong, 100).await.unwrap(),
            SpecialSeedsOutcome::InvalidTerms
        );
        sqlx::query("UPDATE native_sessions SET active = 0").execute(&repository.pool).await.unwrap();
        assert_eq!(
            repository.settle_special_seeds(&owner, &order("order-starter-session"), 100).await.unwrap(),
            SpecialSeedsOutcome::SessionChanged
        );
        let credits: i64 = sqlx::query_scalar("SELECT count(*) FROM bond_seed_credits")
            .fetch_one(&repository.pool).await.unwrap();
        assert_eq!(credits, 0);
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn clock_failure_does_not_debit_or_record_an_order() {
        let (repository, owner) = setup().await;
        fund_fixture(&repository, &owner, 1000).await;
        let original = SubscriptionClock::new(owner.as_str(), 1_000).unwrap();
        sqlx::query(
            "INSERT INTO bond_subscription_clocks(owner_pub_dress,clock_json) VALUES (?, ?)",
        ).bind(owner.as_str())
         .bind(serde_json::to_string(&original).unwrap())
         .execute(&repository.pool).await.unwrap();
        assert!(matches!(
            repository.settle_special_seeds(&owner, &order("order-rollback"), 999).await,
            Err(RepositoryError::CorruptSubscriptionClock)
        ));
        let balance: i64 = sqlx::query_scalar(
            "SELECT balance FROM bond_seeds WHERE owner_pub_dress = ?",
        ).bind(owner.as_str()).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(balance, 1000);
    }

    // ---- AIS product sessions of an attached legacy Bond (#115) ----------

    const PRODUCT_TOKEN: [u8; 32] = [7; 32];

    fn product_hex() -> String {
        PRODUCT_TOKEN.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    async fn run(repository: &IdentityRepository, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sql);
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(&repository.pool).await.expect(sql);
    }

    /// A Bond with no native session at all, attached to an AIS profile whose
    /// only live session is the active product client `PRODUCT_TOKEN`.
    async fn attached_product_bond() -> (IdentityRepository, PubDress, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("directory");
        let url = format!("sqlite://{}", dir.path().join("special.sqlite").display());
        let repository = IdentityRepository::connect(&url).await.expect("legacy store");
        let product = crate::public_info::PublicInfoRepository::connect(&url)
            .await
            .expect("product store");
        crate::public_info::LegacyBondMigrationRepository::prepare(product)
            .await
            .expect("migration store");
        let owner: PubDress = "0x0sky".parse().expect("owner");
        run(&repository, "INSERT INTO identities(pub_dress, identity_kind, created_at) VALUES ('0x0sky', 'human', 1)", &[]).await;
        run(&repository, "INSERT INTO public_profiles (issuer, subject, last_auth_time) VALUES ('https://auth.aiaiaiai.org', 'subject-a', 1)", &[]).await;
        let hex = product_hex();
        run(
            &repository,
            "INSERT INTO product_sessions (token_hash, issuer, subject, profile_id, created_at, expires_at, active) \
             SELECT ?, issuer, subject, profile_id, 1, 100000, 1 FROM public_profiles",
            &[&hex],
        )
        .await;
        run(&repository, "INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES ('0x0sky')", &[]).await;
        run(
            &repository,
            "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) \
             SELECT k.bond_id, p.profile_id, 2 FROM legacy_bond_keys k CROSS JOIN public_profiles p",
            &[],
        )
        .await;
        fund_fixture(&repository, &owner, 1000).await;
        (repository, owner, dir)
    }

    fn product_order(id: &str) -> IssuerSpecialOrderSnapshot {
        IssuerSpecialOrderSnapshot {
            authorization: SpendAuthorization::Session(PRODUCT_TOKEN.to_vec()),
            ..order(id)
        }
    }

    #[tokio::test]
    async fn the_active_product_session_of_an_attached_bond_buys_collects_and_activates() {
        let (repository, owner, _dir) = attached_product_bond().await;
        let id = "product-order-1";
        assert_eq!(
            repository.settle_special_order(&owner, &product_order(id), 100, true).await.unwrap(),
            SpecialSeedsOutcome::IssuedItem
        );
        assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 360);
        let visit = "p".repeat(43);
        repository.create_market_visit(&visit, &owner, &PRODUCT_TOKEN, 200, 1000).await.unwrap();
        assert!(repository.collect_special_item(&owner, id, &visit, &PRODUCT_TOKEN, 250).await.unwrap());
        assert!(repository.activate_special_item(&owner, id, &PRODUCT_TOKEN, 300).await.unwrap());
        let clock = repository.read_subscription_clock(&owner, 300).await.unwrap();
        assert_eq!(clock.special_remaining_ms.get(), JANUARY_MS);
    }

    #[tokio::test]
    async fn a_product_session_that_is_not_the_active_live_bound_client_spends_nothing() {
        for (label, sabotage) in [
            ("inactive", "UPDATE product_sessions SET active = 0"),
            ("expired", "UPDATE product_sessions SET expires_at = 250"),
            ("unbound", "DELETE FROM legacy_bond_bindings"),
            ("signed out", "DELETE FROM product_sessions"),
            ("profile gone", "DELETE FROM public_profiles"),
        ] {
            let (repository, owner, _dir) = attached_product_bond().await;
            run(&repository, sabotage, &[]).await;
            assert_eq!(
                repository.settle_special_order(&owner, &product_order("product-order-x"), 300_000, true).await.unwrap(),
                SpecialSeedsOutcome::SessionChanged,
                "{label}"
            );
            assert_eq!(repository.read_seeds_balance(&owner).await.unwrap(), 1000, "{label}: nothing debited");
            assert!(!repository.collect_special_item(&owner, "product-order-x", &"p".repeat(43), &PRODUCT_TOKEN, 300_000).await.unwrap(), "{label}");
            assert!(!repository.activate_special_item(&owner, "product-order-x", &PRODUCT_TOKEN, 300_000).await.unwrap(), "{label}");
        }
    }

    #[tokio::test]
    async fn a_product_session_cannot_act_for_another_bond_or_by_a_matching_name() {
        let (repository, owner, _dir) = attached_product_bond().await;
        run(&repository, "INSERT INTO identities(pub_dress, identity_kind, created_at) VALUES ('0x1other', 'human', 1)", &[]).await;
        let other: PubDress = "0x1other".parse().unwrap();
        fund_fixture(&repository, &other, 1000).await;
        let mut theirs = product_order("product-order-other");
        theirs.recipient_pub_dress = "0x1other".to_owned();
        assert_eq!(
            repository.settle_special_order(&other, &theirs, 300, true).await.unwrap(),
            SpecialSeedsOutcome::SessionChanged
        );
        // The profile's public name equals the other Bond's address: still no link.
        run(&repository, "UPDATE public_profiles SET pub_dress = '0x1other'", &[]).await;
        assert_eq!(
            repository.settle_special_order(&other, &theirs, 300, true).await.unwrap(),
            SpecialSeedsOutcome::SessionChanged
        );
        assert_eq!(repository.read_seeds_balance(&other).await.unwrap(), 1000);
        // The attached Bond itself still works.
        assert_eq!(
            repository.settle_special_order(&owner, &product_order("product-order-mine"), 300, true).await.unwrap(),
            SpecialSeedsOutcome::IssuedItem
        );
    }

    #[tokio::test]
    async fn an_open_takeover_request_by_another_client_blocks_a_product_session_until_it_lapses() {
        let (repository, owner, _dir) = attached_product_bond().await;
        let other_client = "f".repeat(64);
        run(
            &repository,
            "INSERT INTO product_activation_requests \
             (id, issuer, subject, requester_token_hash, approver_token_hash, requester_label, approver_label, \
              epoch, status, created_at, deadline) \
             SELECT ?, issuer, subject, ?, ?, 'iPhone', 'Mac', 0, 'pending', 1, 1000 FROM public_profiles",
            &[&"a".repeat(32), &other_client, &product_hex()],
        )
        .await;
        assert_eq!(
            repository.settle_special_order(&owner, &product_order("product-order-blocked"), 500_000, true).await.unwrap(),
            SpecialSeedsOutcome::SessionChanged,
            "a request open at 500 s blocks spending"
        );
        assert_eq!(
            repository.settle_special_order(&owner, &product_order("product-order-after"), 1_500_000, true).await.unwrap(),
            SpecialSeedsOutcome::IssuedItem,
            "a lapsed request no longer blocks"
        );
    }
}
