// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! External provider bindings of a human Bond: which Telegram, Discord or
//! GitHub account is connected, and the rules for connecting, disconnecting
//! and looking one up (`nilx-one/0x1` `documents/04-identity.md`,
//! "Provider-binding rules").
//!
//! Core decides; the host reads and writes storage. A host loads a snapshot of
//! what storage holds, asks Core for a decision, and carries it out in the
//! same transaction it read the snapshot in. Core never reads storage itself.
//!
//! The rules enforced here:
//!
//! - one external account binds to at most one Bond (rule 3);
//! - one Bond holds at most one account per provider type (rule 2);
//! - a disconnect never leaves a Bond with no way back in (unlink rule);
//! - a lookup by provider account answers with the public `pub_dress` only,
//!   never an internal identifier, and asks the person to sign in when the
//!   account is not bound.

use core::{fmt, str::FromStr};

use crate::pub_dress::PubDress;

/// The longest provider subject accepted, in bytes. Provider subjects are
/// opaque account identifiers, not names.
pub const MAX_PROVIDER_SUBJECT_BYTES: usize = 256;

/// An external account authority a human Bond can connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderType {
    Telegram,
    Discord,
    Github,
}

impl ProviderType {
    /// Every provider type, in the order a Bond's connections are listed.
    pub const ALL: [Self; 3] = [Self::Telegram, Self::Discord, Self::Github];

    /// Returns the binding-safe provider name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Github => "github",
        }
    }
}

impl fmt::Display for ProviderType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A provider name that is not a supported provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownProvider;

impl fmt::Display for UnknownProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("not a supported provider")
    }
}

impl std::error::Error for UnknownProvider {}

impl FromStr for ProviderType {
    type Err = UnknownProvider;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == value)
            .ok_or(UnknownProvider)
    }
}

/// Why a provider subject was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSubjectError {
    Empty,
    TooLong,
    /// The subject contains whitespace or a control character.
    InvalidCharacter,
}

impl ProviderSubjectError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty_subject",
            Self::TooLong => "subject_too_long",
            Self::InvalidCharacter => "invalid_subject_character",
        }
    }
}

impl fmt::Display for ProviderSubjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("the provider subject is empty"),
            Self::TooLong => write!(
                formatter,
                "the provider subject is longer than {MAX_PROVIDER_SUBJECT_BYTES} bytes"
            ),
            Self::InvalidCharacter => formatter
                .write_str("the provider subject contains whitespace or a control character"),
        }
    }
}

impl std::error::Error for ProviderSubjectError {}

/// A provider's opaque, stable account identifier, kept exactly as the
/// provider issued it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderSubject(String);

impl ProviderSubject {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProviderSubject {
    type Err = ProviderSubjectError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(ProviderSubjectError::Empty);
        }
        if value.len() > MAX_PROVIDER_SUBJECT_BYTES {
            return Err(ProviderSubjectError::TooLong);
        }
        if value
            .chars()
            .any(|scalar| scalar.is_whitespace() || scalar.is_control())
        {
            return Err(ProviderSubjectError::InvalidCharacter);
        }
        Ok(Self(value.to_owned()))
    }
}

/// One external account: its provider type and subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderAccount {
    pub provider: ProviderType,
    pub subject: ProviderSubject,
}

/// What storage holds for a link request, read in the transaction that will
/// carry out the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSnapshot {
    /// Whether the human Bond being linked exists.
    pub bond_exists: bool,
    /// The Bond this account is already bound to, if any.
    pub account_owner: Option<PubDress>,
    /// The subject the Bond already holds for this provider type, if any.
    pub bond_subject: Option<ProviderSubject>,
}

/// Core's answer to "connect this account to this Bond".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkDecision {
    /// Store the binding.
    Insert,
    /// This exact binding already exists; nothing to store.
    AlreadyLinked,
    /// The account is bound to another Bond (rule 3).
    ProviderAlreadyLinked,
    /// The Bond already holds another account of this provider type (rule 2).
    ProviderTypeAlreadyLinked,
    /// There is no human Bond to link.
    IdentityMissing,
}

