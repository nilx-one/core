-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Additive, dormant legacy Bond -> AIS attachment storage. These tables are
-- installed only by the opt-in migration service, after validating that legacy
-- identities and product profiles live in the same SQLite database.
-- Existing legacy/game rows remain in place; public names are never lookup
-- keys for an AIS session.

CREATE TABLE IF NOT EXISTS legacy_bond_keys (
    bond_id TEXT PRIMARY KEY NOT NULL COLLATE BINARY
        DEFAULT (lower(hex(randomblob(16))))
        CHECK (length(bond_id) = 32),
    legacy_pub_dress TEXT NOT NULL COLLATE BINARY UNIQUE,
    FOREIGN KEY (legacy_pub_dress) REFERENCES identities (pub_dress)
        ON UPDATE CASCADE ON DELETE CASCADE
) STRICT;

CREATE TABLE IF NOT EXISTS legacy_bond_bindings (
    bond_id TEXT PRIMARY KEY NOT NULL COLLATE BINARY,
    profile_id TEXT NOT NULL COLLATE BINARY UNIQUE,
    linked_at INTEGER NOT NULL CHECK (linked_at >= 0),
    FOREIGN KEY (bond_id) REFERENCES legacy_bond_keys (bond_id)
        ON DELETE CASCADE,
    FOREIGN KEY (profile_id) REFERENCES public_profiles (profile_id)
        ON DELETE CASCADE
) STRICT;

-- Only opaque digests of server-generated transaction handles and native
-- credential-session tokens are stored. A pending entry grants no game access.
CREATE TABLE IF NOT EXISTS legacy_bond_migration_intents (
    transaction_hash BLOB PRIMARY KEY NOT NULL CHECK (length(transaction_hash) = 32),
    legacy_session_hash BLOB NOT NULL CHECK (length(legacy_session_hash) = 32),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > started_at),
    status TEXT NOT NULL DEFAULT 'pending_ais'
        CHECK (status IN ('pending_ais', 'confirmed', 'committed',
                          'cancelled', 'expired', 'conflict', 'rejected')),
    verified_issuer TEXT COLLATE BINARY,
    verified_subject TEXT COLLATE BINARY,
    verified_auth_time INTEGER,
    profile_id TEXT COLLATE BINARY,
    -- Confirmation is an explicit owner action, not an AIS callback side effect.
    confirmed_at INTEGER CHECK (confirmed_at IS NULL OR confirmed_at >= started_at),
    completed_at INTEGER,
    CHECK ((verified_issuer IS NULL AND verified_subject IS NULL
            AND verified_auth_time IS NULL)
        OR (verified_issuer IS NOT NULL AND verified_subject IS NOT NULL
            AND verified_auth_time IS NOT NULL)),
    CHECK (status NOT IN ('confirmed', 'committed')
        OR (confirmed_at IS NOT NULL AND verified_issuer IS NOT NULL
            AND profile_id IS NOT NULL)),
    CHECK (confirmed_at IS NULL OR confirmed_at < expires_at),
    CHECK (completed_at IS NULL OR completed_at >= started_at)
) STRICT;

-- A terminal outcome is immutable to the migration service. At final commit,
-- recheck expires_at, the original legacy-session digest, fresh credential
-- authentication and revocation in a single BEGIN IMMEDIATE transaction.
-- The issuer/client and pairwise subject must match the verified OIDC callback.
-- No expired status can be treated as proof, even before the expiry sweep.
--
-- No two live migrations may race using the same native proof.
CREATE UNIQUE INDEX IF NOT EXISTS legacy_migration_one_pending_per_session
    ON legacy_bond_migration_intents (legacy_session_hash)
    WHERE status IN ('pending_ais', 'confirmed');

-- Independent of identity deletion: keep only pseudonymous, non-credential
-- evidence of a terminal outcome. This is not a BondChain interaction.
CREATE TABLE IF NOT EXISTS legacy_bond_migration_audit (
    transaction_hash BLOB PRIMARY KEY NOT NULL CHECK (length(transaction_hash) = 32),
    bond_id TEXT NOT NULL COLLATE BINARY CHECK (length(bond_id) = 32),
    ais_reference_hash BLOB NOT NULL CHECK (length(ais_reference_hash) = 32),
    outcome TEXT NOT NULL CHECK (outcome IN ('committed', 'conflict', 'rejected')),
    occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0)
) STRICT;
