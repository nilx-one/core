// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! 0x1's product data attached to an AIS identity. This store has its own
//! database: no passwords, providers, issuer keys or private AIS identifiers.
//! A public profile is a projection, never evidence of a BondChain interaction.

use std::{str::FromStr, time::Duration};

use serde::Serialize;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::{
    PubDress,
    aiaiaiai_sign_in::{Callback, SignInError, SignInFailure, SignInFlow, Started, VerifiedSignIn},
};

const MAX_CARRY_XP: u64 = 1_000_000_000;

#[derive(Debug, thiserror::Error)]
pub enum PublicInfoError {
    #[error("0x1 public information requires the nilx-one OIDC client")]
    WrongClient,
    #[error("the verified identity has an invalid public address")]
    InvalidPublicAddress,
    #[error("the experience carry exceeds the publication limit")]
    InvalidExperience,
    #[error("the authenticated product profile no longer exists")]
    ProfileGone,
    #[error("public information storage is unavailable")]
    Storage(#[from] sqlx::Error),
}

/// The explicit public allowlist. Never serialize a database row or OIDC
/// result as a profile: neither `issuer` nor pairwise `sub` is public data.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicProfile {
    pub profile_id: String,
    pub pub_dress: Option<String>,
    pub experience: PublicExperience,
}

/// Totals carried from the owner's local .bnd, labelled as owner assertions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicExperience {
    pub authority: &'static str,
    pub bond_xp: u64,
    pub avaia_xp: u64,
}

/// An in-process authorization result, not a browser token or a serializable
/// public DTO. Only a successful AIS callback can construct one.
#[derive(Clone)]
pub struct AuthenticatedProfile {
    issuer: String,
    subject: String,
    profile: PublicProfile,
}

impl AuthenticatedProfile {
    pub fn public_profile(&self) -> &PublicProfile {
        &self.profile
    }
}

pub struct PublicInfoSignedIn {
    pub owner: AuthenticatedProfile,
    pub return_path: String,
}

pub struct PublicInfoFinished {
    pub outcome: Result<PublicInfoSignedIn, SignInFailure>,
    pub clear_cookie: String,
}

/// Server-side product storage. Connect it to a separate 0x1 database, not
/// the AIS database. There is no address-based account attachment API.
#[derive(Clone)]
pub struct PublicInfoRepository {
    pool: SqlitePool,
}

impl PublicInfoRepository {
    pub async fn connect(database_url: &str) -> Result<Self, PublicInfoError> {
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(include_str!("../public_info_schema.sql"))
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }

    /// Discovery only. Possessing this public id grants no write access.
    pub async fn read_public(
        &self,
        profile_id: &str,
    ) -> Result<Option<PublicProfile>, PublicInfoError> {
        let row = sqlx::query("SELECT profile_id, pub_dress, bond_xp, avaia_xp FROM public_profiles WHERE profile_id = ?")
            .bind(profile_id).fetch_optional(&self.pool).await?;
        row.map(public_profile).transpose()
    }

    /// Publishes only the existing local .bnd carry semantics: monotonic
    /// owner-asserted totals. This grants no awards, seeds, subscriptions or
    /// BondChain facts. The HTTP adapter must authenticate the session and
    /// enforce CSRF before reusing its server-held authorization result.
    pub async fn publish_experience_carry(
        &self,
        owner: &AuthenticatedProfile,
        bond_xp: u64,
        avaia_xp: u64,
    ) -> Result<PublicProfile, PublicInfoError> {
        if bond_xp > MAX_CARRY_XP || avaia_xp > MAX_CARRY_XP {
            return Err(PublicInfoError::InvalidExperience);
        }
        let row = sqlx::query(
            "UPDATE public_profiles SET bond_xp = MAX(bond_xp, ?), avaia_xp = MAX(avaia_xp, ?) \
             WHERE issuer = ? AND subject = ? AND profile_id = ? \
             RETURNING profile_id, pub_dress, bond_xp, avaia_xp",
        )
        .bind(i64::try_from(bond_xp).map_err(|_| PublicInfoError::InvalidExperience)?)
        .bind(i64::try_from(avaia_xp).map_err(|_| PublicInfoError::InvalidExperience)?)
        .bind(&owner.issuer)
        .bind(&owner.subject)
        .bind(&owner.profile.profile_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PublicInfoError::ProfileGone)?;
        public_profile(row)
    }

