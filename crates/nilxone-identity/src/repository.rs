// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::str::FromStr;

use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use thiserror::Error;

use crate::{
    AvaiaPubDress, BondAccessRole, DecimalU64, GeoCoordinate, PubDress, PubDressLabel,
    StationPubDress,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityProvider {
    Telegram,
    Discord,
    Github,
}

impl IdentityProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Github => "github",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderIdentity {
    pub provider: IdentityProvider,
    pub subject: String,
}

/// A Single Active Client activation request (`nilx-one/0x1`
/// `documents/15-devices-and-recovery.md`). `Pending` is the confirmation
/// window on the active client. `Objectable` is the live-device objection
/// window, and only a credential-authenticated requester reaches it.
/// `Accepted`/`Declined`/`Objected`/`Expired` are terminal. `Expired` is a
/// host-authenticated request past its TTL, a requester that is no longer a
/// live session, or a request superseded by a newer one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivationRequestStatus {
    Pending,
    Objectable,
    Accepted,
    Declined,
    Objected,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationRequest {
    pub id: Vec<u8>,
    pub pub_dress: String,
    pub requester_token_hash: Vec<u8>,
    pub client_label: String,
    /// Copied from the requester session at creation. Route 3 (the silent
    /// objection window) is only for a credential-authenticated requester.
    pub credential_authenticated: bool,
    pub status: ActivationRequestStatus,
    pub responds_by: u64,
    pub objection_deadline: Option<u64>,
}

impl ProviderIdentity {
    pub fn telegram(user_id: i64) -> Self {
        Self {
            provider: IdentityProvider::Telegram,
            subject: user_id.to_string(),
        }
    }

    pub fn discord(user_id: impl Into<String>) -> Self {
        Self {
            provider: IdentityProvider::Discord,
            subject: user_id.into(),
        }
    }

    pub fn github(user_id: u64) -> Self {
        Self {
            provider: IdentityProvider::Github,
            subject: user_id.to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityRecord {
    pub pub_dress: String,
    /// The owned Avaia is identity state. `None` is valid only for a
    /// pre-amendment human Bond that has not yet crossed an authenticated
    /// reconciliation boundary.
    pub avaia_pub_dress: Option<String>,
    /// The DNS A-label allocated to this Bond. `None` means no public label is
    /// allocated; callers must not derive ownership from `pub_dress` instead.
    pub pub_dress_label: Option<String>,
    /// The persisted allocation suffix. Empty for the unsuffixed first claim.
    pub pub_dress_label_suffix: String,
}

impl IdentityRecord {
    /// Creates a transient record before the authenticated reconciliation read.
    /// No public address is claimed until repository state is read back.
    pub fn unresolved(pub_dress: String) -> Self {
        Self {
            pub_dress,
            avaia_pub_dress: None,
            pub_dress_label: None,
            pub_dress_label_suffix: String::new(),
        }
    }

    /// Returns the product-readable public URL only from persisted allocation
    /// state. DNS transport keeps the A-label; Unicode is presentation only.
    pub fn readable_url(&self, zone: &str) -> Option<String> {
        let stored_label = self.pub_dress_label.as_ref()?;
        let label = if stored_label.starts_with("xn--") {
            format!("{}{}", self.pub_dress, self.pub_dress_label_suffix)
        } else {
            stored_label.clone()
        };
        Some(format!("https://{label}.{zone}"))
    }
}

/// A public lookup is authoritative because it comes from the allocated stored
/// DNS label, never by decoding a hostname and guessing which case-sensitive
/// `pub_dress` it meant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicIdentityRecord {
    pub identity: IdentityRecord,
    pub pub_dress_label: String,
    pub pub_dress_label_suffix: String,
}

impl PublicIdentityRecord {
    /// Product-readable URL. DNS still carries `pub_dress_label`; the readable
    /// Unicode form is presentation only and is safe because this record already
    /// proves which Bond owns that A-label.
    pub fn readable_url(&self, zone: &str) -> String {
        let label = if self.pub_dress_label.starts_with("xn--") {
            format!("{}{}", self.identity.pub_dress, self.pub_dress_label_suffix)
        } else {
            self.pub_dress_label.clone()
        };
        format!("https://{label}.{zone}")
    }
}

#[derive(Clone, Debug)]
pub struct IdentityRepository {
    pool: SqlitePool,
}

impl IdentityRepository {
    /// Service-internal input to a partner-specific digest, never a public view.
    pub async fn market_pairwise_subject(&self, owner: &str, digester: &crate::SecretDigester) -> Result<Vec<u8>, sqlx::Error> {
        let bytes: Vec<u8> = sqlx::query_scalar("SELECT subject FROM identity_subjects WHERE pub_dress = ?")
            .bind(owner.to_string()).fetch_one(&self.pool).await?;
        let subject: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(digester.digest("market-pairwise-subject-v1", &subject))
    }
    pub async fn connect(database_url: &str) -> Result<Self, RepositoryError> {
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        let repository = Self { pool };
        repository.initialize().await?;
        Ok(repository)
    }

    async fn initialize(&self) -> Result<(), RepositoryError> {
        sqlx::query(include_str!("../migrations/0001_identities.sql"))
            .execute(&self.pool)
            .await?;

        if self.has_identity_column("tg_id").await? {
            sqlx::raw_sql(include_str!("../migrations/0002_provider_accounts.sql"))
                .execute(&self.pool)
                .await?;
        }
        self.migrate_provider_catalog().await?;

        sqlx::raw_sql(include_str!("../migrations/0003_native_auth.sql"))
            .execute(&self.pool)
            .await?;
        // Retain legacy recovery material, but new credentials have no key.
        let recovery_required: i64 = sqlx::query_scalar(
            "SELECT \"notnull\" FROM pragma_table_info('native_credentials') WHERE name = 'recovery_key_hash'",
        ).fetch_one(&self.pool).await?;
        if recovery_required == 1 {
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            sqlx::raw_sql(include_str!(
                "../migrations/0034_optional_native_recovery.sql"
            ))
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
        }
        if !self.has_identity_column("identity_kind").await? {
            sqlx::raw_sql(include_str!("../migrations/0004_owned_avaia_identity.sql"))
                .execute(&self.pool)
                .await?;
        }
        if !self.has_identity_column("avatar_model").await? {
            sqlx::raw_sql(include_str!("../migrations/0005_avatar_model.sql"))
                .execute(&self.pool)
                .await?;
        }
        if !self.has_identity_column("pub_dress_label").await? {
            sqlx::raw_sql(include_str!("../migrations/0006_pub_dress_label.sql"))
                .execute(&self.pool)
                .await?;
        }
        self.migrate_avatar_catalog().await?;
        self.backfill_pub_dress_labels().await?;
        sqlx::raw_sql(include_str!("../migrations/0013_avaia_location.sql"))
            .execute(&self.pool)
            .await?;
        // Widens the 0013 lifecycle guard for stations, so it runs right after.
        sqlx::raw_sql(include_str!("../migrations/0024_business_stations.sql"))
            .execute(&self.pool)
            .await?;
        if !self.has_business_station_column("caption").await? {
            sqlx::raw_sql(include_str!(
                "../migrations/0025_business_station_caption.sql"
            ))
            .execute(&self.pool)
            .await?;
        }
        // A business is many points, not one location: 0026's table is gone.
        sqlx::raw_sql(include_str!(
            "../migrations/0027_drop_business_locations.sql"
        ))
        .execute(&self.pool)
        .await?;
        if !self.has_avaia_location_column("travel_revision").await? {
            sqlx::raw_sql(include_str!("../migrations/0022_avaia_travel_revision.sql"))
                .execute(&self.pool)
                .await?;
        }
        // Travel writes Bond and Avaia state within this repository's pool.
        sqlx::raw_sql(include_str!("../migrations/0009_bond_location_control.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0017_bond_pub_info.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0020_committed_experience.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0023_orb_spills.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0028_xping_engines.sql"))
            .execute(&self.pool)
            .await?;
        self.migrate_avaia_prefix().await?;
        self.migrate_bond_roles().await?;
        let mut reserved_transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::raw_sql(include_str!("../migrations/0021_reserved_admin_prefix.sql"))
            .execute(&mut *reserved_transaction)
            .await?;
        reserved_transaction.commit().await?;
        sqlx::raw_sql(include_str!("../migrations/0030_open_zero_prefix.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0029_admin_role_requests.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0031_admin_telegram_delivery.sql"
        ))
        .execute(&self.pool)
        .await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0032_admin_telegram_approval_confirmations.sql"
        ))
        .execute(&self.pool)
        .await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0033_telegram_message_slots.sql"
        ))
        .execute(&self.pool)
        .await?;
        sqlx::raw_sql(include_str!("../migrations/0036_cell_orb_claims.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0037_seeds_special_subscriptions.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0038_seeds_credit_provenance.sql"))
            .execute(&self.pool)
            .await?;
        if !self.has_native_sessions_column("active").await? {
            sqlx::raw_sql(include_str!("../migrations/0016_session_activation.sql"))
                .execute(&self.pool)
                .await?;
        }
        // Backfill stable, service-private subjects for existing humans and
        // install the registration trigger for all future native/provider users.
        // The migration is idempotent; it must never rotate an existing subject.
        let mut subject_transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0035_private_identity_subjects.sql"
        ))
        .execute(&mut *subject_transaction)
        .await?;
        subject_transaction.commit().await?;
        sqlx::raw_sql(include_str!("../migrations/0039_business_memberships.sql"))
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0040_market_visits.sql"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Creates `bond_roles` and carries the previous name-derived admins over,
    /// exactly once: a Bond that registers one of those names later gets no
    /// role from it.
    async fn migrate_bond_roles(&self) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema \
             WHERE type = 'table' AND name = 'bond_roles')",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !exists {
            sqlx::raw_sql(include_str!("../migrations/0015_bond_roles.sql"))
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Server-stored access roles, never implicitly derived from 0x0.
    pub async fn role_for(&self, pub_dress: &str) -> Result<BondAccessRole, RepositoryError> {
        let stored = sqlx::query_scalar::<_, String>(
            "SELECT COALESCE(r.role, 'user') FROM identities i \
             LEFT JOIN bond_roles r ON r.pub_dress = i.pub_dress WHERE i.pub_dress = ?",
        )
        .bind(pub_dress)
        .fetch_optional(&self.pool)
        .await?;
        match stored {
            None => Ok(BondAccessRole::default()),
            Some(value) => {
                BondAccessRole::from_stored(&value).ok_or(RepositoryError::CorruptBondRole)
            }
        }
    }

    /// Assigns a role to an existing human Bond. There is no public route to
    /// this: public registration creates only `user` Bonds.
    pub async fn set_role(
        &self,
        pub_dress: &PubDress,
        role: BondAccessRole,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "INSERT INTO bond_roles (pub_dress, role) \
             SELECT pub_dress, ? FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human' \
             ON CONFLICT(pub_dress) DO UPDATE SET role = excluded.role",
        )
        .bind(role.as_str())
        .bind(pub_dress.as_str())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Stored Avaia addresses predate the literal `x` prefix. Rewrite them once,
    /// keeping any configuration state the rename trigger would otherwise flip.
    async fn migrate_avaia_prefix(&self) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let legacy = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities \
             WHERE identity_kind = 'avaia' AND substr(pub_dress, 1, 1) <> 'x' \
               AND owner_pub_dress IS NOT NULL)",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if legacy {
            let configured = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema \
                 WHERE type = 'table' AND name = 'avaia_configuration')",
            )
            .fetch_one(&mut *transaction)
            .await?;
            sqlx::raw_sql(include_str!("../migrations/0014_avaia_x_prefix.sql"))
                .execute(&mut *transaction)
                .await?;
            if configured {
                sqlx::raw_sql(include_str!("../migrations/0007_avaia_configuration.sql"))
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn migrate_provider_catalog(&self) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let schema: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'identity_providers'",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !schema.contains("'github'") {
            sqlx::raw_sql(include_str!("../migrations/0010_github_provider.sql"))
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn migrate_avatar_catalog(&self) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let schema: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'identities'",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !schema.contains("'dasha-v2-study'") {
            sqlx::raw_sql(include_str!("../migrations/0008_dasha_v2_avatar.sql"))
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn has_identity_column(&self, name: &str) -> Result<bool, RepositoryError> {
        let columns = sqlx::query("PRAGMA table_info(identities)")
            .fetch_all(&self.pool)
            .await?;
        Ok(columns
            .iter()
            .any(|column| column.get::<String, _>("name") == name))
    }

    async fn has_avaia_location_column(&self, name: &str) -> Result<bool, RepositoryError> {
        let columns = sqlx::query("PRAGMA table_info(avaia_locations)")
            .fetch_all(&self.pool)
            .await?;
        Ok(columns
            .iter()
            .any(|column| column.get::<String, _>("name") == name))
    }

    async fn has_business_station_column(&self, name: &str) -> Result<bool, RepositoryError> {
        let columns = sqlx::query("PRAGMA table_info(business_stations)")
            .fetch_all(&self.pool)
            .await?;
        Ok(columns
            .iter()
            .any(|column| column.get::<String, _>("name") == name))
    }

    async fn has_native_sessions_column(&self, name: &str) -> Result<bool, RepositoryError> {
        let columns = sqlx::query("PRAGMA table_info(native_sessions)")
            .fetch_all(&self.pool)
            .await?;
        Ok(columns
            .iter()
            .any(|column| column.get::<String, _>("name") == name))
    }

    /// Existing Bonds predate the stored public-label column. Backfill in stable
    /// creation order: the earliest human Bond wins a colliding DNS label and a
    /// later collision stays unallocated rather than inventing mutuality or a
    /// suffix the owner never chose.
    async fn backfill_pub_dress_labels(&self) -> Result<(), RepositoryError> {
        let rows = sqlx::query(
            "SELECT pub_dress FROM identities \
             WHERE identity_kind = 'human' AND pub_dress_label IS NULL \
             ORDER BY created_at ASC, pub_dress COLLATE BINARY ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        for row in rows {
            let raw: String = row.get("pub_dress");
            let pub_dress =
                PubDress::from_str(&raw).map_err(|_| RepositoryError::CorruptHumanPubDress)?;
            let Some(label) = default_pub_dress_label(&pub_dress) else {
                continue;
            };
            sqlx::query(
                "UPDATE OR IGNORE identities \
                 SET pub_dress_label = ?, pub_dress_label_suffix = '' \
                 WHERE pub_dress = ? AND identity_kind = 'human' \
                   AND pub_dress_label IS NULL",
            )
            .bind(label)
            .bind(pub_dress.as_str())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn register(
        &self,
        pub_dress: &PubDress,
        provider_identity: &ProviderIdentity,
        now: u64,
    ) -> Result<RegistrationOutcome, RepositoryError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        if let Some(record) = find_by_provider_in(&mut transaction, provider_identity).await? {
            transaction.commit().await?;
            return Ok(RegistrationOutcome::AlreadyRegistered(record));
        }

        match insert_human_with_public_label_in(&mut transaction, pub_dress, now).await? {
            HumanIdentityInsertOutcome::Inserted => {}
            HumanIdentityInsertOutcome::PubDressUnavailable => {
                transaction.rollback().await?;
                return Ok(RegistrationOutcome::HandleUnavailable);
            }
            HumanIdentityInsertOutcome::PublicLabelUnavailable => {
                transaction.rollback().await?;
                return Ok(RegistrationOutcome::PublicLabelUnavailable);
            }
        }

        sqlx::query(
            "INSERT INTO identity_providers (provider, provider_subject, pub_dress) VALUES (?, ?, ?)",
        )
        .bind(provider_identity.provider.as_str())
        .bind(&provider_identity.subject)
        .bind(pub_dress.as_str())
        .execute(&mut *transaction)
        .await?;

        let record = identity_for_pub_dress_in(&mut transaction, pub_dress.to_string()).await?;
        transaction.commit().await?;
        Ok(RegistrationOutcome::Registered(record))
    }

    pub async fn find_by_provider(
        &self,
        provider_identity: &ProviderIdentity,
    ) -> Result<Option<IdentityRecord>, RepositoryError> {
        let pub_dress = sqlx::query_scalar::<_, String>(
            "SELECT identities.pub_dress \
             FROM identity_providers \
             JOIN identities ON identities.pub_dress = identity_providers.pub_dress \
             WHERE identities.identity_kind = 'human' \
               AND identity_providers.provider = ? \
               AND identity_providers.provider_subject = ?",
        )
        .bind(provider_identity.provider.as_str())
        .bind(&provider_identity.subject)
        .fetch_optional(&self.pool)
        .await?;

        match pub_dress {
            Some(value) => Ok(Some(identity_for_pub_dress(&self.pool, value).await?)),
            None => Ok(None),
        }
    }

    /// Reads the full record of a human Bond. Observational only: a Bond
    /// without an Avaia stays without one until its owner creates it through
    /// the explicit Avaia setup.
    pub async fn resolve_human_identity(
        &self,
        pub_dress: &PubDress,
    ) -> Result<Option<IdentityRecord>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let human_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human')",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !human_exists {
            transaction.rollback().await?;
            return Ok(None);
        }

        let record = identity_for_pub_dress_in(&mut transaction, pub_dress.to_string()).await?;
        transaction.commit().await?;
        Ok(Some(record))
    }

    /// Moves a human Bond to another address it already owns the right to
    /// choose. The Bond, its provider bindings, credentials and sessions are the
    /// same facts under the new address; only the address changes.
    ///
    /// The owned Avaia address is a derivation of its owner's address, so it
    /// moves with the owner rather than outliving the name it was derived from.
    pub async fn rename_pub_dress(
        &self,
        current: &PubDress,
        next: &PubDress,
    ) -> Result<PubDressRenameOutcome, RepositoryError> {
        if current.discriminator() != next.discriminator() {
            return Ok(PubDressRenameOutcome::Unavailable);
        }
        let mut transaction = self.pool.begin().await?;
        // `identities.owner_pub_dress` is the one reference without
        // ON UPDATE CASCADE, so the owned Avaia row is re-pointed by the second
        // statement and the constraint is checked at commit instead of between
        // the two. Every other reference cascades on its own.
        sqlx::query("PRAGMA defer_foreign_keys = ON")
            .execute(&mut *transaction)
            .await?;

        let suffix = sqlx::query_scalar::<_, String>(
            "SELECT pub_dress_label_suffix FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human'",
        )
        .bind(current.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(suffix) = suffix else {
            transaction.rollback().await?;
            return Ok(PubDressRenameOutcome::Unknown);
        };

        let occupied = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities WHERE pub_dress = ?)",
        )
        .bind(next.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if occupied {
            transaction.rollback().await?;
            return Ok(PubDressRenameOutcome::Unavailable);
        }

        let next_label = pub_dress_label_with_suffix(next, &suffix)?;
        if let Some(label) = next_label.as_deref() {
            let label_occupied = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM identities \
                 WHERE identity_kind = 'human' \
                   AND pub_dress_label = ? COLLATE NOCASE \
                   AND pub_dress <> ?)",
            )
            .bind(label)
            .bind(current.as_str())
            .fetch_one(&mut *transaction)
            .await?;
            if label_occupied {
                transaction.rollback().await?;
                return Ok(PubDressRenameOutcome::Unavailable);
            }
        }

        let existing_avaia = owned_avaia_for_owner_in(&mut transaction, current.as_str()).await?;
        // A derived Avaia address is a consequence of its owner's name, so it
        // follows the owner. An address its owner chose is not a derivation:
        // renaming the human keeps the Avaia exactly as it was named, which the
        // shared discriminator leaves canonical.
        let derived_from_current = AvaiaPubDress::derive_default(current).to_string();
        let derived_avaia = existing_avaia
            .as_deref()
            .is_some_and(|existing| existing == derived_from_current)
            .then(|| AvaiaPubDress::derive_default(next).to_string());
        if let Some(derived) = derived_avaia.as_deref() {
            let avaia_occupied = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM identities \
                 WHERE pub_dress = ? AND owner_pub_dress IS NOT ?)",
            )
            .bind(derived)
            .bind(current.as_str())
            .fetch_one(&mut *transaction)
            .await?;
            if avaia_occupied {
                transaction.rollback().await?;
                return Ok(PubDressRenameOutcome::AvaiaUnavailable);
            }
        }

        sqlx::query("UPDATE identities SET pub_dress = ?, pub_dress_label = ? WHERE pub_dress = ?")
            .bind(next.as_str())
            .bind(next_label)
            .bind(current.as_str())
            .execute(&mut *transaction)
            .await?;

        // A Bond without an Avaia keeps none: renaming is not a creation
        // boundary.
        if let Some(existing) = existing_avaia {
            let moved = derived_avaia.unwrap_or(existing);
            sqlx::query(
                "UPDATE identities SET pub_dress = ?, owner_pub_dress = ? \
                 WHERE identity_kind = 'avaia' AND owner_pub_dress = ?",
            )
            .bind(&moved)
            .bind(next.as_str())
            .bind(current.as_str())
            .execute(&mut *transaction)
            .await?;
        }

        let record = identity_for_pub_dress_in(&mut transaction, next.to_string()).await?;
        transaction.commit().await?;
        Ok(PubDressRenameOutcome::Renamed(record))
    }

    /// Names the Avaia a human Bond owns. The address keeps its owner's
    /// discriminator and the canonical `ai` suffix, both of which the parsed
    /// address already carries; only the name in between is a choice.
    pub async fn rename_owned_avaia(
        &self,
        owner: &PubDress,
        next: &AvaiaPubDress,
    ) -> Result<PubDressRenameOutcome, RepositoryError> {
        let mut transaction = self.pool.begin().await?;

        let human_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human')",
        )
        .bind(owner.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !human_exists {
            transaction.rollback().await?;
            return Ok(PubDressRenameOutcome::Unknown);
        }

        let occupied = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities \
             WHERE pub_dress = ? \
               AND NOT (identity_kind = 'avaia' AND owner_pub_dress IS ?))",
        )
        .bind(next.as_str())
        .bind(owner.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if occupied {
            transaction.rollback().await?;
            return Ok(PubDressRenameOutcome::AvaiaUnavailable);
        }

        let existing = owned_avaia_for_owner_in(&mut transaction, owner.as_str()).await?;
        match existing {
            Some(_) => {
                sqlx::query(
                    "UPDATE identities SET pub_dress = ? \
                     WHERE identity_kind = 'avaia' AND owner_pub_dress = ?",
                )
                .bind(next.as_str())
                .bind(owner.as_str())
                .execute(&mut *transaction)
                .await?;
            }
            // Naming is not a creation boundary: only the explicit Avaia
            // setup creates one.
            None => {
                transaction.rollback().await?;
                return Ok(PubDressRenameOutcome::AvaiaMissing);
            }
        }

        let record = identity_for_pub_dress_in(&mut transaction, owner.to_string()).await?;
        transaction.commit().await?;
        Ok(PubDressRenameOutcome::Renamed(record))
    }

    pub async fn is_pub_dress_available(
        &self,
        pub_dress: &PubDress,
    ) -> Result<bool, RepositoryError> {
        let occupied = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities WHERE pub_dress = ?)",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(!occupied)
    }

    pub async fn is_pub_dress_label_available(
        &self,
        label: &PubDressLabel,
    ) -> Result<bool, RepositoryError> {
        let occupied = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities \
             WHERE identity_kind = 'human' AND pub_dress_label = ? COLLATE NOCASE)",
        )
        .bind(label.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(!occupied)
    }

    pub async fn find_by_pub_dress_label(
        &self,
        label: &PubDressLabel,
    ) -> Result<Option<PublicIdentityRecord>, RepositoryError> {
        let row = sqlx::query(
            "SELECT pub_dress, pub_dress_label, pub_dress_label_suffix \
             FROM identities \
             WHERE identity_kind = 'human' AND pub_dress_label = ? COLLATE NOCASE",
        )
        .bind(label.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let pub_dress: String = row.get("pub_dress");
        let pub_dress_label: String = row.get("pub_dress_label");
        let pub_dress_label_suffix: String = row.get("pub_dress_label_suffix");
        let identity = identity_for_pub_dress(&self.pool, pub_dress).await?;
        Ok(Some(PublicIdentityRecord {
            identity,
            pub_dress_label,
            pub_dress_label_suffix,
        }))
    }

    pub async fn find_by_telegram(
        &self,
        telegram_user_id: i64,
    ) -> Result<Option<IdentityRecord>, RepositoryError> {
        self.find_by_provider(&ProviderIdentity::telegram(telegram_user_id))
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn register_native(
        &self,
        pub_dress: &PubDress,
        password_hash: &str,
        password_hash_version: i64,
        recovery_key_hash: &[u8],
        challenge_hash: &[u8],
        idempotency_key_hash: &[u8],
        now: u64,
        challenge_expires_at: u64,
    ) -> Result<NativeRegistrationOutcome, RepositoryError> {
        self.register_native_credential(
            pub_dress,
            password_hash,
            password_hash_version,
            Some((recovery_key_hash, challenge_hash, challenge_expires_at)),
            idempotency_key_hash,
            now,
        )
        .await
    }

    /// Commit an immediately usable password without issuing recovery material.
    pub async fn register_native_password(
        &self,
        pub_dress: &PubDress,
        password_hash: &str,
        password_hash_version: i64,
        idempotency_key_hash: &[u8],
        now: u64,
    ) -> Result<NativeRegistrationOutcome, RepositoryError> {
        self.register_native_credential(
            pub_dress,
            password_hash,
            password_hash_version,
            None,
            idempotency_key_hash,
            now,
        )
        .await
    }

    async fn register_native_credential(
        &self,
        pub_dress: &PubDress,
        password_hash: &str,
        password_hash_version: i64,
        recovery: Option<(&[u8], &[u8], u64)>,
        idempotency_key_hash: &[u8],
        now: u64,
    ) -> Result<NativeRegistrationOutcome, RepositoryError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        let replay = sqlx::query_scalar::<_, String>(
            "SELECT pub_dress FROM native_registration_idempotency \
             WHERE idempotency_key_hash = ?",
        )
        .bind(idempotency_key_hash)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(pub_dress) = replay {
            let record = identity_for_pub_dress_in(&mut transaction, pub_dress).await?;
            transaction.commit().await?;
            return Ok(NativeRegistrationOutcome::IdempotentReplay(record));
        }

        match insert_human_with_public_label_in(&mut transaction, pub_dress, now).await? {
            HumanIdentityInsertOutcome::Inserted => {}
            HumanIdentityInsertOutcome::PubDressUnavailable => {
                transaction.rollback().await?;
                return Ok(NativeRegistrationOutcome::HandleUnavailable);
            }
            HumanIdentityInsertOutcome::PublicLabelUnavailable => {
                transaction.rollback().await?;
                return Ok(NativeRegistrationOutcome::PublicLabelUnavailable);
            }
        }

        sqlx::query(
            "INSERT INTO native_credentials \
             (pub_dress, password_hash, password_hash_version, recovery_key_hash, active, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(pub_dress.as_str())
        .bind(password_hash)
        .bind(password_hash_version)
        .bind(recovery.map(|(key, _, _)| key))
        .bind(recovery.is_none())
        .bind(now as i64)
        .bind(now as i64)
        .execute(&mut *transaction)
        .await?;
        if let Some((_, challenge_hash, challenge_expires_at)) = recovery {
            sqlx::query(
                "INSERT INTO native_registration_challenges \
             (challenge_hash, pub_dress, expires_at, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(challenge_hash)
            .bind(pub_dress.as_str())
            .bind(challenge_expires_at as i64)
            .bind(now as i64)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "INSERT INTO native_registration_idempotency \
             (idempotency_key_hash, pub_dress, created_at) VALUES (?, ?, ?)",
        )
        .bind(idempotency_key_hash)
        .bind(pub_dress.as_str())
        .bind(now as i64)
        .execute(&mut *transaction)
        .await?;

        let record = identity_for_pub_dress_in(&mut transaction, pub_dress.to_string()).await?;
        transaction.commit().await?;
        Ok(NativeRegistrationOutcome::Registered(record))
    }

    /// The provider binding is the authority; a submitted handle cannot select an owner.
    /// An active credential is immutable through this setup path.
    pub async fn set_provider_password(
        &self,
        provider: &ProviderIdentity,
        password_hash: &str,
        password_hash_version: i64,
        now: u64,
    ) -> Result<Option<IdentityRecord>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let pub_dress = sqlx::query_scalar::<_, String>(
            "INSERT INTO native_credentials \
             (pub_dress, password_hash, password_hash_version, recovery_key_hash, active, created_at, updated_at) \
             SELECT p.pub_dress, ?, ?, NULL, 1, ?, ? FROM identity_providers p \
             JOIN identities i ON i.pub_dress = p.pub_dress \
             WHERE p.provider = ? AND p.provider_subject = ? AND i.identity_kind = 'human' \
             ON CONFLICT(pub_dress) DO UPDATE SET password_hash = excluded.password_hash, \
             password_hash_version = excluded.password_hash_version, \
             recovery_key_hash = NULL, active = 1, updated_at = excluded.updated_at \
             WHERE native_credentials.active = 0 RETURNING pub_dress",
        )
        .bind(password_hash)
        .bind(password_hash_version)
        .bind(now as i64)
        .bind(now as i64)
        .bind(provider.provider.as_str())
        .bind(&provider.subject)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(pub_dress) = pub_dress else {
            transaction.rollback().await?;
            return Ok(None);
        };
        // Invalidate a legacy unfinished setup challenge when replacing it.
        sqlx::query("DELETE FROM native_registration_challenges WHERE pub_dress = ?")
            .bind(&pub_dress)
            .execute(&mut *transaction)
            .await?;
        let record = identity_for_pub_dress_in(&mut transaction, pub_dress).await?;
        transaction.commit().await?;
        Ok(Some(record))
    }

    /// The avatar a Bond chose, or `None` while it has chosen none.
    pub async fn avatar_model(&self, pub_dress: &str) -> Result<Option<String>, RepositoryError> {
        Ok(sqlx::query_scalar::<_, Option<String>>(
            "SELECT avatar_model FROM identities \
             WHERE pub_dress = ? AND identity_kind = 'human'",
        )
        .bind(pub_dress)
        .fetch_optional(&self.pool)
        .await?
        .flatten())
    }

    /// Records that choice. Returns false when the address is not a human Bond.
    pub async fn set_avatar_model(
        &self,
        pub_dress: &str,
        model: &str,
    ) -> Result<bool, RepositoryError> {
        let updated = sqlx::query(
            "UPDATE identities SET avatar_model = ? \
             WHERE pub_dress = ? AND identity_kind = 'human'",
        )
        .bind(model)
        .bind(pub_dress)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }

    pub async fn password_required(&self, pub_dress: &str) -> Result<bool, RepositoryError> {
        let active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM native_credentials WHERE pub_dress = ? AND active = 1)",
        )
        .bind(pub_dress)
        .fetch_one(&self.pool)
        .await?;
        Ok(!active)
    }

    pub async fn find_native_credential(
        &self,
        pub_dress: &PubDress,
    ) -> Result<Option<NativeCredentialRecord>, RepositoryError> {
        let row = sqlx::query(
            "SELECT pub_dress, password_hash, password_hash_version, recovery_key_hash, active \
             FROM native_credentials WHERE pub_dress = ?",
        )
        .bind(pub_dress.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(native_credential_from_row))
    }

    pub async fn activate_native_registration(
        &self,
        challenge_hash: &[u8],
        now: u64,
    ) -> Result<Option<IdentityRecord>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let pub_dress = sqlx::query_scalar::<_, String>(
            "SELECT pub_dress FROM native_registration_challenges \
             WHERE challenge_hash = ? AND expires_at > ?",
        )
        .bind(challenge_hash)
        .bind(now as i64)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(pub_dress) = pub_dress else {
            transaction.rollback().await?;
            return Ok(None);
        };

        sqlx::query("UPDATE native_credentials SET active = 1, updated_at = ? WHERE pub_dress = ?")
            .bind(now as i64)
            .bind(&pub_dress)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM native_registration_challenges WHERE challenge_hash = ?")
            .bind(challenge_hash)
            .execute(&mut *transaction)
            .await?;
        let record = identity_for_pub_dress_in(&mut transaction, pub_dress).await?;
        transaction.commit().await?;
        Ok(Some(record))
    }

    /// Called only after password verification. CAS prevents a stale proof from
    /// activating a credential changed by concurrent provider setup or recovery.
    pub async fn complete_pending_native_password(
        &self,
        credential: &NativeCredentialRecord,
        now: u64,
    ) -> Result<bool, RepositoryError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let updated = sqlx::query(
            "UPDATE native_credentials SET active = 1, updated_at = ? \
             WHERE pub_dress = ? AND password_hash = ? AND \
             (active = 1 OR EXISTS (SELECT 1 FROM native_registration_challenges c \
              WHERE c.pub_dress = native_credentials.pub_dress))",
        )
        .bind(now as i64)
        .bind(&credential.pub_dress)
        .bind(&credential.password_hash)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query("DELETE FROM native_registration_challenges WHERE pub_dress = ?")
            .bind(&credential.pub_dress)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// A new session starts active only if the Bond currently holds no other
    /// active, still-valid session — the base case where there is nothing to
    /// activate against. Otherwise it starts inactive: activation is a
    /// separate, explicit step (`SessionActivationRequest`), never implied by
    /// merely authenticating on a new client. See `nilx-one/0x1`
    /// `documents/15-devices-and-recovery.md`, Single Active Client.
    ///
    /// `credential_authenticated` is true only for a password mint (login,
    /// recovery, registration acknowledgement). Host material must pass
    /// false: it is not the long-lived credential and must not silent-takeover.
    ///
    /// `supersede_token_hash` revokes the session this browser already
    /// presented, in the same transaction and before the active-session
    /// check, so a same-browser re-login that was active comes up active
    /// instead of orphaning that row for the rest of its TTL. A different
    /// device presents no cookie and still starts inactive.
    ///
    /// Returns whether the new session became active.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_native_session(
        &self,
        token_hash: &[u8],
        pub_dress: &str,
        now: u64,
        expires_at: u64,
        client_label: &str,
        credential_authenticated: bool,
        supersede_token_hash: Option<&[u8]>,
    ) -> Result<bool, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        if let Some(supersede) = supersede_token_hash.filter(|hash| *hash != token_hash) {
            sqlx::query(
                "UPDATE native_sessions SET revoked_at = ? \
                 WHERE token_hash = ? AND revoked_at IS NULL",
            )
            .bind(now as i64)
            .bind(supersede)
            .execute(&mut *transaction)
            .await?;
        }
        let has_active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM native_sessions \
             WHERE pub_dress = ? AND active = 1 AND revoked_at IS NULL AND expires_at > ?)",
        )
        .bind(pub_dress)
        .bind(now as i64)
        .fetch_one(&mut *transaction)
        .await?;
        let becomes_active = !has_active;
        sqlx::query(
            "INSERT INTO native_sessions \
             (token_hash, pub_dress, expires_at, created_at, active, client_label, \
              credential_authenticated) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(token_hash)
        .bind(pub_dress)
        .bind(expires_at as i64)
        .bind(now as i64)
        .bind(becomes_active)
        .bind(client_label)
        .bind(i64::from(credential_authenticated))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(becomes_active)
    }

    pub async fn find_native_session(
        &self,
        token_hash: &[u8],
        now: u64,
    ) -> Result<Option<IdentityRecord>, RepositoryError> {
        let pub_dress = sqlx::query_scalar::<_, String>(
            "SELECT pub_dress FROM native_sessions \
             WHERE token_hash = ? AND revoked_at IS NULL AND expires_at > ?",
        )
        .bind(token_hash)
        .bind(now as i64)
        .fetch_optional(&self.pool)
        .await?;
        match pub_dress {
            Some(value) => Ok(Some(identity_for_pub_dress(&self.pool, value).await?)),
            None => Ok(None),
        }
    }

    /// Same lookup as `find_native_session`, plus whether this session is
    /// currently the active one. An inactive session still identifies its
    /// Bond for reads; only a caller that would act for the Bond needs the
    /// flag.
    /// The client label a session was minted with, so an action taken in its
    /// name (such as requesting activation) can be shown under the same
    /// name the person already sees for that session.
    pub async fn native_session_client_label(
        &self,
        token_hash: &[u8],
    ) -> Result<Option<String>, RepositoryError> {
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT client_label FROM native_sessions WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn find_native_session_with_activity(
        &self,
        token_hash: &[u8],
        now: u64,
    ) -> Result<Option<(IdentityRecord, bool)>, RepositoryError> {
        let row = sqlx::query(
            "SELECT pub_dress, active FROM native_sessions \
             WHERE token_hash = ? AND revoked_at IS NULL AND expires_at > ?",
        )
        .bind(token_hash)
        .bind(now as i64)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => {
                let pub_dress: String = row.get("pub_dress");
                let active: bool = row.get("active");
                Ok(Some((
                    identity_for_pub_dress(&self.pool, pub_dress).await?,
                    active,
                )))
            }
            None => Ok(None),
        }
    }

    pub async fn revoke_native_session(
        &self,
        token_hash: &[u8],
        now: u64,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "UPDATE native_sessions SET revoked_at = ? \
             WHERE token_hash = ? AND revoked_at IS NULL",
        )
        .bind(now as i64)
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Creates a pending activation request naming the requesting client.
    /// The caller already authenticated the requester (a session cookie or a
    /// fresh provider/credential check) before this is reachable.
    ///
    /// Any other open request for this Bond is expired in the same
    /// transaction first. The inbox shows one request; a hidden sibling must
    /// not be able to accept itself later by polling its own status.
    /// `credential_authenticated` is copied from the requester session so
    /// route 3 follows how that session was minted, not a later edit.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_activation_request(
        &self,
        id: &[u8],
        pub_dress: &str,
        requester_token_hash: &[u8],
        client_label: &str,
        now: u64,
        responds_by: u64,
    ) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        expire_open_activation_requests(&mut transaction, pub_dress, None, now).await?;
        let credential_authenticated = sqlx::query_scalar::<_, i64>(
            "SELECT credential_authenticated FROM native_sessions WHERE token_hash = ?",
        )
        .bind(requester_token_hash)
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or(0)
            == 1;
        sqlx::query(
            "INSERT INTO session_activation_requests \
             (id, pub_dress, requester_token_hash, client_label, credential_authenticated, \
              status, created_at, responds_by) \
             VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)",
        )
        .bind(id)
        .bind(pub_dress)
        .bind(requester_token_hash)
        .bind(client_label)
        .bind(i64::from(credential_authenticated))
        .bind(now as i64)
        .bind(responds_by as i64)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The request the active client should be shown, if any, with the lazy
    /// pending -> objectable and objectable -> accepted transitions applied
    /// first. There is no background sweep; every read of a request passes
    /// through this.
    pub async fn pending_activation_request_for(
        &self,
        pub_dress: &str,
        now: u64,
        objection_window_seconds: u64,
    ) -> Result<Option<ActivationRequest>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, pub_dress, requester_token_hash, client_label, status, \
                    responds_by, objection_deadline, credential_authenticated \
             FROM session_activation_requests \
             WHERE pub_dress = ? AND status IN ('pending', 'objectable') \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(pub_dress)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let request = decode_activation_request(&row)?;
        let request =
            advance_activation_request(&mut transaction, request, now, objection_window_seconds)
                .await?;
        transaction.commit().await?;
        Ok(match request.status {
            ActivationRequestStatus::Pending | ActivationRequestStatus::Objectable => Some(request),
            _ => None,
        })
    }

    /// The requester's own view of a request it created, so it can learn the
    /// outcome without guessing from whether its session became active.
    pub async fn activation_request_status(
        &self,
        id: &[u8],
        requester_token_hash: &[u8],
        now: u64,
        objection_window_seconds: u64,
    ) -> Result<Option<ActivationRequest>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, pub_dress, requester_token_hash, client_label, status, \
                    responds_by, objection_deadline, credential_authenticated \
             FROM session_activation_requests \
             WHERE id = ? AND requester_token_hash = ?",
        )
        .bind(id)
        .bind(requester_token_hash)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let request = decode_activation_request(&row)?;
        let request =
            advance_activation_request(&mut transaction, request, now, objection_window_seconds)
                .await?;
        transaction.commit().await?;
        Ok(Some(request))
    }

    /// Accepts a request on behalf of the currently active session: the
    /// requester becomes active, and whichever session was active becomes
    /// inactive but stays signed in. Returns `false` if there is no such
    /// pending/objectable request for this Bond (already resolved, wrong
    /// Bond, or never existed) rather than erroring, since a stale UI action
    /// racing a lazy sweep is an ordinary outcome, not a failure.
    pub async fn accept_activation_request(
        &self,
        id: &[u8],
        pub_dress: &str,
        now: u64,
        objection_window_seconds: u64,
    ) -> Result<bool, RepositoryError> {
        self.resolve_activation_request(
            id,
            pub_dress,
            now,
            objection_window_seconds,
            ActivationRequestStatus::Accepted,
            true,
        )
        .await
    }

    /// Declines a request: terminal, no session changes. Same "false means
    /// nothing to do" contract as `accept_activation_request`.
    pub async fn decline_activation_request(
        &self,
        id: &[u8],
        pub_dress: &str,
        now: u64,
        objection_window_seconds: u64,
    ) -> Result<bool, RepositoryError> {
        self.resolve_activation_request(
            id,
            pub_dress,
            now,
            objection_window_seconds,
            ActivationRequestStatus::Declined,
            false,
        )
        .await
    }

    /// The active client's objection during the live-device objection
    /// window: only valid while the request is `objectable`, never while it
    /// is still merely `pending` (that path is a decline, not an objection).
    pub async fn object_to_activation_request(
        &self,
        id: &[u8],
        pub_dress: &str,
        now: u64,
        objection_window_seconds: u64,
    ) -> Result<bool, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let Some(row) = sqlx::query(
            "SELECT id, pub_dress, requester_token_hash, client_label, status, \
                    responds_by, objection_deadline, credential_authenticated \
             FROM session_activation_requests \
             WHERE id = ? AND pub_dress = ?",
        )
        .bind(id)
        .bind(pub_dress)
        .fetch_optional(&mut *transaction)
        .await?
        else {
            transaction.commit().await?;
            return Ok(false);
        };
        let request = decode_activation_request(&row)?;
        let request =
            advance_activation_request(&mut transaction, request, now, objection_window_seconds)
                .await?;
        if request.status != ActivationRequestStatus::Objectable {
            transaction.commit().await?;
            return Ok(false);
        }
        sqlx::query(
            "UPDATE session_activation_requests SET status = 'objected', resolved_at = ? \
             WHERE id = ? AND status = 'objectable'",
        )
        .bind(now as i64)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        expire_open_activation_requests(&mut transaction, pub_dress, Some(id), now).await?;
        transaction.commit().await?;
        Ok(true)
    }

    async fn resolve_activation_request(
        &self,
        id: &[u8],
        pub_dress: &str,
        now: u64,
        objection_window_seconds: u64,
        outcome: ActivationRequestStatus,
        activate_requester: bool,
    ) -> Result<bool, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let Some(row) = sqlx::query(
            "SELECT id, pub_dress, requester_token_hash, client_label, status, \
                    responds_by, objection_deadline, credential_authenticated \
             FROM session_activation_requests \
             WHERE id = ? AND pub_dress = ?",
        )
        .bind(id)
        .bind(pub_dress)
        .fetch_optional(&mut *transaction)
        .await?
        else {
            transaction.commit().await?;
            return Ok(false);
        };
        let request = decode_activation_request(&row)?;
        let request =
            advance_activation_request(&mut transaction, request, now, objection_window_seconds)
                .await?;
        if !matches!(
            request.status,
            ActivationRequestStatus::Pending | ActivationRequestStatus::Objectable
        ) {
            transaction.commit().await?;
            return Ok(false);
        }
        if activate_requester {
            let activated = activate_session_in(
                &mut transaction,
                pub_dress,
                &request.requester_token_hash,
                now,
            )
            .await?;
            if !activated {
                sqlx::query(
                    "UPDATE session_activation_requests SET status = 'expired', resolved_at = ? \
                     WHERE id = ? AND status IN ('pending', 'objectable')",
                )
                .bind(now as i64)
                .bind(id)
                .execute(&mut *transaction)
                .await?;
                expire_open_activation_requests(&mut transaction, pub_dress, Some(id), now).await?;
                transaction.commit().await?;
                return Ok(false);
            }
        }
        let status_literal = match outcome {
            ActivationRequestStatus::Accepted => "accepted",
            ActivationRequestStatus::Declined => "declined",
            _ => unreachable!("resolve_activation_request only accepts or declines"),
        };
        sqlx::query(
            "UPDATE session_activation_requests SET status = ?, resolved_at = ? WHERE id = ?",
        )
        .bind(status_literal)
        .bind(now as i64)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        expire_open_activation_requests(&mut transaction, pub_dress, Some(id), now).await?;
        transaction.commit().await?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn replace_native_credential_after_recovery(
        &self,
        pub_dress: &PubDress,
        expected_recovery_key_hash: &[u8],
        password_hash: &str,
        password_hash_version: i64,
        replacement_recovery_key_hash: &[u8],
        now: u64,
    ) -> Result<bool, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let update = sqlx::query(
            "UPDATE native_credentials \
             SET password_hash = ?, password_hash_version = ?, recovery_key_hash = ?, \
                 active = 1, updated_at = ? \
             WHERE pub_dress = ? AND recovery_key_hash = ? AND active = 1",
        )
        .bind(password_hash)
        .bind(password_hash_version)
        .bind(replacement_recovery_key_hash)
        .bind(now as i64)
        .bind(pub_dress.as_str())
        .bind(expected_recovery_key_hash)
        .execute(&mut *transaction)
        .await?;
        if update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "UPDATE native_sessions SET revoked_at = ? \
             WHERE pub_dress = ? AND revoked_at IS NULL",
        )
        .bind(now as i64)
        .bind(pub_dress.as_str())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_password_hash_if_version_advances(
        &self,
        pub_dress: &str,
        current_version: i64,
        new_hash: &str,
        new_version: i64,
        now: u64,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "UPDATE native_credentials SET password_hash = ?, password_hash_version = ?, updated_at = ? \
             WHERE pub_dress = ? AND password_hash_version = ? AND password_hash_version < ?",
        )
        .bind(new_hash)
        .bind(new_version)
        .bind(now as i64)
        .bind(pub_dress)
        .bind(current_version)
        .bind(new_version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

const PUBLIC_LABEL_RETRY_SUFFIXES: [&str; 8] = ["2", "3", "4", "5", "6", "7", "8", "9"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HumanIdentityInsertOutcome {
    Inserted,
    PubDressUnavailable,
    PublicLabelUnavailable,
}

async fn insert_human_with_public_label_in(
    transaction: &mut Transaction<'_, Sqlite>,
    pub_dress: &PubDress,
    now: u64,
) -> Result<HumanIdentityInsertOutcome, RepositoryError> {
    let stem = match PubDressLabel::stem(pub_dress) {
        Ok(stem) => stem,
        Err(_) => {
            let inserted = sqlx::query(
                "INSERT INTO identities \
                 (pub_dress, identity_kind, pub_dress_label, pub_dress_label_suffix, created_at) \
                 VALUES (?, 'human', NULL, '', ?) ON CONFLICT DO NOTHING",
            )
            .bind(pub_dress.as_str())
            .bind(now as i64)
            .execute(&mut **transaction)
            .await?;
            return Ok(if inserted.rows_affected() == 1 {
                HumanIdentityInsertOutcome::Inserted
            } else {
                HumanIdentityInsertOutcome::PubDressUnavailable
            });
        }
    };

    for suffix in core::iter::once("").chain(PUBLIC_LABEL_RETRY_SUFFIXES) {
        let label = match PubDressLabel::compose(&stem, suffix) {
            Ok(label) => label,
            Err(_) => continue,
        };
        let inserted = sqlx::query(
            "INSERT INTO identities \
             (pub_dress, identity_kind, pub_dress_label, pub_dress_label_suffix, created_at) \
             VALUES (?, 'human', ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(pub_dress.as_str())
        .bind(label.as_str())
        .bind(suffix)
        .bind(now as i64)
        .execute(&mut **transaction)
        .await?;
        if inserted.rows_affected() == 1 {
            return Ok(HumanIdentityInsertOutcome::Inserted);
        }

        // The failed INSERT is the collision boundary. This read only
        // classifies which UNIQUE constraint rejected it; it never
        // reserves a label or performs a check-then-insert allocation.
        let pub_dress_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities WHERE pub_dress = ?)",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&mut **transaction)
        .await?;
        if pub_dress_exists {
            return Ok(HumanIdentityInsertOutcome::PubDressUnavailable);
        }
    }

    Ok(HumanIdentityInsertOutcome::PublicLabelUnavailable)
}

fn default_pub_dress_label(pub_dress: &PubDress) -> Option<String> {
    PubDressLabel::stem(pub_dress)
        .ok()
        .map(|stem| stem.as_str().to_owned())
}

fn pub_dress_label_with_suffix(
    pub_dress: &PubDress,
    suffix: &str,
) -> Result<Option<String>, RepositoryError> {
    let Ok(stem) = PubDressLabel::stem(pub_dress) else {
        return if suffix.is_empty() {
            Ok(None)
        } else {
            Err(RepositoryError::CorruptPublicLabelSuffix)
        };
    };
    if suffix.is_empty() {
        return Ok(Some(stem.as_str().to_owned()));
    }
    PubDressLabel::compose(&stem, suffix)
        .map(|label| Some(label.as_str().to_owned()))
        .map_err(|_| RepositoryError::CorruptPublicLabelSuffix)
}

async fn create_owned_avaia_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &PubDress,
    now: u64,
) -> Result<Option<String>, sqlx::Error> {
    if let Some(existing) = owned_avaia_for_owner_in(transaction, owner.as_str()).await? {
        return Ok(Some(existing));
    }

    let candidate = AvaiaPubDress::derive_default(owner).to_string();
    let insert = sqlx::query(
        "INSERT INTO identities \
         (pub_dress, identity_kind, owner_pub_dress, created_at) \
         VALUES (?, 'avaia', ?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(&candidate)
    .bind(owner.as_str())
    .bind(now as i64)
    .execute(&mut **transaction)
    .await?;

    if insert.rows_affected() == 1 {
        return Ok(Some(candidate));
    }

    // A concurrent owner reconciliation can race on the owner-unique index.
    // Re-read the owner before classifying the failed insert as an address
    // collision with another identity.
    owned_avaia_for_owner_in(transaction, owner.as_str()).await
}

async fn owned_avaia_for_owner_in(
    transaction: &mut Transaction<'_, Sqlite>,
    owner_pub_dress: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT pub_dress FROM identities \
         WHERE identity_kind = 'avaia' AND owner_pub_dress = ?",
    )
    .bind(owner_pub_dress)
    .fetch_optional(&mut **transaction)
    .await
}

async fn identity_for_pub_dress(
    pool: &SqlitePool,
    pub_dress: String,
) -> Result<IdentityRecord, sqlx::Error> {
    let public = sqlx::query(
        "SELECT pub_dress_label, pub_dress_label_suffix FROM identities \
         WHERE pub_dress = ? AND identity_kind = 'human'",
    )
    .bind(&pub_dress)
    .fetch_one(pool)
    .await?;
    let avaia_pub_dress = sqlx::query_scalar::<_, String>(
        "SELECT pub_dress FROM identities \
         WHERE identity_kind = 'avaia' AND owner_pub_dress = ?",
    )
    .bind(&pub_dress)
    .fetch_optional(pool)
    .await?;
    Ok(IdentityRecord {
        pub_dress,
        avaia_pub_dress,
        pub_dress_label: public.get("pub_dress_label"),
        pub_dress_label_suffix: public.get("pub_dress_label_suffix"),
    })
}

async fn identity_for_pub_dress_in(
    transaction: &mut Transaction<'_, Sqlite>,
    pub_dress: String,
) -> Result<IdentityRecord, sqlx::Error> {
    let public = sqlx::query(
        "SELECT pub_dress_label, pub_dress_label_suffix FROM identities \
         WHERE pub_dress = ? AND identity_kind = 'human'",
    )
    .bind(&pub_dress)
    .fetch_one(&mut **transaction)
    .await?;
    let avaia_pub_dress = owned_avaia_for_owner_in(transaction, &pub_dress).await?;
    Ok(IdentityRecord {
        pub_dress,
        avaia_pub_dress,
        pub_dress_label: public.get("pub_dress_label"),
        pub_dress_label_suffix: public.get("pub_dress_label_suffix"),
    })
}

/// Makes `requester_token_hash` the one active session for `pub_dress` and
/// deactivates whichever session was active. The deactivated session is not
/// revoked: it stays signed in, only no longer able to act for the Bond.
///
/// Returns false, and changes nothing, when the requester is not a live
/// session (missing, revoked, expired, or an empty hash). Deactivating the
/// real client first would leave the Bond with nobody active.
async fn activate_session_in(
    transaction: &mut Transaction<'_, Sqlite>,
    pub_dress: &str,
    requester_token_hash: &[u8],
    now: u64,
) -> Result<bool, sqlx::Error> {
    let live = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM native_sessions \
         WHERE token_hash = ? AND revoked_at IS NULL AND expires_at > ?)",
    )
    .bind(requester_token_hash)
    .bind(now as i64)
    .fetch_one(&mut **transaction)
    .await?;
    if !live {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE native_sessions SET active = 0 \
         WHERE pub_dress = ? AND active = 1 AND revoked_at IS NULL AND expires_at > ? \
         AND token_hash != ?",
    )
    .bind(pub_dress)
    .bind(now as i64)
    .bind(requester_token_hash)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE native_sessions SET active = 1 \
         WHERE token_hash = ? AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(requester_token_hash)
    .bind(now as i64)
    .execute(&mut **transaction)
    .await?;
    Ok(true)
}

