// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Test-only deterministic support for native, Wasm, and `UniFFI` parity.
//!
//! The fixture interaction below is synthetic proof machinery. It is not exported
//! through a production interaction registry and grants no production authority.

mod fixture;

pub use fixture::{
    EmptyContextValue, Establishment, FixtureAuthorization, FixtureAuthorizationScope,
    FixtureBondChain, FixtureCommand, FixtureEffect, FixtureEvent, FixtureLifecycle,
    FixtureProjection, FixtureProjectionBondChain, FixtureRecord, FixtureState,
    FixtureTerminalOutcome, FixtureTransitionOutcome, FixtureVerifiedContext,
    run_fixture_transition,
};