impl LinkDecision {
    /// Returns the binding-safe outcome code. The two refusals match the codes
    /// a sign-in window shows the person.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Insert => "linked",
            Self::AlreadyLinked => "already_linked",
            Self::ProviderAlreadyLinked => "provider_already_linked",
            Self::ProviderTypeAlreadyLinked => "provider_type_already_linked",
            Self::IdentityMissing => "identity_missing",
        }
    }
}

/// Decides whether `account` may be connected to `bond`.
///
/// Rule 3 is checked before rule 2: an account that belongs to someone else is
/// reported as such even if this Bond also holds another account of its type.
#[must_use]
pub fn decide_link(
    bond: &PubDress,
    account: &ProviderAccount,
    snapshot: &LinkSnapshot,
) -> LinkDecision {
    if !snapshot.bond_exists {
        return LinkDecision::IdentityMissing;
    }
    match &snapshot.account_owner {
        Some(owner) if owner == bond => return LinkDecision::AlreadyLinked,
        Some(_) => return LinkDecision::ProviderAlreadyLinked,
        None => {}
    }
    match &snapshot.bond_subject {
        Some(subject) if *subject == account.subject => LinkDecision::AlreadyLinked,
        Some(_) => LinkDecision::ProviderTypeAlreadyLinked,
        None => LinkDecision::Insert,
    }
}

/// What storage holds for a self-service disconnect, read in the transaction
/// that will carry out the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisconnectSnapshot {
    /// Whether the Bond holds an account of the provider being disconnected.
    pub linked: bool,
    /// How many accounts of other provider types the Bond holds.
    pub other_providers: u32,
    /// Whether the Bond has an active native password.
    pub password_active: bool,
}

/// Core's answer to "disconnect this provider from this Bond".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectDecision {
    /// Remove the binding.
    Delete,
    /// The Bond holds no account of this provider.
    NotLinked,
    /// Refused: it is the Bond's only way back in.
    SoleAccessPath,
}

impl DisconnectDecision {
    /// Returns the binding-safe outcome code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Delete => "disconnected",
            Self::NotLinked => "not_linked",
            Self::SoleAccessPath => "sole_access_path",
        }
    }
}

/// Decides whether a provider may be disconnected at its own request.
#[must_use]
pub const fn decide_self_disconnect(snapshot: DisconnectSnapshot) -> DisconnectDecision {
    if !snapshot.linked {
        return DisconnectDecision::NotLinked;
    }
    if snapshot.other_providers == 0 && !snapshot.password_active {
        return DisconnectDecision::SoleAccessPath;
    }
    DisconnectDecision::Delete
}

/// What a partner (for example a business in the 0x1 world) is told when it
/// asks which Bond a provider account belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderLookupView {
    /// Show the Bond's public handle.
    Linked { pub_dress: PubDress },
    /// The account is not bound: ask the person to sign in with this provider
    /// to connect it.
    SignInRequired { provider: ProviderType },
}

