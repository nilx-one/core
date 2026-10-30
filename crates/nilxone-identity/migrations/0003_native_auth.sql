-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

CREATE TABLE IF NOT EXISTS native_credentials (
    pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL,
    password_hash TEXT NOT NULL,
    password_hash_version INTEGER NOT NULL,
    recovery_key_hash BLOB NOT NULL,
    active INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (pub_dress) REFERENCES identities(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE,
    CHECK (password_hash_version >= 1),
    CHECK (active IN (0, 1))
) STRICT;

CREATE TABLE IF NOT EXISTS native_registration_challenges (
    challenge_hash BLOB PRIMARY KEY NOT NULL,
    pub_dress TEXT UNIQUE NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (pub_dress) REFERENCES native_credentials(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE
) STRICT;

CREATE TABLE IF NOT EXISTS native_registration_idempotency (
    idempotency_key_hash BLOB PRIMARY KEY NOT NULL,
    pub_dress TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (pub_dress) REFERENCES native_credentials(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE
) STRICT;

CREATE TABLE IF NOT EXISTS native_sessions (
    token_hash BLOB PRIMARY KEY NOT NULL,
    pub_dress TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER,
    FOREIGN KEY (pub_dress) REFERENCES identities(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE
) STRICT;

CREATE INDEX IF NOT EXISTS native_sessions_by_identity
ON native_sessions (pub_dress, expires_at);
