// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Opt-in, single-transaction 0x1 client-subject continuity.
//!
//! This is NOT legacy Bond ownership migration (#115). It preserves an already
//! existing 0x1 product profile when the OIDC client_id changes from nilx-one
//! to 0x1. The verified AIS claim is the only way to select the old profile;
//! a public address is neither a lookup nor a fallback. The predecessor
//! remains the storage owner, including product and Bond references.
//!
//! No host invokes this until the separate 0x1 client rollout is approved.

use sqlx::Row as _;

use super::{AuthenticatedProfile, PublicInfoRepository, public_profile};
use crate::aiaiaiai_sign_in::VerifiedClientTransition;

const MAX_FRESH_AUTH_SECONDS: u64 = 600;

#[derive(Debug, thiserror::Error)]
pub enum ClientTransitionError {
    #[error("the AIS continuity proof is no longer fresh")]
    StaleProof,
    #[error("no predecessor product profile exists for this issuer and subject")]
    MissingPredecessor,
    #[error("the new client subject is already used, or the predecessor was already claimed")]
    SubjectConflict,
    #[error("the predecessor profile contains invalid product state")]
    InvalidProfile,
    #[error("product client-transition storage is unavailable")]
    Storage(#[from] sqlx::Error),
}

pub struct ClientTransitionBinding {
    /// Owner of the unchanged predecessor profile. The host can open a new
    /// product session for this owner after a successful, verified callback.
    pub owner: AuthenticatedProfile,
    /// A replay of the same signed binding does not revoke its sessions again.
    pub newly_linked: bool,
}

/// Opt-in preparation: simply opening the ordinary product repository never
/// creates the alias table or enables the new client at runtime.
#[derive(Clone)]
pub struct ClientTransitionRepository {
    product: PublicInfoRepository,
}

impl ClientTransitionRepository {
    pub async fn prepare(product: PublicInfoRepository) -> Result<Self, ClientTransitionError> {
        sqlx::raw_sql(include_str!("../../public_info_client_transition.sql"))
            .execute(&product.pool)
            .await?;
        Ok(Self { product })
    }

    /// A verified, short-lived 0x1-audience token may atomically add exactly
    /// one alias for the existing nilx-one profile. No new profile is created,
    /// and no legacy/game record is modified. Revocation of every predecessor
    /// product session and open activation request occurs in this transaction
    /// so the new client cannot become a second active writer.
    ///
    /// On a repeat with the same issuer/old/new subjects, return the original
    /// owner without touching the existing new client's sessions. A different
    /// subject on either side is an explicit conflict, never "best effort".
    pub async fn bind_verified(
        &self,
        proof: &VerifiedClientTransition,
        now: u64,
    ) -> Result<ClientTransitionBinding, ClientTransitionError> {
        let verified = proof.identity();
        if verified.auth_time > now
            || now.saturating_sub(verified.auth_time) > MAX_FRESH_AUTH_SECONDS
        {
            return Err(ClientTransitionError::StaleProof);
        }
        let now = i64::try_from(now).map_err(|_| ClientTransitionError::StaleProof)?;
        let mut tx = self.product.pool.begin_with("BEGIN IMMEDIATE").await?;

        let profile = sqlx::query(
            "SELECT profile_id, pub_dress, bond_xp, avaia_xp \
             FROM public_profiles WHERE issuer = ? AND subject = ?",
        )
        .bind(&verified.issuer)
        .bind(proof.predecessor_subject())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ClientTransitionError::MissingPredecessor)
        .and_then(|row| public_profile(row).map_err(|_| ClientTransitionError::InvalidProfile))?;

        let owner = AuthenticatedProfile {
            issuer: verified.issuer.clone(),
            subject: proof.predecessor_subject().to_owned(),
            profile: profile.clone(),
            session: None,
        };

