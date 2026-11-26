// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! What the back-end queues for the Telegram bot to say in an owner's chat
//! (`docs/bot-notifications.md`). The back-end decides what happened; the
//! bot only fetches, words and delivers it, then acknowledges it.

use serde::{Deserialize, Serialize};

use crate::bot_login::WIRE_VERSION;

/// POST [`NotificationPullRequest`], behind the bot's signature: the
/// undelivered notifications of the chats this bot signed in, oldest first.
pub const PULL_PATH: &str = "/api/v1/telegram/notifications/pull";
/// POST [`NotificationAck`]: the bot delivered these; the back-end drops them.
pub const ACK_PATH: &str = "/api/v1/telegram/notifications/ack";

/// The most notifications one pull answers, and one acknowledgement names.
pub const MAX_BATCH: usize = 50;
/// The longest label a notification carries, such as `Mac · Safari`.
pub const LABEL_MAX_BYTES: usize = 80;

/// What happened.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// A device asks to take over from the chat, the active client. The bot
    /// shows approve and decline (`bot_login::CHAT_ACTIVATION_ANSWER_PATH`).
    ActivationRequested,
    /// The Bond picked up a find of a notable rarity.
    Find,
    /// The Bond picked up an experience orb of a notable rarity.
    Orb,
    /// The Bond reached a landmark of a notable rarity.
    Landmark,
}

impl NotificationKind {
    /// Every kind, in a stable order.
    pub const ALL: [Self; 4] = [
        Self::ActivationRequested,
        Self::Find,
        Self::Orb,
        Self::Landmark,
    ];

    /// The stored and wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ActivationRequested => "activation_requested",
            Self::Find => "find",
            Self::Orb => "orb",
            Self::Landmark => "landmark",
        }
    }

    /// The kind named `value`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

/// The rarity a notable pick-up is told with, by the scale its object lives
/// on now. Finds still roll on the legacy catalog (`rare`, `legendary`)
/// until it migrates; orbs and landmarks use `RarityV3` (`rare`, `special`,
/// `exclusive`). A legacy `legendary` is never told as `special` or
/// `exclusive` (`docs/rarity-v3.md`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationRarity {
    Rare,
    Special,
    Exclusive,
    Legendary,
}

impl NotificationRarity {
    /// Every rarity, in a stable order.
    pub const ALL: [Self; 4] = [Self::Rare, Self::Special, Self::Exclusive, Self::Legendary];

    /// The stored and wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rare => "rare",
            Self::Special => "special",
            Self::Exclusive => "exclusive",
            Self::Legendary => "legendary",
        }
    }

    /// The rarity named `value`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|rarity| rarity.as_str() == value)
    }
}

/// POST [`PULL_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPullRequest {
    /// The expected wire contract.
    pub version: u8,
    /// At most this many, `1..=`[`MAX_BATCH`].
    pub limit: usize,
}

impl NotificationPullRequest {
    /// Whether the request has the shape the back-end answers.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.version == WIRE_VERSION && (1..=MAX_BATCH).contains(&self.limit)
    }
}

/// One thing to say in one chat.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotNotification {
    /// To acknowledge it with.
    pub id: u64,
    /// The Telegram sender whose private chat it goes to.
    pub provider_actor_id: u64,
    pub kind: NotificationKind,
    /// For a find, an orb or a landmark.
    pub rarity: Option<NotificationRarity>,
    /// For a request, the client that asks; for a pick-up, the object's
    /// display name if the back-end has one. At most [`LABEL_MAX_BYTES`].
    pub label: Option<String>,
    /// For a request, its id, to answer it with.
    pub request_id: Option<String>,
    /// When it happened, in Unix seconds.
    pub created_at: u64,
}

/// The answer to [`PULL_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPull {
    pub notifications: Vec<BotNotification>,
}

/// POST [`ACK_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAck {
    /// The expected wire contract.
    pub version: u8,
    /// Delivered notifications, at most [`MAX_BATCH`].
    pub ids: Vec<u64>,
}

impl NotificationAck {
    /// Whether the acknowledgement has the shape the back-end takes.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.version == WIRE_VERSION && !self.ids.is_empty() && self.ids.len() <= MAX_BATCH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_stay_stable() {
        for kind in NotificationKind::ALL {
            assert_eq!(NotificationKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{}\"", kind.as_str())
            );
        }
        for rarity in NotificationRarity::ALL {
            assert_eq!(NotificationRarity::parse(rarity.as_str()), Some(rarity));
        }
        assert_eq!(NotificationRarity::parse("common"), None);
    }

    #[test]
    fn requests_are_bounded() {
        assert!(
            NotificationPullRequest {
                version: 1,
                limit: 50
            }
            .is_valid()
        );
        assert!(
            !NotificationPullRequest {
                version: 1,
                limit: 0
            }
            .is_valid()
        );
        assert!(
            !NotificationPullRequest {
                version: 1,
                limit: 51
            }
            .is_valid()
        );
        assert!(
            !NotificationPullRequest {
                version: 2,
                limit: 1
            }
            .is_valid()
        );
        assert!(
            NotificationAck {
                version: 1,
                ids: vec![1]
            }
            .is_valid()
        );
        assert!(
            !NotificationAck {
                version: 1,
                ids: vec![]
            }
            .is_valid()
        );
        assert!(
            !NotificationAck {
                version: 1,
                ids: (0..51).collect()
            }
            .is_valid()
        );
    }
}
