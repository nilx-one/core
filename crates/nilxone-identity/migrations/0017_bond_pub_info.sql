-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Public, synced slice of a Bond's .bnd (`pub_info`). Experience is the first
-- fact it holds: one shared total per Bond, the sum of an idempotent event
-- log plus a one-time carry of experience a device earned before sync
-- existed. Event ids are client nonces. They are not places, and they are
-- not part of the public projection — only the totals are.
CREATE TABLE IF NOT EXISTS bond_pub_info (
    owner_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    carried_bond_xp INTEGER NOT NULL DEFAULT 0 CHECK (carried_bond_xp >= 0),
    carried_avaia_xp INTEGER NOT NULL DEFAULT 0 CHECK (carried_avaia_xp >= 0),
    bond_xp INTEGER NOT NULL DEFAULT 0 CHECK (bond_xp >= 0),
    avaia_xp INTEGER NOT NULL DEFAULT 0 CHECK (avaia_xp >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
) STRICT;

CREATE TABLE IF NOT EXISTS bond_experience_events (
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    event_id TEXT NOT NULL COLLATE BINARY,
    earner TEXT NOT NULL CHECK (earner IN ('bond', 'avaia')),
    amount INTEGER NOT NULL CHECK (amount > 0),
    PRIMARY KEY (owner_pub_dress, event_id)
) STRICT;