        let current = sqlx::query(
            "SELECT predecessor_subject, profile_id FROM product_client_subject_links \
             WHERE issuer = ? AND successor_subject = ?",
        )
        .bind(&verified.issuer)
        .bind(&verified.subject)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = current {
            let predecessor: String = row.try_get("predecessor_subject")?;
            let previous_profile_id: String = row.try_get("profile_id")?;
            return if predecessor == proof.predecessor_subject()
                && previous_profile_id == profile.profile_id
            {
                Ok(ClientTransitionBinding {
                    owner,
                    newly_linked: false,
                })
            } else {
                Err(ClientTransitionError::SubjectConflict)
            };
        }
        let occupied: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public_profiles WHERE issuer = ? AND subject = ?)",
        )
        .bind(&verified.issuer)
        .bind(&verified.subject)
        .fetch_one(&mut *tx)
        .await?;
        if occupied {
            return Err(ClientTransitionError::SubjectConflict);
        }
        let inserted = sqlx::query(
            "INSERT INTO product_client_subject_links \
             (issuer, predecessor_subject, successor_subject, profile_id, linked_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&verified.issuer)
        .bind(proof.predecessor_subject())
        .bind(&verified.subject)
        .bind(&profile.profile_id)
        .bind(now)
        .execute(&mut *tx)
        .await;
        if let Err(error) = inserted {
            return match error {
                sqlx::Error::Database(database) if database.is_unique_violation() => {
                    Err(ClientTransitionError::SubjectConflict)
                }
                error => Err(ClientTransitionError::Storage(error)),
            };
        }
        // Remove all active/inactive predecessor sessions, because the old
        // OIDC client may still have access to the same profile until rollout.
        sqlx::query("DELETE FROM product_activation_requests WHERE issuer = ? AND subject = ?")
            .bind(&verified.issuer)
            .bind(proof.predecessor_subject())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM product_sessions WHERE issuer = ? AND subject = ?")
            .bind(&verified.issuer)
            .bind(proof.predecessor_subject())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE public_profiles SET session_epoch = session_epoch + 1 \
             WHERE issuer = ? AND subject = ?",
        )
        .bind(&verified.issuer)
        .bind(proof.predecessor_subject())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ClientTransitionBinding {
            owner,
            newly_linked: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde_json::json;

    use super::*;
    use crate::{
        aiaiaiai_sign_in::{
            Expectation, JsonWebKeySet, VerifiedSignIn, verify_client_transition_token,
        },
        public_info::PublicInfoError,
    };

    const ISSUER: &str = "https://auth.aiaiaiai.org";
    const NOW: u64 = 1_800_000_000;

    fn subject(byte: u8) -> String {
        URL_SAFE_NO_PAD.encode([byte; 32])
    }

    fn proof(old: &str, new: &str, now: u64) -> VerifiedClientTransition {
        let signer = SigningKey::from_bytes(&[7; 32]);
        let header = json!({"alg":"EdDSA", "kid":"transition"});
        let payload = json!({
            "iss": ISSUER,
            "aud": "0x1",
            "sub": new,
            "nonce": "proof-nonce",
            "iat": now,
            "auth_time": now,
            "exp": now + 300,
            "pub_dress": "0x0sky",
            "subject_continuity": {
                "from_client_id": "nilx-one", "sub": old,
            }
        });
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(payload.to_string())
        );
        let token = format!(
            "{signed}.{}",
            URL_SAFE_NO_PAD.encode(signer.sign(signed.as_bytes()).to_bytes())
        );
        let jwks: JsonWebKeySet = serde_json::from_value(json!({
            "keys": [{
                "kty": "OKP", "crv": "Ed25519", "kid": "transition", "use": "sig",
                "x": URL_SAFE_NO_PAD.encode(signer.verifying_key().as_bytes())
            }]
        }))
        .unwrap();
        verify_client_transition_token(
            &token,
            &jwks,
            &Expectation {
                issuer: ISSUER,
                client_id: "0x1",
                nonce: "proof-nonce",
            },
            now,
        )
        .unwrap()
    }

    async fn store() -> (ClientTransitionRepository, PublicInfoRepository) {
        let product = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let client = ClientTransitionRepository::prepare(product.clone())
            .await
            .unwrap();
        (client, product)
    }

    async fn legacy_owner(product: &PublicInfoRepository, old: &str) -> AuthenticatedProfile {
        product
            .attach(&VerifiedSignIn {
                issuer: ISSUER.to_owned(),
                subject: old.to_owned(),
                pub_dress: Some("0x0sky".to_owned()),
                auth_time: NOW,
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn exact_proof_retains_profile_xp_and_restarts_product_session_without_duplicate_writer()
    {
        let (client, product) = store().await;
        let old = subject(1);
        let next = subject(2);
        let owner = legacy_owner(&product, &old).await;
        product
            .open_session(&owner, &"a".repeat(64), "", None, NOW, NOW + 3600)
            .await
            .unwrap();
        let active = product
            .restore_session(&"a".repeat(64), NOW)
            .await
            .unwrap()
            .unwrap();
        let xp = product
            .publish_experience_carry(&active, 120, 45)
            .await
            .unwrap();
        let found = client
            .bind_verified(&proof(&old, &next, NOW), NOW)
            .await
            .unwrap();
        assert!(found.newly_linked);
        assert_eq!(
            found.owner.public_profile().profile_id,
            owner.profile.profile_id
        );
        assert_eq!(found.owner.public_profile().experience, xp.experience);
        assert!(
            product
                .restore_session(&"a".repeat(64), NOW)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            product.read_public(&xp.profile_id).await.unwrap(),
            Some(xp.clone())
        );
        assert!(
            product
                .open_session(&found.owner, &"b".repeat(64), "", None, NOW, NOW + 3600)
                .await
                .unwrap()
        );
        let resumed = product
            .restore_session(&"b".repeat(64), NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resumed.public_profile(), &xp);
        assert_eq!(
            product
                .publish_experience_carry(&resumed, 120, 45)
                .await
                .unwrap(),
            xp
        );
        // The old/new sign-in handlers must not be allowed to upsert a
        // second profile after the continuity link exists. A host must use
        // the verified successor path for every subsequent login.
        for pairwise in [&old, &next] {
            let ordinary = product
                .attach(&VerifiedSignIn {
                    issuer: ISSUER.to_owned(),
                    subject: pairwise.to_owned(),
                    pub_dress: Some("0x0sky".to_owned()),
                    auth_time: NOW + 1,
                })
                .await;
            assert!(matches!(ordinary, Err(PublicInfoError::Storage(_))));
        }
        let repeated = client
            .bind_verified(&proof(&old, &next, NOW), NOW)
            .await
            .unwrap();
        assert!(!repeated.newly_linked);
        assert!(
            product
                .restore_session(&"b".repeat(64), NOW)
                .await
                .unwrap()
                .is_some()
        );
        product.close_session(&"b".repeat(64), NOW).await.unwrap();
        assert!(
            product
                .restore_session(&"b".repeat(64), NOW)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn missing_predecessor_and_conflicting_successors_cannot_create_or_replace_a_profile() {
        let (client, product) = store().await;
        let old = subject(1);
        let next = subject(2);
        let absent = client.bind_verified(&proof(&old, &next, NOW), NOW).await;
        assert!(matches!(
            absent,
            Err(ClientTransitionError::MissingPredecessor)
        ));
        let owner = legacy_owner(&product, &old).await;
        client
            .bind_verified(&proof(&old, &next, NOW), NOW)
            .await
            .unwrap();
        assert!(matches!(
            client
                .bind_verified(&proof(&old, &subject(3), NOW), NOW)
                .await,
            Err(ClientTransitionError::SubjectConflict)
        ));
        let alien = subject(4);
        legacy_owner(&product, &alien).await;
        assert!(matches!(
            client.bind_verified(&proof(&alien, &next, NOW), NOW).await,
            Err(ClientTransitionError::SubjectConflict)
        ));
        assert_eq!(
            product
                .read_public(&owner.profile.profile_id)
                .await
                .unwrap(),
            Some(owner.profile.clone())
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM product_client_subject_links")
            .fetch_one(&product.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn stale_proof_or_preexisting_successor_is_rejected_without_state_change() {
        let (client, product) = store().await;
        let old = subject(1);
        let next = subject(2);
        let owner = legacy_owner(&product, &old).await;
        assert!(matches!(
            client
                .bind_verified(&proof(&old, &next, NOW), NOW + 601)
                .await,
            Err(ClientTransitionError::StaleProof)
        ));
        let other = legacy_owner(&product, &next).await;
        assert!(matches!(
            client.bind_verified(&proof(&old, &next, NOW), NOW).await,
            Err(ClientTransitionError::SubjectConflict)
        ));
        assert_ne!(owner.profile.profile_id, other.profile.profile_id);
        assert_eq!(
            product
                .read_public(&owner.profile.profile_id)
                .await
                .unwrap(),
            Some(owner.profile)
        );
    }

    #[tokio::test]
    async fn old_bond_avaia_seeds_subscription_and_binding_survive_successor_login() {
        use crate::{public_info::LegacyBondMigrationRepository, repository::IdentityRepository};

        let directory = tempfile::tempdir().unwrap();
        let database_url = format!(
            "sqlite://{}",
            directory.path().join("product-and-legacy.sqlite").display()
        );
        let _legacy = IdentityRepository::connect(&database_url).await.unwrap();
        let product = PublicInfoRepository::connect(&database_url).await.unwrap();
        let _migration = LegacyBondMigrationRepository::prepare(product.clone())
            .await
            .unwrap();
        let transition = ClientTransitionRepository::prepare(product.clone())
            .await
            .unwrap();
        sqlx::query("INSERT INTO identities (pub_dress, identity_kind) VALUES (?, 'human')")
            .bind("0x0sky")
            .execute(&product.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO identities (pub_dress, identity_kind, owner_pub_dress) \
             VALUES ('0x0skai', 'avaia', '0x0sky')",
        )
        .execute(&product.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE avaia_configuration SET configuration_state = 'configured' \
             WHERE owner_pub_dress = '0x0sky'",
        )
        .execute(&product.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO bond_seeds (owner_pub_dress, balance) VALUES ('0x0sky', 1337)")
            .execute(&product.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO bond_subscription_clocks (owner_pub_dress, clock_json) \
             VALUES ('0x0sky', '{}')",
        )
        .execute(&product.pool)
        .await
        .unwrap();

        let old = subject(1);
        let next = subject(2);
        let owner = legacy_owner(&product, &old).await;
        sqlx::query("INSERT INTO legacy_bond_keys (legacy_pub_dress) VALUES ('0x0sky')")
            .execute(&product.pool)
            .await
            .unwrap();
        let bond_id: String = sqlx::query_scalar(
            "SELECT bond_id FROM legacy_bond_keys WHERE legacy_pub_dress = '0x0sky'",
        )
        .fetch_one(&product.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO legacy_bond_bindings (bond_id, profile_id, linked_at) \
             VALUES (?, ?, ?)",
        )
        .bind(&bond_id)
        .bind(&owner.profile.profile_id)
        .bind(NOW as i64)
        .execute(&product.pool)
        .await
        .unwrap();
        let linked = transition
            .bind_verified(&proof(&old, &next, NOW), NOW)
            .await
            .unwrap();
        assert!(linked.newly_linked);
        assert_eq!(linked.owner.profile.profile_id, owner.profile.profile_id);

        let saved_bond_id: String = sqlx::query_scalar(
            "SELECT k.bond_id FROM legacy_bond_keys k \
             JOIN legacy_bond_bindings b ON b.bond_id = k.bond_id \
             WHERE b.profile_id = ?",
        )
        .bind(&owner.profile.profile_id)
        .fetch_one(&product.pool)
        .await
        .unwrap();
        assert_eq!(saved_bond_id, bond_id);
        let avaia: (String, String) = sqlx::query_as(
            "SELECT avaia_pub_dress, configuration_state FROM avaia_configuration \
             WHERE owner_pub_dress = '0x0sky'",
        )
        .fetch_one(&product.pool)
        .await
        .unwrap();
        assert_eq!(avaia, ("0x0skai".to_owned(), "configured".to_owned()));
        let balance: i64 = sqlx::query_scalar(
            "SELECT balance FROM bond_seeds WHERE owner_pub_dress = '0x0sky'",
        )
        .fetch_one(&product.pool)
        .await
        .unwrap();
        assert_eq!(balance, 1337);
        let clock: String = sqlx::query_scalar(
            "SELECT clock_json FROM bond_subscription_clocks WHERE owner_pub_dress = '0x0sky'",
        )
        .fetch_one(&product.pool)
        .await
        .unwrap();
        assert_eq!(clock, "{}");
        let existing_humans: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM identities WHERE pub_dress = '0x0sky' AND identity_kind = 'human'",
        )
        .fetch_one(&product.pool)
        .await
        .unwrap();
        assert_eq!(existing_humans, 1);
        // Browser/device .bnd inventory is outside this database. Its exact
        // before/after bytes require the separate Web/Telegram/Discord E2E.
    }

    #[tokio::test]
    async fn migration_does_not_break_existing_client_write_when_not_explicitly_invoked() {
        let (_client, product) = store().await;
        let old = subject(1);
        let owner = legacy_owner(&product, &old).await;
        product
            .open_session(&owner, &"a".repeat(64), "", None, NOW, NOW + 3600)
            .await
            .unwrap();
        let active = product
            .restore_session(&"a".repeat(64), NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            product
                .publish_experience_carry(&active, 1, 2)
                .await
                .unwrap()
                .experience
                .bond_xp,
            1
        );
        assert!(matches!(
            product.publish_experience_carry(&owner, 1, 2).await,
            Err(PublicInfoError::SessionInactive)
        ));
    }
}
