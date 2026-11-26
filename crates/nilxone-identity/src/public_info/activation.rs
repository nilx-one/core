// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Moving the active client between an owner's product sessions
//! (`docs/client-activation.md`). The wire types and the policy numbers are
//! `nilxone_api::activation`; this is the authority behind them.
//!
//! Every step names the caller by its session token digest and re-proves
//! that session in SQL inside one write transaction. A request binds the
//! session that asked, the session that was active then and the owner's
//! session epoch; a step by anyone else finds no request.

use nilxone_api::activation::{
    ActivationRefusal, ActivationRequestView, ActivationRole, ActivationState, ActivationStatus,
    ClientState, HandoffProgress, ParkedView, REQUESTS_PER_HOUR, RESPOND_SECONDS, SYNC_SECONDS,
    SyncDigest, UPLOAD_SECONDS, UploadPartQuery, is_request_id, is_sync_digest, sync_part_len,
};
use sqlx::{Row, Sqlite, SqliteConnection, Transaction, sqlite::SqliteRow};

use super::PublicInfoRepository;

const HOUR_SECONDS: i64 = 60 * 60;
const REQUEST_COLUMNS: &str = "id, requester_token_hash, approver_token_hash, requester_label, \
                               approver_label, epoch, status, created_at, deadline, sync_total, \
                               sync_uploaded, sync_delivered, requester_keeps_state, \
                               approver_keeps_state";

/// Why an activation step did nothing.
#[derive(Debug, thiserror::Error)]
pub enum ActivationError {
    /// The step is refused. The host answers with the refusal's code and
    /// HTTP status.
    #[error("the activation step was refused: {}", .0.code())]
    Refused(ActivationRefusal),
    #[error("activation storage is unavailable")]
    Storage(#[from] sqlx::Error),
}

impl From<ActivationRefusal> for ActivationError {
    fn from(refusal: ActivationRefusal) -> Self {
        Self::Refused(refusal)
    }
}

type Step<T> = Result<T, ActivationError>;

struct Session {
    token_hash: String,
    issuer: String,
    subject: String,
    active: bool,
    client_label: String,
    epoch: i64,
}

struct Request {
    id: String,
    requester: String,
    approver: String,
    requester_label: String,
    approver_label: String,
    epoch: i64,
    status: ActivationStatus,
    created_at: i64,
    deadline: i64,
    sync_total: Option<i64>,
    sync_uploaded: i64,
    sync_delivered: i64,
    /// False for an asking client that keeps no device state: the whole
    /// upload is parked for the owner and the switch follows at once.
    /// Reserved for the shared Telegram session; no production path sets it.
    requester_keeps_state: bool,
    /// False when the client that was active keeps no device state: its
    /// data is parked already, so its approval is the switch.
    approver_keeps_state: bool,
}

impl Request {
    fn role_of(&self, session: &Session) -> Option<ActivationRole> {
        if self.requester == session.token_hash {
            Some(ActivationRole::Requester)
        } else if self.approver == session.token_hash {
            Some(ActivationRole::Approver)
        } else {
            None
        }
    }

    fn view(&self, role: ActivationRole) -> Result<ActivationRequestView, sqlx::Error> {
        Ok(ActivationRequestView {
            id: self.id.clone(),
            role,
            status: self.status,
            from_label: self.approver_label.clone(),
            to_label: self.requester_label.clone(),
            created_at: unsigned(self.created_at)?,
            deadline: self
                .status
                .is_open()
                .then(|| unsigned(self.deadline))
                .transpose()?,
            progress: HandoffProgress::project(
                self.status,
                self.sync_total.map(unsigned).transpose()?,
                unsigned(self.sync_uploaded)?,
                unsigned(self.sync_delivered)?,
            ),
        })
    }
}

impl PublicInfoRepository {
    /// What the session's client is and shows. Applies whatever is due on a
    /// request first, so a state never outlives a deadline.
    pub async fn activation_state(
        &self,
        token_hash: &str,
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = state_of(&mut transaction, token_hash, clock(now)?, None).await;
        settle(transaction, result).await
    }

    /// An inactive session asks to become the active client. Asking again
    /// while its own request is open returns that request: a reloaded page
    /// resumes, it does not ask twice. A request by another inactive session
    /// supersedes the open one.
    pub async fn request_activation(
        &self,
        token_hash: &str,
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = request(&mut transaction, token_hash, clock(now)?, true).await;
        settle(transaction, result).await
    }

    /// The asking session withdraws its request.
    pub async fn cancel_activation(
        &self,
        token_hash: &str,
        request_id: &str,
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = cancel(&mut transaction, token_hash, request_id, clock(now)?).await;
        settle(transaction, result).await
    }

    /// The active session agrees to hand over. Nothing switches yet: its
    /// upload and the asking client's completion still have to follow.
    pub async fn approve_activation(
        &self,
        token_hash: &str,
        request_id: &str,
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = approve(&mut transaction, token_hash, request_id, clock(now)?).await;
        settle(transaction, result).await
    }

    /// The active session refuses: `declined` before it agreed, `objected`
    /// after. Either way it stays active and uploaded data is deleted.
    pub async fn decline_activation(
        &self,
        token_hash: &str,
        request_id: &str,
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = decline(&mut transaction, token_hash, request_id, clock(now)?).await;
        settle(transaction, result).await
    }

