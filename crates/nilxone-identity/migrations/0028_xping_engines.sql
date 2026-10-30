-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- xPing's engine, one per Bond (nilx-one/core docs/xping-engine.md). Core
-- owns the rules; the service stores the state and is its clock, so a
-- breakdown is recorded at the service's time, not the device's. A Bond
-- without a row has a full, working engine.
CREATE TABLE IF NOT EXISTS xping_engines (
    owner_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    energy_permille INTEGER NOT NULL CHECK (energy_permille BETWEEN 0 AND 1000),
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    -- Forcing on an empty engine since it last had energy (Core counts it).
    empty_forced_ms INTEGER NOT NULL DEFAULT 0 CHECK (empty_forced_ms >= 0),
    broken_at_ms INTEGER CHECK (broken_at_ms IS NULL OR broken_at_ms >= 0)
) STRICT, WITHOUT ROWID;