    // This function is deliberately private. The service calls it only with
    // the result of SignInFlow's callback verification for client nilx-one.
    async fn attach(
        &self,
        identity: &VerifiedSignIn,
    ) -> Result<AuthenticatedProfile, PublicInfoError> {
        // An absent display claim preserves the stored snapshot. A supplied
        // but malformed claim is an invalid issuer response: fail closed,
        // even for a known subject or a stale callback, before changing a row.
        if let Some(address) = &identity.pub_dress {
            PubDress::from_str(address).map_err(|_| PublicInfoError::InvalidPublicAddress)?;
        }
        let auth_time = i64::try_from(identity.auth_time)
            .map_err(|_| sqlx::Error::Protocol("auth_time overflow".into()))?;
        // Atomic upsert keys only by (issuer, subject). An address claim can
        // never discover or acquire a different subject's profile. Delayed
        // callbacks cannot roll a newer display snapshot back.
        let row = sqlx::query(
            "INSERT INTO public_profiles (issuer, subject, pub_dress, last_auth_time) VALUES (?, ?, ?, ?) \
             ON CONFLICT (issuer, subject) DO UPDATE SET \
               pub_dress = CASE WHEN excluded.last_auth_time > last_auth_time \
                  THEN COALESCE(excluded.pub_dress, pub_dress) ELSE pub_dress END, \
               last_auth_time = MAX(last_auth_time, excluded.last_auth_time) \
             RETURNING profile_id, pub_dress, bond_xp, avaia_xp"
        ).bind(&identity.issuer).bind(&identity.subject).bind(&identity.pub_dress).bind(auth_time)
            .fetch_one(&self.pool).await?;
        Ok(AuthenticatedProfile {
            issuer: identity.issuer.clone(),
            subject: identity.subject.clone(),
            profile: public_profile(row)?,
        })
    }
}

fn public_profile(row: sqlx::sqlite::SqliteRow) -> Result<PublicProfile, PublicInfoError> {
    let xp = |column| -> Result<u64, sqlx::Error> {
        u64::try_from(row.try_get::<i64, _>(column)?)
            .map_err(|_| sqlx::Error::Protocol("negative public experience".into()))
    };
    Ok(PublicProfile {
        profile_id: row.try_get("profile_id")?,
        pub_dress: row.try_get("pub_dress")?,
        experience: PublicExperience {
            authority: crate::repository::EXPERIENCE_AUTHORITY,
            bond_xp: xp("bond_xp")?,
            avaia_xp: xp("avaia_xp")?,
        },
    })
}

/// Composes the existing OIDC verifier with 0x1 storage; it never registers
/// another identity or reads credentials from the old identity database.
#[derive(Clone)]
pub struct PublicInfoService {
    flow: SignInFlow,
    repository: PublicInfoRepository,
}

impl PublicInfoService {
    pub fn new(
        flow: SignInFlow,
        repository: PublicInfoRepository,
    ) -> Result<Self, PublicInfoError> {
        if flow.client_id() != "nilx-one" {
            return Err(PublicInfoError::WrongClient);
        }
        Ok(Self { flow, repository })
    }

    pub fn start(&self, return_path: Option<&str>, now: u64) -> Result<Started, SignInError> {
        self.flow.start(return_path, now)
    }

    pub async fn finish(
        &self,
        cookie_header: Option<&str>,
        callback: Callback<'_>,
        now: u64,
    ) -> PublicInfoFinished {
        let verified = self
            .flow
            .verify_callback(cookie_header, callback, now)
            .await;
        let outcome = match verified.outcome {
            Ok(verified) => self
                .repository
                .attach(verified.identity())
                .await
                .map(|owner| PublicInfoSignedIn {
                    owner,
                    return_path: verified.return_path().to_owned(),
                })
                .map_err(|error| match error {
                    PublicInfoError::InvalidPublicAddress => SignInFailure::Invalid,
                    _ => SignInFailure::Unavailable,
                }),
            Err(error) => Err(error),
        };
        PublicInfoFinished {
            outcome,
            clear_cookie: verified.clear_cookie,
        }
    }
}

#[cfg(test)]
mod tests;
