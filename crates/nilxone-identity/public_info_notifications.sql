-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- What the back-end queues for the Telegram bot to say in an owner's chat
-- (docs/bot-notifications.md). A row exists only while the owner has a
-- signed-in chat; the bot fetches it, delivers it and acknowledges it, and
-- the acknowledgement deletes it. `dedupe` keeps one row per event: the
-- request id, the attestation id, the orb or the landmark. Rows nobody
-- delivered within a day are dropped on the next pull.
CREATE TABLE IF NOT EXISTS bot_notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    issuer TEXT NOT NULL COLLATE BINARY,
    subject TEXT NOT NULL COLLATE BINARY,
    kind TEXT NOT NULL CHECK (kind IN ('activation_requested', 'find', 'orb', 'landmark')),
    rarity TEXT CHECK (rarity IS NULL OR rarity IN ('rare', 'special', 'exclusive', 'legendary')),
    label TEXT CHECK (label IS NULL OR length(CAST(label AS BLOB)) <= 80),
    request_id TEXT COLLATE BINARY CHECK (request_id IS NULL OR length(request_id) = 32),
    dedupe TEXT NOT NULL COLLATE BINARY CHECK (length(dedupe) BETWEEN 1 AND 128),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    UNIQUE (issuer, subject, kind, dedupe),
    FOREIGN KEY (issuer, subject) REFERENCES public_profiles (issuer, subject) ON DELETE CASCADE
) STRICT;

CREATE INDEX IF NOT EXISTS bot_notifications_by_owner
ON bot_notifications (issuer, subject, id);

CREATE INDEX IF NOT EXISTS bot_notifications_by_age
ON bot_notifications (created_at);
