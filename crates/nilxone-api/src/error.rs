// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The error body every route answers with, and the codes common to all routes.

use serde::{Deserialize, Serialize};

/// `{ "error": { "code": …, "message": … } }`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorEnvelope {
    /// What went wrong.
    pub error: ErrorBody,
}

/// The code a client branches on and a message for a person.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    /// A stable snake-case code. Clients branch on this, never on the message.
    pub code: String,
    /// Human-readable text. It may change.
    pub message: String,
}

impl ErrorEnvelope {
    /// Builds the body for `code` and `message`.
    #[must_use]
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            error: ErrorBody {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        }
    }
}

/// A mutating request carried no CSRF header (HTTP 403).
pub const CSRF_PROTECTION_REQUIRED: &str = "csrf_protection_required";
/// The client is signed in but not the active one (HTTP 403).
pub const SESSION_INACTIVE: &str = "session_inactive";
/// Too many requests; the answer carries a retry delay (HTTP 429).
pub const RATE_LIMITED: &str = "rate_limited";
/// The service cannot answer now; try again later (HTTP 503).
pub const IDENTITY_SERVICE_UNAVAILABLE: &str = "identity_service_unavailable";
