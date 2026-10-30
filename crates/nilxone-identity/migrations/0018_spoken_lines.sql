-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Short lines a Bond speaks aloud on the map. A line is presentation: it is
-- not an Interaction, not BondChain evidence, and carries no coordinate. The
-- id is derived by the producer from the utterance's origin, so a redelivered
-- utterance is the same row. A line is audible only for a short while; old rows
-- are history the API never serves.
CREATE TABLE IF NOT EXISTS spoken_lines (
    line_id TEXT PRIMARY KEY COLLATE BINARY NOT NULL,
    speaker TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    text TEXT NOT NULL,
    spoken_at INTEGER NOT NULL CHECK (spoken_at >= 0),
    received_at INTEGER NOT NULL CHECK (received_at >= 0)
) STRICT;

CREATE INDEX IF NOT EXISTS spoken_lines_by_time ON spoken_lines (spoken_at);
