-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Orbs a find spills when a fog cell opens (nilx-one/core docs/orb-spills.md).
--
-- A spill names its find by the public sha only, like a claim. It holds how
-- many orbs fell and when they are gone, never who spilled them, where, or
-- when anyone played. A find spills once a week; the row stays until its
-- week is two weeks old so it cannot spill again, and is answered only while
-- it lives.
CREATE TABLE IF NOT EXISTS orb_spills (
    artifact_sha TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        CHECK (length(artifact_sha) = 64),
    epoch INTEGER NOT NULL,
    count INTEGER NOT NULL CHECK (count BETWEEN 5 AND 30),
    expires_at_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS orb_spills_by_epoch ON orb_spills (epoch, artifact_sha);

-- One orb, one picker: the first Bond whose award claims it. Deleted with
-- its spill's week.
CREATE TABLE IF NOT EXISTS orb_claims (
    artifact_sha TEXT NOT NULL COLLATE BINARY
        REFERENCES orb_spills(artifact_sha) ON DELETE CASCADE,
    orb INTEGER NOT NULL CHECK (orb BETWEEN 0 AND 29),
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (artifact_sha, orb)
) STRICT, WITHOUT ROWID;
