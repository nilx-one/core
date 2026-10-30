// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use std::sync::Arc;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization as _;

const ARGON2_MEMORY_KIB: u32 = 19 * 1024;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;
const PASSWORD_HASH_VERSION: i64 = 1;
const MIN_SECRET_BYTES: usize = 32;
const MIN_PASSWORD_CODE_POINTS: usize = 8;
const MAX_PASSWORD_CODE_POINTS: usize = 128;

const COMPROMISED_PASSWORDS: &[&str] = &[
    "12345678",
    "123456789",
    "password",
    "password1",
    "qwerty123",
    "iloveyou",
    "admin123",
    "123456789012345",
    "111111111111111",
    "passwordpassword",
    "password123456",
    "qwertyuiopasdfgh",
    "qwerty123456789",
    "letmeinletmein",
    "iloveyouiloveyou",
    "adminadminadmin",
    "correct horse battery staple",
];

#[derive(Clone)]
pub struct NativeAuthConfig {
    auth_secret: Arc<[u8]>,
    password_pepper: Arc<[u8]>,
    pub session_ttl_seconds: u64,
    pub remembered_bond_ttl_seconds: u64,
    pub registration_challenge_ttl_seconds: u64,
    /// How long an activation request waits for the active client to
    /// answer before the live-device objection window opens instead. See
    /// `nilx-one/0x1` `documents/17-protocol-constants-and-open-questions.md`,
    /// "Activation request TTL".
    pub activation_request_ttl_seconds: u64,
    /// The live-device objection window itself, shared by device recovery
    /// and Single Active Client activation. See the same document,
    /// "Live-device objection window".
    pub objection_window_seconds: u64,
}

impl NativeAuthConfig {
    pub fn new(
        auth_secret: impl AsRef<[u8]>,
        password_pepper: impl AsRef<[u8]>,
    ) -> Result<Self, NativeAuthConfigError> {
        let auth_secret = auth_secret.as_ref();
        let password_pepper = password_pepper.as_ref();
        if auth_secret.len() < MIN_SECRET_BYTES {
            return Err(NativeAuthConfigError::AuthSecretTooShort);
        }
        if password_pepper.len() < MIN_SECRET_BYTES {
            return Err(NativeAuthConfigError::PasswordPepperTooShort);
        }

        Ok(Self {
            auth_secret: Arc::from(auth_secret),
            password_pepper: Arc::from(password_pepper),
            session_ttl_seconds: 60 * 60 * 24 * 30,
            remembered_bond_ttl_seconds: 60 * 60 * 24 * 180,
            registration_challenge_ttl_seconds: 60 * 15,
            activation_request_ttl_seconds: 60 * 2,
            objection_window_seconds: 60 * 60 * 24,
        })
    }

    pub fn password_engine(&self) -> PasswordEngine {
        PasswordEngine {
            pepper: Arc::clone(&self.password_pepper),
        }
    }

    pub fn secret_digester(&self) -> SecretDigester {
        SecretDigester {
            secret: Arc::clone(&self.auth_secret),
        }
    }

    pub fn remembered_bond_signer(&self) -> RememberedBondSigner {
        RememberedBondSigner {
            secret: Arc::clone(&self.auth_secret),
        }
    }
}

#[derive(Debug, Error)]
pub enum NativeAuthConfigError {
    #[error("NATIVE_AUTH_SECRET must contain at least 32 bytes")]
    AuthSecretTooShort,
    #[error("PASSWORD_PEPPER must contain at least 32 bytes")]
    PasswordPepperTooShort,
}

#[derive(Clone)]
pub struct PasswordEngine {
    pepper: Arc<[u8]>,
}

impl PasswordEngine {
    pub fn validate(&self, password: &str) -> Result<String, PasswordPolicyError> {
        let normalized = password.nfc().collect::<String>();
        let code_points = normalized.chars().count();
        if !(MIN_PASSWORD_CODE_POINTS..=MAX_PASSWORD_CODE_POINTS).contains(&code_points) {
            return Err(PasswordPolicyError::InvalidLength);
        }
        if normalized.trim() != normalized
            || normalized.chars().any(|character| {
                character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
            })
        {
            return Err(PasswordPolicyError::InvalidLength);
        }
        if COMPROMISED_PASSWORDS.contains(&normalized.as_str()) {
            return Err(PasswordPolicyError::Compromised);
        }
        Ok(normalized)
    }

    pub fn hash(&self, normalized_password: &str) -> Result<String, PasswordHashError> {
        let mut salt = [0_u8; 16];
        getrandom::fill(&mut salt).map_err(|_| PasswordHashError::EntropyUnavailable)?;
        let salt = SaltString::encode_b64(&salt).map_err(|_| PasswordHashError::Encoding)?;
        self.argon2()?
            .hash_password(normalized_password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| PasswordHashError::Hashing)
    }

    pub fn verify(&self, password: &str, encoded_hash: &str) -> bool {
        let normalized = password.nfc().collect::<String>();
        let Ok(hash) = PasswordHash::new(encoded_hash) else {
            return false;
        };
        self.argon2()
            .and_then(|argon2| {
                argon2
                    .verify_password(normalized.as_bytes(), &hash)
                    .map_err(|_| PasswordHashError::Verification)
            })
            .is_ok()
    }

    pub const fn hash_version(&self) -> i64 {
        PASSWORD_HASH_VERSION
    }

    fn argon2(&self) -> Result<Argon2<'_>, PasswordHashError> {
        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            Some(32),
        )
        .map_err(|_| PasswordHashError::Parameters)?;
        Argon2::new_with_secret(&self.pepper, Algorithm::Argon2id, Version::V0x13, params)
            .map_err(|_| PasswordHashError::Parameters)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordPolicyError {
    InvalidLength,
    Compromised,
}

