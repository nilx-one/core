-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- The Bonds allowed to speak on the map. Holding a row is the only thing that
-- grants it: a name grants nothing by itself. The two Bonds that spoke first
-- are carried over once, when this table is created; anyone else is added out
-- of band, and a Bond that registers one of those names later gets nothing.
CREATE TABLE IF NOT EXISTS bond_speakers (
    pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE
) STRICT;

INSERT OR IGNORE INTO bond_speakers (pub_dress)
SELECT pub_dress
FROM identities
WHERE identity_kind = 'human' AND pub_dress IN ('0x0sky', '0xfrSb');
