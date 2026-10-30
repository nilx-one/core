// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use crate::PubDress;
use serde::Deserialize;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct GithubEvidenceRepository {
    pool: SqlitePool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GithubEvidenceRecord {
    pub pub_dress: String,
    pub github_user_id: String,
    pub login: String,
    pub profile_url: String,
    pub avatar_url: String,
    pub encrypted_access_token: Vec<u8>,
    pub connection_state: String,
    pub diagnostic: Option<String>,
    pub connected_at: u64,
    pub refreshed_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GithubEvidenceConnectOutcome {
    Connected,
    Reconnected,
    BondMissing,
    ProviderIdentityMismatch,
    GithubAccountAlreadyConnected,
}

impl GithubEvidenceRepository {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let max_connections = if database_url.contains(":memory:") {
            1
        } else {
            5
        };
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(false)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(include_str!(
            "../migrations/0012_github_evidence_connection.sql"
        ))
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub async fn get(
        &self,
        pub_dress: &PubDress,
    ) -> Result<Option<GithubEvidenceRecord>, sqlx::Error> {
        sqlx::query_as::<_, GithubEvidenceRow>(
            "SELECT pub_dress, github_user_id, login, profile_url, avatar_url, encrypted_access_token, connection_state, diagnostic, connected_at, refreshed_at FROM github_evidence_connections WHERE pub_dress = ?",
        )
        .bind(pub_dress.as_str())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
    }

    pub async fn connect_account(
        &self,
        pub_dress: &PubDress,
        user: &GithubUser,
        encrypted_access_token: &[u8],
        now: u64,
    ) -> Result<GithubEvidenceConnectOutcome, sqlx::Error> {
        let github_user_id = user.id.to_string();
        let mut transaction = self.pool.begin().await?;
        let human_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM identities WHERE pub_dress = ? AND identity_kind = 'human')",
        )
        .bind(pub_dress.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if !human_exists {
            transaction.rollback().await?;
            return Ok(GithubEvidenceConnectOutcome::BondMissing);
        }

        if let Some(bound_subject) = sqlx::query_scalar::<_, String>(
            "SELECT provider_subject FROM identity_providers WHERE pub_dress = ? AND provider = 'github' LIMIT 1",
        )
        .bind(pub_dress.as_str())
        .fetch_optional(&mut *transaction)
        .await?
            && bound_subject != github_user_id
        {
            transaction.commit().await?;
            return Ok(GithubEvidenceConnectOutcome::ProviderIdentityMismatch);
        }
        if let Some(bound_pub_dress) = sqlx::query_scalar::<_, String>(
            "SELECT pub_dress FROM identity_providers WHERE provider = 'github' AND provider_subject = ? LIMIT 1",
        )
        .bind(&github_user_id)
        .fetch_optional(&mut *transaction)
        .await?
            && bound_pub_dress != pub_dress.as_str()
        {
            transaction.commit().await?;
            return Ok(GithubEvidenceConnectOutcome::GithubAccountAlreadyConnected);
        }

        let existing_for_bond = sqlx::query_scalar::<_, String>(
            "SELECT github_user_id FROM github_evidence_connections WHERE pub_dress = ?",
        )
        .bind(pub_dress.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(existing) = &existing_for_bond
            && existing != &github_user_id
        {
            transaction.commit().await?;
            return Ok(GithubEvidenceConnectOutcome::ProviderIdentityMismatch);
        }
        if let Some(existing_pub_dress) = sqlx::query_scalar::<_, String>(
            "SELECT pub_dress FROM github_evidence_connections WHERE github_user_id = ?",
        )
        .bind(&github_user_id)
        .fetch_optional(&mut *transaction)
        .await?
            && existing_pub_dress != pub_dress.as_str()
        {
            transaction.commit().await?;
            return Ok(GithubEvidenceConnectOutcome::GithubAccountAlreadyConnected);
        }

        sqlx::query(
            "INSERT INTO github_evidence_connections (pub_dress, github_user_id, login, profile_url, avatar_url, encrypted_access_token, connection_state, diagnostic, connected_at, refreshed_at) VALUES (?, ?, ?, ?, ?, ?, 'connected', NULL, ?, ?) ON CONFLICT(pub_dress) DO UPDATE SET github_user_id = excluded.github_user_id, login = excluded.login, profile_url = excluded.profile_url, avatar_url = excluded.avatar_url, encrypted_access_token = excluded.encrypted_access_token, connection_state = 'connected', diagnostic = NULL, refreshed_at = excluded.refreshed_at",
        )
        .bind(pub_dress.as_str())
        .bind(&github_user_id)
        .bind(&user.login)
        .bind(&user.html_url)
        .bind(&user.avatar_url)
        .bind(encrypted_access_token)
        .bind(i64_from_u64(now))
        .bind(i64_from_u64(now))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(if existing_for_bond.is_some() {
            GithubEvidenceConnectOutcome::Reconnected
        } else {
            GithubEvidenceConnectOutcome::Connected
        })
    }

    pub async fn mark_degraded(
        &self,
        pub_dress: &PubDress,
        diagnostic: &str,
        now: u64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE github_evidence_connections SET connection_state = 'degraded', diagnostic = ?, refreshed_at = ? WHERE pub_dress = ?",
        )
        .bind(diagnostic)
        .bind(i64_from_u64(now))
        .bind(pub_dress.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn refresh(
        &self,
        pub_dress: &PubDress,
        user: &GithubUser,
        now: u64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE github_evidence_connections SET login = ?, profile_url = ?, avatar_url = ?, connection_state = 'connected', diagnostic = NULL, refreshed_at = ? WHERE pub_dress = ? AND github_user_id = ?",
        )
        .bind(&user.login)
        .bind(&user.html_url)
        .bind(&user.avatar_url)
        .bind(i64_from_u64(now))
        .bind(pub_dress.as_str())
        .bind(user.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, pub_dress: &PubDress) -> Result<bool, sqlx::Error> {
        let deleted = sqlx::query("DELETE FROM github_evidence_connections WHERE pub_dress = ?")
            .bind(pub_dress.as_str())
            .execute(&self.pool)
            .await?;
        Ok(deleted.rows_affected() > 0)
    }
}

#[derive(sqlx::FromRow)]
struct GithubEvidenceRow {
    pub_dress: String,
    github_user_id: String,
    login: String,
    profile_url: String,
    avatar_url: String,
    encrypted_access_token: Vec<u8>,
    connection_state: String,
    diagnostic: Option<String>,
    connected_at: i64,
    refreshed_at: i64,
}

impl From<GithubEvidenceRow> for GithubEvidenceRecord {
    fn from(value: GithubEvidenceRow) -> Self {
        Self {
            pub_dress: value.pub_dress,
            github_user_id: value.github_user_id,
            login: value.login,
            profile_url: value.profile_url,
            avatar_url: value.avatar_url,
            encrypted_access_token: value.encrypted_access_token,
            connection_state: value.connection_state,
            diagnostic: value.diagnostic,
            connected_at: u64_from_i64(value.connected_at),
            refreshed_at: u64_from_i64(value.refreshed_at),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubUser {
    pub id: u64,
    pub login: String,
    pub html_url: String,
    pub avatar_url: String,
}

fn i64_from_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn u64_from_i64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}
