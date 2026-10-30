// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Server-owned Identity authority shared by all clients.
//! Not linked into Wasm/Swift clients. Host authentication, HTTP routes and
//! payment presentation belong to the application adapters.

pub mod cell_orbs;
pub mod credentials;
pub mod finds;
pub mod location;
pub mod market_issuer;
pub mod provider_link;
pub mod provider_secret;
pub mod rate_limit;
pub mod repository {
    include!("repository.rs");
    include!("repository_admin_roles.rs");
    include!("repository_admin_telegram.rs");
    include!("repository_admin_telegram_confirmations.rs");
    include!("repository_telegram_message_slots.rs");
    include!("repository_avaia_lifecycle.rs");
    include!("repository_pub_info.rs");
    include!("repository_awards.rs");
    include!("repository_subscriptions.rs");
    include!("repository_market_visits.rs");
    include!("repository_business_stations.rs");
    include!("repository_business_memberships.rs");
    include!("repository_business_membership_bootstrap.rs");
    include!("repository_business_creation.rs");
    include!("repository_xping.rs");
}
pub use credentials::{
    NativeAuthConfig, PasswordEngine, PasswordPolicyError, RememberedBondSigner, SecretDigester,
    TokenFactory,
};
pub use location::{BondAccessRole, BondLocationRepository, BondLocationRepositoryError};
pub use nilxone_contracts::{
    AvaiaPubDress, AvaiaPubDressError, BondLocation, BondLocationMode, DecimalU64, GeoCoordinate,
    GeoCoordinateError, PubDress, PubDressError, PubDressLabel, PubDressLabelError, StationCaption,
    StationPubDress, StationPubDressError,
};
pub use provider_link::{ProviderLinkOutcome, ProviderLinkRepository, SelfDisconnectOutcome};
pub use provider_secret::{ProviderSecretCipher, ProviderSecretError};
pub use repository::{
    ActivationRequest, ActivationRequestStatus, AdminRoleDecision, AdminRoleRequest,
    AdminRoleTelegramConfirmation, AdminRoleTelegramDelivery, AvaiaConfigurationState,
    AvaiaIdentityRecord, AvaiaLocation, AvaiaTravelOutcome, AvaiaUpdateOutcome,
    BusinessRegistrationOutcome, BusinessStation, BusinessStationOutcome, IdentityProvider,
    IdentityRecord, IdentityRepository, NativeCredentialRecord, NativeRegistrationOutcome,
    ProviderIdentity, PubDressRenameOutcome, PublicIdentityRecord, RegistrationOutcome,
    RepositoryError, TelegramMessageSlot,
};
pub mod github_evidence;
pub mod speech;
pub use github_evidence::GithubEvidenceRepository;
