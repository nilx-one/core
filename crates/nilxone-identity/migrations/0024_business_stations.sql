-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- A business Bond keeps one stationed Avaia per shop. A station never walks:
-- it stands at its shop's point, and every Bond can read where it stands.
--
-- The station is an `identities` row of kind `avaia`, so its address shares
-- the one global namespace and no Bond can take it. It has no
-- `owner_pub_dress`: that column means "the companion a human Bond owns",
-- one per owner, and every owner-keyed query keeps meaning exactly that.
-- Which business keeps the station, and where it stands, live here.
CREATE TABLE IF NOT EXISTS business_stations (
    avaia_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    longitude_e7 INTEGER NOT NULL
        CHECK (longitude_e7 BETWEEN -1800000000 AND 1800000000),
    latitude_e7 INTEGER NOT NULL
        CHECK (latitude_e7 BETWEEN -900000000 AND 900000000),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT;

CREATE INDEX IF NOT EXISTS business_stations_owner
    ON business_stations(owner_pub_dress);

-- A station leaves with its business: the owner's cascade removes this row,
-- and the station's identity goes with it.
CREATE TRIGGER IF NOT EXISTS business_stations_release_identity
AFTER DELETE ON business_stations
BEGIN
    DELETE FROM identities
    WHERE pub_dress = OLD.avaia_pub_dress AND identity_kind = 'avaia'
        AND owner_pub_dress IS NULL;
END;

-- The station counterpart of `avaia_creation_intents`: the explicit station
-- transaction records the address before inserting it.
CREATE TABLE IF NOT EXISTS business_station_intents (
    avaia_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
) STRICT;

-- 0013 installs this guard on every start; this runs right after it and
-- widens it by exactly one case, an ownerless Avaia whose station intent is
-- recorded. Any other Avaia INSERT is still removed in its own transaction.
DROP TRIGGER IF EXISTS identities_reject_implicit_avaia_creation;
CREATE TRIGGER identities_reject_implicit_avaia_creation
AFTER INSERT ON identities
WHEN NEW.identity_kind = 'avaia'
    AND NOT EXISTS (
        SELECT 1
        FROM avaia_creation_intents
        WHERE owner_pub_dress = NEW.owner_pub_dress
    )
    AND NOT (
        NEW.owner_pub_dress IS NULL
        AND EXISTS (
            SELECT 1
            FROM business_station_intents
            WHERE avaia_pub_dress = NEW.pub_dress
        )
    )
BEGIN
    DELETE FROM identities WHERE pub_dress = NEW.pub_dress;
END;