    /// The active session uploads the next part of its data for the asking
    /// client. Parts arrive in order, each `nilxone_api::activation::sync_part_len`
    /// long; the part that makes the data whole moves the request to
    /// `syncing`. The same part again is a retry and changes nothing. The
    /// bytes are opaque here and are kept only while the request is open.
    pub async fn upload_activation_sync_part(
        &self,
        token_hash: &str,
        request_id: &str,
        part: UploadPartQuery,
        data: &[u8],
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = upload(
            &mut transaction,
            token_hash,
            request_id,
            part,
            data,
            clock(now)?,
        )
        .await;
        settle(transaction, result).await
    }

    /// The asking session fetches the part of the uploaded data that starts
    /// at `offset`: the next one, the last one again, or the first one to
    /// start over, as often as it needs to.
    pub async fn download_activation_sync_part(
        &self,
        token_hash: &str,
        request_id: &str,
        offset: u64,
        now: u64,
    ) -> Result<Vec<u8>, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = download(
            &mut transaction,
            token_hash,
            request_id,
            offset,
            clock(now)?,
        )
        .await;
        settle(transaction, result).await
    }

    /// The asking session proves it holds the uploaded bytes and becomes the
    /// active client. This is the only place a signed-in session takes
    /// activity from another: one transaction compares the owner's session
    /// epoch with the request's, deactivates the previous client, activates
    /// this one, advances the epoch and deletes the data.
    pub async fn complete_activation(
        &self,
        token_hash: &str,
        request_id: &str,
        digest: &str,
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = complete(
            &mut transaction,
            token_hash,
            request_id,
            digest,
            clock(now)?,
        )
        .await;
        settle(transaction, result).await
    }
}

impl PublicInfoRepository {
    /// The active client that keeps its state on its device fetches the
    /// part of the parked data that starts at `offset`, as often as it
    /// needs to.
    pub async fn download_parked_part(
        &self,
        token_hash: &str,
        offset: u64,
        now: u64,
    ) -> Result<Vec<u8>, ActivationError> {
        let mut connection = self.pool.begin().await?;
        let session = parking_session(&mut connection, token_hash, clock(now)?).await?;
        let parked = parked_of(&mut connection, &session)
            .await?
            .ok_or(ActivationRefusal::ParkedNotFound)?;
        if sync_part_len(parked.total_bytes, offset).is_none() {
            return Err(ActivationRefusal::SyncOutOfOrder.into());
        }
        let data = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT data FROM product_parked_payload_parts \
             WHERE issuer = ? AND subject = ? AND part_offset = ?",
        )
        .bind(&session.issuer)
        .bind(&session.subject)
        .bind(signed(offset)?)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| sqlx::Error::Protocol("a parked part is missing".into()))?;
        connection.commit().await?;
        Ok(data)
    }

    /// The active client applied the parked data with `digest` over its
    /// own; the back-end deletes it. Again, or with nothing parked, it only
    /// answers the state.
    pub async fn acknowledge_parked(
        &self,
        token_hash: &str,
        digest: &str,
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        if !is_sync_digest(digest) {
            return Err(ActivationRefusal::InvalidSyncDigest.into());
        }
        let now = clock(now)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let session = parking_session(&mut transaction, token_hash, now).await?;
            if let Some(parked) = parked_of(&mut transaction, &session).await? {
                if parked.digest != digest {
                    return Err(ActivationRefusal::SyncIncomplete.into());
                }
                sqlx::query("DELETE FROM product_parked_payloads WHERE issuer = ? AND subject = ?")
                    .bind(&session.issuer)
                    .bind(&session.subject)
                    .execute(&mut *transaction)
                    .await?;
            }
            state_with(&mut transaction, &session, now, None).await
        }
        .await;
        settle(transaction, result).await
    }

    /// The active client that keeps its state on its device parks its data
    /// in parts, before it signs out. Parts arrive in order, as in a
    /// handoff; the first part fixes the size, and a first part with
    /// another size starts over. The part that makes it whole replaces the
    /// owner's parked data. The same part again is a retry.
    pub async fn park_part(
        &self,
        token_hash: &str,
        part: UploadPartQuery,
        data: &[u8],
        now: u64,
    ) -> Result<ActivationState, ActivationError> {
        let now = clock(now)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = park_step(&mut transaction, token_hash, part, data, now).await;
        settle(transaction, result).await
    }
}

/// The live, active session of a client that keeps its state on its
/// device: the only one that parks its data or fetches parked data.
async fn parking_session(
    connection: &mut SqliteConnection,
    token_hash: &str,
    now: i64,
) -> Step<Session> {
    let session = live_session(connection, token_hash, now).await?;
    if !session.active || !keeps_state(connection, token_hash).await? {
        return Err(ActivationRefusal::StepRefused.into());
    }
    Ok(session)
}

