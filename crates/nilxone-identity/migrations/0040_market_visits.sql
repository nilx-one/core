-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- An opaque Market visit: the reference Market keeps on an order so the issuer
-- worker can find, server-side, whose wallet a purchase may debit and which
-- active session confirmed it. Market sees only `visit_id` and the public
-- recipient handle; never the private subject or a native session hash.
--
-- The owner follows a pub_dress rename; `recipient_pub_dress` stays the handle
-- Market was told, which its order echoes back. A new debit is allowed only
-- before `spend_until_ms`; an already committed receipt is confirmed at any time.
CREATE TABLE IF NOT EXISTS market_visits (
    visit_id TEXT PRIMARY KEY COLLATE BINARY NOT NULL CHECK (length(visit_id) = 43),
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    recipient_pub_dress TEXT NOT NULL COLLATE BINARY
        CHECK (length(recipient_pub_dress) BETWEEN 1 AND 128),
    session_token_hash BLOB NOT NULL CHECK (length(session_token_hash) = 32),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    spend_until_ms INTEGER NOT NULL CHECK (spend_until_ms > created_at_ms)
) STRICT;

CREATE INDEX IF NOT EXISTS market_visits_owner
    ON market_visits(owner_pub_dress, created_at_ms);
