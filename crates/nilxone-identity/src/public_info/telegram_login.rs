// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Durable OIDC transactions for Telegram chat sign-in.

use nilxone_api::bot_login::{LoginStartRequest, LoginStartResponse};
use sha2::{Digest as _, Sha256};
use sqlx::Row;

use crate::aiaiaiai_sign_in::{SignInClient, SignInError, SignInTransaction, VerifiedSignIn};

use super::PublicInfoRepository;

const CALLBACK_PATH: &str = "/api/v1/telegram/login/callback";
const LOGIN_LIFETIME_SECONDS: i64 = 300;
const UPDATE_RETENTION_SECONDS: i64 = 86_400;

#[derive(Debug, thiserror::Error)]
pub enum TelegramLoginError {
    #[error("invalid Telegram login input")]
    InvalidInput,
    #[error("Telegram login requires a dedicated 0x1 callback")]
    WrongClient,
    #[error("Telegram login OIDC failed")]
    SignIn(#[from] SignInError),
    #[error("Telegram login storage failed")]
    Storage(#[from] sqlx::Error),
    #[error("invalid stored Telegram login transaction")]
    InvalidTransaction(#[from] serde_json::Error),
}

/// A browser callback that proved an AIS identity for a pending Telegram
/// request. The host must still verify the provider link and require the
/// original sender's confirmation before authorizing a Core session.
pub struct TelegramLoginCallback {
    pub workload_subject: String,
    pub actor_id: u64,
    pub chat_id: i64,
    pub message_id: i32,
    pub identity: VerifiedSignIn,
}

fn check_client(client: &SignInClient) -> Result<&str, TelegramLoginError> {
    let config = client.config();
    let redirect = config.redirect_uri();
    if config.client_id() != "0x1" || redirect.path() != CALLBACK_PATH {
        return Err(TelegramLoginError::WrongClient);
    }
    Ok(redirect.as_str())
}

/// Erase expired PKCE material while preserving update deduplication.
async fn erase_expired_pending(db: &mut sqlx::SqliteConnection, now: i64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE telegram_login_transactions SET pending = NULL
         WHERE expires_at <= ? AND pending IS NOT NULL",
    )
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

impl PublicInfoRepository {
    /// Start or recover the same link for an authenticated workload and a
    /// verified, fresh private Telegram message. The HTTP host authenticates
    /// the workload and validates transport evidence before calling this.
    /// Expired or consumed messages return None during the retention window.
    pub async fn start_telegram_login(
        &self,
        client: &SignInClient,
        workload_subject: &str,
        request: &LoginStartRequest,
        now: u64,
    ) -> Result<Option<LoginStartResponse>, TelegramLoginError> {
        let redirect_uri = check_client(client)?;
        request
            .validate()
            .map_err(|_| TelegramLoginError::InvalidInput)?;
        let actor_id = i64::try_from(request.provider_actor_id)
            .map_err(|_| TelegramLoginError::InvalidInput)?;
        if workload_subject.is_empty()
            || workload_subject.len() > 255
            || actor_id != request.chat_id
        {
            return Err(TelegramLoginError::InvalidInput);
        }
        let now = i64::try_from(now).map_err(|_| TelegramLoginError::InvalidInput)?;
        let expires_at = now
            .checked_add(LOGIN_LIFETIME_SECONDS)
            .ok_or(TelegramLoginError::InvalidInput)?;

        let mut db = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        erase_expired_pending(&mut db, now).await?;
        sqlx::query("DELETE FROM telegram_login_transactions WHERE expires_at <= ?")
            .bind(now.saturating_sub(UPDATE_RETENTION_SECONDS))
            .execute(&mut *db)
            .await?;

        let existing = sqlx::query(
            "SELECT pending, expires_at, issuer, redirect_uri FROM telegram_login_transactions
             WHERE workload_subject = ? AND actor_id = ? AND chat_id = ? AND message_id = ?",
        )
        .bind(workload_subject)
        .bind(actor_id)
        .bind(request.chat_id)
        .bind(request.message_id)
        .fetch_optional(&mut *db)
        .await?;
        let result = if let Some(row) = existing {
            let issuer: String = row.try_get("issuer")?;
            let stored_redirect: String = row.try_get("redirect_uri")?;
            let pending: Option<String> = row.try_get("pending")?;
            match pending {
                Some(pending)
                    if issuer == client.config().issuer() && stored_redirect == redirect_uri =>
                {
                    let expires: i64 = row.try_get("expires_at")?;
                    Some((serde_json::from_str(&pending)?, expires - now))
                }
                _ => None,
            }
        } else {
            let (_, pending) = client.begin()?;
            let state_digest = Sha256::digest(pending.state.as_bytes());
            sqlx::query(
                "INSERT INTO telegram_login_transactions
                 (workload_subject, actor_id, chat_id, message_id, state_digest,
                  issuer, redirect_uri, expires_at, pending)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(workload_subject)
            .bind(actor_id)
            .bind(request.chat_id)
            .bind(request.message_id)
            .bind(&state_digest[..])
            .bind(client.config().issuer())
            .bind(redirect_uri)
            .bind(expires_at)
            .bind(serde_json::to_string(&pending)?)
            .execute(&mut *db)
            .await?;
            Some((pending, LOGIN_LIFETIME_SECONDS))
        };
        db.commit().await?;

        let Some((pending, lifetime)) = result else {
            return Ok(None);
        };
        let mut url = client.authorization_url(&pending);
        url.set_path("/0x1");
        Ok(Some(LoginStartResponse {
            authorization_url: url.into(),
            expires_in_seconds: u64::try_from(lifetime)
                .map_err(|_| TelegramLoginError::InvalidInput)?,
        }))
    }

    /// Claim the matching callback once, before contacting AIS. Any exchange
    /// failure requires a new /login message. No product session is opened.
    pub async fn complete_telegram_login(
        &self,
        client: &SignInClient,
        state: &str,
        code: &str,
        now: u64,
    ) -> Result<Option<TelegramLoginCallback>, TelegramLoginError> {
        let redirect_uri = check_client(client)?;
        if state.len() != 32 || code.is_empty() || code.len() > 512 {
            return Ok(None);
        }
        let now_sql = i64::try_from(now).map_err(|_| TelegramLoginError::InvalidInput)?;
        let state_digest = Sha256::digest(state.as_bytes());
        let mut db = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(
            "SELECT workload_subject, actor_id, chat_id, message_id, pending
             FROM telegram_login_transactions
             WHERE state_digest = ? AND issuer = ? AND redirect_uri = ?
                 AND expires_at > ? AND pending IS NOT NULL",
        )
        .bind(&state_digest[..])
        .bind(client.config().issuer())
        .bind(redirect_uri)
        .bind(now_sql)
        .fetch_optional(&mut *db)
        .await?;
        let Some(row) = row else {
            // A late callback must not leave its verifier behind either.
            erase_expired_pending(&mut db, now_sql).await?;
            db.commit().await?;
            return Ok(None);
        };
        let pending: String = row.try_get("pending")?;
        let pending: SignInTransaction = serde_json::from_str(&pending)?;
        sqlx::query("UPDATE telegram_login_transactions SET pending = NULL WHERE state_digest = ?")
            .bind(&state_digest[..])
            .execute(&mut *db)
            .await?;
        db.commit().await?;

        let identity = client.complete(code, &pending, now).await?;
        Ok(Some(TelegramLoginCallback {
            workload_subject: row.try_get("workload_subject")?,
            actor_id: u64::try_from(row.try_get::<i64, _>("actor_id")?)
                .map_err(|_| TelegramLoginError::InvalidInput)?,
            chat_id: row.try_get("chat_id")?,
            message_id: row.try_get("message_id")?,
            identity,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aiaiaiai_sign_in::SignInClientConfig;

    fn client(client_id: &str, redirect_path: &str) -> SignInClient {
        SignInClient::new(
            SignInClientConfig::public(
                "https://auth.aiaiaiai.org",
                None,
                client_id,
                &format!("https://nilx.one{redirect_path}"),
            )
            .unwrap(),
        )
    }

    fn message() -> LoginStartRequest {
        LoginStartRequest {
            version: nilxone_api::bot_login::WIRE_VERSION,
            provider: "telegram".into(),
            provider_actor_id: 42,
            chat_id: 42,
            message_id: 7,
        }
    }

    #[tokio::test]
    async fn retries_return_the_same_scoped_url_and_never_extend_it() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let client = client("0x1", CALLBACK_PATH);
        let message = message();
        let first = repository
            .start_telegram_login(&client, "bot", &message, 100)
            .await
            .unwrap()
            .unwrap();
        let retry = repository
            .start_telegram_login(&client, "bot", &message, 101)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.authorization_url, retry.authorization_url);
        assert_eq!(retry.expires_in_seconds, 299);
        let url = url::Url::parse(&first.authorization_url).unwrap();
        assert_eq!(url.path(), "/0x1");
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["code_challenge_method"], "S256");
        assert!(!query.contains_key("code_verifier"));
        assert!(
            repository
                .complete_telegram_login(&client, &query["state"], "code", 400)
                .await
                .unwrap()
                .is_none()
        );
        // The late callback alone erased the expired verifier.
        let pending: Option<String> =
            sqlx::query_scalar("SELECT pending FROM telegram_login_transactions")
                .fetch_one(&repository.pool)
                .await
                .unwrap();
        assert!(pending.is_none());
        assert!(
            repository
                .start_telegram_login(&client, "bot", &message, 400)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn callback_checks_state_issuer_and_redirect_then_claims_before_exchange() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let offline = |issuer: &str, redirect: &str| {
            SignInClient::new(
                SignInClientConfig::public(issuer, Some("http://127.0.0.1:0"), "0x1", redirect)
                    .unwrap(),
            )
        };
        let client = offline(
            "https://auth.aiaiaiai.org",
            "https://nilx.one/api/v1/telegram/login/callback",
        );
        let foreign_issuer = offline(
            "https://other.example",
            "https://nilx.one/api/v1/telegram/login/callback",
        );
        let foreign_redirect = offline(
            "https://auth.aiaiaiai.org",
            "https://other.example/api/v1/telegram/login/callback",
        );
        let started = repository
            .start_telegram_login(&client, "bot", &message(), 100)
            .await
            .unwrap()
            .unwrap();
        let url = url::Url::parse(&started.authorization_url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        let state = &query["state"];
        assert!(
            repository
                .complete_telegram_login(&client, &"!".repeat(32), "code", 101)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            repository
                .complete_telegram_login(&foreign_issuer, state, "code", 101)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            repository
                .complete_telegram_login(&foreign_redirect, state, "code", 101)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            repository
                .complete_telegram_login(&client, state, "code", 101)
                .await,
            Err(TelegramLoginError::SignIn(SignInError::Unavailable))
        ));
        assert!(
            repository
                .complete_telegram_login(&client, state, "code", 101)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn refuses_a_different_chat_and_the_web_callback() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let message = LoginStartRequest {
            chat_id: 43,
            ..message()
        };
        assert!(matches!(
            repository
                .start_telegram_login(&client("0x1", CALLBACK_PATH), "bot", &message, 100)
                .await,
            Err(TelegramLoginError::InvalidInput)
        ));
        assert!(matches!(
            repository
                .start_telegram_login(
                    &client("0x1", "/api/v1/auth/aiaiaiai/callback"),
                    "bot",
                    &message,
                    100,
                )
                .await,
            Err(TelegramLoginError::WrongClient)
        ));
    }
}
