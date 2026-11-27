-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- A delivery references an existing canonical spoken line, not a copy of its text.
-- Scope to the exact attached chat session and stable Bond, never just a display address.
CREATE TABLE IF NOT EXISTS speech_telegram_delivery (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    line_id TEXT NOT NULL REFERENCES spoken_lines(line_id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    bond_id TEXT NOT NULL,
    workload_subject TEXT NOT NULL,
    actor_id INTEGER NOT NULL CHECK (actor_id > 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > 0),
    claimed_at INTEGER,
    UNIQUE (line_id, workload_subject, actor_id)
) STRICT;
CREATE INDEX IF NOT EXISTS speech_telegram_delivery_expiry
    ON speech_telegram_delivery (expires_at);
