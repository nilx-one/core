// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Single-use AIS Telegram identity evidence in 0x1's product database.
//! Only verified provider evidence is eligible to be bound to an existing
//! Core session. A successfully consumed proof is not itself authentication,
//! account linking, active-client status or an interaction completion.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};

use crate::aiaiaiai_sign_in::{
    Expectation, JsonWebKeySet, TELEGRAM_PROOF_MAX_LIFETIME_SECONDS, verify_telegram_provider_proof,
};

use super::{PublicInfoError, PublicInfoRepository};

const CHALLENGE_TTL_SECONDS: u64 = 120;

/// AIS identity evidence whose nonce and proof ID Core has consumed.
/// Session authorization requires checking this issuer and subject together.
#[derive(Eq, PartialEq)]
pub struct ConsumedTelegramProviderProof {
    issuer: String,
    pairwise_subject: String,
}

impl ConsumedTelegramProviderProof {
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    #[must_use]
    pub fn pairwise_subject(&self) -> &str {
        &self.pairwise_subject
    }
}

impl std::fmt::Debug for ConsumedTelegramProviderProof {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConsumedTelegramProviderProof")
            .finish_non_exhaustive()
    }
}

fn clock_window(now: u64) -> Result<(i64, i64), PublicInfoError> {
    let expires_at = now
        .checked_add(CHALLENGE_TTL_SECONDS)
        .ok_or(PublicInfoError::TelegramInvalidTime)?;
    Ok((
        i64::try_from(now).map_err(|_| PublicInfoError::TelegramInvalidTime)?,
        i64::try_from(expires_at).map_err(|_| PublicInfoError::TelegramInvalidTime)?,
    ))
}

impl PublicInfoRepository {
    /// Generate a fresh unpredictable request nonce. Only a backend that has
    /// already verified the Telegram transport and caller should request this.
    /// The product database stores its SHA-256 digest, never the raw nonce.
    /// The host derives the context digest from the authenticated workload,
    /// Telegram update and sender, operation, and Core session. Do not use a
    /// digest supplied by an external client.
    ///
    /// # Errors
    ///
    /// Entropy, invalid clock window or SQLite failures stop the operation.
    pub async fn start_telegram_provider_proof(
        &self,
        verified_context_digest: &[u8; 32],
        now: u64,
    ) -> Result<String, PublicInfoError> {
        let (now, expires_at) = clock_window(now)?;
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|_| PublicInfoError::TelegramEntropyUnavailable)?;
        let nonce = URL_SAFE_NO_PAD.encode(random);
        let digest = Sha256::digest(nonce.as_bytes()).to_vec();

