-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- One owner-published location per Avaia, keyed by the owning human Bond.
-- This is distinct from `bond_locations` (the Bond's own operational
-- location) and from the local, never-persisted walking position described
-- in `avaia-walk.md`. No row means the owner has not published a location
-- for their Avaia yet.
CREATE TABLE IF NOT EXISTS avaia_locations (
    owner_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    longitude_e7 INTEGER NOT NULL
        CHECK (longitude_e7 BETWEEN -1800000000 AND 1800000000),
    latitude_e7 INTEGER NOT NULL
        CHECK (latitude_e7 BETWEEN -900000000 AND 900000000),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
) STRICT;

-- The configuration sidecar must exist before its guarded identity trigger can
-- be installed. The dedicated 0007 migration remains responsible for legacy
-- backfill when an Avaia profile is first read/configured.
CREATE TABLE IF NOT EXISTS avaia_configuration (
    avaia_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    owner_pub_dress TEXT NOT NULL UNIQUE COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    configuration_state TEXT NOT NULL
        CHECK (configuration_state IN ('unconfigured', 'configured'))
) STRICT;

-- Avaia creation is an explicit owner action. Registration, renames and
-- identity resolution no longer insert Avaia identities; the database still
-- enforces the lifecycle boundary underneath them as a backstop. The explicit
-- create transaction records its owner here before inserting. Any other Avaia
-- INSERT is immediately removed in the same transaction, leaving the human
-- Bond intact and making implicit creation observationally impossible.
CREATE TABLE IF NOT EXISTS avaia_creation_intents (
    owner_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE
) STRICT;

-- Replace the older unconditional configuration trigger from 0007 with the
-- lifecycle-aware form. This also upgrades databases that already installed
-- the old trigger before this change.
DROP TRIGGER IF EXISTS identities_create_avaia_configuration;
CREATE TRIGGER identities_create_avaia_configuration
AFTER INSERT ON identities
WHEN NEW.identity_kind = 'avaia'
    AND EXISTS (
        SELECT 1
        FROM avaia_creation_intents
        WHERE owner_pub_dress = NEW.owner_pub_dress
    )
BEGIN
    INSERT OR IGNORE INTO avaia_configuration (
        avaia_pub_dress,
        owner_pub_dress,
        configuration_state
    ) VALUES (
        NEW.pub_dress,
        NEW.owner_pub_dress,
        'unconfigured'
    );
END;

DROP TRIGGER IF EXISTS identities_reject_implicit_avaia_creation;
CREATE TRIGGER identities_reject_implicit_avaia_creation
AFTER INSERT ON identities
WHEN NEW.identity_kind = 'avaia'
    AND NOT EXISTS (
        SELECT 1
        FROM avaia_creation_intents
        WHERE owner_pub_dress = NEW.owner_pub_dress
    )
BEGIN
    DELETE FROM identities WHERE pub_dress = NEW.pub_dress;
END;
