// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Dormant verification and explicit confirmation for legacy Bond migration.
//!
//! Only the legacy nilx-one OIDC client may supply a signed callback here.
//! It must be the dedicated migration transaction, not an ordinary product
//! sign-in. No user-supplied public address or unverified subject is accepted.

use sqlx::Row as _;

use super::{LegacyBondMigrationRepository, LegacyMigrationError};
use crate::aiaiaiai_sign_in::{Callback, SignInFlow, safe_return_path};

const CLIENT: &str = "nilx-one";
const ISSUER: &str = "https://auth.aiaiaiai.org";
const MAX_FRESH_SECONDS: u64 = 600;
const MIGRATION_RETURN_PREFIX: &str = "/api/v1/auth/aiaiaiai/migrate/confirm/";

/// The verified callback was attached to the original pending transaction.
/// The host still must require a separate CSRF-protected owner confirmation.
pub struct LegacyMigrationProofRecorded {
    /// The host must clear the OIDC transaction cookie on successful callback.
    pub clear_cookie: String,
}

impl LegacyBondMigrationRepository {
    /// Verify a genuine OIDC callback through the legacy-client SignInFlow
    /// before storing its pairwise identity. The flow must use a separately
    /// scoped migration transaction signer in the host composition root.
    ///
    /// A signed, exact migration return path must match the host's own
    /// server-issued transaction record. This record can never be selected
    /// by an untrusted username, Bond pub_dress, or raw browser token.
    pub async fn record_ais_callback(
        &self,
        flow: &SignInFlow,
        cookie_header: Option<&str>,
        callback: Callback<'_>,
        expected_return_path: &str,
        legacy_session_hash: &[u8],
        transaction_hash: &[u8],
        now: u64,
    ) -> Result<LegacyMigrationProofRecorded, LegacyMigrationError> {
        if flow.client_id() != CLIENT
            || legacy_session_hash.len() != 32
            || transaction_hash.len() != 32
            || !expected_return_path.starts_with(MIGRATION_RETURN_PREFIX)
            || expected_return_path == MIGRATION_RETURN_PREFIX
            || safe_return_path(Some(expected_return_path)) != expected_return_path
        {
            return Err(LegacyMigrationError::InvalidTransaction);
        }
        let verified = flow.verify_callback(cookie_header, callback, now).await;
        let callback = verified
            .outcome
            .map_err(|_| LegacyMigrationError::InvalidAisProof)?;
        if callback.return_path() != expected_return_path {
            return Err(LegacyMigrationError::InvalidAisProof);
        }
        let proof = callback.identity();
        if proof.issuer != ISSUER
            || proof.subject.is_empty()
            || proof.subject.len() > 255
            || proof.auth_time > now
            || now.saturating_sub(proof.auth_time) > MAX_FRESH_SECONDS
        {
            return Err(LegacyMigrationError::InvalidAisProof);
        }
        let now_i64 = i64::try_from(now).map_err(|_| LegacyMigrationError::InvalidTransaction)?;
        let earliest = i64::try_from(now.saturating_sub(MAX_FRESH_SECONDS))
            .map_err(|_| LegacyMigrationError::InvalidTransaction)?;
        let auth_time =
            i64::try_from(proof.auth_time).map_err(|_| LegacyMigrationError::InvalidAisProof)?;
        let mut tx = self.repository.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(
            "SELECT i.status, i.started_at, i.expires_at, \
                    s.pub_dress \
             FROM legacy_bond_migration_intents i \
             JOIN native_sessions s ON s.token_hash = i.legacy_session_hash \
             JOIN identities b ON b.pub_dress = s.pub_dress \
             WHERE i.transaction_hash = ? AND i.legacy_session_hash = ? \
               AND s.active = 1 AND s.credential_authenticated = 1 \
               AND s.revoked_at IS NULL AND s.expires_at > ? \
               AND s.created_at BETWEEN ? AND ? AND b.identity_kind = 'human'",
        )
        .bind(transaction_hash)
        .bind(legacy_session_hash)
        .bind(now_i64)
        .bind(earliest)
        .bind(now_i64)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(LegacyMigrationError::InvalidLegacyProof)?;
        let status: String = row.try_get("status")?;
        let started_at: i64 = row.try_get("started_at")?;
        let expires_at: i64 = row.try_get("expires_at")?;
        if status != "pending_ais" {
            return Err(LegacyMigrationError::AlreadyPending);
        }
        if started_at > now_i64 || expires_at <= now_i64 {
            return Err(LegacyMigrationError::ExpiredConfirmation);
        }
        let updated = sqlx::query(
            "UPDATE legacy_bond_migration_intents \
             SET verified_issuer = ?, verified_subject = ?, verified_auth_time = ? \
             WHERE transaction_hash = ? AND legacy_session_hash = ? \
               AND status = 'pending_ais' AND verified_issuer IS NULL \
               AND verified_subject IS NULL AND verified_auth_time IS NULL",
        )
        .bind(&proof.issuer)
        .bind(&proof.subject)
        .bind(auth_time)
        .bind(transaction_hash)
        .bind(legacy_session_hash)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(LegacyMigrationError::AlreadyPending);
        }
        tx.commit().await?;
        Ok(LegacyMigrationProofRecorded {
            clear_cookie: verified.clear_cookie,
        })
    }

    /// The host calls this only after a visible, CSRF-verified, explicit
    /// confirmation from the authenticated legacy owner. No AIS callback,
    /// push notification or chat delivery can invoke it automatically.
    pub async fn confirm_verified(
        &self,
        legacy_session_hash: &[u8],
        transaction_hash: &[u8],
        now: u64,
    ) -> Result<(), LegacyMigrationError> {
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
            "SELECT i.status, i.started_at, i.expires_at, \
                    i.verified_issuer, i.verified_subject, i.verified_auth_time \
             FROM legacy_bond_migration_intents i \
             JOIN native_sessions s ON s.token_hash = i.legacy_session_hash \
             JOIN identities b ON b.pub_dress = s.pub_dress \
             WHERE i.transaction_hash = ? AND i.legacy_session_hash = ? \
               AND s.active = 1 AND s.credential_authenticated = 1 \
               AND s.revoked_at IS NULL AND s.expires_at > ? \
               AND s.created_at BETWEEN ? AND ? AND b.identity_kind = 'human'",
        )
        .bind(transaction_hash)
        .bind(legacy_session_hash)
        .bind(now_i64)
        .bind(earliest)
        .bind(now_i64)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(LegacyMigrationError::InvalidLegacyProof)?;
        let status: String = intent.try_get("status")?;
        let started_at: i64 = intent.try_get("started_at")?;
        let expires_at: i64 = intent.try_get("expires_at")?;
        if status != "pending_ais" {
            return Err(LegacyMigrationError::NotConfirmed);
        }
        if started_at > now_i64 || now_i64 >= expires_at {
            return Err(LegacyMigrationError::ExpiredConfirmation);
        }
        let issuer: Option<String> = intent.try_get("verified_issuer")?;
        let subject: Option<String> = intent.try_get("verified_subject")?;
        let auth_time: Option<i64> = intent.try_get("verified_auth_time")?;
        if issuer.as_deref() != Some(ISSUER)
            || subject.is_none_or(|s| s.is_empty() || s.len() > 255)
            || auth_time.is_none_or(|t| t < earliest || t > now_i64)
        {
            return Err(LegacyMigrationError::InvalidAisProof);
        }
        let updated = sqlx::query(
            "UPDATE legacy_bond_migration_intents \
             SET status = 'confirmed', confirmed_at = ? \
             WHERE transaction_hash = ? AND legacy_session_hash = ? \
               AND status = 'pending_ais' AND verified_issuer IS NOT NULL \
               AND verified_subject IS NOT NULL AND verified_auth_time IS NOT NULL \
               AND expires_at > ?",
        )
        .bind(now_i64)
        .bind(transaction_hash)
        .bind(legacy_session_hash)
        .bind(now_i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(LegacyMigrationError::NotConfirmed);
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{public_info::PublicInfoRepository, repository::IdentityRepository};

    const NOW: u64 = 1_800_000_000;

    async fn fixture() -> (LegacyBondMigrationRepository, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}",
            directory.path().join("proof.sqlite").display()
        );
        let _identity = IdentityRepository::connect(&url).await.unwrap();
        let product = PublicInfoRepository::connect(&url).await.unwrap();
        (
            LegacyBondMigrationRepository::prepare(product)
                .await
                .unwrap(),
            directory,
        )
    }

    async fn begin(store: &LegacyBondMigrationRepository, session: u8, intent: u8) {
        sqlx::query(
            "INSERT INTO identities (pub_dress, identity_kind) VALUES ('0x1proof', 'human')",
        )
        .execute(&store.repository.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO native_sessions \
             (token_hash, pub_dress, created_at, expires_at, active, credential_authenticated) \
             VALUES (?, '0x1proof', ?, ?, 1, 1)",
        )
        .bind(vec![session; 32])
        .bind(NOW as i64 - 30)
        .bind(NOW as i64 + 900)
        .execute(&store.repository.pool)
        .await
        .unwrap();
        store
            .begin(&[session; 32], &[intent; 32], NOW)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cannot_confirm_without_a_verified_ais_callback() {
        let (store, _directory) = fixture().await;
        begin(&store, 1, 2).await;
        assert!(matches!(
            store.confirm_verified(&[1; 32], &[2; 32], NOW + 2).await,
            Err(LegacyMigrationError::InvalidAisProof)
        ));
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_bindings")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn confirmation_records_consent_but_never_allocates_bond_or_profile() {
        let (store, _directory) = fixture().await;
        begin(&store, 3, 4).await;
        sqlx::query(
            "UPDATE legacy_bond_migration_intents \
             SET verified_issuer = ?, verified_subject = ?, verified_auth_time = ? \
             WHERE transaction_hash = ?",
        )
        .bind(ISSUER)
        .bind("issuer-verified-pairwise")
        .bind(NOW as i64)
        .bind(vec![4_u8; 32])
        .execute(&store.repository.pool)
        .await
        .unwrap();
        store
            .confirm_verified(&[3; 32], &[4; 32], NOW + 2)
            .await
            .unwrap();
        let status: String = sqlx::query_scalar(
            "SELECT status FROM legacy_bond_migration_intents WHERE transaction_hash = ?",
        )
        .bind(vec![4_u8; 32])
        .fetch_one(&store.repository.pool)
        .await
        .unwrap();
        assert_eq!(status, "confirmed");
        let profile_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public_profiles")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(profile_count, 0);
        assert!(matches!(
            store.confirm_verified(&[3; 32], &[4; 32], NOW + 3).await,
            Err(LegacyMigrationError::NotConfirmed)
        ));
    }

    #[tokio::test]
    async fn revoked_legacy_proof_cannot_confirm() {
        let (store, _directory) = fixture().await;
        begin(&store, 5, 6).await;
        sqlx::query("UPDATE native_sessions SET revoked_at = ? WHERE token_hash = ?")
            .bind(NOW as i64)
            .bind(vec![5_u8; 32])
            .execute(&store.repository.pool)
            .await
            .unwrap();
        assert!(matches!(
            store.confirm_verified(&[5; 32], &[6; 32], NOW + 2).await,
            Err(LegacyMigrationError::InvalidLegacyProof)
        ));
    }
}
