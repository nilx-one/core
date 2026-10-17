// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Single-owner `BusinessPoint` declarations and deterministic verification state.
//!
//! A `BusinessPoint` is neither a Bond/Avaia nor a registry-backed physical
//! presence. Host adapters authenticate business representatives, persist
//! decisions, and deliver notifications. This module never performs I/O or
//! decides how manual verification is carried out.

use core::fmt;
use serde::{Deserialize, Serialize, Serializer};

use crate::{DecimalU64, GeoCoordinate, PubDress};

/// Product deadline, measured from the server-accepted submission, in seconds.
pub const BUSINESS_POINT_PENDING_SECONDS: u64 = 72 * 60 * 60;

/// Stable identity of one point within its owning business, not a `pub_dress`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessPointIdentity {
    pub id: String,
    pub name: String,
}

/// Structured declared postal address; optional unit details are private by
/// default, even when the pending marker is public at building-scale precision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessPointAddress {
    pub country: String,
    pub locality: String,
    pub street: String,
    pub number: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub building: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub office: Option<String>,
}

/// The coordinate is declared/geocoded geometry, never proof of occupancy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessPointLocation {
    pub address: BusinessPointAddress,
    pub coordinate: GeoCoordinate,
}

/// Finite application claim states, independent from the organization's
/// verified status, representative authority, and `REG-ATTEST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessPointStatus {
    Draft,
    Pending,
    Verified,
    Rejected,
    Expired,
    Withdrawn,
}

/// What an application map may discover for this point. A listing is not a
/// physical/digital presence right and is not inserted into `map.registry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessPointVisibility {
    Hidden,
    PendingPreview,
    VerifiedListing,
}

/// A manual decision is provided by an authorized host, never invented by Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessPointDecision {
    Verified,
    Rejected,
}

/// Timestamps use canonical decimal strings at JSON boundaries. Only the
/// transition methods create new state; deserialized state is validated.
/// `decided_at` records the verification outcome (or the expiry deadline);
/// `withdrawn_at` records an authorized withdrawal and never overwrites it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BusinessPointVerificationWire")]
pub struct BusinessPointVerification {
    status: BusinessPointStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    submitted_at: Option<DecimalU64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deadline_at: Option<DecimalU64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decided_at: Option<DecimalU64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    withdrawn_at: Option<DecimalU64>,
}

// Input is decoded separately so even standalone deserialization enforces
// the same lifecycle invariants as BusinessPoint's persisted wire boundary.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusinessPointVerificationWire {
    status: BusinessPointStatus,
    submitted_at: Option<DecimalU64>,
    deadline_at: Option<DecimalU64>,
    decided_at: Option<DecimalU64>,
    withdrawn_at: Option<DecimalU64>,
}

impl TryFrom<BusinessPointVerificationWire> for BusinessPointVerification {
    type Error = BusinessPointError;

    fn try_from(raw: BusinessPointVerificationWire) -> Result<Self, Self::Error> {
        let verification = Self {
            status: raw.status,
            submitted_at: raw.submitted_at,
            deadline_at: raw.deadline_at,
            decided_at: raw.decided_at,
            withdrawn_at: raw.withdrawn_at,
        };
        if !verification.valid() {
            return Err(BusinessPointError::InvalidState);
        }
        Ok(verification)
    }
}

impl BusinessPointVerification {
    const fn draft() -> Self {
        Self {
            status: BusinessPointStatus::Draft,
            submitted_at: None,
            deadline_at: None,
            decided_at: None,
            withdrawn_at: None,
        }
    }

    fn valid(&self) -> bool {
        match (
            self.status,
            self.submitted_at,
            self.deadline_at,
            self.decided_at,
            self.withdrawn_at,
        ) {
            (BusinessPointStatus::Draft, None, None, None, None) => true,
            (BusinessPointStatus::Pending, Some(start), Some(end), None, None) => {
                valid_window(start, end)
            }
            (
                BusinessPointStatus::Verified | BusinessPointStatus::Rejected,
                Some(start),
                Some(end),
                Some(decision),
                None,
            ) => valid_window(start, end) && decision >= start && decision < end,
            (BusinessPointStatus::Expired, Some(start), Some(end), Some(decision), None) => {
                valid_window(start, end) && decision == end
            }
            // Withdrawn while pending: no outcome, withdrawn inside the window.
            (BusinessPointStatus::Withdrawn, Some(start), Some(end), None, Some(withdrawn)) => {
                valid_window(start, end) && withdrawn >= start && withdrawn < end
            }
            // Withdrawn after verification: the verified outcome is preserved.
            (
                BusinessPointStatus::Withdrawn,
                Some(start),
                Some(end),
                Some(decision),
                Some(withdrawn),
            ) => {
                valid_window(start, end)
                    && decision >= start
                    && decision < end
                    && withdrawn >= decision
            }
            _ => false,
        }
    }
}

