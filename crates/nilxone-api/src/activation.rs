// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Moving the active client (`docs/client-activation.md`).
//!
//! A Bond's signed-in clients are each `active` or `inactive`, and at most
//! one is active. An inactive client asks; only the active client answers.
//! After it approves, the active client uploads its data in parts, the asking
//! client fetches them, and the switch happens when the asking client proves
//! it holds those exact bytes. Nothing else makes a signed-in client active.
//!
//! Every route but the part download answers with the caller's whole
//! [`ActivationState`], steps and percentage included, so a client draws one
//! value and decides nothing.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// `GET`: the caller's [`ActivationState`].
pub const STATE_PATH: &str = "/api/v1/session/activation";
/// `POST`, no body: an inactive client asks to become the active one (HTTP 201).
pub const REQUESTS_PATH: &str = "/api/v1/session/activation/requests";
/// `POST`, no body: the asking client withdraws its request.
pub const CANCEL_PATH: &str = "/api/v1/session/activation/requests/{id}/cancel";
/// `POST`, no body: the active client agrees to hand over.
pub const APPROVE_PATH: &str = "/api/v1/session/activation/requests/{id}/approve";
/// `POST`, no body: the active client refuses, before or after it agreed.
pub const DECLINE_PATH: &str = "/api/v1/session/activation/requests/{id}/decline";
/// `PUT` with [`UploadPartQuery`] and raw bytes: the active client uploads the
/// next part of its data. `GET` with [`DownloadPartQuery`]: the asking client
/// fetches one part as raw bytes, from offset `0` as often as it needs to
/// start over.
pub const SYNC_PATH: &str = "/api/v1/session/activation/requests/{id}/sync";
/// `POST` [`CompleteRequest`]: the asking client proves it holds the data and
/// becomes the active client.
pub const COMPLETE_PATH: &str = "/api/v1/session/activation/requests/{id}/complete";

/// A request id is 16 random bytes in lowercase hex.
pub const REQUEST_ID_LEN: usize = 32;
/// How long the active client has to answer a request.
pub const RESPOND_SECONDS: u64 = 120;
/// How long the active client has to upload its next part: after it agreed
/// and after every part.
pub const UPLOAD_SECONDS: u64 = 120;
/// How long the asking client has, once the upload is whole, to fetch the
/// data and complete.
pub const SYNC_SECONDS: u64 = 600;
/// The most bytes one handoff may carry.
pub const SYNC_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// The size of every part but the last, which carries what remains.
pub const SYNC_PART_BYTES: u64 = 256 * 1024;
/// The most requests one Bond's clients may open in an hour.
pub const REQUESTS_PER_HOUR: u32 = 8;
/// The longest device-and-host label, such as `iPhone · Safari`.
pub const CLIENT_LABEL_MAX_BYTES: usize = 80;
/// A sync digest is SHA-256 in lowercase hex.
pub const SYNC_DIGEST_LEN: usize = 64;

/// Whether `value` has the shape of a request id.
#[must_use]
pub fn is_request_id(value: &str) -> bool {
    value.len() == REQUEST_ID_LEN && is_lower_hex(value)
}