        // A consumed ID remains reserved beyond challenge expiry. The last
        // valid consume may precede that expiry by only one second.
        sqlx::query(
            "DELETE FROM telegram_provider_proof_challenges
             WHERE expires_at <= ? AND (consumed = 0 OR expires_at <= ?)",
        )
        .bind(now)
        .bind(now.saturating_sub(i64::from(TELEGRAM_PROOF_MAX_LIFETIME_SECONDS)))
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO telegram_provider_proof_challenges
             (nonce_digest, context_digest, expires_at) VALUES (?, ?, ?)",
        )
        .bind(digest)
        .bind(verified_context_digest.as_slice())
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(nonce)
    }

    /// Verify an AIS-signed identity lookup for this nonce, then consume both
    /// its nonce and signed replay identifier atomically in the *product DB*.
    /// The host supplies its pinned issuer and trusted AIS JWKS, not values
    /// from the request or token. It must separately authenticate the Bot
    /// workload, match the returned subject to an existing Core session and
    /// enforce active-client rights before any product read or mutation.
    /// It must recompute the trusted context digest from that same request
    /// context and request the AIS proof for the authenticated sender.
    ///
    /// Returns the AIS issuer and 0x1 pairwise subject for a newly consumed proof.
    /// Unverified, expired and replayed evidence returns `None`; no client
    /// session or provider binding is opened.
    ///
    /// # Errors
    ///
    /// A storage or clock error refuses authorization without mutating Core.
    pub async fn consume_telegram_provider_proof(
        &self,
        verified_context_digest: &[u8; 32],
        nonce: &str,
        proof: &str,
        keys: &JsonWebKeySet,
        issuer: &str,
        now: u64,
    ) -> Result<Option<ConsumedTelegramProviderProof>, PublicInfoError> {
        let (now, _) = clock_window(now)?;
        let verified = match verify_telegram_provider_proof(
            proof,
            keys,
            &Expectation {
                issuer,
                client_id: "0x1",
                nonce,
            },
            u64::try_from(now).map_err(|_| PublicInfoError::TelegramInvalidTime)?,
        ) {
            Ok(verified) => verified,
            Err(_) => return Ok(None),
        };
        let nonce_digest = Sha256::digest(nonce.as_bytes()).to_vec();
        let jti_digest = Sha256::digest(verified.replay_id().as_bytes()).to_vec();

        // One SQLite lock serializes both challenges and proof IDs. Updating
        // with OR IGNORE fails closed on a reused jti (unique constraint);
        // the same challenge or proof cannot authorize another Core action.
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let changed = sqlx::query(
            "UPDATE OR IGNORE telegram_provider_proof_challenges
             SET consumed = 1, proof_jti_digest = ?
             WHERE nonce_digest = ? AND context_digest = ? AND consumed = 0 AND expires_at > ?",
        )
        .bind(jti_digest)
        .bind(nonce_digest)
        .bind(verified_context_digest.as_slice())
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            transaction.rollback().await?;
            return Ok(None);
        }
        transaction.commit().await?;
        Ok(Some(ConsumedTelegramProviderProof {
            issuer: issuer.to_owned(),
            pairwise_subject: verified.pairwise_subject().to_owned(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde_json::json;

    use super::*;

    const CONTEXT: [u8; 32] = [11_u8; 32];
    const ISSUER: &str = "https://auth.aiaiaiai.org";

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7_u8; 32])
    }

    fn keys() -> JsonWebKeySet {
        serde_json::from_value(json!({
            "keys": [{
                "kty": "OKP", "crv": "Ed25519", "use": "sig",
                "alg": "EdDSA", "kid": "key-1",
                "x": URL_SAFE_NO_PAD.encode(key().verifying_key().as_bytes())
            }]
        }))
        .unwrap()
    }

    fn proof(nonce: &str, issued_at: u64, jti: &str) -> String {
        let header = json!({
            "alg": "EdDSA", "kid": "key-1", "typ": "provider-evidence+jwt"
        });
        let claims = json!({
            "iss": ISSUER, "aud": "0x1", "azp": "0x1",
            "sub": URL_SAFE_NO_PAD.encode([17_u8; 32]),
            "provider": "telegram", "purpose": "telegram.identity.resolve",
            "nonce": nonce, "iat": issued_at, "exp": issued_at + 120,
            "jti": jti
        });
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string()),
        );
        let signature = key().sign(signed.as_bytes());
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    #[tokio::test]
    async fn callback_requires_proof_for_the_same_issuer_subject_and_login_attempt() {
        use crate::{aiaiaiai_sign_in::VerifiedSignIn, public_info::TelegramLoginCallback};

        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let callback = |actor_id, issuer: &str, subject: String| TelegramLoginCallback {
            workload_subject: "test-bot".into(),
            actor_id,
            chat_id: i64::try_from(actor_id).unwrap(),
            message_id: 42,
            identity: VerifiedSignIn {
                issuer: issuer.into(),
                subject,
                pub_dress: Some("0x1display".into()),
                auth_time: 100,
            },
        };
        let subject = URL_SAFE_NO_PAD.encode([17_u8; 32]);
        let context = callback(7, ISSUER, subject.clone()).provider_context("original-state");
        let nonce = repository
            .start_telegram_provider_proof(&context, 100)
            .await
            .unwrap();
        let signed = proof(&nonce, 100, "test-login-proof-unique");
        let other_context = callback(8, ISSUER, subject.clone()).provider_context("original-state");
        assert!(
            repository
                .consume_telegram_provider_proof(
                    &other_context,
                    &nonce,
                    &signed,
                    &keys(),
                    ISSUER,
                    101
                )
                .await
                .unwrap()
                .is_none()
        );
        let evidence = repository
            .consume_telegram_provider_proof(&context, &nonce, &signed, &keys(), ISSUER, 101)
            .await
            .unwrap()
            .unwrap();
        assert!(
            callback(7, ISSUER, URL_SAFE_NO_PAD.encode([18_u8; 32]))
                .match_provider(&evidence)
                .is_none()
        );
        assert!(
            callback(7, "https://other.example", subject.clone())
                .match_provider(&evidence)
                .is_none()
        );
        assert_eq!(
            callback(7, ISSUER, subject)
                .match_provider(&evidence)
                .unwrap()
                .callback()
                .actor_id,
            7
        );
    }

    #[tokio::test]
    async fn proof_is_single_use_across_process_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = format!("sqlite://{}", dir.path().join("proof.db").display());
        let repo = PublicInfoRepository::connect(&path).await.unwrap();
        let nonce = repo
            .start_telegram_provider_proof(&CONTEXT, 100)
            .await
            .unwrap();
        assert_eq!(nonce.len(), 43);
        let jwt = proof(&nonce, 100, "replay-id-unique-00001");
        let subject = URL_SAFE_NO_PAD.encode([17_u8; 32]);
        assert_eq!(
            repo.consume_telegram_provider_proof(&CONTEXT, &nonce, &jwt, &keys(), ISSUER, 101)
                .await
                .unwrap(),
            Some(ConsumedTelegramProviderProof {
                issuer: ISSUER.to_owned(),
                pairwise_subject: subject,
            }),
        );
        drop(repo);
        let repo = PublicInfoRepository::connect(&path).await.unwrap();
        assert_eq!(
            repo.consume_telegram_provider_proof(&CONTEXT, &nonce, &jwt, &keys(), ISSUER, 102)
                .await
                .unwrap(),
            None,
        );
        // A second nonce cannot make the same signed jti usable.
        let another = repo
            .start_telegram_provider_proof(&CONTEXT, 103)
            .await
            .unwrap();
        let replay = proof(&another, 103, "replay-id-unique-00001");
        assert_eq!(
            repo.consume_telegram_provider_proof(&CONTEXT, &another, &replay, &keys(), ISSUER, 104)
                .await
                .unwrap(),
            None,
        );
    }

    #[tokio::test]
    async fn consumed_proof_id_survives_challenge_expiry() {
        let repo = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let jti = "replay-id-unique-00001";
        let original = repo
            .start_telegram_provider_proof(&CONTEXT, 100)
            .await
            .unwrap();
        let keys = keys();
        let original_proof = proof(&original, 219, jti);
        assert!(
            repo.consume_telegram_provider_proof(
                &CONTEXT,
                &original,
                &original_proof,
                &keys,
                ISSUER,
                219
            )
            .await
            .unwrap()
            .is_some()
        );

        let next = repo
            .start_telegram_provider_proof(&CONTEXT, 220)
            .await
            .unwrap();
        let replay = proof(&next, 221, jti);
        assert_eq!(
            repo.consume_telegram_provider_proof(&CONTEXT, &next, &replay, &keys, ISSUER, 221)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn concurrent_consumes_require_the_original_verified_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = format!("sqlite://{}", dir.path().join("proof.db").display());
        let first = PublicInfoRepository::connect(&path).await.unwrap();
        let second = PublicInfoRepository::connect(&path).await.unwrap();
        let nonce = first
            .start_telegram_provider_proof(&CONTEXT, 100)
            .await
            .unwrap();
        let key_set = keys();
        let jwt = proof(&nonce, 100, "replay-concurrent-00001");
        let wrong_context = [12_u8; 32];
        assert_eq!(
            first
                .consume_telegram_provider_proof(
                    &wrong_context,
                    &nonce,
                    &jwt,
                    &key_set,
                    ISSUER,
                    101
                )
                .await
                .unwrap(),
            None
        );

        let (a, b) = tokio::join!(
            first.consume_telegram_provider_proof(&CONTEXT, &nonce, &jwt, &key_set, ISSUER, 101),
            second.consume_telegram_provider_proof(&CONTEXT, &nonce, &jwt, &key_set, ISSUER, 101)
        );
        assert_ne!(a.unwrap().is_some(), b.unwrap().is_some());
    }

    #[tokio::test]
    async fn invalid_and_expired_challenges_fail_closed_without_consuming() {
        let repo = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let nonce = repo
            .start_telegram_provider_proof(&CONTEXT, 100)
            .await
            .unwrap();
        let jwt = proof(&nonce, 100, "proof-new-000000000001");
        assert_eq!(
            repo.consume_telegram_provider_proof(
                &CONTEXT,
                &nonce,
                &jwt,
                &keys(),
                "https://evil.example",
                101
            )
            .await
            .unwrap(),
            None,
        );
        let wrong_nonce = repo
            .start_telegram_provider_proof(&CONTEXT, 101)
            .await
            .unwrap();
        assert_eq!(
            repo.consume_telegram_provider_proof(
                &CONTEXT,
                &wrong_nonce,
                &jwt,
                &keys(),
                ISSUER,
                101
            )
            .await
            .unwrap(),
            None,
        );
        assert!(
            repo.consume_telegram_provider_proof(&CONTEXT, &nonce, &jwt, &keys(), ISSUER, 102)
                .await
                .unwrap()
                .is_some()
        );

        let expiring = repo
            .start_telegram_provider_proof(&CONTEXT, 200)
            .await
            .unwrap();
        let later = proof(&expiring, 319, "proof-new-000000000002");
        assert_eq!(
            repo.consume_telegram_provider_proof(&CONTEXT, &expiring, &later, &keys(), ISSUER, 321)
                .await
                .unwrap(),
            None,
            "a still-signed proof cannot recover an expired challenge"
        );
        assert!(
            repo.start_telegram_provider_proof(&CONTEXT, u64::MAX)
                .await
                .is_err()
        );
    }
}
