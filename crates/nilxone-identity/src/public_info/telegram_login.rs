// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Durable OIDC transactions for Telegram chat sign-in.

use std::time::{SystemTime, UNIX_EPOCH};

use nilxone_api::bot_login::{LoginStartRequest, LoginStartResponse};
use sha2::{Digest as _, Sha256};
use sqlx::Row;

use crate::aiaiaiai_sign_in::{SignInClient, SignInError, SignInTransaction, VerifiedSignIn};

use super::{AuthenticatedProfile, PublicInfoError, PublicInfoRepository};

const CALLBACK_PATH: &str = "/api/v1/telegram/login/callback";
const LOGIN_LIFETIME_SECONDS: i64 = 300;
/// How long a verified callback waits for the sender's confirmation.
const CONFIRMATION_LIFETIME_SECONDS: i64 = 300;
/// The label a Telegram chat session shows to the owner's other clients.
pub const TELEGRAM_CHAT_CLIENT_LABEL: &str = "Telegram · Chat";
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

#[derive(Debug, thiserror::Error)]
pub enum TelegramLoginVerificationError {
    #[error(transparent)]
    Callback(#[from] TelegramLoginError),
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

/// Browser sign-in and Telegram binding matched in AIS. This result is
/// eligible for confirmation by the original sender.
/// It is stored by `verify_telegram_login_callback` and redeemed once by
/// `confirm_telegram_login`.
pub struct TelegramLoginAwaitingConfirmation(TelegramLoginCallback);

/// The original sender confirmed a verified callback in the original chat.
pub struct TelegramChatSignedIn {
    /// The owner of the new chat session. It identifies the owner for reads;
    /// a write still requires `restore_telegram_chat_session` and an active
    /// session.
    pub owner: AuthenticatedProfile,
    /// Whether the chat became the active client (Single Active Client).
    pub active: bool,
}

impl TelegramLoginAwaitingConfirmation {
    #[must_use]
    pub fn callback(&self) -> &TelegramLoginCallback {
        &self.0
    }
}

/// A fresh session id that nobody holds: the chat is named by its binding,
/// so only the digest is ever stored.
fn chat_session_token_hash() -> Result<String, PublicInfoError> {
    let mut token = [0u8; 32];
    getrandom::fill(&mut token).map_err(|_| PublicInfoError::TelegramEntropyUnavailable)?;
    Ok(Sha256::digest(token)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn unix_seconds() -> Result<u64, TelegramLoginError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| TelegramLoginError::InvalidInput)
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
        // AIS is asked whether the person's linked Telegram account is the
        // sender of this /login, and answers that one bit in the ID token.
        let mut url = client.telegram_authorization_url(&pending, request.chat_id);
        url.set_path("/0x1");
        Ok(Some(LoginStartResponse {
            authorization_url: url.into(),
            expires_in_seconds: u64::try_from(lifetime)
                .map_err(|_| TelegramLoginError::InvalidInput)?,
        }))
    }

    /// Exchange the callback and verify that AIS links the authenticated
    /// identity to the Telegram sender recorded at login start.
    ///
    /// The proof is the ID token's `telegram_binding` claim, bound to this
    /// transaction's nonce and the sender's account: no AIS credential of the
    /// host takes part. The result only authorizes asking the original sender
    /// for confirmation. It does not create or switch a product session.
    pub async fn verify_telegram_login_callback(
        &self,
        client: &SignInClient,
        state: &str,
        code: &str,
    ) -> Result<Option<TelegramLoginAwaitingConfirmation>, TelegramLoginVerificationError> {
        let Some(callback) = self
            .complete_telegram_login(client, state, code, unix_seconds()?)
            .await?
        else {
            return Ok(None);
        };
        let awaiting = TelegramLoginAwaitingConfirmation(callback);
        self.await_telegram_confirmation(&awaiting, unix_seconds()?)
            .await?;
        Ok(Some(awaiting))
    }

    /// Keep a verified callback until its sender confirms it in the chat.
    async fn await_telegram_confirmation(
        &self,
        awaiting: &TelegramLoginAwaitingConfirmation,
        now: u64,
    ) -> Result<(), TelegramLoginError> {
        let callback = awaiting.callback();
        let now = i64::try_from(now).map_err(|_| TelegramLoginError::InvalidInput)?;
        let actor_id =
            i64::try_from(callback.actor_id).map_err(|_| TelegramLoginError::InvalidInput)?;
        let auth_time = i64::try_from(callback.identity.auth_time)
            .map_err(|_| TelegramLoginError::InvalidInput)?;
        let expires_at = now
            .checked_add(CONFIRMATION_LIFETIME_SECONDS)
            .ok_or(TelegramLoginError::InvalidInput)?;
        let mut db = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM telegram_login_confirmations WHERE expires_at <= ?")
            .bind(now)
            .execute(&mut *db)
            .await?;
        // The callback claim is one-use, so a login message is confirmed at
        // most once; a conflict here is a storage fault, never a retry.
        sqlx::query(
            "INSERT INTO telegram_login_confirmations
             (workload_subject, actor_id, chat_id, message_id, issuer, subject,
              pub_dress, auth_time, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&callback.workload_subject)
        .bind(actor_id)
        .bind(callback.chat_id)
        .bind(callback.message_id)
        .bind(&callback.identity.issuer)
        .bind(&callback.identity.subject)
        .bind(&callback.identity.pub_dress)
        .bind(auth_time)
        .bind(expires_at)
        .execute(&mut *db)
        .await?;
        db.commit().await?;
        Ok(())
    }

    /// The original sender pressed confirm in the original private chat.
    ///
    /// The host passes the bot workload it authenticated and the sender,
    /// chat and `/login` message from a verified Telegram update; nothing
    /// else selects the identity. A different sender, chat, workload or
    /// message, an expired or a used confirmation returns None. Otherwise
    /// the confirmation is consumed and a product session is opened for the
    /// chat under Single Active Client: it is active only when the owner has
    /// no other active client, and it replaces this chat's previous session.
    pub async fn confirm_telegram_login(
        &self,
        workload_subject: &str,
        sender_id: u64,
        chat_id: i64,
        message_id: i32,
        now: u64,
        session_expires_at: u64,
    ) -> Result<Option<TelegramChatSignedIn>, PublicInfoError> {
        let actor_id = i64::try_from(sender_id).map_err(|_| PublicInfoError::InvalidSession)?;
        let now_sql = i64::try_from(now).map_err(|_| PublicInfoError::InvalidSession)?;
        let expires_sql =
            i64::try_from(session_expires_at).map_err(|_| PublicInfoError::InvalidSession)?;
        if workload_subject.is_empty()
            || actor_id <= 0
            || chat_id != actor_id
            || message_id <= 0
            || expires_sql <= now_sql
        {
            return Err(PublicInfoError::InvalidSession);
        }
        // Claim first: a failure below needs a new /login, never a replay.
        let mut db = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM telegram_login_confirmations WHERE expires_at <= ?")
            .bind(now_sql)
            .execute(&mut *db)
            .await?;
        let row = sqlx::query(
            "DELETE FROM telegram_login_confirmations
             WHERE workload_subject = ? AND actor_id = ? AND chat_id = ? AND message_id = ?
             RETURNING issuer, subject, pub_dress, auth_time",
        )
        .bind(workload_subject)
        .bind(actor_id)
        .bind(chat_id)
        .bind(message_id)
        .fetch_optional(&mut *db)
        .await?;
        db.commit().await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let identity = VerifiedSignIn {
            issuer: row.try_get("issuer")?,
            subject: row.try_get("subject")?,
            pub_dress: row.try_get("pub_dress")?,
            auth_time: u64::try_from(row.try_get::<i64, _>("auth_time")?)
                .map_err(|_| PublicInfoError::InvalidSession)?,
        };
        let owner = self.attach(&identity).await?;
        let token_hash = chat_session_token_hash()?;

        let mut db = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT token_hash FROM telegram_chat_sessions
             WHERE workload_subject = ? AND actor_id = ?",
        )
        .bind(workload_subject)
        .bind(actor_id)
        .fetch_optional(&mut *db)
        .await?;
        let active = super::open_session_in(
            &mut db,
            &owner,
            &token_hash,
            TELEGRAM_CHAT_CLIENT_LABEL,
            previous.as_deref(),
            now_sql,
            expires_sql,
        )
        .await?;
        sqlx::query(
            "INSERT INTO telegram_chat_sessions (workload_subject, actor_id, token_hash)
             VALUES (?, ?, ?)
             ON CONFLICT (workload_subject, actor_id) DO UPDATE SET token_hash = excluded.token_hash",
        )
        .bind(workload_subject)
        .bind(actor_id)
        .bind(&token_hash)
        .execute(&mut *db)
        .await?;
        db.commit().await?;
        Ok(Some(TelegramChatSignedIn { owner, active }))
    }

    /// The owner of the live session this chat signed in to, for a later
    /// update from the same bot workload and verified sender. An inactive
    /// session restores too; a write checks activity itself.
    pub async fn restore_telegram_chat_session(
        &self,
        workload_subject: &str,
        sender_id: u64,
        now: u64,
    ) -> Result<Option<AuthenticatedProfile>, PublicInfoError> {
        let Some(token_hash) = self
            .telegram_chat_session(workload_subject, sender_id)
            .await?
        else {
            return Ok(None);
        };
        self.restore_session(&token_hash, now).await
    }

    /// Sign the chat out. Idempotent, like `close_session`.
    pub async fn close_telegram_chat_session(
        &self,
        workload_subject: &str,
        sender_id: u64,
        now: u64,
    ) -> Result<(), PublicInfoError> {
        if let Some(token_hash) = self
            .telegram_chat_session(workload_subject, sender_id)
            .await?
        {
            self.close_session(&token_hash, now).await?;
        }
        Ok(())
    }

    async fn telegram_chat_session(
        &self,
        workload_subject: &str,
        sender_id: u64,
    ) -> Result<Option<String>, PublicInfoError> {
        let Ok(actor_id) = i64::try_from(sender_id) else {
            return Ok(None);
        };
        Ok(sqlx::query_scalar(
            "SELECT token_hash FROM telegram_chat_sessions
             WHERE workload_subject = ? AND actor_id = ?",
        )
        .bind(workload_subject)
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await?)
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
            tracing::warn!(
                reason = "malformed_callback",
                "Telegram login callback refused"
            );
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
            tracing::warn!(
                reason = "unknown_expired_or_used_state",
                "Telegram login callback refused"
            );
            return Ok(None);
        };
        let pending: String = row.try_get("pending")?;
        let pending: SignInTransaction = serde_json::from_str(&pending)?;
        sqlx::query("UPDATE telegram_login_transactions SET pending = NULL WHERE state_digest = ?")
            .bind(&state_digest[..])
            .execute(&mut *db)
            .await?;
        db.commit().await?;

        let chat_id: i64 = row.try_get("chat_id")?;
        let hint = crate::aiaiaiai_sign_in::telegram_binding_hint(&pending.nonce, chat_id);
        let (identity, bound) = client
            .complete_with_binding(code, &pending, now, Some(&hint))
            .await?;
        // AIS confirmed that this identity's linked Telegram account is the
        // sender named in the request; anything else asks for nothing.
        if !bound {
            tracing::warn!(
                reason = "telegram_binding_not_confirmed",
                "Telegram login callback refused"
            );
            return Ok(None);
        }
        Ok(Some(TelegramLoginCallback {
            workload_subject: row.try_get("workload_subject")?,
            actor_id: u64::try_from(row.try_get::<i64, _>("actor_id")?)
                .map_err(|_| TelegramLoginError::InvalidInput)?,
            chat_id,
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
        assert_eq!(query["scope"], "openid pub_dress telegram_binding");
        let nonce = &query["nonce"];
        assert_eq!(
            query["telegram_binding_hint"],
            crate::aiaiaiai_sign_in::telegram_binding_hint(nonce, 42)
        );
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

    fn verified(message_id: i32, subject: &str) -> TelegramLoginAwaitingConfirmation {
        TelegramLoginAwaitingConfirmation(TelegramLoginCallback {
            workload_subject: "bot".into(),
            actor_id: 42,
            chat_id: 42,
            message_id,
            identity: VerifiedSignIn {
                issuer: "https://auth.aiaiaiai.org".into(),
                subject: subject.into(),
                pub_dress: Some("0x0sky".into()),
                auth_time: 90,
            },
        })
    }

    async fn chat_sessions(repository: &PublicInfoRepository) -> Vec<(String, bool)> {
        sqlx::query_as(
            "SELECT client_label, active FROM product_sessions ORDER BY created_at, rowid",
        )
        .fetch_all(&repository.pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn only_the_original_sender_in_the_original_chat_confirms_once() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        repository
            .await_telegram_confirmation(&verified(7, "pairwise"), 100)
            .await
            .unwrap();
        for (workload, sender, chat, message) in [
            ("other-bot", 42, 42, 7),
            ("bot", 43, 43, 7),
            ("bot", 42, 42, 8),
        ] {
            assert!(
                repository
                    .confirm_telegram_login(workload, sender, chat, message, 101, 1_000)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        assert!(matches!(
            repository
                .confirm_telegram_login("bot", 42, 43, 7, 101, 1_000)
                .await,
            Err(PublicInfoError::InvalidSession)
        ));
        assert!(
            repository
                .restore_telegram_chat_session("bot", 42, 101)
                .await
                .unwrap()
                .is_none()
        );
        let signed_in = repository
            .confirm_telegram_login("bot", 42, 42, 7, 101, 1_000)
            .await
            .unwrap()
            .unwrap();
        assert!(signed_in.active, "the first client of a profile is active");
        assert!(
            repository
                .confirm_telegram_login("bot", 42, 42, 7, 102, 1_000)
                .await
                .unwrap()
                .is_none(),
            "a confirmation is one-use"
        );
        let restored = repository
            .restore_telegram_chat_session("bot", 42, 102)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.public_profile(), signed_in.owner.public_profile());
        assert!(
            repository
                .restore_telegram_chat_session("other-bot", 42, 102)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            repository
                .restore_telegram_chat_session("bot", 42, 1_000)
                .await
                .unwrap()
                .is_none(),
            "the chat session expires"
        );
        let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM telegram_login_confirmations")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert_eq!(stored, 0);
    }

    #[tokio::test]
    async fn an_unconfirmed_callback_expires() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        repository
            .await_telegram_confirmation(&verified(7, "pairwise"), 100)
            .await
            .unwrap();
        assert!(
            repository
                .confirm_telegram_login("bot", 42, 42, 7, 400, 1_000)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn the_chat_joins_single_active_client_with_the_owners_other_clients() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        // The owner already signed in in the Mini App, which is active.
        let web = repository
            .attach(&verified(1, "pairwise").callback().identity)
            .await
            .unwrap();
        assert!(
            repository
                .open_session(
                    &web,
                    &"w".repeat(64),
                    "Telegram · Mini App",
                    None,
                    100,
                    1_000
                )
                .await
                .unwrap()
        );
        repository
            .await_telegram_confirmation(&verified(7, "pairwise"), 100)
            .await
            .unwrap();
        let chat = repository
            .confirm_telegram_login("bot", 42, 42, 7, 101, 1_000)
            .await
            .unwrap()
            .unwrap();
        assert!(!chat.active, "signing in never takes activity away");
        assert_eq!(chat.owner.public_profile(), web.public_profile());
        // A second /login in the same chat replaces the chat's session.
        repository
            .await_telegram_confirmation(&verified(8, "pairwise"), 102)
            .await
            .unwrap();
        repository
            .confirm_telegram_login("bot", 42, 42, 8, 103, 1_000)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            chat_sessions(&repository).await,
            [
                ("Telegram · Mini App".to_owned(), true),
                (TELEGRAM_CHAT_CLIENT_LABEL.to_owned(), false),
            ]
        );
        // Signing the chat out leaves the Mini App alone.
        repository
            .close_telegram_chat_session("bot", 42, 104)
            .await
            .unwrap();
        repository
            .close_telegram_chat_session("bot", 42, 104)
            .await
            .unwrap();
        assert!(
            repository
                .restore_telegram_chat_session("bot", 42, 104)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            chat_sessions(&repository).await,
            [("Telegram · Mini App".to_owned(), true)]
        );
        let bindings: i64 = sqlx::query_scalar("SELECT count(*) FROM telegram_chat_sessions")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert_eq!(bindings, 0);
    }
}