async fn park_step(
    connection: &mut SqliteConnection,
    token_hash: &str,
    part: UploadPartQuery,
    data: &[u8],
    now: i64,
) -> Step<ActivationState> {
    let session = parking_session(connection, token_hash, now).await?;
    part.validate(data.len())?;
    let offset = signed(part.offset)?;
    let total = signed(part.total_bytes)?;
    let staged = sqlx::query_as::<_, (i64, i64)>(
        "SELECT total, uploaded FROM product_parking WHERE token_hash = ?",
    )
    .bind(token_hash)
    .fetch_optional(&mut *connection)
    .await?;
    let uploaded = match staged {
        // A first part starts over, also with another size.
        _ if offset == 0 => {
            sqlx::query("DELETE FROM product_parking WHERE token_hash = ?")
                .bind(token_hash)
                .execute(&mut *connection)
                .await?;
            sqlx::query("INSERT INTO product_parking (token_hash, total) VALUES (?, ?)")
                .bind(token_hash)
                .bind(total)
                .execute(&mut *connection)
                .await?;
            0
        }
        // The last PUT can be retried after its transaction removed staging.
        None if offset + signed(data.len() as u64)? == total => {
            let already_parked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM product_parked_payloads p
                 JOIN product_parked_payload_parts b USING (issuer, subject)
                 WHERE p.issuer = ? AND p.subject = ? AND p.total = ?
                   AND b.part_offset = ? AND b.data = ?)",
            )
            .bind(&session.issuer)
            .bind(&session.subject)
            .bind(total)
            .bind(offset)
            .bind(data)
            .fetch_one(&mut *connection)
            .await?;
            if already_parked {
                return state_with(connection, &session, now, None).await;
            }
            return Err(ActivationRefusal::SyncOutOfOrder.into());
        }
        Some((declared, uploaded)) if declared == total => uploaded,
        _ => return Err(ActivationRefusal::SyncOutOfOrder.into()),
    };
    if offset < uploaded {
        let stored = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT data FROM product_parking_parts WHERE token_hash = ? AND part_offset = ?",
        )
        .bind(token_hash)
        .bind(offset)
        .fetch_optional(&mut *connection)
        .await?;
        if stored.as_deref() != Some(data) {
            return Err(ActivationRefusal::SyncOutOfOrder.into());
        }
        return state_with(connection, &session, now, None).await;
    }
    if offset != uploaded {
        return Err(ActivationRefusal::SyncOutOfOrder.into());
    }
    sqlx::query(
        "INSERT INTO product_parking_parts (token_hash, part_offset, data) VALUES (?, ?, ?)",
    )
    .bind(token_hash)
    .bind(offset)
    .bind(data)
    .execute(&mut *connection)
    .await?;
    let uploaded = offset.saturating_add(signed(data.len() as u64)?);
    if uploaded < total {
        sqlx::query("UPDATE product_parking SET uploaded = ? WHERE token_hash = ?")
            .bind(uploaded)
            .bind(token_hash)
            .execute(&mut *connection)
            .await?;
        return state_with(connection, &session, now, None).await;
    }
    // Whole: it replaces what was parked, and the staging rows go.
    let mut digest = SyncDigest::new();
    let parts = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT data FROM product_parking_parts WHERE token_hash = ? ORDER BY part_offset",
    )
    .bind(token_hash)
    .fetch_all(&mut *connection)
    .await?;
    for part in &parts {
        digest.update(part);
    }
    let digest = digest.finish();
    sqlx::query("DELETE FROM product_parked_payloads WHERE issuer = ? AND subject = ?")
        .bind(&session.issuer)
        .bind(&session.subject)
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "INSERT INTO product_parked_payloads (issuer, subject, total, digest, parked_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(total)
    .bind(&digest)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO product_parked_payload_parts (issuer, subject, part_offset, data) \
         SELECT ?, ?, part_offset, data FROM product_parking_parts WHERE token_hash = ?",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(token_hash)
    .execute(&mut *connection)
    .await?;
    sqlx::query("DELETE FROM product_parking WHERE token_hash = ?")
        .bind(token_hash)
        .execute(&mut *connection)
        .await?;
    state_with(connection, &session, now, None).await
}

/// Whether the session's client keeps its state on its device. The Telegram
/// chat does not.
async fn keeps_state(
    connection: &mut SqliteConnection,
    token_hash: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM telegram_chat_sessions WHERE token_hash = ?)",
    )
    .bind(token_hash)
    .fetch_one(&mut *connection)
    .await
}

async fn parked_of(
    connection: &mut SqliteConnection,
    session: &Session,
) -> Result<Option<ParkedView>, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64, String)>(
        "SELECT total, digest FROM product_parked_payloads WHERE issuer = ? AND subject = ?",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|(total, digest)| {
        Ok(ParkedView {
            total_bytes: unsigned(total)?,
            digest,
        })
    })
    .transpose()
}

/// A refusal keeps what was due anyway (a lapsed deadline, a request that
/// lost its active client); only a storage failure undoes the transaction.
async fn settle<T>(transaction: Transaction<'_, Sqlite>, result: Step<T>) -> Step<T> {
    if matches!(result, Err(ActivationError::Storage(_))) {
        transaction.rollback().await?;
    } else {
        transaction.commit().await?;
    }
    result
}

fn clock(now: u64) -> Step<i64> {
    // A time that does not fit is no time at which a session is live.
    i64::try_from(now).map_err(|_| ActivationRefusal::AuthenticationRequired.into())
}

fn unsigned(value: i64) -> Result<u64, sqlx::Error> {
    u64::try_from(value).map_err(|_| sqlx::Error::Protocol("negative activation value".into()))
}

fn signed(value: u64) -> Result<i64, sqlx::Error> {
    i64::try_from(value).map_err(|_| sqlx::Error::Protocol("activation value out of range".into()))
}

