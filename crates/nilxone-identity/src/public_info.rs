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

mod activation;

pub use activation::ActivationError;

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
    #[error("the product session cannot be opened with this token, label or lifetime")]
    InvalidSession,
    #[error("only the active client of a profile may change it")]
    SessionInactive,
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
/// public DTO. Only a successful AIS callback or a restored session can
/// construct one. It identifies its owner for reads. A write also needs the
/// session it was restored from to be the active one, which the write itself
/// checks in SQL; the result of a callback has no session yet and cannot write.
#[derive(Clone)]
pub struct AuthenticatedProfile {
    issuer: String,
    subject: String,
    profile: PublicProfile,
    session: Option<String>,
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
            // A handoff's data is deleted when its request ends; overwrite
            // the freed pages instead of leaving the bytes in the file.
            .pragma("secure_delete", "on")
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(include_str!("../public_info_schema.sql"))
            .execute(&pool)
            .await?;
        add_session_activity(&pool).await?;
        sqlx::raw_sql(include_str!("../public_info_activation.sql"))
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
    ///
    /// Only the active client writes. `owner` must come from
    /// `restore_session`, and that session must be the active one in the
    /// same statement that writes; otherwise `SessionInactive`.
    pub async fn publish_experience_carry(
        &self,
        owner: &AuthenticatedProfile,
        bond_xp: u64,
        avaia_xp: u64,
    ) -> Result<PublicProfile, PublicInfoError> {
        if bond_xp > MAX_CARRY_XP || avaia_xp > MAX_CARRY_XP {
            return Err(PublicInfoError::InvalidExperience);
        }
        let session = owner
            .session
            .as_deref()
            .ok_or(PublicInfoError::SessionInactive)?;
        let row = sqlx::query(
            "UPDATE public_profiles SET bond_xp = MAX(bond_xp, ?), avaia_xp = MAX(avaia_xp, ?) \
             WHERE issuer = ? AND subject = ? AND profile_id = ? \
               AND EXISTS (SELECT 1 FROM product_sessions s \
                  WHERE s.token_hash = ? AND s.issuer = public_profiles.issuer \
                    AND s.subject = public_profiles.subject AND s.active = 1) \
             RETURNING profile_id, pub_dress, bond_xp, avaia_xp",
        )
        .bind(i64::try_from(bond_xp).map_err(|_| PublicInfoError::InvalidExperience)?)
        .bind(i64::try_from(avaia_xp).map_err(|_| PublicInfoError::InvalidExperience)?)
        .bind(&owner.issuer)
        .bind(&owner.subject)
        .bind(&owner.profile.profile_id)
        .bind(session)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            return public_profile(row);
        }
        let profile_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM public_profiles \
             WHERE issuer = ? AND subject = ? AND profile_id = ?)",
        )
        .bind(&owner.issuer)
        .bind(&owner.subject)
        .bind(&owner.profile.profile_id)
        .fetch_one(&self.pool)
        .await?;
        Err(if profile_exists {
            PublicInfoError::SessionInactive
        } else {
            PublicInfoError::ProfileGone
        })
    }

    /// Opens a product session for the owner a verified callback just
    /// returned. `token_hash` is the host's digest of the session token; the
    /// token itself never reaches this store. Expired rows of the same owner
    /// are dropped here, so the table needs no separate sweeper.
    ///
    /// Returns whether the session is the active client. It is active only
    /// when the owner has no other live active session; otherwise it starts
    /// inactive and only the active client can hand over to it
    /// (`docs/client-activation.md`). Signing in never takes activity away.
    ///
    /// `client_label` names the device and host to the owner's other
    /// clients, such as `iPhone · Safari`. `supersede_token_hash` is the
    /// session this same browser presented: it is closed in the same
    /// transaction and before the check, so signing in again on the active
    /// client keeps it active.
    pub async fn open_session(
        &self,
        owner: &AuthenticatedProfile,
        token_hash: &str,
        client_label: &str,
        supersede_token_hash: Option<&str>,
        now: u64,
        expires_at: u64,
    ) -> Result<bool, PublicInfoError> {
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let expires_at = i64::try_from(expires_at).map_err(|_| PublicInfoError::InvalidSession)?;
        if expires_at <= now || !nilxone_api::activation::is_client_label(client_label) {
            return Err(PublicInfoError::InvalidSession);
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "DELETE FROM product_sessions WHERE issuer = ? AND subject = ? AND expires_at <= ?",
        )
        .bind(&owner.issuer)
        .bind(&owner.subject)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        if let Some(previous) = supersede_token_hash.filter(|previous| *previous != token_hash) {
            activation::end_session(
                &mut transaction,
                previous,
                Some((&owner.issuer, &owner.subject)),
                now,
            )
            .await?;
        }
        // Expired rows are gone, so any active row left is a live one.
        let another_is_active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM product_sessions \
             WHERE issuer = ? AND subject = ? AND active = 1)",
        )
        .bind(&owner.issuer)
        .bind(&owner.subject)
        .fetch_one(&mut *transaction)
        .await?;
        let active = !another_is_active;
        // The profile is re-read inside the insert: a session is never
        // opened for a binding that no longer exists.
        let opened = sqlx::query(
            "INSERT INTO product_sessions \
             (token_hash, issuer, subject, profile_id, created_at, expires_at, active, client_label) \
             SELECT ?, issuer, subject, profile_id, ?, ?, ?, ? FROM public_profiles \
             WHERE issuer = ? AND subject = ? AND profile_id = ?",
        )
        .bind(token_hash)
        .bind(now)
        .bind(expires_at)
        .bind(active)
        .bind(client_label)
        .bind(&owner.issuer)
        .bind(&owner.subject)
        .bind(&owner.profile.profile_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(database)
                if database.is_unique_violation() || database.is_check_violation() =>
            {
                PublicInfoError::InvalidSession
            }
            other => PublicInfoError::Storage(other),
        })?;
        if opened.rows_affected() != 1 {
            return Err(PublicInfoError::ProfileGone);
        }
        if active {
            activation::advance_epoch(&mut transaction, &owner.issuer, &owner.subject).await?;
        }
        transaction.commit().await?;
        Ok(active)
    }

    /// The privileged restore path for a later request or another process:
    /// the owner of a live session, re-proved in SQL. The session row must
    /// still match its profile's (issuer, subject, profile_id); nothing the
    /// browser sent other than the token digest takes part.
    ///
    /// An inactive session restores too: it identifies its owner and may
    /// read. Whether it may write is decided by the write.
    pub async fn restore_session(
        &self,
        token_hash: &str,
        now: u64,
    ) -> Result<Option<AuthenticatedProfile>, PublicInfoError> {
        let Ok(now) = i64::try_from(now) else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT p.issuer, p.subject, p.profile_id, p.pub_dress, p.bond_xp, p.avaia_xp \
             FROM product_sessions s JOIN public_profiles p \
               ON p.issuer = s.issuer AND p.subject = s.subject AND p.profile_id = s.profile_id \
             WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(AuthenticatedProfile {
                issuer: row.try_get("issuer")?,
                subject: row.try_get("subject")?,
                profile: public_profile(row)?,
                session: Some(token_hash.to_owned()),
            })
        })
        .transpose()
    }

    /// Sign-out. Idempotent: closing an unknown or expired session is not an
    /// error, and says nothing about whether it existed.
    ///
    /// Signing out ends the session's part in an open activation request,
    /// and when it was the active client the owner has none until a client
    /// signs in.
    pub async fn close_session(&self, token_hash: &str, now: u64) -> Result<(), PublicInfoError> {
        let now = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        activation::end_session(&mut transaction, token_hash, None, now).await?;
        transaction.commit().await?;
        Ok(())
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
            session: None,
        })
    }
}