/// Expires every open request for `pub_dress`, optionally keeping `except_id`.
/// Accept, decline, object, and a newer create all call this so a sibling
/// left pending cannot later activate itself through its own status read.
async fn expire_open_activation_requests(
    transaction: &mut Transaction<'_, Sqlite>,
    pub_dress: &str,
    except_id: Option<&[u8]>,
    now: u64,
) -> Result<(), sqlx::Error> {
    match except_id {
        Some(id) => {
            sqlx::query(
                "UPDATE session_activation_requests SET status = 'expired', resolved_at = ? \
                 WHERE pub_dress = ? AND id != ? AND status IN ('pending', 'objectable')",
            )
            .bind(now as i64)
            .bind(pub_dress)
            .bind(id)
            .execute(&mut **transaction)
            .await?;
        }
        None => {
            sqlx::query(
                "UPDATE session_activation_requests SET status = 'expired', resolved_at = ? \
                 WHERE pub_dress = ? AND status IN ('pending', 'objectable')",
            )
            .bind(now as i64)
            .bind(pub_dress)
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

fn decode_activation_request(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<ActivationRequest, RepositoryError> {
    let status = match row.get::<String, _>("status").as_str() {
        "pending" => ActivationRequestStatus::Pending,
        "objectable" => ActivationRequestStatus::Objectable,
        "accepted" => ActivationRequestStatus::Accepted,
        "declined" => ActivationRequestStatus::Declined,
        "objected" => ActivationRequestStatus::Objected,
        "expired" => ActivationRequestStatus::Expired,
        _ => return Err(RepositoryError::CorruptActivationRequestStatus),
    };
    Ok(ActivationRequest {
        id: row.get("id"),
        pub_dress: row.get("pub_dress"),
        requester_token_hash: row.get("requester_token_hash"),
        client_label: row.get("client_label"),
        credential_authenticated: row.get::<i64, _>("credential_authenticated") == 1,
        status,
        responds_by: row.get::<i64, _>("responds_by") as u64,
        objection_deadline: row
            .get::<Option<i64>, _>("objection_deadline")
            .map(|value| value as u64),
    })
}

/// Applies the lazy `pending` -> `objectable` -> `accepted` transitions this
/// request is due for as of `now`, persisting whichever transition(s) apply,
/// and returns the request in its now-current state. There is no background
/// sweep; every read or action on a request passes through here first, so
/// nothing observes a state this function would have already advanced past.
async fn advance_activation_request(
    transaction: &mut Transaction<'_, Sqlite>,
    request: ActivationRequest,
    now: u64,
    objection_window_seconds: u64,
) -> Result<ActivationRequest, RepositoryError> {
    if request.status == ActivationRequestStatus::Pending && now >= request.responds_by {
        // Route 3 is only for a credential-authenticated requester. Host
        // material (Telegram initData, Discord, GitHub, browser OAuth) is
        // not the long-lived credential: an unanswered request expires and
        // the active client stays active.
        if !request.credential_authenticated {
            sqlx::query(
                "UPDATE session_activation_requests SET status = 'expired', resolved_at = ? \
                 WHERE id = ? AND status = 'pending'",
            )
            .bind(now as i64)
            .bind(&request.id)
            .execute(&mut **transaction)
            .await?;
            return Ok(ActivationRequest {
                status: ActivationRequestStatus::Expired,
                ..request
            });
        }
        // Anchor the objection window at responds_by, not at this read.
        // A late poll must not extend the window by however long the
        // requester waited to ask.
        let objection_deadline = request.responds_by.saturating_add(objection_window_seconds);
        sqlx::query(
            "UPDATE session_activation_requests \
             SET status = 'objectable', objection_deadline = ? \
             WHERE id = ? AND status = 'pending'",
        )
        .bind(objection_deadline as i64)
        .bind(&request.id)
        .execute(&mut **transaction)
        .await?;
        return Box::pin(advance_activation_request(
            transaction,
            ActivationRequest {
                status: ActivationRequestStatus::Objectable,
                objection_deadline: Some(objection_deadline),
                ..request
            },
            now,
            objection_window_seconds,
        ))
        .await;
    }
    if request.status == ActivationRequestStatus::Objectable
        && request
            .objection_deadline
            .is_some_and(|deadline| now >= deadline)
    {
        let activated = activate_session_in(
            transaction,
            &request.pub_dress,
            &request.requester_token_hash,
            now,
        )
        .await?;
        if !activated {
            sqlx::query(
                "UPDATE session_activation_requests SET status = 'expired', resolved_at = ? \
                 WHERE id = ? AND status = 'objectable'",
            )
            .bind(now as i64)
            .bind(&request.id)
            .execute(&mut **transaction)
            .await?;
            return Ok(ActivationRequest {
                status: ActivationRequestStatus::Expired,
                ..request
            });
        }
        sqlx::query(
            "UPDATE session_activation_requests SET status = 'accepted', resolved_at = ? \
             WHERE id = ? AND status = 'objectable'",
        )
        .bind(now as i64)
        .bind(&request.id)
        .execute(&mut **transaction)
        .await?;
        return Ok(ActivationRequest {
            status: ActivationRequestStatus::Accepted,
            ..request
        });
    }
    Ok(request)
}

/// A closed device-and-host label for a session. The raw User-Agent is never
/// stored. Spec examples look like `iPhone · Safari`. `host` is the minting
/// host (`Telegram`, `Discord`, `GitHub`) or `Browser` when the browser
/// itself is the host, in which case the browser name takes that place.
pub fn session_client_label(host: &str, user_agent: Option<&str>) -> String {
    let user_agent = user_agent.unwrap_or("");
    let device = classify_client_device(user_agent);
    let browser = classify_client_browser(user_agent);
    let host_part = if host.eq_ignore_ascii_case("browser") || host.is_empty() {
        browser.unwrap_or("Browser")
    } else {
        host
    };
    let label = match device {
        Some(device) => format!("{device} · {host_part}"),
        None => host_part.to_owned(),
    };
    bounded_client_label(label)
}

fn classify_client_device(user_agent: &str) -> Option<&'static str> {
    let user_agent = ascii_prefix(user_agent, 512).to_ascii_lowercase();
    if user_agent.contains("iphone") {
        Some("iPhone")
    } else if user_agent.contains("ipad") {
        Some("iPad")
    } else if user_agent.contains("android") {
        Some("Android")
    } else if user_agent.contains("cros") {
        Some("Chromebook")
    } else if user_agent.contains("macintosh") || user_agent.contains("mac os x") {
        Some("Mac")
    } else if user_agent.contains("windows") {
        Some("Windows")
    } else if user_agent.contains("linux") {
        Some("Linux")
    } else {
        None
    }
}

fn classify_client_browser(user_agent: &str) -> Option<&'static str> {
    let user_agent = ascii_prefix(user_agent, 512).to_ascii_lowercase();
    if user_agent.contains("edg/") || user_agent.contains("edga/") || user_agent.contains("edge/") {
        Some("Edge")
    } else if user_agent.contains("chrome/") || user_agent.contains("crios/") {
        Some("Chrome")
    } else if user_agent.contains("firefox/") || user_agent.contains("fxios/") {
        Some("Firefox")
    } else if user_agent.contains("safari/") {
        Some("Safari")
    } else {
        None
    }
}

fn ascii_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn bounded_client_label(label: String) -> String {
    const MAX_BYTES: usize = 80;
    if label.len() <= MAX_BYTES {
        return label;
    }
    let mut end = MAX_BYTES;
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    label[..end].to_owned()
}

async fn find_by_provider_in(
    transaction: &mut Transaction<'_, Sqlite>,
    provider_identity: &ProviderIdentity,
) -> Result<Option<IdentityRecord>, sqlx::Error> {
    let pub_dress = sqlx::query_scalar::<_, String>(
        "SELECT identities.pub_dress \
         FROM identity_providers \
         JOIN identities ON identities.pub_dress = identity_providers.pub_dress \
         WHERE identities.identity_kind = 'human' \
           AND identity_providers.provider = ? \
           AND identity_providers.provider_subject = ?",
    )
    .bind(provider_identity.provider.as_str())
    .bind(&provider_identity.subject)
    .fetch_optional(&mut **transaction)
    .await?;
    match pub_dress {
        Some(value) => Ok(Some(identity_for_pub_dress_in(transaction, value).await?)),
        None => Ok(None),
    }
}

fn native_credential_from_row(row: sqlx::sqlite::SqliteRow) -> NativeCredentialRecord {
    NativeCredentialRecord {
        pub_dress: row.get("pub_dress"),
        password_hash: row.get("password_hash"),
        password_hash_version: row.get("password_hash_version"),
        recovery_key_hash: row.get("recovery_key_hash"),
        active: row.get::<i64, _>("active") == 1,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCredentialRecord {
    pub pub_dress: String,
    pub password_hash: String,
    pub password_hash_version: i64,
    pub recovery_key_hash: Option<Vec<u8>>,
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeRegistrationOutcome {
    Registered(IdentityRecord),
    IdempotentReplay(IdentityRecord),
    HandleUnavailable,
    PublicLabelUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PubDressRenameOutcome {
    Renamed(IdentityRecord),
    /// Another identity — human or Avaia — already holds the requested address.
    Unavailable,
    /// The Avaia address the new owner address derives belongs to someone else.
    AvaiaUnavailable,
    /// The owner has not created an Avaia yet, so there is nothing to name.
    AvaiaMissing,
    /// The address the caller presented is no longer a human Bond.
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationOutcome {
    Registered(IdentityRecord),
    AlreadyRegistered(IdentityRecord),
    HandleUnavailable,
    PublicLabelUnavailable,
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("identity storage failed: {0}")]
    Storage(#[from] sqlx::Error),
    #[error("stored human pub_dress is invalid")]
    CorruptHumanPubDress,
    #[error("stored public-label suffix is invalid for this pub_dress")]
    CorruptPublicLabelSuffix,
    #[error("stored Avaia location violates its contract")]
    CorruptAvaiaLocation,
    #[error("stored Bond role is not a known role")]
    CorruptBondRole,
    #[error("stored activation request status is not a known status")]
    CorruptActivationRequestStatus,
    #[error("stored pub_info experience is not a known total")]
    CorruptPubInfo,
    #[error("stored Special/Exclusive subscription clock is invalid")]
    CorruptSubscriptionClock,
    #[error("stored seeds balance is invalid")]
    CorruptSeedsBalance,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        IdentityRepository, NativeRegistrationOutcome, ProviderIdentity, PubDressRenameOutcome,
        RegistrationOutcome,
    };
    use crate::{AvaiaPubDress, PubDress, PubDressLabel};

    #[tokio::test]
    async fn reserved_prefix_upgrade_preserves_explicit_roles_and_renames() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        for address in ["0x0нуль", "0x0other"] {
            sqlx::query("INSERT INTO identities (pub_dress, identity_kind, created_at) VALUES (?, 'human', 1)")
                .bind(address).execute(&repository.pool).await.expect("legacy human");
        }
        let admin: PubDress = "0x0нуль".parse().expect("admin");
        repository
            .set_role(&admin, crate::BondAccessRole::Admin)
            .await
            .expect("existing grant");
        repository.pool.close().await;
        let upgraded = IdentityRepository::connect(&url).await.expect("upgrade");
        assert_eq!(
            upgraded.role_for(admin.as_str()).await.expect("role"),
            crate::BondAccessRole::Admin
        );
        assert_eq!(
            upgraded.role_for("0x0other").await.expect("role"),
            crate::BondAccessRole::User
        );
        let next: PubDress = "0x0renamed".parse().expect("new");
        assert!(matches!(
            upgraded
                .rename_pub_dress(&admin, &next)
                .await
                .expect("rename"),
            PubDressRenameOutcome::Renamed(_)
        ));
        upgraded
            .set_role(&next, crate::BondAccessRole::Business)
            .await
            .expect("demotion");
        assert_eq!(
            upgraded.role_for(next.as_str()).await.expect("role"),
            crate::BondAccessRole::Business
        );
        upgraded.pool.close().await;
        let reopened = IdentityRepository::connect(&url).await.expect("reopen");
        assert_eq!(
            reopened.role_for(next.as_str()).await.expect("persisted"),
            crate::BondAccessRole::Business
        );
    }

    #[tokio::test]
    async fn zero_prefix_registration_creates_users_not_admins() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let reserved: PubDress = "0x0нуль".parse().expect("address");
        assert!(matches!(
            repository
                .register(&reserved, &ProviderIdentity::telegram(7), 1)
                .await
                .expect("register"),
            RegistrationOutcome::Registered(_)
        ));
        assert_eq!(
            repository.role_for(reserved.as_str()).await.expect("role"),
            crate::BondAccessRole::User
        );
        let native: PubDress = "0x0native".parse().expect("address");
        assert!(matches!(
            repository
                .register_native(
                    &native,
                    "hash",
                    1,
                    b"recovery",
                    b"challenge",
                    b"idempotency",
                    1,
                    60
                )
                .await
                .expect("native"),
            NativeRegistrationOutcome::Registered(_)
        ));
        assert_eq!(
            repository.role_for(native.as_str()).await.expect("role"),
            crate::BondAccessRole::User
        );
        let ordinary: PubDress = "0x1ordinary".parse().expect("ordinary");
        assert!(matches!(
            repository
                .register(&ordinary, &ProviderIdentity::telegram(8), 1)
                .await
                .expect("ordinary"),
            RegistrationOutcome::Registered(_)
        ));
        assert!(matches!(
            repository
                .rename_pub_dress(&ordinary, &"0x0elevated".parse().expect("target"))
                .await
                .expect("rename"),
            PubDressRenameOutcome::Unavailable
        ));
    }

    #[tokio::test]
    async fn fourth_avatar_upgrade_preserves_existing_identities_and_provider_bindings() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("legacy-avatars.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let options = sqlx::sqlite::SqliteConnectOptions::from_str(&database_url)
            .expect("database URL")
            .create_if_missing(true)
            .foreign_keys(true);
        let legacy = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("legacy connection");
        for migration in [
            include_str!("../migrations/0001_identities.sql"),
            include_str!("../migrations/0002_provider_accounts.sql"),
            include_str!("../migrations/0003_native_auth.sql"),
            include_str!("../migrations/0004_owned_avaia_identity.sql"),
            include_str!("../migrations/0005_avatar_model.sql"),
            include_str!("../migrations/0006_pub_dress_label.sql"),
        ] {
            sqlx::raw_sql(migration)
                .execute(&legacy)
                .await
                .expect("historical migration");
        }
        sqlx::raw_sql(
            "INSERT INTO identities (pub_dress, avatar_model) VALUES ('0x1Sky', 'dasha-study');
             INSERT INTO identities (pub_dress, identity_kind, owner_pub_dress)
                 VALUES ('1Skai', 'avaia', '0x1Sky');
             INSERT INTO identity_providers (provider, provider_subject, pub_dress)
                 VALUES ('telegram', '12345', '0x1Sky');",
        )
        .execute(&legacy)
        .await
        .expect("legacy data");
        legacy.close().await;

        let address = "0x1Sky";
        let repository = IdentityRepository::connect(&database_url)
            .await
            .expect("upgrade");
        assert_eq!(
            repository.avatar_model(address).await.expect("old choice"),
            Some("dasha-study".to_owned())
        );
        repository
            .set_avatar_model(address, "dasha-v2-study")
            .await
            .expect("fourth choice accepted");
        assert!(
            repository
                .set_avatar_model(address, "not-published")
                .await
                .is_err()
        );
        repository.pool.close().await;

        let reopened = IdentityRepository::connect(&database_url)
            .await
            .expect("idempotent reopen");
        assert_eq!(
            reopened.avatar_model(address).await.expect("new choice"),
            Some("dasha-v2-study".to_owned())
        );
        let owner: String =
            sqlx::query_scalar("SELECT owner_pub_dress FROM identities WHERE pub_dress = 'x1Skai'")
                .fetch_one(&reopened.pool)
                .await
                .expect("owned identity survives");
        assert_eq!(owner, "0x1Sky");
        let provider: String = sqlx::query_scalar(
            "SELECT pub_dress FROM identity_providers WHERE provider_subject = '12345'",
        )
        .fetch_one(&reopened.pool)
        .await
        .expect("provider survives");
        assert_eq!(provider, "0x1Sky");
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&reopened.pool)
                .await
                .expect("foreign key check")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn owned_avaia_migration_is_idempotent_on_reopen() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());

        let repository = IdentityRepository::connect(&database_url)
            .await
            .expect("first initialization");
        drop(repository);

        IdentityRepository::connect(&database_url)
            .await
            .expect("second initialization must not replay additive migration");
    }

    #[tokio::test]
    async fn provider_registration_creates_only_the_human_bond() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let address = PubDress::from_str("0xda-sha.").expect("valid pub_dress");

        let outcome = repository
            .register(&address, &ProviderIdentity::telegram(10), 100)
            .await
            .expect("registration");
        assert!(matches!(
            outcome,
            RegistrationOutcome::Registered(record)
                if record.pub_dress == "0xda-sha." && record.avaia_pub_dress.is_none()
        ));
    }

    #[tokio::test]
    async fn public_label_is_stored_atomically_and_resolves_to_the_allocated_bond() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let address = PubDress::from_str("0x1небо").expect("valid pub_dress");
        let registered = match repository
            .register(&address, &ProviderIdentity::telegram(10), 100)
            .await
            .expect("registration")
        {
            RegistrationOutcome::Registered(record) => record,
            other => panic!("unexpected registration outcome: {other:?}"),
        };

        let label = PubDressLabel::stem(&address).expect("label");
        assert_eq!(label.as_str(), "xn--0x1-dddt1cj");
        assert_eq!(registered.pub_dress_label.as_deref(), Some(label.as_str()));
        assert_eq!(registered.pub_dress_label_suffix, "");
        assert_eq!(
            registered.readable_url("nilx.one").as_deref(),
            Some("https://0x1небо.nilx.one")
        );
        let record = repository
            .find_by_pub_dress_label(
                &PubDressLabel::from_str(label.as_str()).expect("stored label"),
            )
            .await
            .expect("lookup")
            .expect("allocated Bond");
        assert_eq!(record.identity.pub_dress, "0x1небо");
        assert_eq!(record.readable_url("nilx.one"), "https://0x1небо.nilx.one");
    }

    #[tokio::test]
    async fn a_dns_fold_collision_allocates_the_first_decimal_suffix() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let lower = PubDress::from_str("0x1небо").expect("lower");
        let title = PubDress::from_str("0x1Небо").expect("title");
        let first = match repository
            .register(&lower, &ProviderIdentity::telegram(10), 100)
            .await
            .expect("first registration")
        {
            RegistrationOutcome::Registered(record) => record,
            other => panic!("unexpected first outcome: {other:?}"),
        };
        let second = match repository
            .register(&title, &ProviderIdentity::discord("20"), 101)
            .await
            .expect("second registration")
        {
            RegistrationOutcome::Registered(record) => record,
            other => panic!("unexpected second outcome: {other:?}"),
        };
        assert_eq!(first.pub_dress_label_suffix, "");
        assert_eq!(second.pub_dress_label_suffix, "2");
        assert_ne!(first.pub_dress_label, second.pub_dress_label);
        assert_eq!(
            second.readable_url("nilx.one").as_deref(),
            Some("https://0x1Небо2.nilx.one")
        );
    }

    #[tokio::test]
    async fn concurrent_dns_fold_claims_allocate_distinct_labels() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("identity.sqlite");
        let database_url = format!("sqlite://{}", database.display());
        let repository = IdentityRepository::connect(&database_url)
            .await
            .expect("repository must initialize");
        let first_repository = repository.clone();
        let second_repository = repository.clone();
        let lower = PubDress::from_str("0x1небо").expect("lower");
        let title = PubDress::from_str("0x1Небо").expect("title");
        let telegram = ProviderIdentity::telegram(10);
        let discord = ProviderIdentity::discord("20");

        let (first, second) = tokio::join!(
            first_repository.register(&lower, &telegram, 100),
            second_repository.register(&title, &discord, 101),
        );
        let first = match first.expect("first registration") {
            RegistrationOutcome::Registered(record) => record,
            other => panic!("unexpected first outcome: {other:?}"),
        };
        let second = match second.expect("second registration") {
            RegistrationOutcome::Registered(record) => record,
            other => panic!("unexpected second outcome: {other:?}"),
        };

        assert_ne!(first.pub_dress_label, second.pub_dress_label);
        let mut suffixes = [
            first.pub_dress_label_suffix.as_str(),
            second.pub_dress_label_suffix.as_str(),
        ];
        suffixes.sort_unstable();
        assert_eq!(suffixes, ["", "2"]);
    }

    #[tokio::test]
    async fn public_label_retries_are_bounded_and_roll_back_on_exhaustion() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let candidate = PubDress::from_str("0x1Sky").expect("candidate");
        let stem = PubDressLabel::stem(&candidate).expect("stem");
        for suffix in core::iter::once("").chain(super::PUBLIC_LABEL_RETRY_SUFFIXES) {
            let label = PubDressLabel::compose(&stem, suffix).expect("candidate label");
            sqlx::query(
                "INSERT INTO identities \
                 (pub_dress, identity_kind, pub_dress_label, pub_dress_label_suffix, created_at) \
                 VALUES (?, 'human', ?, ?, ?)",
            )
            .bind(format!(
                "0x{}fixture{}",
                suffix.len() + 1,
                if suffix.is_empty() { "aa" } else { suffix }
            ))
            .bind(label.as_str())
            .bind(suffix)
            .bind(1_i64)
            .execute(&repository.pool)
            .await
            .expect("occupy label");
        }

        assert!(matches!(
            repository
                .register(&candidate, &ProviderIdentity::telegram(42), 100)
                .await,
            Ok(RegistrationOutcome::PublicLabelUnavailable)
        ));
        assert!(
            repository
                .is_pub_dress_available(&candidate)
                .await
                .expect("failed allocation must not create the Bond")
        );
        assert!(
            repository
                .find_by_provider(&ProviderIdentity::telegram(42))
                .await
                .expect("provider lookup")
                .is_none()
        );
    }

    #[tokio::test]
    async fn provider_registration_is_not_gated_on_the_default_avaia_address() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let occupying_owner = PubDress::from_str("0x1sky").expect("valid first owner");
        repository
            .register(&occupying_owner, &ProviderIdentity::telegram(10), 100)
            .await
            .expect("occupying registration");
        let occupied = AvaiaPubDress::derive_default(&occupying_owner);
        repository
            .configure_owned_avaia(&occupying_owner, &occupied, 100)
            .await
            .expect("explicit Avaia creation");

        // Core deliberately maps both `sky` and `sk` to the same default
        // Avaia stem. Registration creates no Avaia, so that collision is no
        // longer a reason to refuse the human Bond.
        let candidate_owner = PubDress::from_str("0x1sk").expect("valid second owner");
        assert_eq!(occupied, AvaiaPubDress::derive_default(&candidate_owner));
        let registered = repository
            .register(&candidate_owner, &ProviderIdentity::discord("20"), 101)
            .await
            .expect("registration");
        assert!(matches!(
            registered,
            RegistrationOutcome::Registered(ref record) if record.avaia_pub_dress.is_none()
        ));
    }

    #[tokio::test]
    async fn repeated_provider_registration_reports_no_avaia_it_did_not_create() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let owner = PubDress::from_str("0x1sky").expect("valid owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(11), 100)
            .await
            .expect("registration");
        let again = repository
            .register(&owner, &ProviderIdentity::telegram(11), 101)
            .await
            .expect("repeated registration");
        assert!(matches!(
            again,
            RegistrationOutcome::AlreadyRegistered(ref record) if record.avaia_pub_dress.is_none()
        ));
        assert_eq!(
            repository.owned_avaia_identity(&owner).await.expect("read"),
            None
        );
    }

    #[tokio::test]
    async fn renaming_a_bond_without_an_avaia_creates_none() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let current = PubDress::from_str("0x1sky").expect("valid owner");
        let next = PubDress::from_str("0x1mira").expect("valid next");
        repository
            .register(&current, &ProviderIdentity::telegram(12), 100)
            .await
            .expect("registration");
        let renamed = repository
            .rename_pub_dress(&current, &next)
            .await
            .expect("rename");
        assert!(matches!(
            renamed,
            PubDressRenameOutcome::Renamed(ref record) if record.avaia_pub_dress.is_none()
        ));
    }

    #[tokio::test]
    async fn naming_an_avaia_that_does_not_exist_is_refused() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let owner = PubDress::from_str("0x1sky").expect("valid owner");
        repository
            .register(&owner, &ProviderIdentity::telegram(13), 100)
            .await
            .expect("registration");
        let next = AvaiaPubDress::from_str("x1newai").expect("valid Avaia");
        assert_eq!(
            repository
                .rename_owned_avaia(&owner, &next)
                .await
                .expect("rename"),
            PubDressRenameOutcome::AvaiaMissing
        );
        assert_eq!(
            repository.owned_avaia_identity(&owner).await.expect("read"),
            None
        );
    }

    #[tokio::test]
    async fn pre_amendment_human_is_resolved_without_gaining_an_avaia() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        sqlx::query("INSERT INTO identities (pub_dress) VALUES ('0xda-sha.')")
            .execute(&repository.pool)
            .await
            .expect("legacy human fixture");
        let address = PubDress::from_str("0xda-sha.").expect("valid pub_dress");

        let resolved = repository
            .resolve_human_identity(&address)
            .await
            .expect("resolution")
            .expect("human exists");
        assert_eq!(resolved.pub_dress, "0xda-sha.");
        assert_eq!(resolved.avaia_pub_dress, None);
        let avaia_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identities WHERE identity_kind = 'avaia'",
        )
        .fetch_one(&repository.pool)
        .await
        .expect("count");
        assert_eq!(avaia_count, 0);
    }

    #[tokio::test]
    async fn provider_subjects_remain_namespaced_and_existing_registration_is_idempotent() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let telegram_address = PubDress::from_str("0x1sky").expect("valid pub_dress");
        let discord_address = PubDress::from_str("0x7sky").expect("valid pub_dress");

        repository
            .register(&telegram_address, &ProviderIdentity::telegram(42), 100)
            .await
            .expect("telegram insert");
        repository
            .register(&discord_address, &ProviderIdentity::discord("42"), 100)
            .await
            .expect("discord insert");

        let telegram = repository
            .find_by_provider(&ProviderIdentity::telegram(42))
            .await
            .expect("lookup")
            .expect("identity");
        assert_eq!(telegram.avaia_pub_dress, None);
        assert_eq!(
            telegram.readable_url("nilx.one").as_deref(),
            Some("https://0x1sky.nilx.one")
        );
        assert!(matches!(
            repository
                .register(&discord_address, &ProviderIdentity::discord("42"), 101)
                .await,
            Ok(RegistrationOutcome::AlreadyRegistered(record))
                if record.pub_dress == "0x7sky" && record.avaia_pub_dress.is_none()
        ));
    }

    #[tokio::test]
    async fn availability_is_global_case_sensitive_and_without_reservation() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let lower = PubDress::from_str("0x1sky").expect("valid pub_dress");
        let title = PubDress::from_str("0x1Sky").expect("valid pub_dress");

        assert!(
            repository
                .is_pub_dress_available(&lower)
                .await
                .expect("lookup")
        );
        repository
            .register(&lower, &ProviderIdentity::telegram(42), 100)
            .await
            .expect("registration");
        assert!(
            !repository
                .is_pub_dress_available(&lower)
                .await
                .expect("lookup")
        );
        assert!(
            repository
                .is_pub_dress_available(&title)
                .await
                .expect("lookup")
        );
    }

    #[tokio::test]
    async fn password_registration_is_active_keyless_and_atomic() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let address = PubDress::from_str("0x0new").expect("address");
        let result = repository
            .register_native_password(&address, "hash", 1, b"first", 100)
            .await
            .expect("register");
        assert!(matches!(result, NativeRegistrationOutcome::Registered(_)));
        let credential = repository
            .find_native_credential(&address)
            .await
            .expect("lookup")
            .expect("credential");
        assert!(credential.active);
        assert!(credential.recovery_key_hash.is_none());
        assert_eq!(
            repository.role_for(address.as_str()).await.expect("role"),
            crate::BondAccessRole::User
        );
        let challenges: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM native_registration_challenges")
                .fetch_one(&repository.pool)
                .await
                .expect("challenges");
        assert_eq!(challenges, 0);
        assert!(matches!(
            repository
                .register_native_password(&address, "different", 1, b"second", 101)
                .await
                .expect("collision"),
            NativeRegistrationOutcome::HandleUnavailable
        ));
        assert_eq!(
            repository
                .find_native_credential(&address)
                .await
                .expect("lookup")
                .expect("credential")
                .password_hash,
            "hash"
        );
    }

    #[tokio::test]
    async fn stale_password_proof_cannot_finish_changed_pending_credentials() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository");
        let address = PubDress::from_str("0x0pending").expect("address");
        repository
            .register_native(
                &address,
                "old-hash",
                1,
                b"key",
                b"challenge",
                b"idempotency",
                100,
                200,
            )
            .await
            .expect("pending");
        let stale = repository
            .find_native_credential(&address)
            .await
            .expect("lookup")
            .expect("credential");
        sqlx::query(
            "UPDATE native_credentials SET password_hash = 'replacement-hash' WHERE pub_dress = ?",
        )
        .bind(address.as_str())
        .execute(&repository.pool)
        .await
        .expect("concurrent change");
        assert!(
            !repository
                .complete_pending_native_password(&stale, 101)
                .await
                .expect("stale completion")
        );
        assert!(
            !repository
                .find_native_credential(&address)
                .await
                .expect("lookup")
                .expect("credential")
                .active
        );
        let current = repository
            .find_native_credential(&address)
            .await
            .expect("lookup")
            .expect("credential");
        assert!(
            repository
                .complete_pending_native_password(&current, 101)
                .await
                .expect("current completion")
        );
        assert!(
            repository
                .activate_native_registration(b"challenge", 102)
                .await
                .expect("old challenge")
                .is_none()
        );
    }

    #[tokio::test]
    async fn nullable_recovery_migration_preserves_legacy_credentials_and_challenges() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let legacy = sqlx::SqlitePool::connect(&url)
            .await
            .expect("legacy database");
        sqlx::raw_sql(include_str!("../migrations/0001_identities.sql"))
            .execute(&legacy)
            .await
            .expect("identities");
        sqlx::raw_sql(include_str!("../migrations/0002_provider_accounts.sql"))
            .execute(&legacy)
            .await
            .expect("providers");
        sqlx::raw_sql(include_str!("../migrations/0003_native_auth.sql"))
            .execute(&legacy)
            .await
            .expect("credentials");
        sqlx::query("INSERT INTO identities(pub_dress, created_at) VALUES ('0x1legacy', 1)")
            .execute(&legacy)
            .await
            .expect("identity");
        sqlx::query("INSERT INTO native_credentials VALUES ('0x1legacy', 'hash', 1, ?, 0, 1, 1)")
            .bind(b"issued-key".as_slice())
            .execute(&legacy)
            .await
            .expect("credential");
        sqlx::query("INSERT INTO native_registration_challenges VALUES (?, '0x1legacy', 200, 1)")
            .bind(b"issued-challenge".as_slice())
            .execute(&legacy)
            .await
            .expect("challenge");
        legacy.close().await;
        let upgraded = IdentityRepository::connect(&url).await.expect("upgrade");
        let address = PubDress::from_str("0x1legacy").expect("address");
        let credential = upgraded
            .find_native_credential(&address)
            .await
            .expect("lookup")
            .expect("credential");
        assert_eq!(
            credential.recovery_key_hash.as_deref(),
            Some(b"issued-key".as_slice())
        );
        assert!(
            upgraded
                .activate_native_registration(b"issued-challenge", 100)
                .await
                .expect("activate")
                .is_some()
        );
        upgraded.pool.close().await;
        let reopened = IdentityRepository::connect(&url)
            .await
            .expect("idempotent reopen");
        assert!(
            reopened
                .find_native_credential(&address)
                .await
                .expect("lookup")
                .expect("credential")
                .active
        );
    }

    #[tokio::test]
    async fn native_registration_is_atomic_and_requires_recovery_acknowledgement() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let address = PubDress::from_str("0x1sky").expect("valid pub_dress");

        let outcome = repository
            .register_native(
                &address,
                "$argon2id$test",
                1,
                b"recovery",
                b"challenge",
                b"idempotency",
                100,
                200,
            )
            .await
            .expect("native registration");
        assert!(matches!(
            outcome,
            NativeRegistrationOutcome::Registered(record)
                if record.avaia_pub_dress.is_none()
                    && record.readable_url("nilx.one").as_deref() == Some("https://0x1sky.nilx.one")
        ));
        assert!(
            !repository
                .find_native_credential(&address)
                .await
                .expect("credential lookup")
                .expect("credential")
                .active
        );
        assert_eq!(
            repository
                .activate_native_registration(b"challenge", 199)
                .await
                .expect("activation")
                .expect("identity")
                .avaia_pub_dress,
            None
        );
    }

    #[tokio::test]
    async fn native_idempotency_replays_without_creating_an_avaia() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let address = PubDress::from_str("0x1sky").expect("valid pub_dress");

        repository
            .register_native(
                &address,
                "hash",
                1,
                b"recovery-1",
                b"challenge-1",
                b"idem-1",
                100,
                200,
            )
            .await
            .expect("first registration");
        assert!(matches!(
            repository
                .register_native(
                    &address,
                    "different",
                    1,
                    b"recovery-2",
                    b"challenge-2",
                    b"idem-1",
                    101,
                    201,
                )
                .await,
            Ok(NativeRegistrationOutcome::IdempotentReplay(record))
                if record.avaia_pub_dress.is_none()
                    && record.pub_dress_label.as_deref() == Some("0x1sky")
        ));
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM identities WHERE identity_kind = 'avaia' AND owner_pub_dress = '0x1sky'",
        )
        .fetch_one(&repository.pool)
        .await
        .expect("count");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn sessions_are_revocable_and_keep_the_owned_avaia_projection() {
        let repository = IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let address = PubDress::from_str("0x1sky").expect("valid pub_dress");
        repository
            .register_native(
                &address,
                "hash",
                1,
                b"recovery",
                b"challenge",
                b"idem",
                100,
                200,
            )
            .await
            .expect("registration");
        repository
            .activate_native_registration(b"challenge", 101)
            .await
            .expect("activation");
        let avaia = AvaiaPubDress::from_str("x1skai").expect("valid Avaia");
        repository
            .configure_owned_avaia(&address, &avaia, 101)
            .await
            .expect("explicit Avaia creation");
        repository
            .create_native_session(b"session", "0x1sky", 101, 200, "test", false, None)
            .await
            .expect("session");
        let session = repository
            .find_native_session(b"session", 199)
            .await
            .expect("session lookup")
            .expect("active session");
        assert_eq!(session.avaia_pub_dress.as_deref(), Some("x1skai"));
        assert_eq!(
            session.readable_url("nilx.one").as_deref(),
            Some("https://0x1sky.nilx.one")
        );
        assert_eq!(
            repository
                .find_native_session(b"session", 200)
                .await
                .expect("expired"),
            None
        );
        repository
            .revoke_native_session(b"session", 150)
            .await
            .expect("revocation");
        assert_eq!(
            repository
                .find_native_session(b"session", 151)
                .await
                .expect("revoked"),
            None
        );
    }

    #[test]
    fn session_labels_name_a_device_and_host_from_a_closed_set() {
        let iphone_safari = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
             AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
        assert_eq!(
            super::session_client_label("Browser", Some(iphone_safari)),
            "iPhone · Safari"
        );
        let android_chrome = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
        assert_eq!(
            super::session_client_label("Browser", Some(android_chrome)),
            "Android · Chrome"
        );
        let windows_edge = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0";
        assert_eq!(
            super::session_client_label("Browser", Some(windows_edge)),
            "Windows · Edge"
        );
        assert_eq!(
            super::session_client_label("Telegram", Some(iphone_safari)),
            "iPhone · Telegram"
        );
        assert_eq!(super::session_client_label("Telegram", None), "Telegram");
        assert_eq!(super::session_client_label("Browser", None), "Browser");
        assert!(!super::session_client_label("Browser", Some(iphone_safari)).contains("Mozilla"));
    }

    #[tokio::test]
    async fn re_login_on_the_presented_session_stays_active() {
        let repository = registered_repository().await;
        assert!(
            repository
                .create_native_session(b"first", "0x1sky", 101, 10_000, "Mac · Safari", true, None)
                .await
                .expect("first session")
        );
        assert!(
            repository
                .create_native_session(
                    b"second",
                    "0x1sky",
                    102,
                    10_000,
                    "Mac · Safari",
                    true,
                    Some(b"first"),
                )
                .await
                .expect("same-browser re-login")
        );
        assert_eq!(
            repository
                .find_native_session(b"first", 102)
                .await
                .expect("lookup"),
            None
        );
        let (_, active) = repository
            .find_native_session_with_activity(b"second", 102)
            .await
            .expect("lookup")
            .expect("live session");
        assert!(active);
    }

    #[tokio::test]
    async fn a_revoked_requester_cannot_deactivate_the_live_session() {
        let repository = registered_repository().await;
        assert!(
            repository
                .create_native_session(b"active", "0x1sky", 101, 10_000, "A", false, None)
                .await
                .expect("active session")
        );
        assert!(
            !repository
                .create_native_session(b"inactive", "0x1sky", 102, 10_000, "B", false, None)
                .await
                .expect("inactive session")
        );
        repository
            .create_activation_request(b"req", "0x1sky", b"inactive", "B", 102, 500)
            .await
            .expect("request");
        repository
            .revoke_native_session(b"inactive", 103)
            .await
            .expect("revoke requester");
        assert!(
            !repository
                .accept_activation_request(b"req", "0x1sky", 150, 86_400)
                .await
                .expect("accept")
        );
        let (_, still_active) = repository
            .find_native_session_with_activity(b"active", 150)
            .await
            .expect("lookup")
            .expect("the original session is still live");
        assert!(still_active);
    }

    #[tokio::test]
    async fn private_subject_stays_opaque_and_stable_across_rename_and_restart() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        let current: PubDress = "0x0sky".parse().expect("human");
        let next: PubDress = "0x0renamed".parse().expect("new name");
        assert!(matches!(
            repository
                .register_native_password(&current, "hash", 1, b"idem-subject", 100)
                .await
                .expect("register"),
            NativeRegistrationOutcome::Registered(_)
        ));

        let subject: Vec<u8> = sqlx::query_scalar(
            "SELECT subject FROM identity_subjects WHERE pub_dress = ?",
        )
        .bind(current.as_str())
        .fetch_one(&repository.pool)
        .await
        .expect("private subject");
        assert_eq!(subject.len(), 32);

        assert!(matches!(
            repository
                .rename_pub_dress(&current, &next)
                .await
                .expect("rename"),
            PubDressRenameOutcome::Renamed(_)
        ));
        let renamed_subject: Vec<u8> = sqlx::query_scalar(
            "SELECT subject FROM identity_subjects WHERE pub_dress = ?",
        )
        .bind(next.as_str())
        .fetch_one(&repository.pool)
        .await
        .expect("subject after rename");
        assert_eq!(subject, renamed_subject);
        repository.pool.close().await;

        let reopened = IdentityRepository::connect(&url).await.expect("reopen");
        let reopened_subject: Vec<u8> = sqlx::query_scalar(
            "SELECT subject FROM identity_subjects WHERE pub_dress = ?",
        )
        .bind(next.as_str())
        .fetch_one(&reopened.pool)
        .await
        .expect("subject after restart");
        assert_eq!(subject, reopened_subject);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identity_subjects")
            .fetch_one(&reopened.pool)
            .await
            .expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn private_subject_migration_backfills_existing_humans() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        let old: PubDress = "0x0legacy".parse().expect("human");
        repository
            .register(&old, &ProviderIdentity::telegram(12345), 100)
            .await
            .expect("register provider human");

        // Model a contract-14 database with no 0035 objects: an existing
        // human must be backfilled, not assigned a new public identity.
        sqlx::query("DROP TRIGGER identity_subjects_after_human_insert")
            .execute(&repository.pool)
            .await
            .expect("drop trigger");
        sqlx::query("DROP TABLE identity_subjects")
            .execute(&repository.pool)
            .await
            .expect("drop 0035 table");
        repository.pool.close().await;

        let migrated = IdentityRepository::connect(&url).await.expect("upgrade");
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM identity_subjects WHERE pub_dress = ?",
        )
        .bind(old.as_str())
        .fetch_one(&migrated.pool)
        .await
        .expect("backfilled subject");
        assert_eq!(count, 1);
        let newer: PubDress = "0x0newcomer".parse().expect("newcomer");
        migrated
            .register(&newer, &ProviderIdentity::discord("9876"), 200)
            .await
            .expect("register future human");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identity_subjects")
            .fetch_one(&migrated.pool)
            .await
            .expect("all humans");
        assert_eq!(count, 2);
    }

    async fn registered_repository() -> super::IdentityRepository {
        let repository = super::IdentityRepository::connect("sqlite::memory:")
            .await
            .expect("repository must initialize");
        let address = PubDress::from_str("0x1sky").expect("valid pub_dress");
        repository
            .register_native(
                &address,
                "hash",
                1,
                b"recovery",
                b"challenge",
                b"idem",
                100,
                200,
            )
            .await
            .expect("registration");
        repository
            .activate_native_registration(b"challenge", 101)
            .await
            .expect("activation");
        repository
    }

    #[tokio::test]
    async fn business_memberships_replay_and_follow_identity_renames() {
        let database = tempfile::NamedTempFile::new().expect("database");
        let url = format!("sqlite://{}", database.path().display());
        let repository = IdentityRepository::connect(&url).await.expect("repository");
        for address in ["0x0business", "0x0member"] {
            sqlx::query("INSERT INTO identities (pub_dress, identity_kind, created_at) VALUES (?, 'human', 1)")
                .bind(address).execute(&repository.pool).await.expect("identity");
        }
        sqlx::query("INSERT INTO business_memberships VALUES ('0x0business', '0x0member', 'owner', 1)")
            .execute(&repository.pool)
            .await
            .expect("membership");
        for (business, member, role) in [
            ("0x0missing", "0x0member", "owner"),
            ("0x0business", "0x0member", "member"),
            ("0x0member", "0x0member", "owner"),
            ("0x0member", "0x0business", "special"),
        ] {
            assert!(
                sqlx::query("INSERT INTO business_memberships VALUES (?, ?, ?, 1)")
                    .bind(business)
                    .bind(member)
                    .bind(role)
                    .execute(&repository.pool)
                    .await
                    .is_err(),
                "{business}/{member}/{role} accepted"
            );
        }
        repository.pool.close().await;
        let replayed = IdentityRepository::connect(&url).await.expect("replay");
        let member: PubDress = "0x0member".parse().expect("member");
        let renamed: PubDress = "0x0renamed".parse().expect("renamed");
        assert!(matches!(
            replayed.rename_pub_dress(&member, &renamed).await.expect("rename"),
            PubDressRenameOutcome::Renamed(_)
        ));
        let row: (String, String) =
            sqlx::query_as("SELECT member_pub_dress, role FROM business_memberships")
                .fetch_one(&replayed.pool)
                .await
                .expect("membership after rename");
        assert_eq!(row, ("0x0renamed".to_owned(), "owner".to_owned()));
        sqlx::query("DELETE FROM identities WHERE pub_dress = '0x0business'")
            .execute(&replayed.pool)
            .await
            .expect("delete business");
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM business_memberships")
            .fetch_one(&replayed.pool)
            .await
            .expect("count");
        assert_eq!(remaining, 0);
    }
}

#[cfg(test)]
mod partner_subject_boundary_tests {
    use super::*;

    #[tokio::test]
    async fn adapter_receives_only_the_existing_pairwise_digest() {
        let repository = IdentityRepository::connect("sqlite::memory:").await.unwrap();
        let owner: PubDress = "0x1test".parse().unwrap();
        repository.register(&owner, &ProviderIdentity::telegram(12345), 100).await.unwrap();
        let config = crate::NativeAuthConfig::new(
            "test-auth-secret-that-is-at-least-thirty-two-bytes",
            "test-password-pepper-that-is-at-least-thirty-two-bytes",
        ).unwrap();
        let digester = config.secret_digester();
        let raw: Vec<u8> = sqlx::query_scalar("SELECT subject FROM identity_subjects WHERE pub_dress = ?")
            .bind(owner.as_str()).fetch_one(&repository.pool).await.unwrap();
        let encoded: String = raw.iter().map(|byte| format!("{byte:02x}")).collect();
        let result = repository.market_pairwise_subject(owner.as_str(), &digester).await.unwrap();
        assert_eq!(result, digester.digest("market-pairwise-subject-v1", &encoded));
        assert_ne!(result, raw);
    }
}