#[derive(Debug, Error)]
pub enum PasswordHashError {
    #[error("secure entropy is unavailable")]
    EntropyUnavailable,
    #[error("password hash parameters are invalid")]
    Parameters,
    #[error("password hash encoding failed")]
    Encoding,
    #[error("password hashing failed")]
    Hashing,
    #[error("password verification failed")]
    Verification,
}

pub struct TokenFactory;

impl TokenFactory {
    pub fn session() -> Result<String, TokenError> {
        Self::random("0x1s_", 32)
    }

    pub fn registration_challenge() -> Result<String, TokenError> {
        Self::random("0x1c_", 32)
    }

    pub fn recovery_key() -> Result<String, TokenError> {
        Self::random("0x1-rk-", 32)
    }

    pub fn opaque_hint_id() -> Result<String, TokenError> {
        Self::random("", 16)
    }

    fn random(prefix: &str, bytes: usize) -> Result<String, TokenError> {
        let mut value = vec![0_u8; bytes];
        getrandom::fill(&mut value).map_err(|_| TokenError::EntropyUnavailable)?;
        Ok(format!("{prefix}{}", URL_SAFE_NO_PAD.encode(value)))
    }
}

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("secure entropy is unavailable")]
    EntropyUnavailable,
}

#[derive(Clone)]
pub struct SecretDigester {
    secret: Arc<[u8]>,
}

impl SecretDigester {
    pub fn digest(&self, domain: &str, value: &str) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret)
            .expect("HMAC accepts keys of every length");
        mac.update(domain.as_bytes());
        mac.update(&[0]);
        mac.update(value.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }
}

#[derive(Clone)]
pub struct RememberedBondSigner {
    secret: Arc<[u8]>,
}

impl RememberedBondSigner {
    pub fn issue(&self, pub_dress: &str, expires_at: u64) -> Result<String, TokenError> {
        let hint_id = TokenFactory::opaque_hint_id()?;
        let encoded_pub_dress = URL_SAFE_NO_PAD.encode(pub_dress.as_bytes());
        let payload = format!("v1.{expires_at}.{hint_id}.{encoded_pub_dress}");
        let signature = self.signature(&payload);
        Ok(format!("{payload}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    pub fn verify(&self, value: &str, now: u64) -> Option<String> {
        let fields = value.split('.').collect::<Vec<_>>();
        let [
            "v1",
            expires_at,
            hint_id,
            encoded_pub_dress,
            encoded_signature,
        ] = fields.as_slice()
        else {
            return None;
        };
        if hint_id.is_empty() || expires_at.parse::<u64>().ok()? <= now {
            return None;
        }
        let payload = format!("v1.{expires_at}.{hint_id}.{encoded_pub_dress}");
        let signature = URL_SAFE_NO_PAD.decode(encoded_signature).ok()?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).ok()?;
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).ok()?;
        String::from_utf8(URL_SAFE_NO_PAD.decode(encoded_pub_dress).ok()?).ok()
    }

    fn signature(&self, payload: &str) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret)
            .expect("HMAC accepts keys of every length");
        mac.update(payload.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeAuthConfig, PasswordPolicyError, RememberedBondSigner, TokenFactory};

    fn config() -> NativeAuthConfig {
        NativeAuthConfig::new(
            "test-auth-secret-that-is-at-least-thirty-two-bytes",
            "test-password-pepper-that-is-at-least-thirty-two-bytes",
        )
        .expect("valid test configuration")
    }

    #[test]
    fn password_policy_normalizes_before_hashing_and_verification() {
        let engine = config().password_engine();
        let decomposed = "a secure phrase e\u{301} for 0x1";
        let normalized = engine.validate(decomposed).expect("valid password");
        let hash = engine.hash(&normalized).expect("password hash");

        assert!(engine.verify("a secure phrase é for 0x1", &hash));
        assert_eq!(
            engine.validate("passwordpassword"),
            Err(PasswordPolicyError::Compromised)
        );
    }

    #[test]
    fn password_policy_accepts_eight_code_points_and_rejects_seven() {
        let engine = config().password_engine();
        assert_eq!(
            engine.validate("1234567"),
            Err(PasswordPolicyError::InvalidLength)
        );
        assert_eq!(engine.validate("eight-ok").as_deref(), Ok("eight-ok"));
        assert_eq!(
            engine.validate("password"),
            Err(PasswordPolicyError::Compromised)
        );
    }

    #[test]
    fn password_policy_rejects_boundary_whitespace_and_control_characters() {
        let engine = config().password_engine();
        for password in ["        ", "        12345678", "1234568\n78"] {
            assert_eq!(
                engine.validate(password),
                Err(PasswordPolicyError::InvalidLength),
                "password should be rejected: {password:?}"
            );
        }
        assert_eq!(engine.validate("four four").as_deref(), Ok("four four"));
    }

    #[test]
    fn remembered_hint_is_public_identity_only_and_tamper_evident() {
        let signer: RememberedBondSigner = config().remembered_bond_signer();
        let hint = signer.issue("0x0Sky", 500).expect("signed hint");
        assert_eq!(signer.verify(&hint, 499).as_deref(), Some("0x0Sky"));
        assert_eq!(signer.verify(&hint, 500), None);
        assert_eq!(signer.verify(&format!("{hint}x"), 499), None);
    }

    #[test]
    fn secret_tokens_use_distinct_explicit_prefixes() {
        assert!(
            TokenFactory::session()
                .expect("session")
                .starts_with("0x1s_")
        );
        assert!(
            TokenFactory::registration_challenge()
                .expect("challenge")
                .starts_with("0x1c_")
        );
        assert!(
            TokenFactory::recovery_key()
                .expect("recovery")
                .starts_with("0x1-rk-")
        );
    }
}
