// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Core-owned delivery leases for the speech producer. No Bot API access.

use nilxone_api::bot_speech::{LEASE_SECONDS, MAX_BATCH, SpeechAck, SpeechDelivery};
use sqlx::Row;

use super::{PublicInfoError, PublicInfoRepository};

const AUTHORIZED: &str = "SELECT d.id, d.actor_id, d.claimed_at, d.expires_at, l.speaker, l.text
    FROM speech_telegram_delivery d
    JOIN spoken_lines l ON l.line_id = d.line_id
    JOIN telegram_chat_sessions c ON c.token_hash = d.token_hash
       AND c.workload_subject = d.workload_subject AND c.actor_id = d.actor_id
    JOIN product_sessions s ON s.token_hash = c.token_hash
    JOIN legacy_bond_bindings b ON b.profile_id = s.profile_id AND b.bond_id = d.bond_id
    JOIN legacy_bond_keys k ON k.bond_id = b.bond_id
    JOIN identities i ON i.pub_dress = k.legacy_pub_dress AND i.identity_kind = 'human'
    JOIN identity_providers p ON p.pub_dress = i.pub_dress AND p.provider = 'telegram'
       AND p.provider_subject = CAST(c.actor_id AS TEXT)
    WHERE d.workload_subject = ? AND d.expires_at > ? AND s.expires_at > ?";

