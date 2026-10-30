-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Issuer-authenticated, immutable seeds credit provenance. A client-provided
-- inventory quantity is NEVER an allowed credit source. Source IDs are
-- idempotency keys, unique within a Bond and economic origin.
-- A spent wallet stays in bond_seeds; this ledger records trusted issuance.
CREATE TABLE IF NOT EXISTS bond_seed_credits (
    owner_pub_dress TEXT COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind = 'starter'),
    source_id TEXT NOT NULL CHECK (source_id = 'once'),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    committed_at_ms INTEGER NOT NULL CHECK (committed_at_ms >= 0),
    PRIMARY KEY (owner_pub_dress, source_kind, source_id)
) STRICT;
