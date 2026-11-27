// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Bridge the existing legacy admin outbox to the signed Bot host.
//! Leases and message IDs are shared with the old Web worker until cutover.

use nilxone_api::bot_admin::{
    AdminDeliveryAck, AdminDeliveryKind, AdminDeliveryLease, AdminInboxDelivery, AdminInboxEntry,
};
use sqlx::Row;

use super::{PublicInfoError, PublicInfoRepository};

const LEASE_SECONDS: i64 = 300;

// Resolve the actual attached Bond through the live AIS product session.
// The stored Telegram binding must agree, and the Bond must still be admin.
// Never attach by a display address supplied by the Bot.
const ELIGIBLE: &str = "SELECT c.actor_id
    FROM telegram_chat_sessions c
    JOIN product_sessions s ON s.token_hash = c.token_hash
    JOIN legacy_bond_bindings b ON b.profile_id = s.profile_id
    JOIN legacy_bond_keys k ON k.bond_id = b.bond_id
    JOIN identities i ON i.pub_dress = k.legacy_pub_dress AND i.identity_kind = 'human'
    JOIN bond_roles r ON r.pub_dress = i.pub_dress AND r.role = 'admin'
    JOIN identity_providers p ON p.pub_dress = i.pub_dress
        AND p.provider = 'telegram' AND p.provider_subject = CAST(c.actor_id AS TEXT)
    WHERE c.workload_subject = ? AND s.expires_at > ?";

