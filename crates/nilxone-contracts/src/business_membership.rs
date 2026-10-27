// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Membership is scoped to one business, never a global Bond role or subscription.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessMemberRole {
    Member,
    SpecialMember,
    Maintainer,
    Administrator,
    CoOwner,
    Owner,
}

impl BusinessMemberRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::SpecialMember => "special_member",
            Self::Maintainer => "maintainer",
            Self::Administrator => "administrator",
            Self::CoOwner => "co_owner",
            Self::Owner => "owner",
        }
    }
}

/// A creator may choose only Owner (default) or Maintainer for their own membership.
/// Maintainer creation does not fabricate another owner or upgrade the creator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessCreatorRole {
    #[default]
    Owner,
    Maintainer,
}

impl From<BusinessCreatorRole> for BusinessMemberRole {
    fn from(role: BusinessCreatorRole) -> Self {
        match role {
            BusinessCreatorRole::Owner => Self::Owner,
            BusinessCreatorRole::Maintainer => Self::Maintainer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_wire_is_closed_and_not_subscription_wire() {
        for role in [
            BusinessMemberRole::Member,
            BusinessMemberRole::SpecialMember,
            BusinessMemberRole::Maintainer,
            BusinessMemberRole::Administrator,
            BusinessMemberRole::CoOwner,
            BusinessMemberRole::Owner,
        ] {
            let value = serde_json::to_value(role).unwrap();
            assert_eq!(value, role.as_str());
            assert_eq!(
                serde_json::from_value::<BusinessMemberRole>(value).unwrap(),
                role
            );
        }
        for invalid in ["special", "exclusive", "admin", "business"] {
            assert!(
                serde_json::from_value::<BusinessMemberRole>(serde_json::json!(invalid)).is_err()
            );
        }
    }

    #[test]
    fn creator_cannot_request_arbitrary_role() {
        assert_eq!(
            BusinessMemberRole::from(BusinessCreatorRole::default()),
            BusinessMemberRole::Owner
        );
        assert_eq!(
            BusinessMemberRole::from(BusinessCreatorRole::Maintainer),
            BusinessMemberRole::Maintainer
        );
        for invalid in ["member", "special_member", "administrator", "co_owner"] {
            assert!(
                serde_json::from_value::<BusinessCreatorRole>(serde_json::json!(invalid)).is_err()
            );
        }
    }
}