/// Answers a lookup from the Bond the storage reports for the account.
#[must_use]
pub fn provider_lookup_view(
    account: &ProviderAccount,
    owner: Option<PubDress>,
) -> ProviderLookupView {
    match owner {
        Some(pub_dress) => ProviderLookupView::Linked { pub_dress },
        None => ProviderLookupView::SignInRequired {
            provider: account.provider,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pub_dress(value: &str) -> PubDress {
        value.parse().expect("pub_dress")
    }

    fn account(subject: &str) -> ProviderAccount {
        ProviderAccount {
            provider: ProviderType::Telegram,
            subject: subject.parse().expect("subject"),
        }
    }

    fn snapshot(owner: Option<&str>, subject: Option<&str>) -> LinkSnapshot {
        LinkSnapshot {
            bond_exists: true,
            account_owner: owner.map(pub_dress),
            bond_subject: subject.map(|value| value.parse().expect("subject")),
        }
    }

    #[test]
    fn provider_names_round_trip_and_unknown_names_fail() {
        for provider in ProviderType::ALL {
            assert_eq!(provider.as_str().parse::<ProviderType>(), Ok(provider));
        }
        assert_eq!("apple".parse::<ProviderType>(), Err(UnknownProvider));
        assert_eq!("Telegram".parse::<ProviderType>(), Err(UnknownProvider));
    }

    #[test]
    fn subjects_are_opaque_but_bounded() {
        assert_eq!(
            "123456789"
                .parse::<ProviderSubject>()
                .map(|s| s.as_str().to_owned()),
            Ok("123456789".to_owned())
        );
        assert_eq!(
            "".parse::<ProviderSubject>(),
            Err(ProviderSubjectError::Empty)
        );
        assert_eq!(
            "12 34".parse::<ProviderSubject>(),
            Err(ProviderSubjectError::InvalidCharacter)
        );
        assert_eq!(
            "12\u{0}".parse::<ProviderSubject>(),
            Err(ProviderSubjectError::InvalidCharacter)
        );
        assert_eq!(
            "9".repeat(MAX_PROVIDER_SUBJECT_BYTES + 1)
                .parse::<ProviderSubject>(),
            Err(ProviderSubjectError::TooLong)
        );
    }

    #[test]
    fn a_free_account_links_to_a_bond_without_one_of_its_type() {
        let bond = pub_dress("0x0Sky");
        assert_eq!(
            decide_link(&bond, &account("1"), &snapshot(None, None)),
            LinkDecision::Insert
        );
    }

    #[test]
    fn relinking_the_same_account_is_already_linked() {
        let bond = pub_dress("0x0Sky");
        assert_eq!(
            decide_link(&bond, &account("1"), &snapshot(Some("0x0Sky"), Some("1"))),
            LinkDecision::AlreadyLinked
        );
        assert_eq!(
            decide_link(&bond, &account("1"), &snapshot(None, Some("1"))),
            LinkDecision::AlreadyLinked
        );
    }

    #[test]
    fn an_account_of_another_bond_is_refused_before_the_type_rule() {
        let bond = pub_dress("0x0Sky");
        let decision = decide_link(&bond, &account("1"), &snapshot(Some("0x1Other"), Some("2")));
        assert_eq!(decision, LinkDecision::ProviderAlreadyLinked);
        assert_eq!(decision.code(), "provider_already_linked");
    }

    #[test]
    fn a_bond_holds_one_account_per_provider_type() {
        let bond = pub_dress("0x0Sky");
        let decision = decide_link(&bond, &account("1"), &snapshot(None, Some("2")));
        assert_eq!(decision, LinkDecision::ProviderTypeAlreadyLinked);
        assert_eq!(decision.code(), "provider_type_already_linked");
    }

    #[test]
    fn a_missing_bond_is_never_linked() {
        let missing = LinkSnapshot {
            bond_exists: false,
            account_owner: None,
            bond_subject: None,
        };
        assert_eq!(
            decide_link(&pub_dress("0x0Sky"), &account("1"), &missing),
            LinkDecision::IdentityMissing
        );
    }

    #[test]
    fn a_disconnect_never_strands_a_bond() {
        let only_way_in = DisconnectSnapshot {
            linked: true,
            other_providers: 0,
            password_active: false,
        };
        assert_eq!(
            decide_self_disconnect(only_way_in),
            DisconnectDecision::SoleAccessPath
        );
        assert_eq!(
            decide_self_disconnect(DisconnectSnapshot {
                password_active: true,
                ..only_way_in
            }),
            DisconnectDecision::Delete
        );
        assert_eq!(
            decide_self_disconnect(DisconnectSnapshot {
                other_providers: 1,
                ..only_way_in
            }),
            DisconnectDecision::Delete
        );
        assert_eq!(
            decide_self_disconnect(DisconnectSnapshot {
                linked: false,
                ..only_way_in
            }),
            DisconnectDecision::NotLinked
        );
    }

    #[test]
    fn a_lookup_shows_the_handle_or_asks_to_sign_in() {
        let telegram = account("1");
        assert_eq!(
            provider_lookup_view(&telegram, Some(pub_dress("0x0Sky"))),
            ProviderLookupView::Linked {
                pub_dress: pub_dress("0x0Sky")
            }
        );
        assert_eq!(
            provider_lookup_view(&telegram, None),
            ProviderLookupView::SignInRequired {
                provider: ProviderType::Telegram
            }
        );
    }
}