fn valid_window(start: DecimalU64, end: DecimalU64) -> bool {
    start.get().checked_add(BUSINESS_POINT_PENDING_SECONDS) == Some(end.get())
}

/// Persisted business-owned point. Its business subject does not become a
/// representative authority merely because it owns a Point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BusinessPointWire")]
pub struct BusinessPoint {
    #[serde(serialize_with = "serialize_owner")]
    owner_pub_dress: PubDress,
    identity: BusinessPointIdentity,
    location: BusinessPointLocation,
    verification: BusinessPointVerification,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusinessPointWire {
    owner_pub_dress: String,
    identity: BusinessPointIdentity,
    location: BusinessPointLocation,
    verification: BusinessPointVerification,
}

fn serialize_owner<S>(owner: &PubDress, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(owner.as_str())
}

impl TryFrom<BusinessPointWire> for BusinessPoint {
    type Error = BusinessPointError;

    fn try_from(raw: BusinessPointWire) -> Result<Self, Self::Error> {
        Self::validate_identity(&raw.identity)?;
        Self::validate_location(&raw.location)?;
        if !raw.verification.valid() {
            return Err(BusinessPointError::InvalidState);
        }
        Ok(Self {
            owner_pub_dress: raw
                .owner_pub_dress
                .parse()
                .map_err(|_| BusinessPointError::InvalidIdentity)?,
            identity: raw.identity,
            location: raw.location,
            verification: raw.verification,
        })
    }
}

impl BusinessPoint {
    /// Creates a private draft. The host must authenticate the `BBond` owner;
    /// parsing a `pub_dress` is not evidence that it is a business.
    ///
    /// # Errors
    ///
    /// Rejects empty, unbounded or control-character identity/address input.
    pub fn new_draft(
        owner_pub_dress: PubDress,
        identity: BusinessPointIdentity,
        location: BusinessPointLocation,
    ) -> Result<Self, BusinessPointError> {
        Self::validate_identity(&identity)?;
        Self::validate_location(&location)?;
        Ok(Self {
            owner_pub_dress,
            identity,
            location,
            verification: BusinessPointVerification::draft(),
        })
    }

    fn validate_identity(identity: &BusinessPointIdentity) -> Result<(), BusinessPointError> {
        if !valid_text(&identity.id, 128) || !valid_text(&identity.name, 256) {
            return Err(BusinessPointError::InvalidIdentity);
        }
        Ok(())
    }

    fn validate_location(location: &BusinessPointLocation) -> Result<(), BusinessPointError> {
        let addr = &location.address;
        if !valid_text(&addr.country, 128)
            || !valid_text(&addr.locality, 256)
            || !valid_text(&addr.street, 256)
            || !valid_text(&addr.number, 64)
            || !addr.building.as_deref().is_none_or(|x| valid_text(x, 128))
            || !addr.office.as_deref().is_none_or(|x| valid_text(x, 128))
        {
            return Err(BusinessPointError::InvalidAddress);
        }
        Ok(())
    }

    #[must_use]
    pub fn owner(&self) -> &PubDress {
        &self.owner_pub_dress
    }

    #[must_use]
    pub fn identity(&self) -> &BusinessPointIdentity {
        &self.identity
    }

    #[must_use]
    pub fn location(&self) -> &BusinessPointLocation {
        &self.location
    }

    #[must_use]
    pub fn verification(&self) -> &BusinessPointVerification {
        &self.verification
    }

    /// Treat an unresolved pending claim as expired at its exact deadline,
    /// irrespective of whether an expiry/cleanup job has run.
    #[must_use]
    pub fn effective_status(&self, now: DecimalU64) -> BusinessPointStatus {
        if self.verification.status == BusinessPointStatus::Pending
            && self.verification.deadline_at.is_some_and(|end| now >= end)
        {
            BusinessPointStatus::Expired
        } else {
            self.verification.status
        }
    }

