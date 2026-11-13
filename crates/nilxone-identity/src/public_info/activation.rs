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
    ClientState, HandoffProgress, REQUESTS_PER_HOUR, RESPOND_SECONDS, SYNC_SECONDS, SyncDigest,
    UPLOAD_SECONDS, UploadPartQuery, is_request_id, is_sync_digest, sync_part_len,
};
use sqlx::{Row, Sqlite, SqliteConnection, Transaction, sqlite::SqliteRow};

use super::PublicInfoRepository;

const HOUR_SECONDS: i64 = 60 * 60;
const REQUEST_COLUMNS: &str = "id, requester_token_hash, approver_token_hash, requester_label, \
                               approver_label, epoch, status, created_at, deadline, sync_total, \
                               sync_uploaded, sync_delivered";

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
        let result = request(&mut transaction, token_hash, clock(now)?).await;
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

async fn request(
    connection: &mut SqliteConnection,
    token_hash: &str,
    now: i64,
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
          approver_label, epoch, status, created_at, deadline) \
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?) \
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
    .fetch_one(&mut *connection)
    .await?;
    let created = decode(&row)?;
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
        .bind(digest)
        .bind(&request.id)
        .execute(&mut *connection)
        .await?;
    } else {
        return Err(ActivationRefusal::SyncOutOfOrder.into());
    }
    let shown = (ActivationRole::Approver, &request);
    state_with(connection, &session, now, Some(shown)).await
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
    Ok(ActivationState {
        state,
        active: session.active,
        may_act: state.may_act(),
        client_label: session.client_label.clone(),
        active_client_label,
        request,
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
