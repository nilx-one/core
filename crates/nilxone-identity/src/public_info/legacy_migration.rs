// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Dormant first step of the opt-in legacy Bond migration (#115).
//!
//! A server-minted handle is stored by digest. Only a fresh, active,
//! credential-authenticated native session may begin a migration; provider
//! authentication and a matching public name never prove legacy ownership.
//! Nothing in this module accepts an AIS callback, attaches a Bond, revokes a
//! session, or authorizes gameplay yet. Those changes require a separate,
//! reviewed atomic commit implementation before a host can activate the flow.

use sqlx::Row;

mod commit;
mod proof;
mod session;
pub use commit::LegacyMigrationAttached;
pub use proof::LegacyMigrationProofRecorded;
pub use session::BoundLegacyBond;

use super::PublicInfoRepository;

/// A migration start is not an ownership transfer or an authorized Bond.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct LegacyMigrationStarted {
    pub expires_at: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum LegacyMigrationError {
    #[error("legacy and product tables must share one verified SQLite store")]
    UnsupportedTopology,
    #[error("a fresh, active, credential-authenticated legacy session is required")]
    InvalidLegacyProof,
    #[error("a server-generated 32-byte transaction digest is required")]
    InvalidTransaction,
    #[error("the legacy Bond is already bound to a product profile")]
    AlreadyBound,
    #[error("a migration with this credential session or handle is already pending")]
    AlreadyPending,
    #[error("the AIS migration intent has not been explicitly confirmed")]
    NotConfirmed,
    #[error("the confirmed migration has expired or has an invalid confirmation time")]
    ExpiredConfirmation,
    #[error("the issuer or age of the verified AIS evidence is invalid")]
    InvalidAisProof,
    #[error("this verified AIS subject already has a product profile")]
    IdentityConflict,
    #[error("legacy Bond migration storage is unavailable")]
    Storage(#[from] sqlx::Error),
}

/// Deliberately not constructed by PublicInfoRepository::connect. The
/// composition root must explicitly prepare this store after both schemas
/// exist, and the host feature flag must remain off until #115 and #621 pass
/// their independent E2E and rollout gates.
#[derive(Clone)]
pub struct LegacyBondMigrationRepository {
    repository: PublicInfoRepository,
}

impl LegacyBondMigrationRepository {
    pub async fn prepare(repository: PublicInfoRepository) -> Result<Self, LegacyMigrationError> {
        let required: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' \
             AND name IN ('identities', 'native_sessions', 'native_credentials', \
                          'public_profiles', 'product_sessions')",
        )
        .fetch_one(&repository.pool)
        .await?;
        let session_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('native_sessions') \
             WHERE name IN ('active', 'credential_authenticated', 'created_at', \
                            'expires_at', 'revoked_at', 'pub_dress', 'token_hash')",
        )
        .fetch_one(&repository.pool)
        .await?;
        let identity_kind: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('identities') \
             WHERE name = 'identity_kind'",
        )
        .fetch_one(&repository.pool)
        .await?;

        if required != 5 || session_columns != 7 || identity_kind != 1 {
            return Err(LegacyMigrationError::UnsupportedTopology);
        }

        sqlx::raw_sql(include_str!("../../legacy_bond_migration_schema.sql"))
            .execute(&repository.pool)
            .await?;
        Ok(Self { repository })
    }

    /// Mark expired intents terminal in a separately committed transaction.
    /// A later failed start must not roll this transition back or leave an
    /// expired row occupying the unique pending-session slot. This is only
    /// housekeeping: commit must independently check expires_at, not just
    /// the status field, before any Bond attachment.
    async fn expire_intents(&self, now: i64) -> Result<(), LegacyMigrationError> {
        let mut transaction = self.repository.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "UPDATE legacy_bond_migration_intents SET status = 'expired' \
             WHERE status IN ('pending_ais', 'confirmed') AND expires_at <= ?",
        )
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// No public name, AIS subject, or browser-selected Bond identifier is
    /// accepted. Both opaque digests must be derived server-side. The native
    /// proof is read inside the same BEGIN IMMEDIATE transaction as the
    /// pending intent, so logout, activation or a competing start cannot
    /// slip between the check and insert.
    pub async fn begin(
        &self,
        legacy_session_hash: &[u8],
        transaction_hash: &[u8],
        now: u64,
    ) -> Result<LegacyMigrationStarted, LegacyMigrationError> {
        if legacy_session_hash.len() != 32 {
            return Err(LegacyMigrationError::InvalidLegacyProof);
        }
        if transaction_hash.len() != 32 {
            return Err(LegacyMigrationError::InvalidTransaction);
        }
        let now_i64 = i64::try_from(now).map_err(|_| LegacyMigrationError::InvalidTransaction)?;
        let expires_at = now
            .checked_add(600)
            .ok_or(LegacyMigrationError::InvalidTransaction)?;
        let expires_i64 =
            i64::try_from(expires_at).map_err(|_| LegacyMigrationError::InvalidTransaction)?;
        let earliest = i64::try_from(now.saturating_sub(600))
            .map_err(|_| LegacyMigrationError::InvalidTransaction)?;

        // An invalid/revoked proof must not roll back independent expiry.
        self.expire_intents(now_i64).await?;
        let mut tx = self.repository.pool.begin_with("BEGIN IMMEDIATE").await?;
        let proof = sqlx::query(
            "SELECT s.pub_dress FROM native_sessions s \
             JOIN identities i ON i.pub_dress = s.pub_dress \
             WHERE s.token_hash = ? AND s.active = 1 \
               AND s.credential_authenticated = 1 AND s.revoked_at IS NULL \
               AND s.expires_at > ? AND s.created_at BETWEEN ? AND ? \
               AND i.identity_kind = 'human'",
        )
        .bind(legacy_session_hash)
        .bind(now_i64)
        .bind(earliest)
        .bind(now_i64)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = proof else {
            return Err(LegacyMigrationError::InvalidLegacyProof);
        };
        let address: String = row.try_get("pub_dress")?;
        let already_bound: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM legacy_bond_keys k \
             JOIN legacy_bond_bindings b ON b.bond_id = k.bond_id \
             WHERE k.legacy_pub_dress = ?)",
        )
        .bind(&address)
        .fetch_one(&mut *tx)
        .await?;
        if already_bound {
            return Err(LegacyMigrationError::AlreadyBound);
        }

        let insert = sqlx::query(
            "INSERT INTO legacy_bond_migration_intents \
             (transaction_hash, legacy_session_hash, started_at, expires_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(transaction_hash)
        .bind(legacy_session_hash)
        .bind(now_i64)
        .bind(expires_i64)
        .execute(&mut *tx)
        .await;
        match insert {
            Ok(_) => (),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                return Err(LegacyMigrationError::AlreadyPending);
            }
            Err(error) => return Err(error.into()),
        }
        tx.commit().await?;
        Ok(LegacyMigrationStarted { expires_at })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::IdentityRepository;

    const NOW: u64 = 1_800_000_000;

    async fn store() -> (LegacyBondMigrationRepository, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("migration.sqlite");
        let url = format!("sqlite://{}", database.display());
        let _legacy = IdentityRepository::connect(&url).await.unwrap();
        let product = PublicInfoRepository::connect(&url).await.unwrap();
        (
            LegacyBondMigrationRepository::prepare(product)
                .await
                .unwrap(),
            directory,
        )
    }

    async fn human_session(
        store: &LegacyBondMigrationRepository,
        suffix: &str,
        token: &[u8],
        created_at: i64,
        expires_at: i64,
        active: bool,
        credential_authenticated: bool,
    ) {
        let address = format!("0x1{suffix}");
        sqlx::query("INSERT INTO identities (pub_dress, identity_kind) VALUES (?, 'human')")
            .bind(&address)
            .execute(&store.repository.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO native_sessions \
             (token_hash, pub_dress, created_at, expires_at, active, credential_authenticated) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(token)
        .bind(&address)
        .bind(created_at)
        .bind(expires_at)
        .bind(active)
        .bind(credential_authenticated)
        .execute(&store.repository.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn refuses_a_product_only_database_without_creating_migration_tables() {
        let product = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        assert!(matches!(
            LegacyBondMigrationRepository::prepare(product.clone()).await,
            Err(LegacyMigrationError::UnsupportedTopology)
        ));
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'legacy_bond_keys'",
        )
        .fetch_one(&product.pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn starts_only_with_fresh_password_authenticated_active_native_proof() {
        let (store, _directory) = store().await;
        human_session(
            &store,
            "valid",
            &[1; 32],
            NOW as i64 - 30,
            NOW as i64 + 300,
            true,
            true,
        )
        .await;
        assert_eq!(
            store.begin(&[1; 32], &[2; 32], NOW).await.unwrap(),
            LegacyMigrationStarted {
                expires_at: NOW + 600
            }
        );
        assert!(matches!(
            store.begin(&[1; 32], &[3; 32], NOW).await,
            Err(LegacyMigrationError::AlreadyPending)
        ));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_bindings")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "begin is not a binding or consent");
    }

    #[tokio::test]
    async fn rejects_provider_inactive_stale_future_and_expired_sessions() {
        let (store, _directory) = store().await;
        for (n, active, credential, created, expires) in [
            (1_u8, true, false, NOW as i64 - 10, NOW as i64 + 100),
            (2, false, true, NOW as i64 - 10, NOW as i64 + 100),
            (3, true, true, NOW as i64 - 601, NOW as i64 + 100),
            (4, true, true, NOW as i64 + 1, NOW as i64 + 100),
            (5, true, true, NOW as i64 - 10, NOW as i64),
        ] {
            human_session(
                &store,
                &format!("reject{n}"),
                &[n; 32],
                created,
                expires,
                active,
                credential,
            )
            .await;
            assert!(matches!(
                store.begin(&[n; 32], &[n + 10; 32], NOW).await,
                Err(LegacyMigrationError::InvalidLegacyProof)
            ));
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_migration_intents")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn rejects_revoked_active_proof_and_avaia_owner_impersonation() {
        let (store, _directory) = store().await;
        human_session(
            &store,
            "revoked",
            &[21; 32],
            NOW as i64 - 1,
            NOW as i64 + 100,
            true,
            true,
        )
        .await;
        // Revocation does not clear the active flag in the legacy schema.
        sqlx::query("UPDATE native_sessions SET revoked_at = ? WHERE token_hash = ?")
            .bind(NOW as i64 - 1)
            .bind(&[21; 32][..])
            .execute(&store.repository.pool)
            .await
            .unwrap();
        let remains_active: i64 =
            sqlx::query_scalar("SELECT active FROM native_sessions WHERE token_hash = ?")
                .bind(&[21; 32][..])
                .fetch_one(&store.repository.pool)
                .await
                .unwrap();
        assert_eq!(remains_active, 1);
        assert!(matches!(
            store.begin(&[21; 32], &[22; 32], NOW).await,
            Err(LegacyMigrationError::InvalidLegacyProof)
        ));

        human_session(
            &store,
            "avaia",
            &[23; 32],
            NOW as i64 - 1,
            NOW as i64 + 100,
            true,
            true,
        )
        .await;
        sqlx::query("UPDATE identities SET identity_kind = 'avaia' WHERE pub_dress = '0x1avaia'")
            .execute(&store.repository.pool)
            .await
            .unwrap();
        assert!(matches!(
            store.begin(&[23; 32], &[24; 32], NOW).await,
            Err(LegacyMigrationError::InvalidLegacyProof)
        ));
        assert!(matches!(
            store.begin(&[1; 31], &[25; 32], NOW).await,
            Err(LegacyMigrationError::InvalidLegacyProof)
        ));
        assert!(matches!(
            store.begin(&[23; 32], &[1; 31], NOW).await,
            Err(LegacyMigrationError::InvalidTransaction)
        ));
        let intents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_migration_intents")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(intents, 0);
    }

    #[tokio::test]
    async fn confirmed_intent_requires_verified_identity_but_does_not_allocate_profile() {
        let (store, _directory) = store().await;
        human_session(
            &store,
            "confirmation",
            &[51; 32],
            NOW as i64 - 1,
            NOW as i64 + 100,
            true,
            true,
        )
        .await;
        store.begin(&[51; 32], &[52; 32], NOW).await.unwrap();
        assert!(
            sqlx::query(
                "UPDATE legacy_bond_migration_intents \
                 SET status = 'confirmed' WHERE transaction_hash = ?",
            )
            .bind(&[52; 32][..])
            .execute(&store.repository.pool)
            .await
            .is_err(),
            "confirmation cannot be represented without verified AIS proof",
        );
        sqlx::query(
            "UPDATE legacy_bond_migration_intents \
             SET verified_issuer = ?, verified_subject = ?, verified_auth_time = ?, \
                 confirmed_at = ?, status = 'confirmed' WHERE transaction_hash = ?",
        )
        .bind("https://auth.aiaiaiai.org")
        .bind("pairwise-subject")
        .bind(NOW as i64)
        .bind(NOW as i64 + 1)
        .bind(&[52; 32][..])
        .execute(&store.repository.pool)
        .await
        .unwrap();
        let profiles: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public_profiles")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(profiles, 0, "confirmation is not a profile allocation");
        assert!(
            sqlx::query(
                "UPDATE legacy_bond_migration_intents \
                 SET status = 'committed' WHERE transaction_hash = ?",
            )
            .bind(&[52; 32][..])
            .execute(&store.repository.pool)
            .await
            .is_err(),
            "a committed intent requires an actual profile",
        );
        let bindings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_bindings")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(bindings, 0, "no synthetic Bond attachment on confirmation");
    }

    #[tokio::test]
    async fn pending_hash_expires_without_binding_or_credential_side_effects() {
        let (store, _directory) = store().await;
        human_session(
            &store,
            "expiry",
            &[31; 32],
            NOW as i64 - 10,
            NOW as i64 + 3_000,
            true,
            true,
        )
        .await;
        store.begin(&[31; 32], &[32; 32], NOW).await.unwrap();
        assert!(matches!(
            store.begin(&[31; 32], &[33; 32], NOW + 601).await,
            Err(LegacyMigrationError::InvalidLegacyProof)
        )); // Native proof is now older than ten minutes.
        let status: String = sqlx::query_scalar(
            "SELECT status FROM legacy_bond_migration_intents WHERE transaction_hash = ?",
        )
        .bind(&[32; 32][..])
        .fetch_one(&store.repository.pool)
        .await
        .unwrap();
        assert_eq!(
            status, "expired",
            "the expired intent remains terminal even when fresh proof fails"
        );
        let bindings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_bindings")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(bindings, 0, "expiry must never attach or move a Bond");
        let revoked: Option<i64> =
            sqlx::query_scalar("SELECT revoked_at FROM native_sessions WHERE token_hash = ?")
                .bind(&[31; 32][..])
                .fetch_one(&store.repository.pool)
                .await
                .unwrap();
        assert_eq!(revoked, None);
    }

    #[tokio::test]
    async fn one_to_one_constraints_and_rename_cascade_do_not_move_legacy_state() {
        let (store, _directory) = store().await;
        human_session(
            &store,
            "binding",
            &[41; 32],
            NOW as i64 - 1,
            NOW as i64 + 100,
            true,
            true,
        )
        .await;
        sqlx::query(
            "INSERT INTO public_profiles (issuer, subject, pub_dress, last_auth_time) \
             VALUES ('https://auth.aiaiaiai.org', 'pairwise-one', '0x1binding', ?)",
        )
        .bind(NOW as i64)
        .execute(&store.repository.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES ('0x1binding')")
            .execute(&store.repository.pool)
            .await
            .unwrap();
        let bond_id: String = sqlx::query_scalar(
            "SELECT bond_id FROM legacy_bond_keys WHERE legacy_pub_dress = '0x1binding'",
        )
        .fetch_one(&store.repository.pool)
        .await
        .unwrap();
        let profile_id: String = sqlx::query_scalar(
            "SELECT profile_id FROM public_profiles WHERE subject = 'pairwise-one'",
        )
        .fetch_one(&store.repository.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) VALUES (?, ?, ?)",
        )
        .bind(&bond_id)
        .bind(&profile_id)
        .bind(NOW as i64)
        .execute(&store.repository.pool)
        .await
        .unwrap();
        assert!(matches!(
            store.begin(&[41; 32], &[42; 32], NOW).await,
            Err(LegacyMigrationError::AlreadyBound)
        ));
        human_session(
            &store,
            "other",
            &[43; 32],
            NOW as i64 - 1,
            NOW as i64 + 100,
            true,
            true,
        )
        .await;
        sqlx::query("INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES ('0x1other')")
            .execute(&store.repository.pool)
            .await
            .unwrap();
        let other_bond_id: String = sqlx::query_scalar(
            "SELECT bond_id FROM legacy_bond_keys WHERE legacy_pub_dress = '0x1other'",
        )
        .fetch_one(&store.repository.pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) VALUES (?, ?, ?)",
            )
            .bind(&other_bond_id)
            .bind(&profile_id)
            .bind(NOW as i64)
            .execute(&store.repository.pool)
            .await
            .is_err(),
            "a second Bond must never acquire the same AIS profile",
        );
        sqlx::query(
            "UPDATE identities SET pub_dress = '0x1renamed' WHERE pub_dress = '0x1binding'",
        )
        .execute(&store.repository.pool)
        .await
        .unwrap();
        let stored: String =
            sqlx::query_scalar("SELECT legacy_pub_dress FROM legacy_bond_keys WHERE bond_id = ?")
                .bind(&bond_id)
                .fetch_one(&store.repository.pool)
                .await
                .unwrap();
        assert_eq!(stored, "0x1renamed");
        sqlx::query("DELETE FROM identities WHERE pub_dress = '0x1renamed'")
            .execute(&store.repository.pool)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_bond_bindings")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
