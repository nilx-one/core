// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Existing admin inbox delivery, without Telegram API access in Core.

use serde::{Deserialize, Serialize};

use crate::bot_login::{ChatActivationQuery, WIRE_VERSION};

pub const PULL_PATH: &str = "/api/v1/telegram/admin/delivery/pull";
pub const ACK_PATH: &str = "/api/v1/telegram/admin/delivery/ack";
pub const DECIDE_PATH: &str = "/api/v1/telegram/admin/decide";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminDeliveryKind {
    Pending,
    Approved,
}

/// A lease for one existing outbox row, not permission to decide a request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminDeliveryLease {
    pub kind: AdminDeliveryKind,
    pub candidate_pub_dress: String,
    pub provider_actor_id: u64,
    pub claimed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdminInboxEntry {
    pub request_token: String,
    pub pub_dress: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdminInboxDelivery {
    pub lease: AdminDeliveryLease,
    /// Existing Telegram-issued ID, so the Bot edits rather than duplicates it.
    pub admin_message_id: Option<i32>,
    pub pending: Vec<AdminInboxEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdminDeliveryPull {
    pub deliveries: Vec<AdminInboxDelivery>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminDeliveryAck {
    pub version: u8,
    pub lease: AdminDeliveryLease,
    pub message_id: i32,
}

impl AdminDeliveryAck {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.version == WIRE_VERSION
            && self.message_id > 0
            && self.lease.provider_actor_id > 0
            && i64::try_from(self.lease.provider_actor_id).is_ok()
            && self.lease.claimed_at > 0
            && i64::try_from(self.lease.claimed_at).is_ok()
            && !self.lease.candidate_pub_dress.is_empty()
            && self.lease.candidate_pub_dress.len() <= 128
    }
}

/// The Telegram callback is a request, never role authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminDecisionRequest {
    pub chat: ChatActivationQuery,
    pub source_message_id: i32,
    pub request_token: String,
    pub approve: bool,
}

impl AdminDecisionRequest {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.chat.is_valid()
            && self.source_message_id > 0
            && self.request_token.len() == 32
            && self
                .request_token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminDecisionOutcome {
    Approved,
    Denied,
    AlreadyDecided,
    Forbidden,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdminDecisionResponse {
    pub outcome: AdminDecisionOutcome,
    /// Empty for a refused caller. Never grants authority to a button.
    pub pending: Vec<AdminInboxEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_delivery_and_ack_match_bot_wire() {
        let pull = serde_json::json!({"version": 1, "limit": 1});
        let request: crate::bot_notifications::NotificationPullRequest =
            serde_json::from_value(pull.clone()).unwrap();
        assert!(request.is_valid());
        assert_eq!(serde_json::to_value(request).unwrap(), pull);
        let lease = serde_json::json!({
            "kind": "approved",
            "candidate_pub_dress": "0x0new",
            "provider_actor_id": 42,
            "claimed_at": 1_800_000_000
        });
        let delivery = serde_json::json!({"deliveries": [{
            "lease": lease.clone(),
            "admin_message_id": 7,
            "pending": [{
                "request_token": "0123456789abcdef0123456789abcdef",
                "pub_dress": "0x0next"
            }]
        }]});
        let response: AdminDeliveryPull = serde_json::from_value(delivery.clone()).unwrap();
        assert_eq!(serde_json::to_value(response).unwrap(), delivery);
        let ack = serde_json::json!({"version": 1, "lease": lease, "message_id": 9});
        let receipt: AdminDeliveryAck = serde_json::from_value(ack.clone()).unwrap();
        assert!(receipt.is_valid());
        assert_eq!(serde_json::to_value(receipt).unwrap(), ack);
    }

    #[test]
    fn acknowledgement_carries_only_a_delivery_lease_and_message_receipt() {
        let body = serde_json::json!({
            "version": 1,
            "lease": {
                "kind": "pending",
                "candidate_pub_dress": "0x0new",
                "provider_actor_id": 42,
                "claimed_at": 1_800_000_000
            },
            "message_id": 17
        });
        let ack: AdminDeliveryAck = serde_json::from_value(body.clone()).unwrap();
        assert!(ack.is_valid());
        let mut forged = body;
        forged["approve"] = true.into();
        assert!(serde_json::from_value::<AdminDeliveryAck>(forged).is_err());
        assert!(
            !AdminDeliveryAck {
                message_id: 0,
                ..ack
            }
            .is_valid()
        );
    }
}