impl PublicInfoRepository {
    /// A mixed-version producer may not have initialized its outbox yet.
    async fn speech_delivery_available(&self) -> Result<bool, PublicInfoError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'speech_telegram_delivery' AND type = 'table')",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn claim_speech_deliveries(
        &self,
        workload: &str,
        limit: usize,
        now: u64,
    ) -> Result<Vec<SpeechDelivery>, PublicInfoError> {
        if !self.speech_delivery_available().await? {
            return Ok(Vec::new());
        }
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let cutoff = now.saturating_sub(i64::try_from(LEASE_SECONDS).unwrap_or(60));
        let limit = i64::try_from(limit.clamp(1, MAX_BATCH)).unwrap_or(1);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM speech_telegram_delivery WHERE expires_at <= ?")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        let query = format!(
            "SELECT * FROM ({AUTHORIZED}) WHERE claimed_at IS NULL OR claimed_at <= ? ORDER BY id LIMIT ?"
        );
        let rows = sqlx::query(&query)
            .bind(workload)
            .bind(now)
            .bind(now)
            .bind(cutoff)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
        let mut deliveries = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.try_get("id")?;
            sqlx::query("UPDATE speech_telegram_delivery SET claimed_at = ? WHERE id = ?")
                .bind(now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            let unsigned =
                |value: i64| u64::try_from(value).map_err(|_| PublicInfoError::InvalidSession);
            let speaker = row.try_get::<String, _>("speaker")?;
            let text = row.try_get::<String, _>("text")?;
            let _: crate::PubDress = speaker
                .parse()
                .map_err(|_| PublicInfoError::InvalidPublicAddress)?;
            let _: nilxone_contracts::SpokenText =
                text.parse().map_err(|_| PublicInfoError::InvalidSession)?;
            deliveries.push(SpeechDelivery {
                id: unsigned(id)?,
                provider_actor_id: unsigned(row.try_get("actor_id")?)?,
                speaker,
                text,
                expires_at: unsigned(row.try_get("expires_at")?)?,
                claimed_at: unsigned(now)?,
            });
        }
        tx.commit().await?;
        Ok(deliveries)
    }

    /// The acknowledgement is scoped to the exact live recipient and lease.
    /// It is only transport evidence, never a completed Interaction.
    pub async fn acknowledge_speech_delivery(
        &self,
        workload: &str,
        ack: &SpeechAck,
        now: u64,
    ) -> Result<bool, PublicInfoError> {
        if !ack.is_valid()
            || ack.claimed_at > now
            || now.saturating_sub(ack.claimed_at) >= LEASE_SECONDS
            || !self.speech_delivery_available().await?
        {
            return Ok(false);
        }
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let id = i64::try_from(ack.id).map_err(|_| PublicInfoError::InvalidSession)?;
        let claimed = i64::try_from(ack.claimed_at).map_err(|_| PublicInfoError::InvalidSession)?;
        let query = format!(
            "DELETE FROM speech_telegram_delivery WHERE id IN
              (SELECT id FROM ({AUTHORIZED}) WHERE id = ? AND claimed_at = ?)"
        );
        let result = sqlx::query(&query)
            .bind(workload)
            .bind(now)
            .bind(now)
            .bind(id)
            .bind(claimed)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IdentityRepository, ProviderIdentity, aiaiaiai_sign_in::VerifiedSignIn,
        public_info::LegacyBondMigrationRepository, speech::SpeechRepository,
    };
    use nilxone_api::bot_speech::TTL_SECONDS;
    use nilxone_contracts::{DecimalU64, SpokenLine};

    const NOW: u64 = 1_800_000_000;
    const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    async fn setup() -> (tempfile::TempDir, PublicInfoRepository, SpeechRepository) {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("speech.db").display());
        let legacy = IdentityRepository::connect(&url).await.unwrap();
        let repository = PublicInfoRepository::connect(&url).await.unwrap();
        LegacyBondMigrationRepository::prepare(repository.clone())
            .await
            .unwrap();
        for (address, actor) in [("0x1speaker", 41), ("0x1listener", 42), ("0x0guest", 43)] {
            legacy
                .register(
                    &address.parse().unwrap(),
                    &ProviderIdentity::telegram(actor),
                    1,
                )
                .await
                .unwrap();
        }
        let owner = repository
            .attach(&VerifiedSignIn {
                issuer: "https://auth.aiaiaiai.org".into(),
                subject: "listener-subject".into(),
                pub_dress: None,
                auth_time: NOW,
            })
            .await
            .unwrap();
        repository
            .open_session(&owner, TOKEN, "Telegram · Chat", None, NOW, NOW + 1_000)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO telegram_chat_sessions (workload_subject, actor_id, token_hash)
             VALUES ('bot', 42, ?)",
        )
        .bind(TOKEN)
        .execute(&repository.pool)
        .await
        .unwrap();
        let bond_id: String = sqlx::query_scalar(
            "INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES ('0x1listener')
             RETURNING bond_id",
        )
        .fetch_one(&repository.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO legacy_bond_bindings VALUES (?, ?, ?)")
            .bind(bond_id)
            .bind(&owner.public_profile().profile_id)
            .bind(i64::try_from(NOW).unwrap())
            .execute(&repository.pool)
            .await
            .unwrap();
        let speech = SpeechRepository::connect(&url).await.unwrap();
        (dir, repository, speech)
    }

    fn line(suffix: &str, spoken_at: u64) -> SpokenLine {
        SpokenLine::new(
            format!("line_{}", suffix.repeat(64)).try_into().unwrap(),
            "0x1speaker".parse().unwrap(),
            "Привіт! 🐈".parse().unwrap(),
            DecimalU64::new(spoken_at),
        )
    }

    #[tokio::test]
    async fn a_spoken_line_has_one_recipient_lease_and_an_exact_receipt() {
        let (_dir, repository, speech) = setup().await;
        let line = line("a", NOW);
        assert_eq!(
            speech
                .record_with_telegram_recipients(&line, NOW, &[41, 42, 42, 43])
                .await
                .unwrap(),
            (true, 1)
        );
        assert_eq!(
            speech
                .record_with_telegram_recipients(&line, NOW + 1, &[42])
                .await
                .unwrap(),
            (false, 0)
        );
        assert!(
            repository
                .claim_speech_deliveries("other", 10, NOW)
                .await
                .unwrap()
                .is_empty()
        );
        let first = repository
            .claim_speech_deliveries("bot", 10, NOW)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(first.provider_actor_id, 42);
        assert_eq!(first.speaker, "0x1speaker");
        assert_eq!(first.text, "Привіт! 🐈");
        assert_eq!(first.expires_at, NOW + TTL_SECONDS);
        assert!(
            repository
                .claim_speech_deliveries("bot", 10, NOW + 1)
                .await
                .unwrap()
                .is_empty()
        );
        let second = repository
            .claim_speech_deliveries("bot", 10, NOW + LEASE_SECONDS)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(second.id, first.id);
        let mut ack = SpeechAck {
            version: 1,
            id: first.id,
            claimed_at: first.claimed_at,
        };
        assert!(
            !repository
                .acknowledge_speech_delivery("bot", &ack, NOW + LEASE_SECONDS)
                .await
                .unwrap()
        );
        ack.claimed_at = second.claimed_at;
        assert!(
            !repository
                .acknowledge_speech_delivery("other", &ack, NOW + LEASE_SECONDS)
                .await
                .unwrap()
        );
        assert!(
            repository
                .acknowledge_speech_delivery("bot", &ack, NOW + LEASE_SECONDS)
                .await
                .unwrap()
        );
        assert!(
            !repository
                .acknowledge_speech_delivery("bot", &ack, NOW + LEASE_SECONDS)
                .await
                .unwrap()
        );
        assert_eq!(speech.recent(NOW).await.unwrap(), vec![line]);
    }

    #[tokio::test]
    async fn a_revoked_or_expired_recipient_cannot_receive_a_pending_line() {
        let (_dir, repository, speech) = setup().await;
        for suffix in ["b", "c"] {
            assert_eq!(
                speech
                    .record_with_telegram_recipients(&line(suffix, NOW), NOW, &[42])
                    .await
                    .unwrap(),
                (true, 1)
            );
        }
        let delivery = repository
            .claim_speech_deliveries("bot", 1, NOW)
            .await
            .unwrap()
            .pop()
            .unwrap();
        let ack = SpeechAck {
            version: 1,
            id: delivery.id,
            claimed_at: delivery.claimed_at,
        };
        sqlx::query("DELETE FROM telegram_chat_sessions WHERE token_hash = ?")
            .bind(TOKEN)
            .execute(&repository.pool)
            .await
            .unwrap();
        assert!(
            !repository
                .acknowledge_speech_delivery("bot", &ack, NOW + 1)
                .await
                .unwrap()
        );
        assert!(
            repository
                .claim_speech_deliveries("bot", 10, NOW + 1)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            repository
                .claim_speech_deliveries("bot", 10, NOW + TTL_SECONDS)
                .await
                .unwrap()
                .is_empty()
        );
        let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM speech_telegram_delivery")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert_eq!(queued, 0);
    }
}
