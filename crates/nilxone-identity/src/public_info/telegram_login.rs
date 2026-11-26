// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Durable OIDC transactions for Telegram chat sign-in.

use std::time::{SystemTime, UNIX_EPOCH};

use nilxone_api::bot_login::{LoginStartRequest, LoginStartResponse};
use sha2::{Digest as _, Sha256};
use sqlx::Row;

use crate::aiaiaiai_sign_in::{SignInClient, SignInError, SignInTransaction, VerifiedSignIn};

use nilxone_api::activation::ClientState;

use super::{
    AuthenticatedProfile, PublicInfoError, PublicInfoRepository,
    activation::{ActivationError, request_for_stateless_client},
};

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
/// request. A session opens only when the bot claims it for the original
/// /login message in the original private chat.
pub struct TelegramLoginCallback {
    pub workload_subject: String,
    pub actor_id: u64,
    pub chat_id: i64,
    pub message_id: i32,
    pub identity: VerifiedSignIn,
}

/// Browser sign-in and Telegram binding matched in AIS. This result is
/// eligible to be claimed by the bot for the original /login message.
/// It is stored by `verify_telegram_login_callback` and redeemed once by
/// `confirm_telegram_login`.
pub struct TelegramLoginAwaitingConfirmation(TelegramLoginCallback);

/// The bot claimed a verified callback for the original /login message.
pub struct TelegramChatSignedIn {
    /// The owner of the new chat session. It identifies the owner for reads;
    /// a write still requires `restore_telegram_chat_session` and an active
    /// session.
    pub owner: AuthenticatedProfile,
    /// Whether the chat became the active client (Single Active Client).
    pub active: bool,
}

/// Where a Telegram sign-in stands, for the page it was started on. It names
/// no account and carries no session value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramLoginProgress {
    /// The bot has not claimed the verified callback yet.
    Waiting,
    /// The chat signed in while another client is active and asks that
    /// client to agree.
    AwaitingApproval {
        /// The label of the client that has to agree.
        active_client_label: String,
    },
    /// The chat is signed in and active.
    SignedIn,
    /// The active client refused, or the request ended without its
    /// agreement: it ran out of time, the active client signed out, or
    /// another request replaced it.
    Declined,
    /// The browser step failed; the bot says so in the chat.
    Failed,
}

impl TelegramLoginAwaitingConfirmation {
    #[must_use]
    pub fn callback(&self) -> &TelegramLoginCallback {
        &self.0
    }
}

/// The /login message a transaction belongs to.
struct LoginMessage {
    workload_subject: String,
    actor_id: i64,
    chat_id: i64,
    message_id: i32,
}