    #[must_use]
    pub fn visibility(
        &self,
        now: DecimalU64,
        organization_verified: bool,
    ) -> BusinessPointVisibility {
        if !organization_verified {
            return BusinessPointVisibility::Hidden;
        }
        match self.effective_status(now) {
            BusinessPointStatus::Pending
                if self
                    .verification
                    .submitted_at
                    .is_some_and(|start| now >= start) =>
            {
                BusinessPointVisibility::PendingPreview
            }
            BusinessPointStatus::Verified => BusinessPointVisibility::VerifiedListing,
            _ => BusinessPointVisibility::Hidden,
        }
    }

    /// Starts the 72-hour period. An in-window retry does not reset it.
    /// An expired pending record cannot be renewed through the same call;
    /// explicitly expire and resubmit with a new host operation identifier.
    ///
    /// # Errors
    ///
    /// The host must have authenticated the representative; Core checks only
    /// the asserted organization status and the existing claim state.
    pub fn submit(
        &self,
        now: DecimalU64,
        organization_verified: bool,
    ) -> Result<Self, BusinessPointError> {
        if !organization_verified {
            return Err(BusinessPointError::UnverifiedOrganization);
        }
        match self.effective_status(now) {
            BusinessPointStatus::Draft => self.start_pending(now),
            BusinessPointStatus::Pending => Ok(self.clone()),
            BusinessPointStatus::Expired
                if self.verification.status == BusinessPointStatus::Pending =>
            {
                Err(BusinessPointError::Expired)
            }
            _ => Err(BusinessPointError::InvalidTransition),
        }
    }

    /// A fresh explicit submission after a terminal rejected/expired/
    /// withdrawn claim. The host owns operation-id deduplication.
    ///
    /// # Errors
    ///
    /// Rejects any current/pending/verified claim or unverified organization.
    pub fn resubmit(
        &self,
        now: DecimalU64,
        organization_verified: bool,
    ) -> Result<Self, BusinessPointError> {
        if !organization_verified {
            return Err(BusinessPointError::UnverifiedOrganization);
        }
        match self.verification.status {
            BusinessPointStatus::Rejected
            | BusinessPointStatus::Expired
            | BusinessPointStatus::Withdrawn => {
                let last = self
                    .verification
                    .withdrawn_at
                    .or(self.verification.decided_at);
                if last.is_some_and(|last| now < last) {
                    return Err(BusinessPointError::ClockReversal);
                }
                self.start_pending(now)
            }
            _ => Err(BusinessPointError::InvalidTransition),
        }
    }

    fn start_pending(&self, now: DecimalU64) -> Result<Self, BusinessPointError> {
        let deadline = now
            .get()
            .checked_add(BUSINESS_POINT_PENDING_SECONDS)
            .ok_or(BusinessPointError::TimestampOverflow)?;
        let mut next = self.clone();
        next.verification = BusinessPointVerification {
            status: BusinessPointStatus::Pending,
            submitted_at: Some(now),
            deadline_at: Some(DecimalU64::new(deadline)),
            decided_at: None,
            withdrawn_at: None,
        };
        Ok(next)
    }

    /// Records an authorized verification outcome. There is no decision
    /// procedure here, and a late result never revives an expired point.
    ///
    /// # Errors
    ///
    /// Rejects stale, early, or non-pending decisions.
    pub fn decide(
        &self,
        decision: BusinessPointDecision,
        now: DecimalU64,
    ) -> Result<Self, BusinessPointError> {
        let (start, deadline) = self.pending_window()?;
        if now < start {
            return Err(BusinessPointError::ClockReversal);
        }
        if now >= deadline {
            return Err(BusinessPointError::Expired);
        }
        let mut next = self.clone();
        next.verification.status = match decision {
            BusinessPointDecision::Verified => BusinessPointStatus::Verified,
            BusinessPointDecision::Rejected => BusinessPointStatus::Rejected,
        };
        next.verification.decided_at = Some(now);
        Ok(next)
    }

    /// Materializes expiry at the deadline, not at a delayed cleanup time.
    ///
    /// # Errors
    ///
    /// Fails when the claim is not pending or its deadline has not elapsed.
    pub fn expire(&self, now: DecimalU64) -> Result<Self, BusinessPointError> {
        let (_, deadline) = self.pending_window()?;
        if now < deadline {
            return Err(BusinessPointError::NotExpired);
        }
        let mut next = self.clone();
        next.verification.status = BusinessPointStatus::Expired;
        next.verification.decided_at = Some(deadline);
        Ok(next)
    }

