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
//! This grants nothing by itself. Like `restore_session`, it deliberately
//! accepts a live session whose `active` flag is 0: that is read identity,
//! not write authority. A host must re-check `active = 1` and the
//! single-active-client rule atomically with every game mutation; this
//! lookup is never that check.

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
    /// Whether this session is the profile's active client. Read identity is
    /// allowed either way; hosts use this only to answer early, and every
    /// write still re-checks `active = 1` atomically in Core.
    pub active: bool,
}

impl LegacyBondMigrationRepository {
    /// An inactive but live session still resolves (read identity); writes
    /// must independently require `active = 1`.
    ///
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
            "SELECT k.bond_id, k.legacy_pub_dress, b.profile_id, s.active \
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
                active: row.try_get::<i64, _>("active")? == 1,
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
    async fn chat_location_requires_active_session_and_preserves_manual_and_newer_data() {
        let (store, _dir) = store().await;
        let token = token('a');
        let profile_id = profile(&store, "subject-a", &token).await;
        attach(&store, "0x1owner", &profile_id).await;
        exec(
            &store,
            "INSERT INTO telegram_chat_sessions VALUES ('bot', 42, ?)",
            &[&token],
        )
        .await;
        let first = crate::GeoCoordinate::new(30, 40).unwrap();
        let next = crate::GeoCoordinate::new(50, 60).unwrap();
        let repository = &store.repository;
        let submit = |bot, actor, point, observed, now| async move {
            repository
                .record_chat_live_location(bot, actor, point, observed, now)
                .await
                .unwrap()
        };

        assert!(!submit("bot", 42, first, NOW, NOW).await);
        exec(
            &store,
            "UPDATE product_sessions SET active = 1 WHERE token_hash = ?",
            &[&token],
        )
        .await;
        assert!(!submit("other-bot", 42, first, NOW, NOW).await);
        assert!(!submit("bot", 99, first, NOW, NOW).await);
        assert!(!submit("bot", 42, first, NOW - 121, NOW).await);
        assert!(submit("bot", 42, first, NOW, NOW).await);
        assert!(!submit("bot", 42, next, NOW, NOW + 1).await);
        assert!(submit("bot", 42, next, NOW + 1, NOW + 1).await);
        assert!(!submit("bot", 42, first, NOW, NOW + 2).await);
        // A few seconds of skew between Telegram and this host is accepted.
        assert!(submit("bot", 42, next, NOW + 4, NOW + 2).await);
        assert!(!submit("bot", 42, next, NOW + 8, NOW + 2).await);
        let longitude: i32 = sqlx::query_scalar("SELECT longitude_e7 FROM bond_locations")
            .fetch_one(&store.repository.pool)
            .await
            .unwrap();
        assert_eq!(longitude, 50);

        exec(&store, "UPDATE bond_locations SET mode = 'manual'", &[]).await;
        assert!(!submit("bot", 42, first, NOW + 2, NOW + 2).await);
        assert!(!submit("bot", 42, first, NOW + 600, NOW + 600).await);
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
                active: false,
            })
        );
    }

    #[tokio::test]
    async fn chat_state_reads_only_the_live_bound_owners_avaia_and_wallet() {
        let (store, dir) = store().await;
        let token_a = token('a');
        let bound = profile(&store, "subject-a", &token_a).await;
        attach(&store, "0x1owner", &bound).await;
        exec(
            &store,
            "INSERT INTO bond_seeds (owner_pub_dress, balance) VALUES ('0x1owner', 51)",
            &[],
        )
        .await;
        let owner = store
            .repository
            .restore_session(&token_a, NOW)
            .await
            .unwrap()
            .unwrap();
        let snapshot = store
            .repository
            .read_chat_bond_state(&owner, NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.seeds, 51);
        assert!(snapshot.avaia.is_none());
        let legacy = IdentityRepository::connect(&format!(
            "sqlite://{}",
            dir.path().join("session.sqlite").display()
        ))
        .await
        .unwrap();
        let address = "0x1owner".parse().unwrap();
        let avaia = crate::AvaiaPubDress::derive_default(&address);
        legacy
            .configure_owned_avaia(&address, &avaia, NOW)
            .await
            .unwrap();
        legacy
            .write_avaia_location(
                &address,
                crate::AvaiaLocation {
                    coordinate: crate::GeoCoordinate::new(305200000, 504500000).unwrap(),
                    updated_at: crate::DecimalU64::new(NOW),
                    travel_revision: 0,
                },
            )
            .await
            .unwrap();
        let snapshot = store
            .repository
            .read_chat_bond_state(&owner, NOW)
            .await
            .unwrap()
            .unwrap();
        let avatar = snapshot.avaia.unwrap();
        assert_eq!(avatar.pub_dress, avaia.to_string());
        assert!(avatar.configured);
        assert_eq!(avatar.location_e7, Some((504500000, 305200000)));

        assert!(
            store
                .repository
                .read_chat_bond_state(&owner, NOW + 600)
                .await
                .unwrap()
                .is_none()
        );

        let token_b = token('b');
        profile(&store, "subject-b", &token_b).await;
        let unrelated = store
            .repository
            .restore_session(&token_b, NOW)
            .await
            .unwrap()
            .unwrap();
        assert!(
            store
                .repository
                .read_chat_bond_state(&unrelated, NOW)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn wallet_history_is_bounded_ordered_and_session_owned() {
        use nilxone_api::bot_login::ChatSeedsEntryKind;

        let (store, _dir) = store().await;
        let a = profile(&store, "subject-a", &token('a')).await;
        let b = profile(&store, "subject-b", &token('b')).await;
        attach(&store, "0x1owner", &a).await;
        attach(&store, "0x1other", &b).await;
        exec(
            &store,
            "INSERT INTO bond_seeds VALUES ('0x1owner', 51)",
            &[],
        )
        .await;
        exec(
            &store,
            "INSERT INTO bond_seed_credits VALUES ('0x1owner', 'starter', 'once', 60, 10),
                                                 ('0x1other', 'starter', 'once', 99, 20)",
            &[],
        )
        .await;
        exec(
            &store,
            "INSERT INTO craft_receipts VALUES ('fixture-craft', '0x1owner', 'fixture', 9, 20)",
            &[],
        )
        .await;
        let owner = store
            .repository
            .restore_session(&token('a'), NOW)
            .await
            .unwrap()
            .unwrap();
        let wallet = store
            .repository
            .read_chat_wallet(&owner, NOW)
            .await
            .unwrap();
        assert_eq!(wallet.seeds, Some(51));
        assert_eq!(
            wallet
                .recent
                .iter()
                .map(|entry| (entry.kind, entry.delta))
                .collect::<Vec<_>>(),
            [
                (ChatSeedsEntryKind::Craft, -9),
                (ChatSeedsEntryKind::Starter, 60)
            ]
        );
        let expired = store
            .repository
            .read_chat_wallet(&owner, NOW + 600)
            .await
            .unwrap();
        assert!(expired.seeds.is_none());
        assert!(expired.recent.is_empty());

        for n in 0..12 {
            sqlx::query("INSERT INTO craft_receipts VALUES (?, '0x1owner', 'fixture', 1, ?)")
                .bind(format!("fixture-{n}"))
                .bind(30 + n)
                .execute(&store.repository.pool)
                .await
                .unwrap();
        }
        let wallet = store
            .repository
            .read_chat_wallet(&owner, NOW)
            .await
            .unwrap();
        assert_eq!(wallet.recent.len(), 10);
        assert_eq!(wallet.recent[0].at_ms, 41);
        assert_eq!(wallet.recent[9].at_ms, 32);
    }

    #[tokio::test]
    async fn wallet_history_shows_a_pickup_credit_and_a_sale_once_each() {
        use nilxone_api::bot_login::ChatSeedsEntryKind;

        let (store, _dir) = store().await;
        let a = profile(&store, "subject-a", &token('a')).await;
        attach(&store, "0x1owner", &a).await;
        // A find credited on pickup, a find sold later (no pickup credit), and
        // an unsold find without credit, which is no wallet movement.
        for (n, pickup, sold) in [(1, 5, None), (2, 0, Some((25, 7))), (3, 0, None)] {
            sqlx::query(
                "INSERT INTO find_attestations (attestation_id, owner_pub_dress, artifact_id,
                    artifact_sha, epoch, tier, item_id, finder, found_at_ms, longitude_e7,
                    latitude_e7, attested_at_ms, pickup_credit, sold_at_ms, sale_credit)
                 VALUES (?, '0x1owner', 'artifact', ?, 1, 1, 'item', 'bond', 10, 0, 0, ?, ?, ?, ?)",
            )
            .bind(format!("00000000-0000-0000-0000-00000000000{n}"))
            .bind(format!("{n}").repeat(64))
            .bind(10 + n)
            .bind(pickup)
            .bind(sold.map(|(at, _)| at))
            .bind(sold.map(|(_, credit)| credit))
            .execute(&store.repository.pool)
            .await
            .unwrap();
        }
        let owner = store
            .repository
            .restore_session(&token('a'), NOW)
            .await
            .unwrap()
            .unwrap();
        let wallet = store
            .repository
            .read_chat_wallet(&owner, NOW)
            .await
            .unwrap();
        assert_eq!(
            wallet
                .recent
                .iter()
                .map(|entry| (entry.kind, entry.delta, entry.at_ms))
                .collect::<Vec<_>>(),
            [
                (ChatSeedsEntryKind::Sale, 7, 25),
                (ChatSeedsEntryKind::Find, 5, 11)
            ]
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
    async fn an_inactive_live_session_resolves_for_reading_but_stays_inactive() {
        let (store, _dir) = store().await;
        let owner = profile(&store, "subject-a", &token('a')).await;
        let bond_id = attach(&store, "0x1owner", &owner).await;
        let active = || async {
            sqlx::query_scalar::<_, i64>("SELECT active FROM product_sessions WHERE token_hash = ?")
                .bind(token('a'))
                .fetch_one(&store.repository.pool)
                .await
                .unwrap()
        };
        assert_eq!(
            active().await,
            0,
            "the fixture session is not the active client"
        );
        let resolved = store
            .resolve_session_bond(&token('a'), NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved.bond_id, bond_id);
        assert!(!resolved.active, "an inactive session reports it");
        // Resolving neither activates the session nor changes its owner.
        assert_eq!(active().await, 0);

        exec(&store, "UPDATE product_sessions SET active = 1", &[]).await;
        let now_active = store
            .resolve_session_bond(&token('a'), NOW)
            .await
            .unwrap()
            .unwrap();
        assert!(now_active.active);
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
