// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! A person's first sign-in through AIS creates their Bond (#115).
//!
//! The AIS profile of a verified sign-in has no Bond yet. In one
//! `BEGIN IMMEDIATE` transaction this creates the human Bond at the address
//! AIS assigned, gives it its opaque product key and binds it to the profile,
//! so the profile's product session acts for the new Bond from then on.
//!
//! It never links an existing Bond. If the address already belongs to a legacy
//! Bond, nothing is created or bound: that Bond's owner proves ownership
//! through the migration flow, not through a matching name. A retry for a
//! profile that is already bound is a no-op that names its Bond.

use std::str::FromStr as _;

use sqlx::Row as _;

use super::{LegacyBondMigrationRepository, LegacyMigrationError};
use crate::{
    PubDress,
    repository::{HumanIdentityInsertOutcome, insert_human_with_public_label_in},
};

/// The Bond a profile acts for after [`LegacyBondMigrationRepository::provision_new_bond`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvisionedBond {
    pub bond_id: String,
    pub pub_dress: String,
    pub profile_id: String,
    /// False when the profile was already bound and nothing changed.
    pub created: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("no AIS profile exists for this sign-in")]
    NoProfile,
    #[error("the AIS profile carries no usable address for a new Bond")]
    NoAddress,
    #[error("this address already belongs to a Bond that was not created from this profile")]
    AddressTaken,
    #[error("no public label is free for this address")]
    LabelUnavailable,
    #[error(transparent)]
    Migration(#[from] LegacyMigrationError),
    #[error("Bond storage is unavailable")]
    Repository(#[from] crate::repository::RepositoryError),
}

impl From<sqlx::Error> for ProvisionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Migration(LegacyMigrationError::Storage(error))
    }
}

impl LegacyBondMigrationRepository {
    /// Creates and binds the Bond of the AIS profile `(issuer, subject)` at the
    /// address AIS assigned it. The caller has already verified the sign-in
    /// that created the profile; nothing here reads a name from a request.
    pub async fn provision_new_bond(
        &self,
        issuer: &str,
        subject: &str,
        now: u64,
    ) -> Result<ProvisionedBond, ProvisionError> {
        let now_i64 = i64::try_from(now).map_err(|_| LegacyMigrationError::InvalidTransaction)?;
        let mut tx = self.repository.pool.begin_with("BEGIN IMMEDIATE").await?;
        let profile = sqlx::query(
            "SELECT profile_id, pub_dress FROM public_profiles WHERE issuer = ? AND subject = ?",
        )
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ProvisionError::NoProfile)?;
        let profile_id: String = profile.try_get("profile_id")?;
        let claimed: Option<String> = profile.try_get("pub_dress")?;

        let bound = sqlx::query(
            "SELECT k.bond_id, k.legacy_pub_dress FROM legacy_bond_bindings b \
             JOIN legacy_bond_keys k ON k.bond_id = b.bond_id WHERE b.profile_id = ?",
        )
        .bind(&profile_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = bound {
            tx.commit().await?;
            return Ok(ProvisionedBond {
                bond_id: row.try_get("bond_id")?,
                pub_dress: row.try_get("legacy_pub_dress")?,
                profile_id,
                created: false,
            });
        }

        let pub_dress = claimed
            .as_deref()
            .and_then(|value| PubDress::from_str(value).ok())
            .ok_or(ProvisionError::NoAddress)?;
        match insert_human_with_public_label_in(&mut tx, &pub_dress, now).await? {
            HumanIdentityInsertOutcome::Inserted => {}
            HumanIdentityInsertOutcome::PubDressUnavailable => {
                tx.rollback().await?;
                return Err(ProvisionError::AddressTaken);
            }
            HumanIdentityInsertOutcome::PublicLabelUnavailable => {
                tx.rollback().await?;
                return Err(ProvisionError::LabelUnavailable);
            }
        }
        let bond_id: String = sqlx::query_scalar(
            "INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES (?) RETURNING bond_id",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) VALUES (?, ?, ?)",
        )
        .bind(&bond_id)
        .bind(&profile_id)
        .bind(now_i64)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ProvisionedBond {
            bond_id,
            pub_dress: pub_dress.as_str().to_owned(),
            profile_id,
            created: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{public_info::PublicInfoRepository, repository::IdentityRepository};

    const NOW: u64 = 1_800_000_000;
    const ISSUER: &str = "https://auth.aiaiaiai.org";

    async fn store() -> (LegacyBondMigrationRepository, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("provision.sqlite").display());
        let _legacy = IdentityRepository::connect(&url).await.unwrap();
        let product = PublicInfoRepository::connect(&url).await.unwrap();
        (
            LegacyBondMigrationRepository::prepare(product)
                .await
                .unwrap(),
            dir,
        )
    }

    async fn profile(store: &LegacyBondMigrationRepository, subject: &str, address: Option<&str>) {
        sqlx::query(
            "INSERT INTO public_profiles (issuer, subject, pub_dress, last_auth_time) VALUES (?, ?, ?, 1)",
        )
        .bind(ISSUER)
        .bind(subject)
        .bind(address)
        .execute(&store.repository.pool)
        .await
        .unwrap();
    }

