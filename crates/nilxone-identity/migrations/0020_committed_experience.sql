-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Committed experience and claims on rare finds (docs/avaia-outings.md, R3).
--
-- An award arrives as its commitment: an HMAC of a record that stays on the
-- device. The commitment is stored as the event id in bond_experience_events,
-- next to the earner and the amount the service priced it at. No record, no
-- place, no time of play is stored.

-- The head of each device chain. An award is accepted only on top of it.
CREATE TABLE IF NOT EXISTS experience_chains (
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    chain TEXT NOT NULL COLLATE BINARY,
    head TEXT NOT NULL COLLATE BINARY,
    length INTEGER NOT NULL CHECK (length > 0),
    PRIMARY KEY (owner_pub_dress, chain)
) STRICT;

-- How many awards of each kind a Bond had accepted in a week: the caps that
-- bound a client inventing awards that follow the rules. `tier` is 0 for a
-- kind that has none.
CREATE TABLE IF NOT EXISTS experience_kind_counts (
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    epoch INTEGER NOT NULL,
    kind TEXT NOT NULL,
    tier INTEGER NOT NULL CHECK (tier BETWEEN 0 AND 6),
    count INTEGER NOT NULL CHECK (count > 0),
    PRIMARY KEY (owner_pub_dress, epoch, kind, tier)
) STRICT;

-- One rare find, one Bond. A claim names the find by its public sha, the
-- week, and the tier. It holds no time and no device, and it is deleted once
-- its week is two weeks old: the find no longer exists by then.
CREATE TABLE IF NOT EXISTS find_claims (
    artifact_sha TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        CHECK (length(artifact_sha) = 64),
    epoch INTEGER NOT NULL,
    tier INTEGER NOT NULL CHECK (tier BETWEEN 4 AND 6),
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- The claimed set is read per week and per sha bucket, and pruned per week.
CREATE INDEX IF NOT EXISTS find_claims_by_epoch ON find_claims (epoch, artifact_sha);