/// An inactive session that keeps no device state asks to become the active
/// client inside the caller's transaction. The active client agrees and
/// uploads as usual; the back-end parks the whole upload for the owner and
/// switches, since nobody would fetch it. Reserved for the shared Telegram
/// session: no production path opens such a request yet (owner decision of
/// 2026-11-26), so only tests drive it.
#[cfg(test)]
pub(super) async fn request_for_stateless_client(
    connection: &mut SqliteConnection,
    token_hash: &str,
    now: i64,
) -> Step<ActivationState> {
    request(connection, token_hash, now, false).await
}

async fn request(
    connection: &mut SqliteConnection,
    token_hash: &str,
    now: i64,
    requester_keeps_state: bool,
) -> Step<ActivationState> {
    let session = live_session(connection, token_hash, now).await?;
    if session.active {
        return Err(ActivationRefusal::AlreadyActive.into());
    }
    let open = match open_request(connection, &session).await? {
        Some(open) => Some(advance(connection, &session, open, now).await?),
        None => None,
    }
    .filter(|open| open.status.is_open());
    if let Some(own) = open
        .as_ref()
        .filter(|open| open.requester == session.token_hash)
    {
        let shown = (ActivationRole::Requester, own);
        return state_with(connection, &session, now, Some(shown)).await;
    }
    let (approver, approver_label) = sqlx::query_as::<_, (String, String)>(
        "SELECT token_hash, client_label FROM product_sessions \
         WHERE issuer = ? AND subject = ? AND active = 1 AND expires_at > ?",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(now)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(ActivationRefusal::NoActiveClient)?;
    let approver_keeps_state = keeps_state(connection, &approver).await?;
    let recent = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM product_activation_requests \
         WHERE issuer = ? AND subject = ? AND created_at > ?",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(now.saturating_sub(HOUR_SECONDS))
    .fetch_one(&mut *connection)
    .await?;
    if recent >= i64::from(REQUESTS_PER_HOUR) {
        return Err(ActivationRefusal::RequestLimit.into());
    }
    if let Some(other) = open {
        close(connection, &other.id, ActivationStatus::Superseded, now).await?;
    }
    let row = sqlx::query(&format!(
        "INSERT INTO product_activation_requests \
         (id, issuer, subject, requester_token_hash, approver_token_hash, requester_label, \
          approver_label, epoch, status, created_at, deadline, requester_keeps_state, \
          approver_keeps_state) \
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?) \
         RETURNING {REQUEST_COLUMNS}"
    ))
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(&session.token_hash)
    .bind(approver)
    .bind(&session.client_label)
    .bind(approver_label)
    .bind(session.epoch)
    .bind(now)
    .bind(deadline(now, RESPOND_SECONDS))
    .bind(requester_keeps_state)
    .bind(approver_keeps_state)
    .fetch_one(&mut *connection)
    .await?;
    let created = decode(&row)?;
    // The chat cannot poll for a request: the bot hears it from the queue
    // and shows approve and decline.
    if !created.approver_keeps_state {
        super::bot_notifications::enqueue_in(
            connection,
            &session.issuer,
            &session.subject,
            &super::NewBotNotification {
                kind: nilxone_api::bot_notifications::NotificationKind::ActivationRequested,
                rarity: None,
                label: Some(session.client_label.clone()),
                request_id: Some(created.id.clone()),
                dedupe: created.id.clone(),
            },
            now,
        )
        .await?;
    }
    let shown = (ActivationRole::Requester, &created);
    state_with(connection, &session, now, Some(shown)).await
}

async fn cancel(
    connection: &mut SqliteConnection,
    token_hash: &str,
    request_id: &str,
    now: i64,
) -> Step<ActivationState> {
    let (session, mut request) = step_request(
        connection,
        token_hash,
        request_id,
        ActivationRole::Requester,
        now,
    )
    .await?;
    match request.status {
        ActivationStatus::Cancelled => {}
        status if status.is_open() => {
            close(connection, &request.id, ActivationStatus::Cancelled, now).await?;
            request.status = ActivationStatus::Cancelled;
        }
        _ => return Err(ActivationRefusal::StepRefused.into()),
    }
    let shown = (ActivationRole::Requester, &request);
    state_with(connection, &session, now, Some(shown)).await
}

async fn approve(
    connection: &mut SqliteConnection,
    token_hash: &str,
    request_id: &str,
    now: i64,
) -> Step<ActivationState> {
    let (session, mut request) = step_request(
        connection,
        token_hash,
        request_id,
        ActivationRole::Approver,
        now,
    )
    .await?;
    match request.status {
        // A second tap or a replayed call changes nothing and is not an error.
        ActivationStatus::Approved | ActivationStatus::Syncing | ActivationStatus::Completed => {}
        // The active client keeps no device state: its data is parked
        // already, so agreeing is the switch. The asking client fetches the
        // parked data once it is active.
        ActivationStatus::Pending if !request.approver_keeps_state => {
            switch_active_client(connection, &session, &request, now).await?;
            close(connection, &request.id, ActivationStatus::Completed, now).await?;
            request.status = ActivationStatus::Completed;
            let session = live_session(connection, token_hash, now).await?;
            let shown = (ActivationRole::Approver, &request);
            return state_with(connection, &session, now, Some(shown)).await;
        }
        ActivationStatus::Pending => {
            request.status = ActivationStatus::Approved;
            request.deadline = deadline(now, UPLOAD_SECONDS);
            sqlx::query(
                "UPDATE product_activation_requests SET status = 'approved', deadline = ? \
                 WHERE id = ? AND status = 'pending'",
            )
            .bind(request.deadline)
            .bind(&request.id)
            .execute(&mut *connection)
            .await?;
        }
        _ => return Err(ActivationRefusal::StepRefused.into()),
    }
    let shown = (ActivationRole::Approver, &request);
    state_with(connection, &session, now, Some(shown)).await
}

async fn decline(
    connection: &mut SqliteConnection,
    token_hash: &str,
    request_id: &str,
    now: i64,
) -> Step<ActivationState> {
    let (session, mut request) = step_request(
        connection,
        token_hash,
        request_id,
        ActivationRole::Approver,
        now,
    )
    .await?;
    let outcome = match request.status {
        ActivationStatus::Declined | ActivationStatus::Objected => None,
        ActivationStatus::Pending => Some(ActivationStatus::Declined),
        ActivationStatus::Approved | ActivationStatus::Syncing => Some(ActivationStatus::Objected),
        _ => return Err(ActivationRefusal::StepRefused.into()),
    };
    if let Some(outcome) = outcome {
        close(connection, &request.id, outcome, now).await?;
        request.status = outcome;
    }
    let shown = (ActivationRole::Approver, &request);
    state_with(connection, &session, now, Some(shown)).await
}

async fn upload(
    connection: &mut SqliteConnection,
    token_hash: &str,
    request_id: &str,
    part: UploadPartQuery,
    data: &[u8],
    now: i64,
) -> Step<ActivationState> {
    let (session, mut request) = step_request(
        connection,
        token_hash,
        request_id,
        ActivationRole::Approver,
        now,
    )
    .await?;
    // The last part again, after it parked the data and switched: the
    // answer was lost, not the step. Nothing changes.
    if request.status == ActivationStatus::Completed && !request.requester_keeps_state {
        let shown = (ActivationRole::Approver, &request);
        return state_with(connection, &session, now, Some(shown)).await;
    }
    let uploading = request.status == ActivationStatus::Approved;
    if !uploading && request.status != ActivationStatus::Syncing {
        return Err(ActivationRefusal::StepRefused.into());
    }
    part.validate(data.len())?;
    // Both fit: the total is within the size limit and the offset within it.
    let offset = signed(part.offset)?;
    let total = signed(part.total_bytes)?;
    if request.sync_total.is_some_and(|declared| declared != total) {
        return Err(ActivationRefusal::SyncOutOfOrder.into());
    }
    if !uploading || offset < request.sync_uploaded {
        // A part that is already here is a retry when it carries the same
        // bytes. Different bytes are not: the asking client may hold them.
        let stored = stored_part(connection, &request.id, offset).await?;
        if stored.as_deref() != Some(data) {
            return Err(ActivationRefusal::SyncOutOfOrder.into());
        }
    } else if offset == request.sync_uploaded {
        sqlx::query(
            "INSERT INTO product_activation_sync (request_id, part_offset, data) VALUES (?, ?, ?)",
        )
        .bind(&request.id)
        .bind(offset)
        .bind(data)
        .execute(&mut *connection)
        .await?;
        request.sync_total = Some(total);
        request.sync_uploaded = offset.saturating_add(signed(data.len() as u64)?);
        let digest = if request.sync_uploaded == total {
            request.status = ActivationStatus::Syncing;
            request.deadline = deadline(now, SYNC_SECONDS);
            Some(digest_of_parts(connection, &request.id, part.total_bytes).await?)
        } else {
            // The active client has this long again for its next part.
            request.deadline = deadline(now, UPLOAD_SECONDS);
            None
        };
        sqlx::query(
            "UPDATE product_activation_requests \
             SET status = ?, deadline = ?, sync_total = ?, sync_uploaded = ?, sync_digest = ? \
             WHERE id = ? AND status = 'approved'",
        )
        .bind(request.status.as_str())
        .bind(request.deadline)
        .bind(total)
        .bind(request.sync_uploaded)
        .bind(&digest)
        .bind(&request.id)
        .execute(&mut *connection)
        .await?;
        // Nobody will fetch for a client without device state: the whole
        // upload is parked for the owner and the switch happens now, in
        // this transaction. This session is inactive afterwards.
        if let (false, Some(digest)) = (request.requester_keeps_state, digest) {
            park(connection, &session, &request.id, total, &digest, now).await?;
            switch_active_client(connection, &session, &request, now).await?;
            close(connection, &request.id, ActivationStatus::Completed, now).await?;
            request.status = ActivationStatus::Completed;
            let session = live_session(connection, token_hash, now).await?;
            let shown = (ActivationRole::Approver, &request);
            return state_with(connection, &session, now, Some(shown)).await;
        }
    } else {
        return Err(ActivationRefusal::SyncOutOfOrder.into());
    }
    let shown = (ActivationRole::Approver, &request);
    state_with(connection, &session, now, Some(shown)).await
}

/// Replaces the owner's parked data with the request's whole upload. The
/// request's own parts are deleted when it closes.
async fn park(
    connection: &mut SqliteConnection,
    session: &Session,
    request_id: &str,
    total: i64,
    digest: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM product_parked_payloads WHERE issuer = ? AND subject = ?")
        .bind(&session.issuer)
        .bind(&session.subject)
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "INSERT INTO product_parked_payloads (issuer, subject, total, digest, parked_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(total)
    .bind(digest)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO product_parked_payload_parts (issuer, subject, part_offset, data) \
         SELECT ?, ?, part_offset, data FROM product_activation_sync WHERE request_id = ?",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(request_id)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn download(
    connection: &mut SqliteConnection,
    token_hash: &str,
    request_id: &str,
    offset: u64,
    now: i64,
) -> Step<Vec<u8>> {
    let (_, request) = step_request(
        connection,
        token_hash,
        request_id,
        ActivationRole::Requester,
        now,
    )
    .await?;
    if request.status != ActivationStatus::Syncing {
        return Err(ActivationRefusal::StepRefused.into());
    }
    let total = unsigned(request.sync_total.unwrap_or(0))?;
    let delivered = unsigned(request.sync_delivered)?;
    let Some(len) = sync_part_len(total, offset) else {
        return Err(ActivationRefusal::SyncOutOfOrder.into());
    };
    let end = offset.saturating_add(len);
    // The first part starts a pass over, the next part continues it, and the
    // part just handed out may be fetched again. Nothing else is in order.
    if offset != 0 && offset != delivered && end != delivered {
        return Err(ActivationRefusal::SyncOutOfOrder.into());
    }
    let data = stored_part(connection, &request.id, signed(offset)?)
        .await?
        .ok_or_else(|| sqlx::Error::Protocol("an uploaded part is missing".into()))?;
    sqlx::query(
        "UPDATE product_activation_requests SET sync_delivered = ? \
         WHERE id = ? AND status = 'syncing'",
    )
    .bind(signed(end)?)
    .bind(&request.id)
    .execute(&mut *connection)
    .await?;
    Ok(data)
}

async fn complete(
    connection: &mut SqliteConnection,
    token_hash: &str,
    request_id: &str,
    digest: &str,
    now: i64,
) -> Step<ActivationState> {
    let (session, mut request) = step_request(
        connection,
        token_hash,
        request_id,
        ActivationRole::Requester,
        now,
    )
    .await?;
    if !is_sync_digest(digest) {
        return Err(ActivationRefusal::InvalidSyncDigest.into());
    }
    match request.status {
        ActivationStatus::Completed => {}
        ActivationStatus::Syncing => {
            let stored = sqlx::query_scalar::<_, Option<String>>(
                "SELECT sync_digest FROM product_activation_requests WHERE id = ?",
            )
            .bind(&request.id)
            .fetch_one(&mut *connection)
            .await?;
            if stored.as_deref() != Some(digest) {
                return Err(ActivationRefusal::SyncIncomplete.into());
            }
            switch_active_client(connection, &session, &request, now).await?;
            close(connection, &request.id, ActivationStatus::Completed, now).await?;
            request.status = ActivationStatus::Completed;
        }
        _ => return Err(ActivationRefusal::StepRefused.into()),
    }
    let shown = (ActivationRole::Requester, &request);
    // The session is the active one now; read it again rather than assume.
    let session = live_session(connection, token_hash, now).await?;
    state_with(connection, &session, now, Some(shown)).await
}

/// The compare-and-swap. `advance` has already held the request against the
/// owner's epoch and both sessions in this same transaction; the conditions
/// are repeated in the statements so that a wrong assumption fails the
/// transaction instead of activating anyone.
async fn switch_active_client(
    connection: &mut SqliteConnection,
    session: &Session,
    request: &Request,
    now: i64,
) -> Result<(), sqlx::Error> {
    let advanced = sqlx::query(
        "UPDATE public_profiles SET session_epoch = session_epoch + 1 \
         WHERE issuer = ? AND subject = ? AND session_epoch = ?",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(request.epoch)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE product_sessions SET active = 0 WHERE issuer = ? AND subject = ? AND active = 1",
    )
    .bind(&session.issuer)
    .bind(&session.subject)
    .execute(&mut *connection)
    .await?;
    let activated = sqlx::query(
        "UPDATE product_sessions SET active = 1 \
         WHERE token_hash = ? AND issuer = ? AND subject = ? AND expires_at > ?",
    )
    .bind(&request.requester)
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    if advanced.rows_affected() == 1 && activated.rows_affected() == 1 {
        Ok(())
    } else {
        Err(sqlx::Error::Protocol(
            "the active client changed during the switch".into(),
        ))
    }
}

/// The live session behind `token_hash`, with its owner's epoch.
async fn live_session(
    connection: &mut SqliteConnection,
    token_hash: &str,
    now: i64,
) -> Step<Session> {
    let row = sqlx::query(
        "SELECT s.issuer, s.subject, s.active, s.client_label, p.session_epoch \
         FROM product_sessions s JOIN public_profiles p \
           ON p.issuer = s.issuer AND p.subject = s.subject AND p.profile_id = s.profile_id \
         WHERE s.token_hash = ? AND s.expires_at > ?",
    )
    .bind(token_hash)
    .bind(now)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(ActivationRefusal::AuthenticationRequired)?;
    Ok(Session {
        token_hash: token_hash.to_owned(),
        issuer: row.try_get("issuer")?,
        subject: row.try_get("subject")?,
        active: row.try_get("active")?,
        client_label: row.try_get("client_label")?,
        epoch: row.try_get("session_epoch")?,
    })
}

/// The caller's session and the request it names, with what is due applied.
/// A request of another owner, or one in which the caller does not hold
/// `role`, does not exist for the caller.
async fn step_request(
    connection: &mut SqliteConnection,
    token_hash: &str,
    request_id: &str,
    role: ActivationRole,
    now: i64,
) -> Step<(Session, Request)> {
    let session = live_session(connection, token_hash, now).await?;
    if !is_request_id(request_id) {
        return Err(ActivationRefusal::RequestNotFound.into());
    }
    let row = sqlx::query(&format!(
        "SELECT {REQUEST_COLUMNS} FROM product_activation_requests \
         WHERE id = ? AND issuer = ? AND subject = ?"
    ))
    .bind(request_id)
    .bind(&session.issuer)
    .bind(&session.subject)
    .fetch_optional(&mut *connection)
    .await?;
    let request = match row {
        Some(row) => decode(&row)?,
        None => return Err(ActivationRefusal::RequestNotFound.into()),
    };
    if request.role_of(&session) != Some(role) {
        return Err(ActivationRefusal::RequestNotFound.into());
    }
    let request = advance(connection, &session, request, now).await?;
    Ok((session, request))
}

async fn open_request(
    connection: &mut SqliteConnection,
    session: &Session,
) -> Result<Option<Request>, sqlx::Error> {
    sqlx::query(&format!(
        "SELECT {REQUEST_COLUMNS} FROM product_activation_requests \
         WHERE issuer = ? AND subject = ? AND status IN ('pending', 'approved', 'syncing')"
    ))
    .bind(&session.issuer)
    .bind(&session.subject)
    .fetch_optional(&mut *connection)
    .await?
    .map(|row| decode(&row))
    .transpose()
}

/// Applies what is due on an open request as of `now` and returns it as it
/// stands. A request lives only while the owner's epoch is the one it was
/// made under, the session that was active then is still the live active
/// one, the asking session is still live, and its step is within time.
async fn advance(
    connection: &mut SqliteConnection,
    session: &Session,
    mut request: Request,
    now: i64,
) -> Result<Request, sqlx::Error> {
    if !request.status.is_open() {
        return Ok(request);
    }
    let approver_is_active = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM product_sessions \
         WHERE token_hash = ? AND issuer = ? AND subject = ? AND active = 1 AND expires_at > ?)",
    )
    .bind(&request.approver)
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(now)
    .fetch_one(&mut *connection)
    .await?;
    let requester_is_live = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM product_sessions \
         WHERE token_hash = ? AND issuer = ? AND subject = ? AND expires_at > ?)",
    )
    .bind(&request.requester)
    .bind(&session.issuer)
    .bind(&session.subject)
    .bind(now)
    .fetch_one(&mut *connection)
    .await?;
    let outcome = if session.epoch != request.epoch || !approver_is_active {
        Some(ActivationStatus::Interrupted)
    } else if !requester_is_live {
        Some(ActivationStatus::Expired)
    } else if now < request.deadline {
        None
    } else if request.status == ActivationStatus::Approved {
        // The active client agreed and never uploaded: its side broke off.
        Some(ActivationStatus::Interrupted)
    } else {
        Some(ActivationStatus::Expired)
    };
    if let Some(outcome) = outcome {
        close(connection, &request.id, outcome, now).await?;
        request.status = outcome;
    }
    Ok(request)
}

