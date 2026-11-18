// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Which legacy Bond a live AIS product session acts for (#115).
//!
//! The only path from a product session to a legacy Bond is the durable
//! binding written by `commit_confirmed`. A profile without a binding is not
//! a legacy Bond: no public name, display name or AIS subject is ever used to
//! guess one. The lookup re-proves the session in SQL exactly as
//! `PublicInfoRepository::restore_session` does, then follows the binding to
//! the Bond's *current* address, so a renamed Bond keeps its access and a
//! deleted and recreated address never inherits it.
//!
//! This grants nothing by itself. The host still applies the existing
//! product authorization and single-active-client rules before any write.

use sqlx::Row as _;

use super::{LegacyBondMigrationRepository, LegacyMigrationError};

/// A product session's verified route to an attached legacy Bond.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundLegacyBond {
    /// Opaque product Bond key; stable across renames of the address.
    pub bond_id: String,
    /// The legacy address as it is now, for the existing game repositories.
    pub pub_dress: String,
    pub profile_id: String,
}

impl LegacyBondMigrationRepository {
    /// `None` for an unknown, expired or signed-out session, and for a live
    /// session whose profile has no binding. Never an error for either: the
    /// caller treats both as "not an existing Bond".
    pub async fn resolve_session_bond(
        &self,
        product_token_hash: &str,
        now: u64,
    ) -> Result<Option<BoundLegacyBond>, LegacyMigrationError> {
        let Ok(now) = i64::try_from(now) else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT k.bond_id, k.legacy_pub_dress, b.profile_id \
             FROM product_sessions s \
             JOIN public_profiles p \
               ON p.issuer = s.issuer AND p.subject = s.subject AND p.profile_id = s.profile_id \
             JOIN legacy_bond_bindings b ON b.profile_id = p.profile_id \
             JOIN legacy_bond_keys k ON k.bond_id = b.bond_id \
             JOIN identities i ON i.pub_dress = k.legacy_pub_dress \
             WHERE s.token_hash = ? AND s.expires_at > ? AND i.identity_kind = 'human'",
        )
        .bind(product_token_hash)
        .bind(now)
        .fetch_optional(&self.repository.pool)
        .await?;
        row.map(|row| {
            Ok(BoundLegacyBond {
                bond_id: row.try_get("bond_id")?,
                pub_dress: row.try_get("legacy_pub_dress")?,
                profile_id: row.try_get("profile_id")?,
            })
        })
        .transpose()
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
        let url = format!("sqlite://{}", dir.path().join("session.sqlite").display());
        let _legacy = IdentityRepository::connect(&url).await.unwrap();
        let product = PublicInfoRepository::connect(&url).await.unwrap();
        (
            LegacyBondMigrationRepository::prepare(product)
                .await
                .unwrap(),
            dir,
        )
    }

    async fn exec(store: &LegacyBondMigrationRepository, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sql);
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(&store.repository.pool).await.unwrap();
    }

    /// A profile with a live session; `token` is the stored digest.
    async fn profile(store: &LegacyBondMigrationRepository, subject: &str, token: &str) -> String {
        exec(
            store,
            "INSERT INTO public_profiles (issuer, subject, last_auth_time) VALUES (?, ?, 1)",
            &[ISSUER, subject],
        )
        .await;
        let profile_id: String = sqlx::query_scalar(
            "SELECT profile_id FROM public_profiles WHERE issuer = ? AND subject = ?",
        )
        .bind(ISSUER)
        .bind(subject)
        .fetch_one(&store.repository.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO product_sessions (token_hash, issuer, subject, profile_id, created_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(token)
        .bind(ISSUER)
        .bind(subject)
        .bind(&profile_id)
        .bind(NOW as i64 - 10)
        .bind(NOW as i64 + 600)
        .execute(&store.repository.pool)
        .await
        .unwrap();
        profile_id
    }

    /// A human legacy Bond attached to `profile_id`; returns its bond key.
    async fn attach(
        store: &LegacyBondMigrationRepository,
        address: &str,
        profile_id: &str,
    ) -> String {
        exec(
            store,
            "INSERT INTO identities (pub_dress, identity_kind) VALUES (?, 'human')",
            &[address],
        )
        .await;
        exec(
            store,
            "INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES (?)",
            &[address],
        )
        .await;
        let bond_id: String =
            sqlx::query_scalar("SELECT bond_id FROM legacy_bond_keys WHERE legacy_pub_dress = ?")
                .bind(address)
                .fetch_one(&store.repository.pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) VALUES (?, ?, ?)",
        )
        .bind(&bond_id)
        .bind(profile_id)
        .bind(NOW as i64 - 5)
        .execute(&store.repository.pool)
        .await
        .unwrap();
        bond_id
    }

    fn token(n: char) -> String {
        n.to_string().repeat(64)
    }

    #[tokio::test]
    async fn a_live_bound_session_resolves_its_bond() {
        let (store, _dir) = store().await;
        let profile_id = profile(&store, "subject-a", &token('a')).await;
        let bond_id = attach(&store, "0x1owner", &profile_id).await;

        assert_eq!(
            store.resolve_session_bond(&token('a'), NOW).await.unwrap(),
            Some(BoundLegacyBond {
                bond_id,
                pub_dress: "0x1owner".into(),
                profile_id,
            })
        );
    }

    #[tokio::test]
    async fn an_unbound_profile_with_a_matching_name_is_not_a_bond() {
        let (store, _dir) = store().await;
        profile(&store, "subject-a", &token('a')).await;
        // Same public name as a legacy Bond, but no binding.
        exec(
            &store,
            "UPDATE public_profiles SET pub_dress = '0x1owner' WHERE subject = 'subject-a'",
            &[],
        )
        .await;
        exec(
            &store,
            "INSERT INTO identities (pub_dress, identity_kind) VALUES ('0x1owner', 'human')",
            &[],
        )
        .await;
        assert_eq!(
            store.resolve_session_bond(&token('a'), NOW).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn expired_unknown_and_foreign_sessions_resolve_nothing() {
        let (store, _dir) = store().await;
        let owner = profile(&store, "subject-a", &token('a')).await;
        attach(&store, "0x1owner", &owner).await;
        profile(&store, "subject-b", &token('b')).await;

        assert_eq!(
            store
                .resolve_session_bond(&token('a'), NOW + 600)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            store.resolve_session_bond(&token('c'), NOW).await.unwrap(),
            None
        );
        // Another verified AIS identity's live session never reaches the Bond.
        assert_eq!(
            store.resolve_session_bond(&token('b'), NOW).await.unwrap(),
            None
        );
        // A session row that names a profile it does not belong to.
        exec(
            &store,
            "UPDATE product_sessions SET profile_id = ? WHERE token_hash = ?",
            &[&owner, &token('b')],
        )
        .await;
        assert_eq!(
            store.resolve_session_bond(&token('b'), NOW).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn signing_out_ends_access_to_the_bond() {
        let (store, _dir) = store().await;
        let owner = profile(&store, "subject-a", &token('a')).await;
        attach(&store, "0x1owner", &owner).await;
        store
            .repository
            .close_session(&token('a'), NOW)
            .await
            .unwrap();
        assert_eq!(
            store.resolve_session_bond(&token('a'), NOW).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_rename_keeps_access_and_a_recreated_address_does_not_inherit_it() {
        let (store, _dir) = store().await;
        let owner = profile(&store, "subject-a", &token('a')).await;
        let bond_id = attach(&store, "0x1owner", &owner).await;

        exec(
            &store,
            "UPDATE identities SET pub_dress = '0x1renamed' WHERE pub_dress = '0x1owner'",
            &[],
        )
        .await;
        let renamed = store
            .resolve_session_bond(&token('a'), NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(renamed.bond_id, bond_id);
        assert_eq!(renamed.pub_dress, "0x1renamed");

        exec(
            &store,
            "DELETE FROM identities WHERE pub_dress = '0x1renamed'",
            &[],
        )
        .await;
        exec(
            &store,
            "INSERT INTO identities (pub_dress, identity_kind) VALUES ('0x1renamed', 'human')",
            &[],
        )
        .await;
        assert_eq!(
            store.resolve_session_bond(&token('a'), NOW).await.unwrap(),
            None
        );
    }
}