    /// Withdrawal keeps any verified `decided_at`, records `withdrawn_at`, and
    /// does not create a `BondChain`.
    ///
    /// # Errors
    ///
    /// Only a non-expired pending or verified point can be withdrawn.
    pub fn withdraw(&self, now: DecimalU64) -> Result<Self, BusinessPointError> {
        if !matches!(
            self.verification.status,
            BusinessPointStatus::Pending | BusinessPointStatus::Verified
        ) {
            return Err(BusinessPointError::InvalidTransition);
        }
        if self.effective_status(now) == BusinessPointStatus::Expired {
            return Err(BusinessPointError::Expired);
        }
        let start = self
            .verification
            .submitted_at
            .ok_or(BusinessPointError::InvalidState)?;
        if now < start || self.verification.decided_at.is_some_and(|at| now < at) {
            return Err(BusinessPointError::ClockReversal);
        }
        let mut next = self.clone();
        next.verification.status = BusinessPointStatus::Withdrawn;
        next.verification.withdrawn_at = Some(now);
        Ok(next)
    }

    /// An address/coordinate change removes public eligibility and requires
    /// a new submission; stable identity and owner are preserved.
    ///
    /// # Errors
    ///
    /// Rejects invalid location components.
    pub fn amend_location(
        &self,
        location: BusinessPointLocation,
    ) -> Result<Self, BusinessPointError> {
        Self::validate_location(&location)?;
        if self.location == location {
            return Ok(self.clone());
        }
        let mut next = self.clone();
        next.location = location;
        next.verification = BusinessPointVerification::draft();
        Ok(next)
    }

    /// Changes the display name without resetting verified location truth.
    ///
    /// # Errors
    ///
    /// Rejects an invalid display name.
    pub fn rename(&self, name: String) -> Result<Self, BusinessPointError> {
        if !valid_text(&name, 256) {
            return Err(BusinessPointError::InvalidIdentity);
        }
        let mut next = self.clone();
        next.identity.name = name;
        Ok(next)
    }

    fn pending_window(&self) -> Result<(DecimalU64, DecimalU64), BusinessPointError> {
        if self.verification.status != BusinessPointStatus::Pending {
            return Err(BusinessPointError::InvalidTransition);
        }
        Ok((
            self.verification
                .submitted_at
                .ok_or(BusinessPointError::InvalidState)?,
            self.verification
                .deadline_at
                .ok_or(BusinessPointError::InvalidState)?,
        ))
    }
}

fn valid_text(value: &str, max_scalars: usize) -> bool {
    let n = value.chars().count();
    (1..=max_scalars).contains(&n)
        && !value.trim().is_empty()
        && !value.chars().any(char::is_control)
}

/// Stable, typed failures for service adapters and bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessPointError {
    InvalidIdentity,
    InvalidAddress,
    InvalidState,
    InvalidTransition,
    UnverifiedOrganization,
    Expired,
    NotExpired,
    ClockReversal,
    TimestampOverflow,
}

impl BusinessPointError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidIdentity => "invalid_identity",
            Self::InvalidAddress => "invalid_address",
            Self::InvalidState => "invalid_state",
            Self::InvalidTransition => "invalid_transition",
            Self::UnverifiedOrganization => "unverified_organization",
            Self::Expired => "expired",
            Self::NotExpired => "not_expired",
            Self::ClockReversal => "clock_reversal",
            Self::TimestampOverflow => "timestamp_overflow",
        }
    }
}

impl fmt::Display for BusinessPointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for BusinessPointError {}

/// One host-requested transition at the binding boundary. The host has
/// already authenticated the representative and the verification authority.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum BusinessPointCommand {
    Create {
        owner: String,
        identity: BusinessPointIdentity,
        location: BusinessPointLocation,
    },
    Submit {
        organization_verified: bool,
    },
    Resubmit {
        organization_verified: bool,
    },
    Decide {
        decision: BusinessPointDecision,
    },
    Expire,
    Withdraw,
    AmendLocation {
        location: BusinessPointLocation,
    },
    Rename {
        name: String,
    },
}

const INVALID: &str = "invalid";

fn wire_failure(code: &str) -> String {
    serde_json::json!({ "ok": false, "error": code }).to_string()
}

fn read_point(state: &str) -> Result<BusinessPoint, &'static str> {
    serde_json::from_str(state).map_err(|_| INVALID)
}

