-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- One active operational location per human Bond. The mode preserves whether
-- the coordinate came from an explicit current-location observation (`live`)
-- or an authorized declared map point (`manual`). No row means no submitted
-- Bond location yet.
CREATE TABLE IF NOT EXISTS bond_locations (
    pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    mode TEXT NOT NULL CHECK (mode IN ('live', 'manual')),
    longitude_e7 INTEGER NOT NULL
        CHECK (longitude_e7 BETWEEN -1800000000 AND 1800000000),
    latitude_e7 INTEGER NOT NULL
        CHECK (latitude_e7 BETWEEN -900000000 AND 900000000),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
) STRICT;