impl LoginMessage {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            workload_subject: row.try_get("workload_subject")?,
            actor_id: row.try_get("actor_id")?,
            chat_id: row.try_get("chat_id")?,
            message_id: row.try_get("message_id")?,
        })
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
    /// host takes part. The result waits for the bot to claim it; it does not
    /// create or switch a product session. A refused callback is recorded
    /// for the bot as a failure.
    ///
    /// `telegram_bot_token` verifies the Telegram `initData` the AIS page
    /// collected inside Telegram, when AIS has no Telegram link to confirm.
    pub async fn verify_telegram_login_callback(
        &self,
        client: &SignInClient,
        state: &str,
        code: &str,
        telegram_bot_token: Option<&str>,
    ) -> Result<Option<TelegramLoginAwaitingConfirmation>, TelegramLoginVerificationError> {
        let Some(callback) = self
            .complete_telegram_login(client, state, code, telegram_bot_token, unix_seconds()?)
            .await?
        else {
            return Ok(None);
        };
        let awaiting = TelegramLoginAwaitingConfirmation(callback);
        self.await_telegram_confirmation(&awaiting, unix_seconds()?)
            .await?;
        Ok(Some(awaiting))
    }

    /// Whether the identity of a verified callback already has a 0x1
    /// profile: a returning sign-in rather than a first one. It only picks
    /// the words the success page shows; nothing is attached or created.
    pub async fn telegram_login_returns(
        &self,
        awaiting: &TelegramLoginAwaitingConfirmation,
    ) -> Result<bool, TelegramLoginError> {
        let identity = &awaiting.callback().identity;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public_profiles WHERE issuer = ? AND subject = ?)",
        )
        .bind(&identity.issuer)
        .bind(&identity.subject)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
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

    /// The bot claims the verified callback of its /login message. It asks
    /// right after /login and keeps asking until the browser step is done:
    /// the callback already proved that the identity belongs to this
    /// Telegram sender, so no separate press is needed.
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
        // The password signed the chat in; another client is active, so the
        // chat asks it at once. The chat keeps no device state: once the
        // active client agrees and uploads, the back-end parks the data and
        // switches. A refusal (the hourly limit) leaves the chat signed in
        // and inactive, as before.
        if !active {
            match request_for_stateless_client(&mut db, &token_hash, now_sql).await {
                Ok(_) => {}
                Err(ActivationError::Refused(refusal)) => {
                    tracing::warn!(
                        refusal = refusal.code(),
                        "Telegram chat could not ask the active client"
                    );
                }
                Err(ActivationError::Storage(error)) => return Err(error.into()),
            }
        }
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

    /// Where the sign-in started for `state` stands, for the page the
    /// callback rendered. `None` for an unknown or malformed state. The state
    /// was already spent at the callback; it only reads here and grants
    /// nothing.
    pub async fn telegram_login_progress(
        &self,
        state: &str,
        now: u64,
    ) -> Result<Option<TelegramLoginProgress>, PublicInfoError> {
        if state.len() != 32 {
            return Ok(None);
        }
        let Ok(now_sql) = i64::try_from(now) else {
            return Ok(None);
        };
        let state_digest = Sha256::digest(state.as_bytes());
        let Some(row) = sqlx::query(
            "SELECT workload_subject, actor_id, chat_id, message_id, expires_at
             FROM telegram_login_transactions WHERE state_digest = ?",
        )
        .bind(&state_digest[..])
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let login = LoginMessage::from_row(&row)?;
        let started_at = row
            .try_get::<i64, _>("expires_at")?
            .saturating_sub(LOGIN_LIFETIME_SECONDS);
        let failed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM telegram_login_failures
             WHERE workload_subject = ? AND actor_id = ? AND chat_id = ? AND message_id = ?
                 AND expires_at > ?)",
        )
        .bind(&login.workload_subject)
        .bind(login.actor_id)
        .bind(login.chat_id)
        .bind(login.message_id)
        .bind(now_sql)
        .fetch_one(&self.pool)
        .await?;
        if failed {
            return Ok(Some(TelegramLoginProgress::Failed));
        }
        // Only a chat session opened by this sign-in counts, not one the
        // chat already had from an earlier /login.
        let token_hash: Option<String> = sqlx::query_scalar(
            "SELECT c.token_hash FROM telegram_chat_sessions c
             JOIN product_sessions s ON s.token_hash = c.token_hash
             WHERE c.workload_subject = ? AND c.actor_id = ? AND s.created_at >= ?",
        )
        .bind(&login.workload_subject)
        .bind(login.actor_id)
        .bind(started_at)
        .fetch_optional(&self.pool)
        .await?;
        let Some(token_hash) = token_hash else {
            return Ok(Some(TelegramLoginProgress::Waiting));
        };
        let state = match self.activation_state(&token_hash, now).await {
            Ok(state) => state,
            // The session ended in between: signed out, or replaced.
            Err(ActivationError::Refused(_)) => return Ok(Some(TelegramLoginProgress::Declined)),
            Err(ActivationError::Storage(error)) => return Err(error.into()),
        };
        Ok(Some(match state.state {
            ClientState::AuthenticatedActive | ClientState::ApprovalRequested => {
                TelegramLoginProgress::SignedIn
            }
            ClientState::ActivationPending
            | ClientState::ActivationApproved
            | ClientState::ActivationSyncing => TelegramLoginProgress::AwaitingApproval {
                active_client_label: state
                    .request
                    .map(|request| request.from_label)
                    .or(state.active_client_label)
                    .unwrap_or_default(),
            },
            _ => TelegramLoginProgress::Declined,
        }))
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
    ///
    /// The sender is proven when AIS confirms `telegram_binding`, or when
    /// the AIS page ran inside the sender's own Telegram: its `initData`,
    /// verified with `telegram_bot_token`, names the `/login` sender.
    pub async fn complete_telegram_login(
        &self,
        client: &SignInClient,
        state: &str,
        code: &str,
        telegram_bot_token: Option<&str>,
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

        let login = LoginMessage::from_row(&row)?;
        let chat_id = login.chat_id;
        let hint = crate::aiaiaiai_sign_in::telegram_binding_hint(&pending.nonce, chat_id);
        let (identity, bound, init_data) = match client
            .complete_with_binding(code, &pending, now, Some(&hint))
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.record_telegram_login_failure(&login, now_sql).await?;
                return Err(error.into());
            }
        };
        // Without an AIS link, the person signed in inside Telegram: the
        // account Telegram signed the page for must be the /login sender. A
        // link forwarded to someone else carries their account, not this one.
        let telegram_signed_in = match (telegram_bot_token, init_data.as_deref()) {
            (Some(bot_token), Some(init_data)) if !bound => {
                match crate::telegram_init_data::verify(bot_token, init_data, now) {
                    Ok(account) => i64::try_from(account).ok() == Some(chat_id),
                    Err(rejection) => {
                        tracing::warn!(
                            reason = rejection.reason(),
                            "Telegram login init data refused"
                        );
                        false
                    }
                }
            }
            _ => false,
        };
        // AIS confirmed that this identity's linked Telegram account is the
        // sender, or the sender's own Telegram signed the sign-in page.
        if !bound && !telegram_signed_in {
            tracing::warn!(
                reason = "telegram_binding_not_confirmed",
                "Telegram login callback refused"
            );
            self.record_telegram_login_failure(&login, now_sql).await?;
            return Ok(None);
        }
        Ok(Some(TelegramLoginCallback {
            actor_id: u64::try_from(login.actor_id)
                .map_err(|_| TelegramLoginError::InvalidInput)?,
            workload_subject: login.workload_subject,
            chat_id,
            message_id: login.message_id,
            identity,
        }))
    }

    /// AIS answered the callback with an error, so the person never signed
    /// in. Claim the transaction like a callback would, so its state cannot
    /// be used later, and record the failure for the bot. An unknown, used
    /// or expired state changes nothing.
    pub async fn refuse_telegram_login(
        &self,
        client: &SignInClient,
        state: &str,
        now: u64,
    ) -> Result<(), TelegramLoginError> {
        let redirect_uri = check_client(client)?;
        if state.len() != 32 {
            return Ok(());
        }
        let now_sql = i64::try_from(now).map_err(|_| TelegramLoginError::InvalidInput)?;
        let state_digest = Sha256::digest(state.as_bytes());
        let mut db = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(
            "UPDATE telegram_login_transactions SET pending = NULL
             WHERE state_digest = ? AND issuer = ? AND redirect_uri = ?
                 AND expires_at > ? AND pending IS NOT NULL
             RETURNING workload_subject, actor_id, chat_id, message_id",
        )
        .bind(&state_digest[..])
        .bind(client.config().issuer())
        .bind(redirect_uri)
        .bind(now_sql)
        .fetch_optional(&mut *db)
        .await?;
        db.commit().await?;
        if let Some(row) = row {
            self.record_telegram_login_failure(&LoginMessage::from_row(&row)?, now_sql)
                .await?;
        }
        Ok(())
    }

    /// Remember that the browser step of this /login failed, until the bot
    /// reads it or it expires like a confirmation would.
    async fn record_telegram_login_failure(
        &self,
        login: &LoginMessage,
        now: i64,
    ) -> Result<(), TelegramLoginError> {
        let expires_at = now
            .checked_add(CONFIRMATION_LIFETIME_SECONDS)
            .ok_or(TelegramLoginError::InvalidInput)?;
        let mut db = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM telegram_login_failures WHERE expires_at <= ?")
            .bind(now)
            .execute(&mut *db)
            .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO telegram_login_failures
             (workload_subject, actor_id, chat_id, message_id, expires_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&login.workload_subject)
        .bind(login.actor_id)
        .bind(login.chat_id)
        .bind(login.message_id)
        .bind(expires_at)
        .execute(&mut *db)
        .await?;
        db.commit().await?;
        Ok(())
    }

    /// Whether the browser step of this /login reached the callback and
    /// failed. The bot asks after `confirm_telegram_login` found nothing, to
    /// tell a failure from a browser step that is still unfinished.
    pub async fn telegram_login_failed(
        &self,
        workload_subject: &str,
        sender_id: u64,
        chat_id: i64,
        message_id: i32,
        now: u64,
    ) -> Result<bool, PublicInfoError> {
        let (Ok(actor_id), Ok(now)) = (i64::try_from(sender_id), i64::try_from(now)) else {
            return Ok(false);
        };
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM telegram_login_failures
             WHERE workload_subject = ? AND actor_id = ? AND chat_id = ? AND message_id = ?
                 AND expires_at > ?",
        )
        .bind(workload_subject)
        .bind(actor_id)
        .bind(chat_id)
        .bind(message_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?
        .is_some())
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
                .complete_telegram_login(&client, &query["state"], "code", None, 400)
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
                .complete_telegram_login(&client, &"!".repeat(32), "code", None, 101)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            repository
                .complete_telegram_login(&foreign_issuer, state, "code", None, 101)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            repository
                .complete_telegram_login(&foreign_redirect, state, "code", None, 101)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !repository
                .telegram_login_failed("bot", 42, 42, 7, 101)
                .await
                .unwrap(),
            "foreign callbacks do not fail the login"
        );
        assert!(matches!(
            repository
                .complete_telegram_login(&client, state, "code", None, 101)
                .await,
            Err(TelegramLoginError::SignIn(SignInError::Unavailable))
        ));
        assert!(
            repository
                .telegram_login_failed("bot", 42, 42, 7, 101)
                .await
                .unwrap(),
            "a failed exchange tells the bot at once"
        );
        assert!(
            repository
                .complete_telegram_login(&client, state, "code", None, 101)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_issuer_error_fails_only_its_own_login_and_uses_up_the_state() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let client = SignInClient::new(
            SignInClientConfig::public(
                "https://auth.aiaiaiai.org",
                Some("http://127.0.0.1:0"),
                "0x1",
                "https://nilx.one/api/v1/telegram/login/callback",
            )
            .unwrap(),
        );
        let started = repository
            .start_telegram_login(&client, "bot", &message(), 100)
            .await
            .unwrap()
            .unwrap();
        let url = url::Url::parse(&started.authorization_url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        let state = &query["state"];

        repository
            .refuse_telegram_login(&client, &"a".repeat(32), 101)
            .await
            .unwrap();
        assert!(
            !repository
                .telegram_login_failed("bot", 42, 42, 7, 101)
                .await
                .unwrap()
        );
        repository
            .refuse_telegram_login(&client, state, 101)
            .await
            .unwrap();
        for (workload, sender, chat, message, failed) in [
            ("bot", 42, 42, 7, true),
            ("bot", 42, 42, 8, false),
            ("other", 42, 42, 7, false),
            ("bot", 43, 43, 7, false),
        ] {
            assert_eq!(
                repository
                    .telegram_login_failed(workload, sender, chat, message, 102)
                    .await
                    .unwrap(),
                failed,
                "{workload} {sender} {message}"
            );
        }
        // The state is spent: a later callback with it changes nothing.
        assert!(
            repository
                .complete_telegram_login(&client, state, "code", None, 102)
                .await
                .unwrap()
                .is_none()
        );
        // The failure is kept as long as a confirmation would be.
        assert!(
            !repository
                .telegram_login_failed("bot", 42, 42, 7, 101 + 300)
                .await
                .unwrap()
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
    async fn a_callback_returns_only_for_an_identity_with_a_profile() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let first = verified(7, "pairwise");
        assert!(!repository.telegram_login_returns(&first).await.unwrap());
        repository.attach(&first.callback().identity).await.unwrap();
        assert!(repository.telegram_login_returns(&first).await.unwrap());
        assert!(
            !repository
                .telegram_login_returns(&verified(8, "someone-else"))
                .await
                .unwrap()
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

    /// A started sign-in for message `message_id`, as login start stores it,
    /// known by `state`.
    async fn started(repository: &PublicInfoRepository, message_id: i32, state: &str, at: i64) {
        sqlx::query(
            "INSERT INTO telegram_login_transactions
             (workload_subject, actor_id, chat_id, message_id, state_digest,
              issuer, redirect_uri, expires_at, pending)
             VALUES ('bot', 42, 42, ?, ?, 'https://auth.aiaiaiai.org', 'r', ?, NULL)",
        )
        .bind(message_id)
        .bind(&Sha256::digest(state.as_bytes())[..])
        .bind(at + LOGIN_LIFETIME_SECONDS)
        .execute(&repository.pool)
        .await
        .unwrap();
    }

    /// The owner with the Mini App active, and a chat that signed in after
    /// it for message `message_id`.
    async fn chat_beside_the_mini_app(state: &str) -> (PublicInfoRepository, String) {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let web = repository
            .attach(&verified(1, "pairwise").callback().identity)
            .await
            .unwrap();
        let web_token = "w".repeat(64);
        assert!(
            repository
                .open_session(&web, &web_token, "Telegram · Mini App", None, 100, 1_000)
                .await
                .unwrap()
        );
        started(&repository, 7, state, 100).await;
        assert_eq!(
            repository
                .telegram_login_progress(state, 100)
                .await
                .unwrap(),
            Some(TelegramLoginProgress::Waiting)
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
        assert!(!chat.active, "the password alone never takes activity away");
        assert_eq!(
            repository
                .telegram_login_progress(state, 101)
                .await
                .unwrap(),
            Some(TelegramLoginProgress::AwaitingApproval {
                active_client_label: "Telegram · Mini App".into()
            })
        );
        (repository, web_token)
    }

    #[tokio::test]
    async fn a_chat_signed_in_beside_an_active_client_takes_over_once_its_data_is_parked() {
        use nilxone_api::activation::{UploadPartQuery, sync_digest};

        let state = "a".repeat(32);
        let (repository, web) = chat_beside_the_mini_app(&state).await;
        let asked = repository.activation_state(&web, 102).await.unwrap();
        assert_eq!(asked.state, ClientState::ApprovalRequested);
        let request = asked.request.unwrap();
        assert_eq!(request.to_label, TELEGRAM_CHAT_CLIENT_LABEL);
        // Agreeing alone switches nothing: the active client's data comes
        // first, as in any handoff.
        let agreed = repository
            .approve_activation(&web, &request.id, 103)
            .await
            .unwrap();
        assert_eq!(agreed.state, ClientState::UploadRequired);
        assert_eq!(
            repository
                .telegram_login_progress(&state, 103)
                .await
                .unwrap(),
            Some(TelegramLoginProgress::AwaitingApproval {
                active_client_label: "Telegram · Mini App".into()
            })
        );
        let data = br#"{"format":"nilx-one.activation-payload","version":1,"records":[]}"#;
        let part = UploadPartQuery {
            offset: 0,
            total_bytes: data.len() as u64,
        };
        // The whole upload is parked for the owner and the chat is active.
        let uploaded = repository
            .upload_activation_sync_part(&web, &request.id, part, data, 104)
            .await
            .unwrap();
        assert_eq!(uploaded.state, ClientState::ActiveElsewhere);
        assert!(!uploaded.active);
        assert_eq!(
            chat_sessions(&repository).await,
            [
                ("Telegram · Mini App".to_owned(), false),
                (TELEGRAM_CHAT_CLIENT_LABEL.to_owned(), true),
            ]
        );
        let parked: (i64, String) =
            sqlx::query_as("SELECT total, digest FROM product_parked_payloads")
                .fetch_one(&repository.pool)
                .await
                .unwrap();
        assert_eq!(parked, (data.len() as i64, sync_digest(data)));
        let parts: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT data FROM product_parked_payload_parts")
                .fetch_all(&repository.pool)
                .await
                .unwrap();
        assert_eq!(parts, [data.to_vec()]);
        let in_transit: i64 = sqlx::query_scalar("SELECT count(*) FROM product_activation_sync")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert_eq!(in_transit, 0, "the handoff's own copy is gone");
        assert_eq!(
            repository
                .telegram_login_progress(&state, 105)
                .await
                .unwrap(),
            Some(TelegramLoginProgress::SignedIn)
        );
        // The same last part again only answers what happened.
        assert_eq!(
            repository
                .upload_activation_sync_part(&web, &request.id, part, data, 106)
                .await
                .unwrap()
                .state,
            ClientState::ActiveElsewhere
        );
    }

    #[tokio::test]
    async fn a_refused_chat_stays_inactive_and_its_page_says_so() {
        let state = "b".repeat(32);
        let (repository, web) = chat_beside_the_mini_app(&state).await;
        let id = repository
            .activation_state(&web, 102)
            .await
            .unwrap()
            .request
            .unwrap()
            .id;
        let answered = repository.decline_activation(&web, &id, 103).await.unwrap();
        assert_eq!(answered.state, ClientState::AuthenticatedActive);
        assert_eq!(
            repository
                .telegram_login_progress(&state, 104)
                .await
                .unwrap(),
            Some(TelegramLoginProgress::Declined)
        );
        assert_eq!(
            chat_sessions(&repository).await,
            [
                ("Telegram · Mini App".to_owned(), true),
                (TELEGRAM_CHAT_CLIENT_LABEL.to_owned(), false),
            ]
        );
        assert_eq!(
            repository
                .telegram_login_progress(&"c".repeat(32), 104)
                .await
                .unwrap(),
            None,
            "an unknown state names nothing"
        );
        assert_eq!(
            repository
                .telegram_login_progress("short", 104)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_first_client_signs_in_active_and_asks_nobody() {
        let repository = PublicInfoRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let state = "d".repeat(32);
        started(&repository, 7, &state, 100).await;
        repository
            .await_telegram_confirmation(&verified(7, "pairwise"), 100)
            .await
            .unwrap();
        assert!(
            repository
                .confirm_telegram_login("bot", 42, 42, 7, 101, 1_000)
                .await
                .unwrap()
                .unwrap()
                .active
        );
        assert_eq!(
            repository
                .telegram_login_progress(&state, 102)
                .await
                .unwrap(),
            Some(TelegramLoginProgress::SignedIn)
        );
        let requests: i64 = sqlx::query_scalar("SELECT count(*) FROM product_activation_requests")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert_eq!(requests, 0);
    }
}