/// Ends an open request with a final status and deletes its uploaded data.
async fn close(
    connection: &mut SqliteConnection,
    request_id: &str,
    status: ActivationStatus,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE product_activation_requests SET status = ?, resolved_at = ? \
         WHERE id = ? AND status IN ('pending', 'approved', 'syncing')",
    )
    .bind(status.as_str())
    .bind(now)
    .bind(request_id)
    .execute(&mut *connection)
    .await?;
    sqlx::query("DELETE FROM product_activation_sync WHERE request_id = ?")
        .bind(request_id)
        .execute(&mut *connection)
        .await?;
    Ok(())
}

async fn stored_part(
    connection: &mut SqliteConnection,
    request_id: &str,
    offset: i64,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT data FROM product_activation_sync WHERE request_id = ? AND part_offset = ?",
    )
    .bind(request_id)
    .bind(offset)
    .fetch_optional(&mut *connection)
    .await
}

/// The digest of an upload that is whole, read back part by part: what the
/// asking client must show it holds.
async fn digest_of_parts(
    connection: &mut SqliteConnection,
    request_id: &str,
    total: u64,
) -> Result<String, sqlx::Error> {
    let mut digest = SyncDigest::new();
    let mut offset = 0;
    while let Some(len) = sync_part_len(total, offset) {
        let part = stored_part(connection, request_id, signed(offset)?)
            .await?
            .filter(|part| part.len() as u64 == len)
            .ok_or_else(|| sqlx::Error::Protocol("an uploaded part is missing".into()))?;
        digest.update(&part);
        offset = offset.saturating_add(len);
        if len == 0 {
            break;
        }
    }
    Ok(digest.finish())
}