/// Whether `value` has the shape of a sync digest.
#[must_use]
pub fn is_sync_digest(value: &str) -> bool {
    value.len() == SYNC_DIGEST_LEN && is_lower_hex(value)
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether `value` may name a client: at most [`CLIENT_LABEL_MAX_BYTES`] and
/// no control characters. An empty label is allowed and names nothing.
#[must_use]
pub fn is_client_label(value: &str) -> bool {
    value.len() <= CLIENT_LABEL_MAX_BYTES && !value.chars().any(char::is_control)
}

/// One of the per-request paths, such as [`APPROVE_PATH`], for the request
/// `id`; `None` when `id` is not a request id.
#[must_use]
pub fn request_path(template: &str, id: &str) -> Option<String> {
    is_request_id(id).then(|| template.replace("{id}", id))
}

/// How many bytes the part at `offset` of a `total_bytes` handoff carries:
/// [`SYNC_PART_BYTES`], or what remains for the last part. `None` when no
/// part starts there: the total is over [`SYNC_MAX_BYTES`], or the offset is
/// not a part boundary or is at or past the end. An empty handoff has one
/// empty part at offset `0`.
#[must_use]
pub fn sync_part_len(total_bytes: u64, offset: u64) -> Option<u64> {
    if total_bytes > SYNC_MAX_BYTES || !offset.is_multiple_of(SYNC_PART_BYTES) {
        return None;
    }
    if total_bytes == 0 {
        return (offset == 0).then_some(0);
    }
    (offset < total_bytes).then(|| SYNC_PART_BYTES.min(total_bytes - offset))
}

/// [`sync_part_len`] as JSON, for a binding that passes strings. Both
/// arguments are canonical decimal integers. The part at `offset` is
/// `{"kind":"part","offset":0,"end":262144,"last":false}`: the bytes from
/// `offset` up to `end`, and whether it is the last one. `{"kind":"none"}`
/// when no part starts there.
#[must_use]
pub fn sync_part_wire(total_bytes: &str, offset: &str) -> String {
    let part = decimal(total_bytes)
        .zip(decimal(offset))
        .and_then(|(total, offset)| {
            let end = offset + sync_part_len(total, offset)?;
            Some(serde_json::json!({
                "kind": "part",
                "offset": offset,
                "end": end,
                "last": end == total,
            }))
        });
    part.unwrap_or_else(|| serde_json::json!({ "kind": "none" }))
        .to_string()
}

/// A canonical decimal `u64`: digits only and no leading zero.
fn decimal(value: &str) -> Option<u64> {
    let canonical = value == "0"
        || (!value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit()));
    canonical.then(|| value.parse().ok()).flatten()
}

/// The digest that proves a client holds `data`: SHA-256 in lowercase hex.
#[must_use]
pub fn sync_digest(data: &[u8]) -> String {
    let mut digest = SyncDigest::new();
    digest.update(data);
    digest.finish()
}

/// [`sync_digest`] over data that arrives in parts, in order.
#[derive(Clone, Default)]
pub struct SyncDigest(Sha256);

impl SyncDigest {
    /// A digest of no bytes yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the next part.
    pub fn update(&mut self, part: &[u8]) {
        self.0.update(part);
    }

    /// The digest of everything added.
    #[must_use]
    pub fn finish(self) -> String {
        use std::fmt::Write as _;

        self.0
            .finalize()
            .iter()
            .fold(String::with_capacity(SYNC_DIGEST_LEN), |mut hex, byte| {
                // Writing to a String cannot fail.
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }
}

/// Where a request stands. The first three are open; the rest are final.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationStatus {
    /// Waiting for the active client's answer.
    Pending,
    /// The active client agreed and is uploading its data.
    Approved,
    /// The data is whole on the service; the asking client fetches and
    /// completes.
    Syncing,
    /// The asking client became the active one.
    Completed,
    /// The active client refused before agreeing.
    Declined,
    /// The active client took its agreement back before the switch.
    Objected,
    /// The asking client withdrew, or signed out.
    Cancelled,
    /// A newer request replaced this one.
    Superseded,
    /// Nobody answered in time, the asking client did not complete in time,
    /// or its session ran out.
    Expired,
    /// The active client's side broke off: it signed out, its session ran
    /// out, the active client changed, or it never uploaded.
    Interrupted,
}

impl ActivationStatus {
    /// Every status, open ones first.
    pub const ALL: &'static [Self] = &[
        Self::Pending,
        Self::Approved,
        Self::Syncing,
        Self::Completed,
        Self::Declined,
        Self::Objected,
        Self::Cancelled,
        Self::Superseded,
        Self::Expired,
        Self::Interrupted,
    ];

    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Syncing => "syncing",
            Self::Completed => "completed",
            Self::Declined => "declined",
            Self::Objected => "objected",
            Self::Cancelled => "cancelled",
            Self::Superseded => "superseded",
            Self::Expired => "expired",
            Self::Interrupted => "interrupted",
        }
    }

    /// Parses the wire spelling.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|status| status.as_str() == value)
    }

    /// Whether the request can still move.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::Approved | Self::Syncing)
    }
}

/// Which side of a request the caller is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationRole {
    /// The client that asked.
    Requester,
    /// The client that was active when it was asked.
    Approver,
}