/// Applies one command to a stored `BusinessPoint`; `state` is empty only
/// for `create`. `now_s` is the service clock in seconds.
///
/// The answer is `{"ok":true,"state":…,"status":…}` with the next state to
/// store and its effective status, or `{"ok":false,"error":"<code>"}` with a
/// [`BusinessPointError`] code or `invalid` for a malformed state or command.
/// A refused command leaves the stored state as it was.
#[must_use]
pub fn apply_business_point_command(state: &str, command: &str, now_s: u64) -> String {
    let Ok(command) = serde_json::from_str::<BusinessPointCommand>(command) else {
        return wire_failure(INVALID);
    };
    let now = DecimalU64::new(now_s);
    let next = match command {
        BusinessPointCommand::Create {
            owner,
            identity,
            location,
        } => {
            if !state.is_empty() {
                return wire_failure(BusinessPointError::InvalidTransition.code());
            }
            owner
                .parse()
                .map_err(|_| BusinessPointError::InvalidIdentity)
                .and_then(|owner| BusinessPoint::new_draft(owner, identity, location))
        }
        command => {
            let point = match read_point(state) {
                Ok(point) => point,
                Err(code) => return wire_failure(code),
            };
            match command {
                BusinessPointCommand::Create { .. } => unreachable!("handled above"),
                BusinessPointCommand::Submit {
                    organization_verified,
                } => point.submit(now, organization_verified),
                BusinessPointCommand::Resubmit {
                    organization_verified,
                } => point.resubmit(now, organization_verified),
                BusinessPointCommand::Decide { decision } => point.decide(decision, now),
                BusinessPointCommand::Expire => point.expire(now),
                BusinessPointCommand::Withdraw => point.withdraw(now),
                BusinessPointCommand::AmendLocation { location } => point.amend_location(location),
                BusinessPointCommand::Rename { name } => point.rename(name),
            }
        }
    };
    match next {
        Ok(point) => serde_json::json!({
            "ok": true,
            "status": point.effective_status(now),
            "state": point,
        })
        .to_string(),
        Err(error) => wire_failure(error.code()),
    }
}