async fn state_of(
    connection: &mut SqliteConnection,
    token_hash: &str,
    now: i64,
    shown: Option<(ActivationRole, &Request)>,
) -> Step<ActivationState> {
    let session = live_session(connection, token_hash, now).await?;
    state_with(connection, &session, now, shown).await
}

/// The caller's state. `shown` is the request a step just acted on. Without
/// one, the active client is shown an open request it must answer and an
/// inactive client the last request it made.
async fn state_with(
    connection: &mut SqliteConnection,
    session: &Session,
    now: i64,
    shown: Option<(ActivationRole, &Request)>,
) -> Step<ActivationState> {
    let request = if let Some((role, request)) = shown {
        Some(request.view(role)?)
    } else if session.active {
        match open_request(connection, session).await? {
            Some(open) => {
                let open = advance(connection, session, open, now).await?;
                (open.status.is_open() && open.approver == session.token_hash)
                    .then(|| open.view(ActivationRole::Approver))
                    .transpose()?
            }
            None => None,
        }
    } else {
        let last = sqlx::query(&format!(
            "SELECT {REQUEST_COLUMNS} FROM product_activation_requests \
             WHERE requester_token_hash = ? AND issuer = ? AND subject = ? \
             ORDER BY created_at DESC, rowid DESC LIMIT 1"
        ))
        .bind(&session.token_hash)
        .bind(&session.issuer)
        .bind(&session.subject)
        .fetch_optional(&mut *connection)
        .await?;
        match last {
            Some(row) => {
                let last = advance(connection, session, decode(&row)?, now).await?;
                Some(last.view(ActivationRole::Requester)?)
            }
            None => None,
        }
    };
    let active_client_label = if session.active {
        None
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT client_label FROM product_sessions \
             WHERE issuer = ? AND subject = ? AND active = 1 AND expires_at > ?",
        )
        .bind(&session.issuer)
        .bind(&session.subject)
        .bind(now)
        .fetch_optional(&mut *connection)
        .await?
    };
    let state = ClientState::project(
        session.active,
        active_client_label.is_some(),
        request
            .as_ref()
            .map(|request| (request.role, request.status)),
    );
    let parked = if session.active && keeps_state(connection, &session.token_hash).await? {
        parked_of(connection, session).await?
    } else {
        None
    };
    Ok(ActivationState {
        state,
        active: session.active,
        may_act: state.may_act(),
        client_label: session.client_label.clone(),
        active_client_label,
        request,
        parked,
    })
}