/// What a client shows. One value per screen; a client never combines fields
/// to work it out.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientState {
    /// This is the active client and nothing is asked of it.
    AuthenticatedActive,
    /// Active: another client asks to take over. Show approve and decline.
    ApprovalRequested,
    /// Active: it agreed and must upload its data now.
    UploadRequired,
    /// Active: its data is uploaded and the other client is fetching it. It
    /// may still decline.
    HandoffSyncing,
    /// Inactive: another client is active. Show the request action.
    ActiveElsewhere,
    /// Inactive, and no client is active: only signing in again makes one.
    SignInRequired,
    /// Inactive: its request waits for the active client. Show cancel.
    ActivationPending,
    /// Inactive: the active client agreed and is uploading.
    ActivationApproved,
    /// Inactive: fetch the data, apply it and complete.
    ActivationSyncing,
    /// Inactive: the active client declined its last request.
    ActivationRejected,
    /// Inactive: the active client agreed, then took it back.
    ActivationObjected,
    /// Inactive: it withdrew its last request.
    ActivationCancelled,
    /// Inactive: a newer request replaced its last one.
    ActivationSuperseded,
    /// Inactive: its last request ran out of time.
    ActivationExpired,
    /// Inactive: the active client's side broke off its last request.
    ActivationInterrupted,
}

impl ClientState {
    /// The state of a client that is `active` or not, given whether
    /// `another_is_active` and the request that concerns it: for the active
    /// client an open request it must answer, for an inactive one the last
    /// request it made.
    #[must_use]
    pub const fn project(
        active: bool,
        another_is_active: bool,
        request: Option<(ActivationRole, ActivationStatus)>,
    ) -> Self {
        use ActivationRole::{Approver, Requester};
        use ActivationStatus::{
            Approved, Cancelled, Declined, Expired, Interrupted, Objected, Pending, Superseded,
            Syncing,
        };

        if active {
            return match request {
                Some((Approver, Pending)) => Self::ApprovalRequested,
                Some((Approver, Approved)) => Self::UploadRequired,
                Some((Approver, Syncing)) => Self::HandoffSyncing,
                _ => Self::AuthenticatedActive,
            };
        }
        match request {
            Some((Requester, Pending)) => return Self::ActivationPending,
            Some((Requester, Approved)) => return Self::ActivationApproved,
            Some((Requester, Syncing)) => return Self::ActivationSyncing,
            _ => {}
        }
        if !another_is_active {
            return Self::SignInRequired;
        }
        match request {
            Some((Requester, Declined)) => Self::ActivationRejected,
            Some((Requester, Objected)) => Self::ActivationObjected,
            Some((Requester, Cancelled)) => Self::ActivationCancelled,
            Some((Requester, Superseded)) => Self::ActivationSuperseded,
            Some((Requester, Expired)) => Self::ActivationExpired,
            Some((Requester, Interrupted)) => Self::ActivationInterrupted,
            _ => Self::ActiveElsewhere,
        }
    }

    /// Whether this client may act for the Bond and change its own data. The
    /// active client stops once it agrees to hand over, so what it uploads
    /// stays the whole truth.
    #[must_use]
    pub const fn may_act(self) -> bool {
        matches!(self, Self::AuthenticatedActive | Self::ApprovalRequested)
    }
}

/// A step of the handoff, in the order a client lists them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStep {
    /// The active client confirms the request.
    Confirm,
    /// The active client's data travels to the asking client.
    Sync,
    /// The asking client becomes the active one.
    Switch,
}

/// Where a step stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Not reached yet.
    Waiting,
    /// Under way. While a request is open exactly one step is.
    Current,
    /// Finished.
    Done,
}

/// One row of the handoff's step list.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HandoffStepView {
    /// Which step.
    pub step: HandoffStep,
    /// Where it stands.
    pub state: StepState,
}

/// How far a handoff is, the same for both clients. A client shows these
/// values and never derives a step or a percentage of its own.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HandoffProgress {
    /// [`HandoffStep::Confirm`], [`HandoffStep::Sync`], [`HandoffStep::Switch`],
    /// in that order.
    pub steps: [HandoffStepView; 3],
    /// `0..=100`. The upload is the first half and the delivery the second;
    /// it is `100` only once the switch happened. A restarted fetch lowers it.
    pub percent: u8,
    /// The size of the data, known once the first part arrived.
    pub total_bytes: Option<u64>,
    /// How much of it the active client has uploaded.
    pub uploaded_bytes: u64,
    /// How much of it the asking client has been handed in its current pass.
    pub delivered_bytes: u64,
}

