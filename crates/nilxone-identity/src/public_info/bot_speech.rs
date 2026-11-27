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
            let unsigned = |value: i64| u64::try_from(value).map_err(|_| PublicInfoError::InvalidSession);
            let speaker = row.try_get::<String, _>("speaker")?;
            let text = row.try_get::<String, _>("text")?;
            let _: crate::PubDress = speaker.parse().map_err(|_| PublicInfoError::InvalidPublicAddress)?;
            let _: nilxone_contracts::SpokenText = text.parse().map_err(|_| PublicInfoError::InvalidSession)?;
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
