// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use chacha20poly1305::{
    ChaCha20Poly1305, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use getrandom::fill;

const VERSION: u8 = 1;
const NONCE_BYTES: usize = 12;

#[derive(Clone)]
pub struct ProviderSecretCipher {
    cipher: ChaCha20Poly1305,
}

impl ProviderSecretCipher {
    pub fn from_hex_key(value: &str) -> Result<Self, ProviderSecretError> {
        let bytes = decode_hex(value)?;
        if bytes.len() != 32 {
            return Err(ProviderSecretError::InvalidKey);
        }
        let key = Key::from_slice(&bytes);
        Ok(Self {
            cipher: ChaCha20Poly1305::new(key),
        })
    }

    pub fn seal(&self, plaintext: &str, aad: &[u8]) -> Result<Vec<u8>, ProviderSecretError> {
        let mut nonce_bytes = [0_u8; NONCE_BYTES];
        fill(&mut nonce_bytes).map_err(|_| ProviderSecretError::RandomnessUnavailable)?;
        let ciphertext = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad,
                },
            )
            .map_err(|_| ProviderSecretError::EncryptionFailed)?;
        let mut sealed = Vec::with_capacity(1 + NONCE_BYTES + ciphertext.len());
        sealed.push(VERSION);
        sealed.extend_from_slice(&nonce_bytes);
        sealed.extend_from_slice(&ciphertext);
        Ok(sealed)
    }

    pub fn open(&self, sealed: &[u8], aad: &[u8]) -> Result<String, ProviderSecretError> {
        if sealed.len() <= 1 + NONCE_BYTES || sealed[0] != VERSION {
            return Err(ProviderSecretError::InvalidCiphertext);
        }
        let nonce = Nonce::from_slice(&sealed[1..1 + NONCE_BYTES]);
        let plaintext = self
            .cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &sealed[1 + NONCE_BYTES..],
                    aad,
                },
            )
            .map_err(|_| ProviderSecretError::InvalidCiphertext)?;
        String::from_utf8(plaintext).map_err(|_| ProviderSecretError::InvalidCiphertext)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderSecretError {
    #[error("provider secret key must be exactly 32 bytes encoded as 64 hexadecimal characters")]
    InvalidKey,
    #[error("secure randomness is unavailable")]
    RandomnessUnavailable,
    #[error("provider secret encryption failed")]
    EncryptionFailed,
    #[error("provider secret ciphertext is invalid")]
    InvalidCiphertext,
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ProviderSecretError> {
    if !value.len().is_multiple_of(2) || !value.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(ProviderSecretError::InvalidKey);
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| ProviderSecretError::InvalidKey)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ProviderSecretCipher;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn seals_without_exposing_plaintext_and_uses_fresh_nonces() {
        let cipher = ProviderSecretCipher::from_hex_key(KEY).expect("cipher");
        let first = cipher
            .seal("gho_secret-token", b"github:0x0sky:42")
            .expect("seal");
        let second = cipher
            .seal("gho_secret-token", b"github:0x0sky:42")
            .expect("seal");
        assert_ne!(first, second);
        assert!(
            !first
                .windows("gho_secret-token".len())
                .any(|window| window == b"gho_secret-token")
        );
        assert_eq!(
            cipher.open(&first, b"github:0x0sky:42").expect("open"),
            "gho_secret-token"
        );
    }

    #[test]
    fn row_bound_aad_prevents_cross_record_decryption() {
        let cipher = ProviderSecretCipher::from_hex_key(KEY).expect("cipher");
        let sealed = cipher.seal("token", b"github:0x0sky:42").expect("seal");
        assert!(cipher.open(&sealed, b"github:0x1sky:42").is_err());
        assert!(cipher.open(&sealed, b"github:0x0sky:43").is_err());
    }
}
