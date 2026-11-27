// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Short-lived Telegram presentation of a canonical spoken line.
//! No coordinates, identity credentials, or `BondChain` evidence.

use serde::{Deserialize, Serialize};

use crate::bot_login::WIRE_VERSION;

pub const PULL_PATH: &str = "/api/v1/telegram/speech/pull";
pub const ACK_PATH: &str = "/api/v1/telegram/speech/ack";
pub const TTL_SECONDS: u64 = 600;
pub const LEASE_SECONDS: u64 = 60;
pub const MAX_RECIPIENTS: usize = 100;
pub const MAX_BATCH: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechDelivery {
    pub id: u64,
    pub provider_actor_id: u64,
    pub speaker: String,
    pub text: String,
    pub expires_at: u64,
    pub claimed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechPull {
    pub deliveries: Vec<SpeechDelivery>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechAck {
    pub version: u8,
    pub id: u64,
    pub claimed_at: u64,
}

impl SpeechAck {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.version == WIRE_VERSION
            && self.id > 0
            && i64::try_from(self.id).is_ok()
            && i64::try_from(self.claimed_at).is_ok()
    }
}