/// Brings a store created before sessions were active or inactive up to the
/// current shape. Each owner's most recently opened session becomes the
/// active one and the rest stay signed in as inactive, so nobody loses a
/// session and no owner ends up with two active clients.
async fn add_session_activity(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let has_column = |table: &'static str, column: &'static str| {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?) WHERE name = ?)",
        )
        .bind(table)
        .bind(column)
    };
    if !has_column("public_profiles", "session_epoch")
        .fetch_one(&mut *transaction)
        .await?
    {
        sqlx::query(
            "ALTER TABLE public_profiles ADD COLUMN session_epoch INTEGER NOT NULL DEFAULT 0 \
             CHECK (session_epoch >= 0)",
        )
        .execute(&mut *transaction)
        .await?;
    }
    if !has_column("product_sessions", "active")
        .fetch_one(&mut *transaction)
        .await?
    {
        sqlx::raw_sql(
            "ALTER TABLE product_sessions ADD COLUMN active INTEGER NOT NULL DEFAULT 0 \
               CHECK (active IN (0, 1)); \
             ALTER TABLE product_sessions ADD COLUMN client_label TEXT NOT NULL DEFAULT ''; \
             UPDATE product_sessions SET active = 1 WHERE rowid IN ( \
               SELECT rowid FROM ( \
                 SELECT rowid, ROW_NUMBER() OVER ( \
                   PARTITION BY issuer, subject ORDER BY created_at DESC, rowid DESC) AS position \
                 FROM product_sessions) \
               WHERE position = 1);",
        )
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
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

    /// The product store, for the host's session and public-read routes.
    pub fn repository(&self) -> &PublicInfoRepository {
        &self.repository
    }

    /// The issuer this service signs in through, for the host's public
    /// configuration answer.
    pub fn issuer(&self) -> &str {
        self.flow.issuer()
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