fn decode(row: &SqliteRow) -> Result<Request, sqlx::Error> {
    let status: String = row.try_get("status")?;
    Ok(Request {
        id: row.try_get("id")?,
        requester: row.try_get("requester_token_hash")?,
        approver: row.try_get("approver_token_hash")?,
        requester_label: row.try_get("requester_label")?,
        approver_label: row.try_get("approver_label")?,
        epoch: row.try_get("epoch")?,
        status: ActivationStatus::parse(&status)
            .ok_or_else(|| sqlx::Error::Protocol("unknown activation request status".into()))?,
        created_at: row.try_get("created_at")?,
        deadline: row.try_get("deadline")?,
        sync_total: row.try_get("sync_total")?,
        sync_uploaded: row.try_get("sync_uploaded")?,
        sync_delivered: row.try_get("sync_delivered")?,
        requester_keeps_state: row.try_get("requester_keeps_state")?,
        approver_keeps_state: row.try_get("approver_keeps_state")?,
    })
}

fn deadline(now: i64, seconds: u64) -> i64 {
    now.saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX))
}

/// Counts a change of the owner's active client. Every open request made
/// under the previous epoch is void from here on.
pub(super) async fn advance_epoch(
    connection: &mut SqliteConnection,
    issuer: &str,
    subject: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE public_profiles SET session_epoch = session_epoch + 1 \
         WHERE issuer = ? AND subject = ?",
    )
    .bind(issuer)
    .bind(subject)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Deletes a session and ends its part in an open request: the asking