    async fn count(store: &LegacyBondMigrationRepository, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&store.repository.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_first_sign_in_creates_and_binds_the_bond_at_the_assigned_address() {
        let (store, _dir) = store().await;
        profile(&store, "subject-a", Some("0x1sky")).await;
        let made = store
            .provision_new_bond(ISSUER, "subject-a", NOW)
            .await
            .unwrap();
        assert!(made.created);
        assert_eq!(made.pub_dress, "0x1sky");
        let kind: String =
            sqlx::query_scalar("SELECT identity_kind FROM identities WHERE pub_dress = '0x1sky'")
                .fetch_one(&store.repository.pool)
                .await
                .unwrap();
        assert_eq!(kind, "human");
        assert_eq!(
            (
                count(&store, "legacy_bond_keys").await,
                count(&store, "legacy_bond_bindings").await
            ),
            (1, 1)
        );

        // The profile's live product session now acts for that Bond.
        let token = "e".repeat(64);
        sqlx::query(
            "INSERT INTO product_sessions (token_hash, issuer, subject, profile_id, created_at, expires_at, active) \
             SELECT ?, issuer, subject, profile_id, 1, ?, 1 FROM public_profiles",
        )
        .bind(&token)
        .bind(NOW as i64 + 600)
        .execute(&store.repository.pool)
        .await
        .unwrap();
        let bond = store
            .resolve_session_bond(&token, NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bond.bond_id, made.bond_id);
        assert_eq!(bond.pub_dress, "0x1sky");
        assert!(bond.active);
    }

    #[tokio::test]
    async fn a_retry_changes_nothing_and_concurrent_first_sign_ins_make_one_bond() {
        let (store, _dir) = store().await;
        profile(&store, "subject-a", Some("0x1sky")).await;
        let (first, second) = tokio::join!(
            store.provision_new_bond(ISSUER, "subject-a", NOW),
            store.provision_new_bond(ISSUER, "subject-a", NOW),
        );
        let (first, second) = (first.unwrap(), second.unwrap());
        assert_eq!(first.bond_id, second.bond_id);
        assert_eq!(
            [first.created, second.created]
                .iter()
                .filter(|c| **c)
                .count(),
            1
        );
        let again = store
            .provision_new_bond(ISSUER, "subject-a", NOW + 5)
            .await
            .unwrap();
        assert!(!again.created);
        assert_eq!(again.bond_id, first.bond_id);
        assert_eq!(count(&store, "identities").await, 1);
        assert_eq!(count(&store, "legacy_bond_bindings").await, 1);
    }

    #[tokio::test]
    async fn an_address_that_already_has_a_bond_is_never_linked_by_name() {
        let (store, _dir) = store().await;
        // A legacy Bond that exists on its own, unbound.
        sqlx::query("INSERT INTO identities (pub_dress, identity_kind, created_at) VALUES ('0x1sky', 'human', 1)")
            .execute(&store.repository.pool)
            .await
            .unwrap();
        profile(&store, "subject-a", Some("0x1sky")).await;
        assert!(matches!(
            store.provision_new_bond(ISSUER, "subject-a", NOW).await,
            Err(ProvisionError::AddressTaken)
        ));
        assert_eq!(
            (
                count(&store, "identities").await,
                count(&store, "legacy_bond_keys").await,
                count(&store, "legacy_bond_bindings").await
            ),
            (1, 0, 0)
        );

        // A second AIS account cannot take over a Bond created for the first.
        sqlx::query("DELETE FROM identities")
            .execute(&store.repository.pool)
            .await
            .unwrap();
        store
            .provision_new_bond(ISSUER, "subject-a", NOW)
            .await
            .unwrap();
        profile(&store, "subject-b", Some("0x1sky")).await;
        assert!(matches!(
            store.provision_new_bond(ISSUER, "subject-b", NOW).await,
            Err(ProvisionError::AddressTaken)
        ));
        assert_eq!(count(&store, "legacy_bond_bindings").await, 1);
    }

    #[tokio::test]
    async fn no_profile_or_no_address_creates_nothing() {
        let (store, _dir) = store().await;
        assert!(matches!(
            store.provision_new_bond(ISSUER, "nobody", NOW).await,
            Err(ProvisionError::NoProfile)
        ));
        profile(&store, "no-name", None).await;
        assert!(matches!(
            store.provision_new_bond(ISSUER, "no-name", NOW).await,
            Err(ProvisionError::NoAddress)
        ));
        profile(&store, "bad-name", Some("not an address")).await;
        assert!(matches!(
            store.provision_new_bond(ISSUER, "bad-name", NOW).await,
            Err(ProvisionError::NoAddress)
        ));
        assert_eq!(
            (
                count(&store, "identities").await,
                count(&store, "legacy_bond_keys").await
            ),
            (0, 0)
        );
    }
}