impl HandoffProgress {
    /// The progress of a request in `status` with these byte counts; `None`
    /// for a request that ended without a switch, which has none to show.
    #[must_use]
    pub fn project(
        status: ActivationStatus,
        total_bytes: Option<u64>,
        uploaded_bytes: u64,
        delivered_bytes: u64,
    ) -> Option<Self> {
        use StepState::{Current, Done, Waiting};

        // Each status bounds the counts: nothing travels before the active
        // client agreed, nothing is delivered before the upload is whole, and
        // a switch means all of it arrived.
        let total = total_bytes.unwrap_or(0);
        let (uploaded, delivered) = match status {
            ActivationStatus::Pending => (0, 0),
            ActivationStatus::Approved => (uploaded_bytes.min(total), 0),
            ActivationStatus::Syncing => (total, delivered_bytes.min(total)),
            ActivationStatus::Completed => (total, total),
            _ => return None,
        };
        let (steps, percent) = match status {
            ActivationStatus::Pending => ([Current, Waiting, Waiting], 0),
            ActivationStatus::Syncing if delivered == total => ([Done, Done, Current], 99),
            ActivationStatus::Completed => ([Done, Done, Done], 100),
            _ => (
                [Done, Current, Waiting],
                transfer_percent(total, uploaded, delivered),
            ),
        };
        let [confirm, sync, switch] = steps;
        let step = |step, state| HandoffStepView { step, state };
        Some(Self {
            steps: [
                step(HandoffStep::Confirm, confirm),
                step(HandoffStep::Sync, sync),
                step(HandoffStep::Switch, switch),
            ],
            percent,
            total_bytes,
            uploaded_bytes: uploaded,
            delivered_bytes: delivered,
        })
    }
}

/// The upload fills `0..=50` and the delivery `50..=99`.
fn transfer_percent(total: u64, uploaded: u64, delivered: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    let done = u128::from(uploaded) * 50 + u128::from(delivered) * 49;
    u8::try_from(done / u128::from(total)).unwrap_or(99)
}

/// One request, as the caller sees it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivationRequestView {
    /// The request's id.
    pub id: String,
    /// Which side the caller is.
    pub role: ActivationRole,
    /// Where it stands.
    pub status: ActivationStatus,
    /// The client that was active when it was asked, by device and host.
    pub from_label: String,
    /// The asking client, by device and host.
    pub to_label: String,
    /// When it was made, in Unix seconds.
    pub created_at: u64,
    /// When the current step runs out, in Unix seconds. Absent once final.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<u64>,
    /// [`HandoffProgress::project`] for this request. Absent once it ended
    /// without a switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<HandoffProgress>,
}

impl ActivationRequestView {
    /// Whether `progress` is what [`HandoffProgress::project`] gives for this
    /// request's own status and byte counts.
    fn progress_is_consistent(&self) -> bool {
        let expected = match &self.progress {
            Some(progress) => HandoffProgress::project(
                self.status,
                progress.total_bytes,
                progress.uploaded_bytes,
                progress.delivered_bytes,
            ),
            None => HandoffProgress::project(self.status, None, 0, 0),
        };
        self.progress == expected
    }
}

/// The answer of every activation route: what the caller is and what it
/// shows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivationState {
    /// What to show.
    pub state: ClientState,
    /// Whether this is the active client.
    pub active: bool,
    /// [`ClientState::may_act`] for `state`.
    pub may_act: bool,
    /// This client, by device and host.
    pub client_label: String,
    /// The active client, by device and host, when it is another one.
    pub active_client_label: Option<String>,
    /// The request that concerns this client, if any.
    pub request: Option<ActivationRequestView>,
}

/// The query of `PUT` [`SYNC_PATH`]. The body is the part's raw bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadPartQuery {
    /// Where the part starts: the `uploaded_bytes` of the last answer.
    pub offset: u64,
    /// The size of the whole data, the same on every part.
    pub total_bytes: u64,
}

