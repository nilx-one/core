-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Separate 0x1 product database. Neither public name nor a subject received
-- by a different OIDC client can attach an existing profile.
CREATE TABLE IF NOT EXISTS public_profiles (
    issuer TEXT NOT NULL COLLATE BINARY CHECK (length(issuer) > 0),
    subject TEXT NOT NULL COLLATE BINARY CHECK (length(subject) BETWEEN 1 AND 255),
    profile_id TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))),
    pub_dress TEXT COLLATE BINARY,
    last_auth_time INTEGER NOT NULL CHECK (last_auth_time >= 0),
    bond_xp INTEGER NOT NULL DEFAULT 0 CHECK (bond_xp BETWEEN 0 AND 1000000000),
    avaia_xp INTEGER NOT NULL DEFAULT 0 CHECK (avaia_xp BETWEEN 0 AND 1000000000),
    PRIMARY KEY (issuer, subject)
) STRICT;

-- Product sessions the HTTP host opens after a verified AIS callback. The
-- host keeps the cookie and stores only a digest of its token here. A session
-- names the exact (issuer, subject, profile_id) it was opened for, so a
-- restore re-proves that binding in SQL instead of trusting anything the
-- browser sent, and a deleted profile takes its sessions with it.
CREATE TABLE IF NOT EXISTS product_sessions (
    token_hash TEXT PRIMARY KEY NOT NULL COLLATE BINARY CHECK (length(token_hash) BETWEEN 32 AND 128),
    issuer TEXT NOT NULL COLLATE BINARY,
    subject TEXT NOT NULL COLLATE BINARY,
    profile_id TEXT NOT NULL COLLATE BINARY,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > created_at),
    FOREIGN KEY (issuer, subject) REFERENCES public_profiles (issuer, subject) ON DELETE CASCADE
) STRICT;
CREATE INDEX IF NOT EXISTS product_sessions_by_owner ON product_sessions (issuer, subject);
