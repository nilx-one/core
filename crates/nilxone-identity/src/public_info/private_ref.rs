// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use super::{AuthenticatedProfile, PublicInfoRepository};

const PRISM_AUDIENCE: &str = "prism-hub";

/// Opaque subject reference for Prism. Only serialize it in the authorized
/// server-to-server envelope, never as part of a public profile.
#[derive(Clone, PartialEq, Eq)]
pub struct BondPrivateRef(String);

impl BondPrivateRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PrivateRefError {
    #[error("an active, unexpired 0x1 session is required")]
    SessionInactive,
    #[error("unable to generate a private reference")]
    EntropyUnavailable,
    #[error("private reference storage is unavailable")]
    Storage(#[from] sqlx::Error),
}

impl PublicInfoRepository {
    /// Issue or reuse the active owner's Prism-scoped reference. The caller
    /// must already have authorized the specific operation that needs it.
    /// The reference itself neither authorizes an operation nor signs it.
    pub async fn prism_private_ref(
        &self,
        owner: &AuthenticatedProfile,
        now: u64,
    ) -> Result<BondPrivateRef, PrivateRefError> {
        let session = owner
            .session
            .as_deref()
            .ok_or(PrivateRefError::SessionInactive)?;
        let now = i64::try_from(now).map_err(|_| PrivateRefError::SessionInactive)?;
        let mut entropy = [0_u8; 32];
        getrandom::fill(&mut entropy).map_err(|_| PrivateRefError::EntropyUnavailable)?;
        let candidate = URL_SAFE_NO_PAD.encode(entropy);

        // Resolve the same profile and live active session in the statement
        // that writes. A retry or concurrent issuer gets the persisted value.
        let reference: Option<String> = sqlx::query_scalar(
            "INSERT INTO bond_private_refs (profile_id, audience, priv_id) \
             SELECT p.profile_id, ?, ? FROM product_sessions s JOIN public_profiles p \
               ON p.issuer = s.issuer AND p.subject = s.subject AND p.profile_id = s.profile_id \
             WHERE s.token_hash = ? AND p.issuer = ? AND p.subject = ? AND p.profile_id = ? \
               AND s.active = 1 AND s.expires_at > ? \
             ON CONFLICT (profile_id, audience) DO UPDATE SET priv_id = bond_private_refs.priv_id \
             RETURNING priv_id",
        )
        .bind(PRISM_AUDIENCE)
        .bind(candidate)
        .bind(session)
        .bind(&owner.issuer)
        .bind(&owner.subject)
        .bind(&owner.profile.profile_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        reference
            .map(BondPrivateRef)
            .ok_or(PrivateRefError::SessionInactive)
    }
}