/// What a stored `BusinessPoint` is and shows at `now_s` (seconds), given the
/// host-asserted organization verification: `{"ok":true,"status":…,
/// "visibility":…}` or `{"ok":false,"error":"invalid"}`.
#[must_use]
pub fn business_point_view(state: &str, now_s: u64, organization_verified: bool) -> String {
    match read_point(state) {
        Ok(point) => {
            let now = DecimalU64::new(now_s);
            serde_json::json!({
                "ok": true,
                "status": point.effective_status(now),
                "visibility": point.visibility(now, organization_verified),
            })
            .to_string()
        }
        Err(code) => wire_failure(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_json;

    const START: u64 = 1_800_000_000;

    fn at(value: u64) -> DecimalU64 {
        DecimalU64::new(value)
    }

    fn location() -> BusinessPointLocation {
        BusinessPointLocation {
            address: BusinessPointAddress {
                country: "UA".into(),
                locality: "Kyiv".into(),
                street: "Volodymyrska".into(),
                number: "40".into(),
                building: Some("2".into()),
                office: Some("405".into()),
            },
            coordinate: GeoCoordinate::from_degrees(30.5131, 50.4489).unwrap(),
        }
    }

    fn draft() -> BusinessPoint {
        BusinessPoint::new_draft(
            "0xda-market".parse().unwrap(),
            BusinessPointIdentity {
                id: "point-1".into(),
                name: "Zoloti Vorota".into(),
            },
            location(),
        )
        .unwrap()
    }

    fn pending() -> BusinessPoint {
        draft().submit(at(START), true).unwrap()
    }

    #[test]
    fn standalone_verification_rejects_invalid_lifecycle_states() {
        assert!(serde_json::from_str::<BusinessPointVerification>(r#"{"status":"verified"}"#).is_err());

        let mut wrong_deadline = serde_json::to_value(pending().verification()).unwrap();
        wrong_deadline["deadline_at"] = serde_json::json!((START + 1).to_string());
        assert!(serde_json::from_value::<BusinessPointVerification>(wrong_deadline).is_err());

        let mut missing_withdrawal = serde_json::to_value(pending().verification()).unwrap();
        missing_withdrawal["status"] = serde_json::json!("withdrawn");
        assert!(serde_json::from_value::<BusinessPointVerification>(missing_withdrawal).is_err());

        let mut forged_withdrawal = serde_json::to_value(pending().verification()).unwrap();
        forged_withdrawal["withdrawn_at"] = serde_json::json!((START + 1).to_string());
        assert!(serde_json::from_value::<BusinessPointVerification>(forged_withdrawal).is_err());

        let mut extra_field = serde_json::to_value(pending().verification()).unwrap();
        extra_field["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<BusinessPointVerification>(extra_field).is_err());
    }

    #[test]
    fn standalone_verification_roundtrips_reachable_states() {
        let cases = [
            draft(),
            pending(),
            pending()
                .decide(BusinessPointDecision::Verified, at(START + 1))
                .unwrap(),
            pending()
                .decide(BusinessPointDecision::Rejected, at(START + 1))
                .unwrap(),
            pending()
                .expire(at(START + BUSINESS_POINT_PENDING_SECONDS))
                .unwrap(),
            pending().withdraw(at(START + 1)).unwrap(),
            pending()
                .decide(BusinessPointDecision::Verified, at(START + 1))
                .unwrap()
                .withdraw(at(START + 2))
                .unwrap(),
        ];
        for point in cases {
            let serialized = serde_json::to_value(point.verification()).unwrap();
            let parsed: BusinessPointVerification = serde_json::from_value(serialized).unwrap();
            assert_eq!(&parsed, point.verification());
        }
    }

    #[test]
    fn new_draft_is_private_and_unsubmitted() {
        let point = draft();
        assert_eq!(point.identity().id, "point-1");
        assert_eq!(point.owner().as_str(), "0xda-market");
        assert_eq!(
            point.effective_status(at(START)),
            BusinessPointStatus::Draft
        );
        assert_eq!(
            point.visibility(at(START), true),
            BusinessPointVisibility::Hidden
        );
    }

    #[test]
    fn seventy_two_hours_exact_and_retries_do_not_restart() {
        let point = pending();
        assert_eq!(point.submit(at(START + 40), true).unwrap(), point);
        assert_eq!(
            point.visibility(at(START + BUSINESS_POINT_PENDING_SECONDS - 1), true),
            BusinessPointVisibility::PendingPreview
        );
        assert_eq!(
            point.visibility(at(START + BUSINESS_POINT_PENDING_SECONDS), true),
            BusinessPointVisibility::Hidden
        );
        assert_eq!(
            point.submit(at(START + BUSINESS_POINT_PENDING_SECONDS), true),
            Err(BusinessPointError::Expired)
        );
        assert_eq!(
            point.effective_status(at(START + BUSINESS_POINT_PENDING_SECONDS)),
            BusinessPointStatus::Expired
        );
    }

    #[test]
    fn decision_must_be_in_pending_window() {
        let point = pending();
        assert_eq!(
            point.decide(BusinessPointDecision::Verified, at(START - 1)),
            Err(BusinessPointError::ClockReversal)
        );
        assert_eq!(
            point.decide(
                BusinessPointDecision::Verified,
                at(START + BUSINESS_POINT_PENDING_SECONDS)
            ),
            Err(BusinessPointError::Expired)
        );
        let verified = point
            .decide(BusinessPointDecision::Verified, at(START + 2))
            .unwrap();
        assert_eq!(
            verified.visibility(at(START + BUSINESS_POINT_PENDING_SECONDS + 10), true),
            BusinessPointVisibility::VerifiedListing
        );
        assert_eq!(
            verified.visibility(at(START + 3), false),
            BusinessPointVisibility::Hidden
        );
        assert_eq!(
            verified.decide(BusinessPointDecision::Rejected, at(START + 4)),
            Err(BusinessPointError::InvalidTransition)
        );
    }

    #[test]
    fn expiry_is_idempotently_deterministic_across_late_cleanup() {
        let point = pending();
        assert_eq!(
            point.expire(at(START + 1)),
            Err(BusinessPointError::NotExpired)
        );
        let exact = point
            .expire(at(START + BUSINESS_POINT_PENDING_SECONDS))
            .unwrap();
        let late = point
            .expire(at(START + BUSINESS_POINT_PENDING_SECONDS + 900))
            .unwrap();
        assert_eq!(exact, late);
        assert_eq!(
            exact.effective_status(at(START + 999_999)),
            BusinessPointStatus::Expired
        );
        assert_eq!(
            exact.visibility(at(START + 999_999), true),
            BusinessPointVisibility::Hidden
        );
    }

    #[test]
    fn rejected_expired_and_withdrawn_require_explicit_resubmission() {
        let rejected = pending()
            .decide(BusinessPointDecision::Rejected, at(START + 1))
            .unwrap();
        assert_eq!(
            rejected.submit(at(START + 2), true),
            Err(BusinessPointError::InvalidTransition)
        );
        let resubmitted = rejected.resubmit(at(START + 2), true).unwrap();
        assert_eq!(
            resubmitted.effective_status(at(START + 2)),
            BusinessPointStatus::Pending
        );
        let expired = pending()
            .expire(at(START + BUSINESS_POINT_PENDING_SECONDS))
            .unwrap();
        assert_eq!(
            expired.resubmit(at(START + BUSINESS_POINT_PENDING_SECONDS - 1), true),
            Err(BusinessPointError::ClockReversal)
        );
        assert!(
            expired
                .resubmit(at(START + BUSINESS_POINT_PENDING_SECONDS + 1), true)
                .is_ok()
        );
        let withdrawn = resubmitted.withdraw(at(START + 3)).unwrap();
        assert_eq!(
            withdrawn.visibility(at(START + 3), true),
            BusinessPointVisibility::Hidden
        );
        assert!(withdrawn.resubmit(at(START + 4), true).is_ok());
    }

    #[test]
    fn withdrawal_preserves_the_verified_decision() {
        let verified = pending()
            .decide(BusinessPointDecision::Verified, at(START + 1))
            .unwrap();
        let withdrawn = verified.withdraw(at(START + 5)).unwrap();
        assert_eq!(withdrawn.verification().decided_at, Some(at(START + 1)));
        assert_eq!(withdrawn.verification().withdrawn_at, Some(at(START + 5)));
        assert_eq!(
            withdrawn.resubmit(at(START + 4), true),
            Err(BusinessPointError::ClockReversal)
        );
        let reparsed: BusinessPoint =
            serde_json::from_value(serde_json::to_value(&withdrawn).unwrap()).unwrap();
        assert_eq!(reparsed, withdrawn);

        let from_pending = pending().withdraw(at(START + 2)).unwrap();
        assert_eq!(from_pending.verification().decided_at, None);
        assert_eq!(
            from_pending.verification().withdrawn_at,
            Some(at(START + 2))
        );

        let serialized = serde_json::to_value(&from_pending).unwrap();
        let mut forged = serialized.clone();
        forged["verification"]["withdrawn_at"] =
            serde_json::json!((START + BUSINESS_POINT_PENDING_SECONDS).to_string());
        assert!(serde_json::from_value::<BusinessPoint>(forged).is_err());
        let mut forged = serialized;
        forged["verification"]
            .as_object_mut()
            .unwrap()
            .remove("withdrawn_at");
        assert!(serde_json::from_value::<BusinessPoint>(forged).is_err());
    }

    #[test]
    fn organization_verification_does_not_automatically_verify_points() {
        let point = draft();
        assert_eq!(
            point.submit(at(START), false),
            Err(BusinessPointError::UnverifiedOrganization)
        );
        let pending = pending();
        assert_eq!(
            pending.visibility(at(START + 1), false),
            BusinessPointVisibility::Hidden
        );
        assert_eq!(
            pending.visibility(at(START + 1), true),
            BusinessPointVisibility::PendingPreview
        );
    }

    #[test]
    fn moving_a_point_preserves_identity_but_demands_new_claim() {
        let verified = pending()
            .decide(BusinessPointDecision::Verified, at(START + 1))
            .unwrap();
        let same = verified.amend_location(location()).unwrap();
        assert_eq!(same, verified);
        let mut new_location = location();
        new_location.address.number = "42".into();
        let amended = verified.amend_location(new_location).unwrap();
        assert_eq!(amended.identity().id, verified.identity().id);
        assert_eq!(amended.owner(), verified.owner());
        assert_eq!(
            amended.effective_status(at(START + 1)),
            BusinessPointStatus::Draft
        );
        assert_eq!(
            amended.visibility(at(START + 1), true),
            BusinessPointVisibility::Hidden
        );
        assert_eq!(
            verified
                .rename("Updated caption".into())
                .unwrap()
                .verification(),
            verified.verification()
        );
    }

    #[test]
    fn rejects_invalid_and_forged_serialized_claims() {
        let mut invalid = location();
        invalid.address.office = Some(String::new());
        assert_eq!(
            BusinessPoint::new_draft(
                "0xda-market".parse().unwrap(),
                BusinessPointIdentity {
                    id: "p".into(),
                    name: "test".into()
                },
                invalid,
            ),
            Err(BusinessPointError::InvalidAddress)
        );
        let serialized = serde_json::to_value(pending()).unwrap();
        let mut forged = serialized.clone();
        forged["verification"]["deadline_at"] = serde_json::json!(START + 1);
        assert!(serde_json::from_value::<BusinessPoint>(forged).is_err());
        let mut forged = serialized.clone();
        forged["verification"]["deadline_at"] = serde_json::json!((START + 1).to_string());
        assert!(serde_json::from_value::<BusinessPoint>(forged).is_err());
        let mut forged = serialized;
        forged["avaia"] = serde_json::json!("x0skai");
        assert!(serde_json::from_value::<BusinessPoint>(forged).is_err());
    }

    #[test]
    fn wire_roundtrip_never_uses_numeric_json_tokens() {
        let point = pending();
        let bytes = canonical_json(&point).unwrap();
        let reparsed: BusinessPoint = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(point, reparsed);
        let serialized = String::from_utf8(bytes).unwrap();
        assert!(serialized.contains("\"deadline_at\":\"1800259200\""));
        assert!(serialized.contains("\"latitude_e7\":\"504489000\""));
    }

    #[test]
    fn refuses_overflow() {
        assert_eq!(
            draft().submit(at(u64::MAX), true),
            Err(BusinessPointError::TimestampOverflow)
        );
    }

    fn ok_state(answer: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(answer).unwrap();
        assert_eq!(value["ok"], true, "{answer}");
        value["state"].to_string()
    }

    #[test]
    fn wire_drives_the_lifecycle() {
        let create = serde_json::json!({
            "op": "create",
            "owner": "0xda-market",
            "identity": { "id": "point-1", "name": "Zoloti Vorota" },
            "location": serde_json::to_value(location()).unwrap(),
        })
        .to_string();
        let created = apply_business_point_command("", &create, START);
        assert!(created.contains(r#""status":"draft""#), "{created}");
        let draft = ok_state(&created);
        assert_eq!(
            apply_business_point_command(&draft, &create, START),
            r#"{"error":"invalid_transition","ok":false}"#
        );
        assert_eq!(
            apply_business_point_command(
                &draft,
                r#"{"op":"submit","organization_verified":false}"#,
                START
            ),
            r#"{"error":"unverified_organization","ok":false}"#
        );
        let pending = ok_state(&apply_business_point_command(
            &draft,
            r#"{"op":"submit","organization_verified":true}"#,
            START,
        ));
        assert_eq!(
            business_point_view(&pending, START + 1, true),
            r#"{"ok":true,"status":"pending","visibility":"pending_preview"}"#
        );
        assert_eq!(
            business_point_view(&pending, START + BUSINESS_POINT_PENDING_SECONDS, true),
            r#"{"ok":true,"status":"expired","visibility":"hidden"}"#
        );
        let verified = ok_state(&apply_business_point_command(
            &pending,
            r#"{"op":"decide","decision":"verified"}"#,
            START + 2,
        ));
        assert_eq!(
            business_point_view(&verified, START + 3, true),
            r#"{"ok":true,"status":"verified","visibility":"verified_listing"}"#
        );
        assert_eq!(
            business_point_view(&verified, START + 3, false),
            r#"{"ok":true,"status":"verified","visibility":"hidden"}"#
        );
        let withdrawn = apply_business_point_command(&verified, r#"{"op":"withdraw"}"#, START + 4);
        assert!(withdrawn.contains(r#""status":"withdrawn""#), "{withdrawn}");
        assert!(
            withdrawn.contains(r#""decided_at":"1800000002""#),
            "{withdrawn}"
        );
    }

    #[test]
    fn wire_rejects_malformed_and_forged_input() {
        let invalid = r#"{"error":"invalid","ok":false}"#;
        assert_eq!(
            apply_business_point_command(
                "",
                r#"{"op":"submit","organization_verified":true}"#,
                START
            ),
            invalid
        );
        assert_eq!(
            apply_business_point_command("", r#"{"op":"verify"}"#, START),
            invalid
        );
        assert_eq!(
            apply_business_point_command("", r#"{"op":"expire","now":"1"}"#, START),
            invalid
        );
        let pending = serde_json::to_string(&pending()).unwrap();
        assert_eq!(
            apply_business_point_command(&pending, r#"{"op":"decide","decision":"maybe"}"#, START),
            invalid
        );
        let forged = pending.replace("\"pending\"", "\"verified\"");
        assert_eq!(business_point_view(&forged, START, true), invalid);
        assert_eq!(
            apply_business_point_command(&pending, r#"{"op":"expire"}"#, START + 1),
            r#"{"error":"not_expired","ok":false}"#
        );
    }
}