impl UploadPartQuery {
    /// Checks that a part of `part_bytes` starts at `offset` of a handoff of
    /// `total_bytes`.
    ///
    /// # Errors
    ///
    /// [`ActivationRefusal::SyncTooLarge`] when the total is over
    /// [`SYNC_MAX_BYTES`]; [`ActivationRefusal::SyncOutOfOrder`] when
    /// [`sync_part_len`] has no part there or it is of another size.
    pub fn validate(&self, part_bytes: usize) -> Result<(), ActivationRefusal> {
        if self.total_bytes > SYNC_MAX_BYTES {
            return Err(ActivationRefusal::SyncTooLarge);
        }
        match sync_part_len(self.total_bytes, self.offset) {
            Some(expected) if u64::try_from(part_bytes) == Ok(expected) => Ok(()),
            _ => Err(ActivationRefusal::SyncOutOfOrder),
        }
    }
}

/// The query of `GET` [`SYNC_PATH`]. The answer is the part's raw bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadPartQuery {
    /// Where the part starts: `0`, then the end of the part before.
    pub offset: u64,
}

/// `POST` [`COMPLETE_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteRequest {
    /// [`sync_digest`] of the bytes the client fetched.
    pub digest: String,
}

impl CompleteRequest {
    /// Checks the digest's shape.
    ///
    /// # Errors
    ///
    /// [`ActivationRefusal::InvalidSyncDigest`] when it is not
    /// [`is_sync_digest`].
    pub fn validate(&self) -> Result<&str, ActivationRefusal> {
        if is_sync_digest(&self.digest) {
            Ok(&self.digest)
        } else {
            Err(ActivationRefusal::InvalidSyncDigest)
        }
    }
}

refusals! {
    /// Why an activation route did nothing.
    ActivationRefusal {
        AuthenticationRequired => ("provider_authentication_required", 401),
        AlreadyActive => ("already_active", 409),
        NoActiveClient => ("no_active_client", 409),
        RequestLimit => ("activation_request_limit", 429),
        RequestNotFound => ("activation_request_not_found", 404),
        StepRefused => ("activation_step_refused", 409),
        SyncTooLarge => ("sync_payload_too_large", 413),
        SyncOutOfOrder => ("sync_out_of_order", 409),
        InvalidSyncDigest => ("invalid_sync_digest", 422),
        SyncIncomplete => ("sync_incomplete", 409),
    }
}

/// What a client makes of the answer to an activation route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivationReading {
    /// The caller's state.
    State(ActivationState),
    /// The service refused; nothing changed. Read the state again.
    Refused {
        /// Why.
        reason: ActivationRefusal,
    },
    /// Nobody is signed in on this client.
    AuthenticationRequired,
    /// No usable answer. Try later; never assume a switch happened.
    Unavailable,
}

/// Reads an activation answer: `ok` is whether the HTTP status was a success.
#[must_use]
pub fn read_activation(ok: bool, body: &str) -> ActivationReading {
    if !ok {
        return match crate::error::error_code(body)
            .as_deref()
            .and_then(ActivationRefusal::from_code)
        {
            Some(ActivationRefusal::AuthenticationRequired) => {
                ActivationReading::AuthenticationRequired
            }
            Some(reason) => ActivationReading::Refused { reason },
            None => ActivationReading::Unavailable,
        };
    }
    serde_json::from_str::<ActivationState>(body).map_or(ActivationReading::Unavailable, |state| {
        // The state is the service's; a body whose own fields disagree is not
        // one to draw from.
        let consistent = state.may_act == state.state.may_act()
            && state
                .request
                .as_ref()
                .is_none_or(ActivationRequestView::progress_is_consistent);
        if consistent {
            ActivationReading::State(state)
        } else {
            ActivationReading::Unavailable
        }
    })
}

/// [`read_activation`] as JSON, for a binding that passes strings:
/// `{"kind":"state","state":…,"active":…,"may_act":…,"client_label":…,"active_client_label":…,"request":…}`,
/// `{"kind":"refused","reason":…}`, `{"kind":"authentication_required"}` or
/// `{"kind":"unavailable"}`.
#[must_use]
pub fn read_activation_wire(ok: bool, body: &str) -> String {
    serde_json::to_string(&read_activation(ok, body))
        .unwrap_or_else(|_| r#"{"kind":"unavailable"}"#.to_owned())
}

#[cfg(test)]
mod tests;