/// session leaving cancels it, the active session leaving interrupts it.
/// `owner` limits the deletion to that owner's sessions.
pub(super) async fn end_session(
    connection: &mut SqliteConnection,
    token_hash: &str,
    owner: Option<(&str, &str)>,
    now: i64,
) -> Result<(), sqlx::Error> {
    let Some(row) =
        sqlx::query("SELECT issuer, subject, active FROM product_sessions WHERE token_hash = ?")
            .bind(token_hash)
            .fetch_optional(&mut *connection)
            .await?
    else {
        return Ok(());
    };
    let issuer: String = row.try_get("issuer")?;
    let subject: String = row.try_get("subject")?;
    let active: bool = row.try_get("active")?;
    if owner.is_some_and(|owner| owner != (issuer.as_str(), subject.as_str())) {
        return Ok(());
    }
    let open = sqlx::query(
        "SELECT id, requester_token_hash, approver_token_hash FROM product_activation_requests \
         WHERE issuer = ? AND subject = ? AND status IN ('pending', 'approved', 'syncing')",
    )
    .bind(&issuer)
    .bind(&subject)
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(open) = open {
        let id: String = open.try_get("id")?;
        let outcome = if open.try_get::<String, _>("requester_token_hash")? == token_hash {
            Some(ActivationStatus::Cancelled)
        } else if open.try_get::<String, _>("approver_token_hash")? == token_hash {
            Some(ActivationStatus::Interrupted)
        } else {
            None
        };
        if let Some(outcome) = outcome {
            close(connection, &id, outcome, now).await?;
        }
    }
    sqlx::query("DELETE FROM product_sessions WHERE token_hash = ?")
        .bind(token_hash)
        .execute(&mut *connection)
        .await?;
    if active {
        advance_epoch(connection, &issuer, &subject).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
