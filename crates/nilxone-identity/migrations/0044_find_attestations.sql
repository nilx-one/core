-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Verified pick-ups (docs/find-attestation.md). Owner decision 2026-11-08:
-- a pick-up is attested by the service, which keeps the find, the finder,
-- when and where it was picked up, and answers an opaque attestation id. The
-- device keeps the thing; only this id leaves it again, to sell the thing.
--
-- The row is also the provenance of every seeds credit a find makes: money
-- (small change) credits once at attestation, a thing credits once when it
-- is sold. Neither amount is ever a client's: both are Core's price of the
-- item Core derives from the artifact id.
CREATE TABLE IF NOT EXISTS find_attestations (
    attestation_id TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        CHECK (length(attestation_id) = 36),
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    artifact_id TEXT NOT NULL COLLATE BINARY CHECK (length(artifact_id) <= 96),
    artifact_sha TEXT NOT NULL COLLATE BINARY CHECK (length(artifact_sha) = 64),
    epoch INTEGER NOT NULL,
    tier INTEGER NOT NULL CHECK (tier BETWEEN 1 AND 6),
    item_id TEXT NOT NULL CHECK (length(item_id) BETWEEN 1 AND 64),
    finder TEXT NOT NULL CHECK (finder IN ('bond', 'avaia')),
    found_at_ms INTEGER NOT NULL CHECK (found_at_ms >= 0),
    longitude_e7 INTEGER NOT NULL CHECK (longitude_e7 BETWEEN -1800000000 AND 1800000000),
    latitude_e7 INTEGER NOT NULL CHECK (latitude_e7 BETWEEN -900000000 AND 900000000),
    attested_at_ms INTEGER NOT NULL CHECK (attested_at_ms >= 0),
    -- Money credited on the spot (small change), else 0.
    pickup_credit INTEGER NOT NULL DEFAULT 0 CHECK (pickup_credit >= 0),
    sold_at_ms INTEGER,
    sale_credit INTEGER,
    CHECK ((sold_at_ms IS NULL) = (sale_credit IS NULL)),
    CHECK (sale_credit IS NULL OR sale_credit > 0),
    CHECK (sold_at_ms IS NULL OR pickup_credit = 0)
) STRICT;

-- One Bond picks up one find once; a find of a claimed tier is one Bond's.
CREATE UNIQUE INDEX IF NOT EXISTS find_attestations_once_per_bond
    ON find_attestations(owner_pub_dress, artifact_sha);
CREATE UNIQUE INDEX IF NOT EXISTS find_attestations_claimed_once
    ON find_attestations(artifact_sha) WHERE tier >= 4;
