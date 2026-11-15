// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Disabled-by-default atomic commit for confirmed legacy Bond migrations.
//! This code does NOT verify browser OIDC callbacks or confirm intents; the
//! host must do both with migration-bound proof before calling it.

use sha2::{Digest as _, Sha256};
use sqlx::Row as _;

use super::{LegacyBondMigrationRepository, LegacyMigrationError};

const MAX_FRESH_SECONDS: u64 = 600;
const AIS_ISSUER: &str = "https://auth.aiaiaiai.org";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyMigrationAttached {
    pub bond_id: String,
    pub profile_id: String,
}

impl LegacyBondMigrationRepository {
    /// Attach exactly one verified AIS profile to exactly one existing Bond.
    /// A confirmed intent is not created here: that remains the responsibility
    /// of a separate, CSRF-protected, OIDC-verified migration flow.
    /// No profile is merged or selected by a public address.
    pub async fn commit_confirmed(
        &self,
        legacy_session_hash: &[u8],
        transaction_hash: &[u8],
        now: u64,
    ) -> Result<LegacyMigrationAttached, LegacyMigrationError> {
        if legacy_session_hash.len() != 32 {
            return Err(LegacyMigrationError::InvalidLegacyProof);
        }
        if transaction_hash.len() != 32 {
            return Err(LegacyMigrationError::InvalidTransaction);
        }
        let now_i64 = i64::try_from(now).map_err(|_| LegacyMigrationError::InvalidTransaction)?;
        let earliest = i64::try_from(now.saturating_sub(MAX_FRESH_SECONDS))
            .map_err(|_| LegacyMigrationError::InvalidTransaction)?;
        let mut tx = self.repository.pool.begin_with("BEGIN IMMEDIATE").await?;
        let intent = sqlx::query(
            "SELECT status, started_at, expires_at, confirmed_at, verified_issuer, \
                    verified_subject, verified_auth_time \
             FROM legacy_bond_migration_intents \
             WHERE transaction_hash = ? AND legacy_session_hash = ?",
        )
        .bind(transaction_hash)
        .bind(legacy_session_hash)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(LegacyMigrationError::InvalidTransaction)?;
        let status: String = intent.try_get("status")?;
        if status == "committed" {
            return Err(LegacyMigrationError::AlreadyBound);
        }
        if status != "confirmed" {
            return Err(LegacyMigrationError::NotConfirmed);
        }
        let started_at: i64 = intent.try_get("started_at")?;
        let expires_at: i64 = intent.try_get("expires_at")?;
        let confirmed_at: Option<i64> = intent.try_get("confirmed_at")?;
        if started_at > now_i64
            || now_i64 >= expires_at
            || confirmed_at.is_none_or(|t| t < started_at || t > now_i64)
        {
            return Err(LegacyMigrationError::ExpiredConfirmation);
        }
        let issuer: Option<String> = intent.try_get("verified_issuer")?;
        let subject: Option<String> = intent.try_get("verified_subject")?;
        let auth_time: Option<i64> = intent.try_get("verified_auth_time")?;
        let (Some(issuer), Some(subject), Some(auth_time)) = (issuer, subject, auth_time) else {
            return Err(LegacyMigrationError::InvalidAisProof);
        };
        if issuer != AIS_ISSUER
            || subject.is_empty()
            || subject.len() > 255
            || auth_time > now_i64
            || auth_time < earliest
        {
            return Err(LegacyMigrationError::InvalidAisProof);
        }
        let proof = sqlx::query(
            "SELECT s.pub_dress FROM native_sessions s \
             JOIN identities i ON i.pub_dress = s.pub_dress \
             WHERE s.token_hash = ? AND s.active = 1 \
               AND s.credential_authenticated = 1 AND s.revoked_at IS NULL \
               AND s.created_at BETWEEN ? AND ? AND s.expires_at > ? \
               AND i.identity_kind = 'human'",
        )
        .bind(legacy_session_hash)
        .bind(earliest)
        .bind(now_i64)
        .bind(now_i64)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(LegacyMigrationError::InvalidLegacyProof)?;
        let address: String = proof.try_get("pub_dress")?;
        let already_bound: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM legacy_bond_keys k \
             JOIN legacy_bond_bindings b ON b.bond_id = k.bond_id \
             WHERE k.legacy_pub_dress = ?)",
        )
        .bind(&address)
        .fetch_one(&mut *tx)
        .await?;
        if already_bound != 0 {
            return Err(LegacyMigrationError::AlreadyBound);
        }
        let occupied: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public_profiles WHERE issuer = ? AND subject = ?)",
        )
        .bind(&issuer)
        .bind(&subject)
        .fetch_one(&mut *tx)
        .await?;
        if occupied != 0 {
            return Err(LegacyMigrationError::IdentityConflict);
        }

        sqlx::query("INSERT OR IGNORE INTO legacy_bond_keys (legacy_pub_dress) VALUES (?)")
            .bind(&address)
            .execute(&mut *tx)
            .await?;
        let bond_id: String = sqlx::query_scalar(
            "SELECT bond_id FROM legacy_bond_keys WHERE legacy_pub_dress = ?",
        )
        .bind(&address)
        .fetch_one(&mut *tx)
        .await?;
        let profile = sqlx::query(
            "INSERT INTO public_profiles (issuer, subject, pub_dress, last_auth_time) \
             VALUES (?, ?, NULL, ?) RETURNING profile_id",
        )
        .bind(&issuer)
        .bind(&subject)
        .bind(auth_time)
        .fetch_one(&mut *tx)
        .await?;
        let profile_id: String = profile.try_get("profile_id")?;
        sqlx::query(
            "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) \
             VALUES (?, ?, ?)",
        )
        .bind(&bond_id)
        .bind(&profile_id)
        .bind(now_i64)
        .execute(&mut *tx)
        .await?;
        // Legacy sessions cannot continue writing once AIS takes ownership.
        sqlx::query(
            "UPDATE native_sessions SET active = 0, revoked_at = ? \
             WHERE pub_dress = ? AND revoked_at IS NULL",
        )
        .bind(now_i64)
        .bind(&address)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE session_activation_requests SET status = 'expired', resolved_at = ? \
             WHERE pub_dress = ? AND status IN ('pending', 'objectable')",
        )
        .bind(now_i64)
        .bind(&address)
        .execute(&mut *tx)
        .await?;
        let updated = sqlx::query(
            "UPDATE legacy_bond_migration_intents \
             SET status = 'committed', profile_id = ?, completed_at = ? \
             WHERE transaction_hash = ? AND status = 'confirmed' AND expires_at > ?",
        )
        .bind(&profile_id)
        .bind(now_i64)
        .bind(transaction_hash)
        .bind(now_i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(LegacyMigrationError::ExpiredConfirmation);
        }
        let mut hash = Sha256::new();
        hash.update(b"0x1-legacy-ais-binding-v1\0");
        hash.update(issuer.as_bytes());
        hash.update([0]);
        hash.update(subject.as_bytes());
        sqlx::query(
            "INSERT INTO legacy_bond_migration_audit \
             (transaction_hash, bond_id, ais_reference_hash, outcome, occurred_at) \
             VALUES (?, ?, ?, 'committed', ?)",
        )
        .bind(transaction_hash)
        .bind(&bond_id)
        .bind(hash.finalize().to_vec())
        .bind(now_i64)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(LegacyMigrationAttached { bond_id, profile_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{public_info::PublicInfoRepository, repository::IdentityRepository};

    const NOW: u64 = 1_800_000_000;

    async fn fixture() -> (LegacyBondMigrationRepository, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("legacy.sqlite").display());
        let _legacy = IdentityRepository::connect(&url).await.unwrap();
        let product = PublicInfoRepository::connect(&url).await.unwrap();
        (LegacyBondMigrationRepository::prepare(product).await.unwrap(), dir)
    }

    async fn confirmed(
        store: &LegacyBondMigrationRepository,
        address: &str,
        session: u8,
        intent: u8,
        subject: &str,
    ) {
        sqlx::query("INSERT INTO identities (pub_dress, identity_kind) VALUES (?, 'human')")
            .bind(address).execute(&store.repository.pool).await.unwrap();
        sqlx::query(
            "INSERT INTO native_sessions \
             (token_hash, pub_dress, created_at, expires_at, active, credential_authenticated) \
             VALUES (?, ?, ?, ?, 1, 1)",
        )
        .bind(vec![session; 32])
        .bind(address)
        .bind(NOW as i64 - 30)
        .bind(NOW as i64 + 900)
        .execute(&store.repository.pool).await.unwrap();
        store.begin(&[session; 32], &[intent; 32], NOW).await.unwrap();
        sqlx::query(
            "UPDATE legacy_bond_migration_intents \
             SET status = 'confirmed', verified_issuer = ?, verified_subject = ?, \
                 verified_auth_time = ?, confirmed_at = ? WHERE transaction_hash = ?",
        )
        .bind(AIS_ISSUER).bind(subject).bind(NOW as i64)
        .bind(NOW as i64 + 1).bind(vec![intent; 32])
        .execute(&store.repository.pool).await.unwrap();
    }

    #[tokio::test]
    async fn confirmed_attachment_is_single_transaction_and_revokes_old_session() {
        let (store, _dir) = fixture().await;
        confirmed(&store, "0x1oldbond", 1, 2, "ais-sub-one").await;
        let result = store.commit_confirmed(&[1; 32], &[2; 32], NOW + 2).await.unwrap();
        let legacy: String = sqlx::query_scalar(
            "SELECT k.legacy_pub_dress FROM legacy_bond_keys k \
             JOIN legacy_bond_bindings b ON b.bond_id = k.bond_id \
             WHERE b.profile_id = ?",
        )
        .bind(&result.profile_id).fetch_one(&store.repository.pool).await.unwrap();
        assert_eq!(legacy, "0x1oldbond");
        let active: i64 = sqlx::query_scalar(
            "SELECT active FROM native_sessions WHERE token_hash = ?",
        )
        .bind(vec![1_u8; 32]).fetch_one(&store.repository.pool).await.unwrap();
        assert_eq!(active, 0);
        let status: String = sqlx::query_scalar(
            "SELECT status FROM legacy_bond_migration_intents WHERE transaction_hash = ?",
        )
        .bind(vec![2_u8; 32]).fetch_one(&store.repository.pool).await.unwrap();
        assert_eq!(status, "committed");
        assert!(matches!(
            store.commit_confirmed(&[1; 32], &[2; 32], NOW + 3).await,
            Err(LegacyMigrationError::AlreadyBound)
        ));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_bindings")
            .fetch_one(&store.repository.pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn conflicting_ais_profile_retains_its_xp_and_does_not_revoke_legacy() {
        let (store, _dir) = fixture().await;
        confirmed(&store, "0x1conflict", 3, 4, "ais-existing").await;
        sqlx::query(
            "INSERT INTO public_profiles (issuer, subject, bond_xp, last_auth_time) \
             VALUES (?, ?, 55, ?)",
        )
        .bind(AIS_ISSUER).bind("ais-existing").bind(NOW as i64)
        .execute(&store.repository.pool).await.unwrap();
        assert!(matches!(
            store.commit_confirmed(&[3; 32], &[4; 32], NOW + 2).await,
            Err(LegacyMigrationError::IdentityConflict)
        ));
        let xp: i64 = sqlx::query_scalar(
            "SELECT bond_xp FROM public_profiles WHERE issuer = ? AND subject = ?",
        )
        .bind(AIS_ISSUER).bind("ais-existing")
        .fetch_one(&store.repository.pool).await.unwrap();
        assert_eq!(xp, 55);
        let still_active: i64 = sqlx::query_scalar(
            "SELECT active FROM native_sessions WHERE token_hash = ?",
        )
        .bind(vec![3_u8; 32]).fetch_one(&store.repository.pool).await.unwrap();
        assert_eq!(still_active, 1);
    }

    #[tokio::test]
    async fn expired_confirmation_and_revoked_native_proof_do_not_attach() {
        let (store, _dir) = fixture().await;
        confirmed(&store, "0x1revoked", 5, 6, "ais-revoked").await;
        sqlx::query("UPDATE native_sessions SET revoked_at = ? WHERE token_hash = ?")
            .bind(NOW as i64).bind(vec![5_u8; 32])
            .execute(&store.repository.pool).await.unwrap();
        assert!(matches!(
            store.commit_confirmed(&[5; 32], &[6; 32], NOW + 2).await,
            Err(LegacyMigrationError::InvalidLegacyProof)
        ));
        confirmed(&store, "0x1expired", 7, 8, "ais-expired").await;
        assert!(matches!(
            store.commit_confirmed(&[7; 32], &[8; 32], NOW + 601).await,
            Err(LegacyMigrationError::ExpiredConfirmation)
        ));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_bindings")
            .fetch_one(&store.repository.pool).await.unwrap();
        assert_eq!(count, 0);
    }
}