impl PublicInfoRepository {
    /// Claims at most one inbox refresh per eligible private chat per pull.
    /// Existing deliveries remain pending if there is no verified AIS chat.
    /// No role decision, BondChain event or Telegram call occurs here.
    pub async fn claim_admin_inbox_deliveries(
        &self,
        workload: &str,
        limit: usize,
        now: u64,
    ) -> Result<Vec<AdminInboxDelivery>, PublicInfoError> {
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let cutoff = now.saturating_sub(LEASE_SECONDS);
        let limit = i64::try_from(limit.clamp(1, 50)).unwrap_or(1);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let query = format!(
            "WITH eligible AS ({ELIGIBLE}), candidates AS (
                SELECT q.pub_dress, e.actor_id, 'pending' AS kind, 1 AS priority,
                       q.requested_at AS event_time
                FROM eligible e CROSS JOIN admin_role_requests q
                LEFT JOIN admin_role_telegram_deliveries d
                  ON d.pub_dress = q.pub_dress AND d.telegram_user_id = e.actor_id
                WHERE q.status = 'pending' AND d.delivered_at IS NULL
                  AND (d.last_attempt_at IS NULL OR d.last_attempt_at <= ?)
                UNION ALL
                SELECT q.pub_dress, e.actor_id, 'approved', 0, q.decided_at
                FROM eligible e
                JOIN admin_role_telegram_confirmations d ON d.telegram_user_id = e.actor_id
                JOIN admin_role_requests q ON q.pub_dress = d.pub_dress
                JOIN bond_roles r ON r.pub_dress = q.pub_dress AND r.role = 'admin'
                WHERE q.status = 'approved' AND d.delivered_at IS NULL
                  AND (d.last_attempt_at IS NULL OR d.last_attempt_at <= ?)
            ), ranked AS (
                SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY actor_id ORDER BY priority, event_time, pub_dress
                ) AS rank FROM candidates
            )
            SELECT pub_dress, actor_id, kind FROM ranked WHERE rank = 1
            ORDER BY priority, event_time, actor_id LIMIT ?"
        );
        let rows = sqlx::query(&query)
            .bind(workload)
            .bind(now)
            .bind(cutoff)
            .bind(cutoff)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
        let mut deliveries = Vec::with_capacity(rows.len());
        for row in rows {
            let candidate: String = row.try_get("pub_dress")?;
            let actor: i64 = row.try_get("actor_id")?;
            let kind: String = row.try_get("kind")?;
            let (kind, changed) = if kind == "pending" {
                let changed = sqlx::query(
                    "INSERT INTO admin_role_telegram_deliveries
                     (pub_dress, telegram_user_id, last_attempt_at, attempts)
                     VALUES (?, ?, ?, 1)
                     ON CONFLICT(pub_dress, telegram_user_id) DO UPDATE SET
                       last_attempt_at = excluded.last_attempt_at, attempts = attempts + 1
                     WHERE delivered_at IS NULL AND last_attempt_at <= ?",
                )
                .bind(&candidate)
                .bind(actor)
                .bind(now)
                .bind(cutoff)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                (AdminDeliveryKind::Pending, changed)
            } else {
                let changed = sqlx::query(
                    "UPDATE admin_role_telegram_confirmations SET
                       attempts = attempts + 1, last_attempt_at = ?
                     WHERE pub_dress = ? AND telegram_user_id = ?
                       AND delivered_at IS NULL
                       AND (last_attempt_at IS NULL OR last_attempt_at <= ?)",
                )
                .bind(now)
                .bind(&candidate)
                .bind(actor)
                .bind(cutoff)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                (AdminDeliveryKind::Approved, changed)
            };
            if changed != 1 {
                continue;
            }
            let admin_message_id = sqlx::query_scalar::<_, Option<i32>>(
                "SELECT admin_message_id FROM telegram_message_slots WHERE chat_id = ?",
            )
            .bind(actor)
            .fetch_optional(&mut *tx)
            .await?
            .flatten();
            let inbox = sqlx::query(
                "SELECT request_token, pub_dress FROM admin_role_requests
                 WHERE status = 'pending' ORDER BY requested_at, pub_dress LIMIT 10",
            )
            .fetch_all(&mut *tx)
            .await?;
            let pending = inbox
                .into_iter()
                .map(|row| {
                    Ok(AdminInboxEntry {
                        request_token: row.try_get("request_token")?,
                        pub_dress: row.try_get("pub_dress")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;
            deliveries.push(AdminInboxDelivery {
                lease: AdminDeliveryLease {
                    kind,
                    candidate_pub_dress: candidate,
                    provider_actor_id: u64::try_from(actor)
                        .map_err(|_| PublicInfoError::InvalidSession)?,
                    claimed_at: u64::try_from(now)
                        .map_err(|_| PublicInfoError::InvalidSession)?,
                },
                admin_message_id,
                pending,
            });
        }
        tx.commit().await?;
        Ok(deliveries)
    }

    /// Persist the exact claimed delivery and its Telegram message slot
    /// atomically. Stale/replayed ACKs cannot overwrite a newer message ID.
    pub async fn acknowledge_admin_inbox_delivery(
        &self,
        workload: &str,
        ack: &AdminDeliveryAck,
        now: u64,
    ) -> Result<bool, PublicInfoError> {
        if !ack.is_valid() || ack.lease.claimed_at > now {
            return Ok(false);
        }
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let actor = i64::try_from(ack.lease.provider_actor_id)
            .map_err(|_| PublicInfoError::InvalidSession)?;
        let claim = i64::try_from(ack.lease.claimed_at)
            .map_err(|_| PublicInfoError::InvalidSession)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let query = format!("SELECT EXISTS(SELECT 1 FROM ({ELIGIBLE}) WHERE actor_id = ?)");
        let eligible: bool = sqlx::query_scalar(&query)
            .bind(workload)
            .bind(now)
            .bind(actor)
            .fetch_one(&mut *tx)
            .await?;
        if !eligible {
            return Ok(false);
        }
        let table = match ack.lease.kind {
            AdminDeliveryKind::Pending => "admin_role_telegram_deliveries",
            AdminDeliveryKind::Approved => "admin_role_telegram_confirmations",
        };
        let query = format!(
            "UPDATE {table} SET delivered_at = ?
             WHERE pub_dress = ? AND telegram_user_id = ?
               AND last_attempt_at = ? AND delivered_at IS NULL"
        );
        let changed = sqlx::query(&query)
            .bind(now)
            .bind(&ack.lease.candidate_pub_dress)
            .bind(actor)
            .bind(claim)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed != 1 {
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO telegram_message_slots (chat_id, admin_message_id) VALUES (?, ?)
             ON CONFLICT(chat_id) DO UPDATE SET admin_message_id = excluded.admin_message_id",
        )
        .bind(actor)
        .bind(ack.message_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BondAccessRole, IdentityRepository, ProviderIdentity, TelegramMessageSlot,
        aiaiaiai_sign_in::VerifiedSignIn, public_info::LegacyBondMigrationRepository,
    };

    #[tokio::test]
    async fn only_a_live_attached_admin_chat_claims_and_acks_the_existing_slot() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("admin.db").display());
        let legacy = IdentityRepository::connect(&url).await.unwrap();
        let repository = PublicInfoRepository::connect(&url).await.unwrap();
        LegacyBondMigrationRepository::prepare(repository.clone())
            .await
            .unwrap();
        for (address, actor) in [("0x1admin", 42), ("0x0new", 43)] {
            legacy
                .register(&address.parse().unwrap(), &ProviderIdentity::telegram(actor), 1)
                .await
                .unwrap();
        }
        legacy
            .set_role(&"0x1admin".parse().unwrap(), BondAccessRole::Admin)
            .await
            .unwrap();
        legacy.request_admin_review_on_login("0x0new", 10).await.unwrap();
        legacy
            .save_telegram_message_id(42, TelegramMessageSlot::Admin, Some(7))
            .await
            .unwrap();
        let now = 1_800_000_000;
        let owner = repository
            .attach(&VerifiedSignIn {
                issuer: "https://auth.aiaiaiai.org".into(),
                subject: "admin-subject".into(),
                pub_dress: None,
                auth_time: now,
            })
            .await
            .unwrap();
        let token = "a".repeat(64);
        repository
            .open_session(&owner, &token, "Telegram · Chat", None, now, now + 1000)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO telegram_chat_sessions (workload_subject, actor_id, token_hash)
             VALUES ('bot', 42, ?)",
        )
        .bind(&token)
        .execute(&repository.pool)
        .await
        .unwrap();
        assert!(
            repository.claim_admin_inbox_deliveries("bot", 10, now).await.unwrap().is_empty(),
            "a provider chat alone is not an attached admin Bond"
        );
        let bond_id: String = sqlx::query_scalar(
            "INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES ('0x1admin')
             RETURNING bond_id",
        )
        .fetch_one(&repository.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO legacy_bond_bindings VALUES (?, ?, ?)")
            .bind(bond_id)
            .bind(&owner.public_profile().profile_id)
            .bind(i64::try_from(now).unwrap())
            .execute(&repository.pool)
            .await
            .unwrap();
        assert!(
            repository.claim_admin_inbox_deliveries("other", 10, now).await.unwrap().is_empty()
        );
        let first = repository
            .claim_admin_inbox_deliveries("bot", 10, now)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(first.admin_message_id, Some(7));
        assert_eq!(first.pending.len(), 1);
        assert!(
            repository.claim_admin_inbox_deliveries("bot", 10, now + 1).await.unwrap().is_empty()
        );
        let second = repository
            .claim_admin_inbox_deliveries("bot", 10, now + 301)
            .await
            .unwrap()
            .pop()
            .unwrap();
        let mut ack = AdminDeliveryAck {
            version: 1,
            lease: first.lease,
            message_id: 9,
        };
        assert!(!repository.acknowledge_admin_inbox_delivery("bot", &ack, now + 302).await.unwrap());
        ack.lease = second.lease;
        assert!(!repository.acknowledge_admin_inbox_delivery("other", &ack, now + 302).await.unwrap());
        assert!(repository.acknowledge_admin_inbox_delivery("bot", &ack, now + 302).await.unwrap());
        ack.message_id = 99;
        assert!(!repository.acknowledge_admin_inbox_delivery("bot", &ack, now + 303).await.unwrap());
        assert_eq!(
            legacy.telegram_message_id(42, TelegramMessageSlot::Admin).await.unwrap(),
            Some(9)
        );
    }
}
