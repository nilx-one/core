// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use ox1_contracts::{BondChainId, DecimalU64};

/// Explicit nondeterministic/external boundary required by Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Clock,
    Entropy,
    IdentifierGeneration,
    CryptographicVerification,
    PersistenceEffect,
    TransportEffect,
}

/// Observable absence or failure of an external port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortError {
    pub port: PortKind,
}

/// Supplies time; the kernel never reads a wall clock directly.
pub trait Clock {
    fn now_unix_ms(&self) -> Result<DecimalU64, PortError>;
}

/// Supplies explicit entropy; the kernel never reads ambient randomness directly.
pub trait Entropy {
    fn bytes(&mut self, length: usize) -> Result<Vec<u8>, PortError>;
}

/// Supplies canonical identifiers without granting identity authority.
pub trait IdentifierGeneration {
    fn next_bond_chain_id(&mut self) -> Result<BondChainId, PortError>;
}

/// Generic verification boundary. Phase 0 defines no production authority profile.
pub trait CryptographicVerification {
    type Request;
    type Verified;

    fn verify(&self, request: &Self::Request) -> Result<Self::Verified, PortError>;
}

/// Dispatches a persistence request. Success here is not protocol evidence.
pub trait PersistenceEffectSink<E> {
    fn dispatch(&mut self, effect: &E) -> Result<(), PortError>;
}

/// Dispatches a transport request. Success here is not counterpart action.
pub trait TransportEffectSink<E> {
    fn dispatch(&mut self, effect: &E) -> Result<(), PortError>;
}
