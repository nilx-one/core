-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Experience Orbs a Bond picked up from a cell that opened for it
-- (docs/exp-orbs.md). A row is a SHA-256 of the orb's id and nothing else:
-- not the cell, its landmarks or when it opened. It exists only so the same
-- orb pays once, to the Bond or its Avaia, whoever reached it first.
CREATE TABLE IF NOT EXISTS cell_orb_claims (
    owner_pub_dress TEXT NOT NULL COLLATE BINARY,
    orb_sha TEXT NOT NULL COLLATE BINARY,
    PRIMARY KEY (owner_pub_dress, orb_sha)
) WITHOUT ROWID;
