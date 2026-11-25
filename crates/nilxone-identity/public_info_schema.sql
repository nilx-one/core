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
    -- Counts every change of the active client (docs/client-activation.md).
    session_epoch INTEGER NOT NULL DEFAULT 0 CHECK (session_epoch >= 0),
    PRIMARY KEY (issuer, subject)
) STRICT;

-- Product sessions the HTTP host opens after a verified AIS callback. The
-- host keeps the cookie and stores only a digest of its token here. A session
-- names the exact (issuer, subject, profile_id) it was opened for, so a
-- restore re-proves that binding in SQL instead of trusting anything the
-- browser sent, and a deleted profile takes its sessions with it. A session
-- is active or inactive and nothing else; `client_label` names its device
-- and host to the owner's other clients.
CREATE TABLE IF NOT EXISTS product_sessions (
    token_hash TEXT PRIMARY KEY NOT NULL COLLATE BINARY CHECK (length(token_hash) BETWEEN 32 AND 128),
    issuer TEXT NOT NULL COLLATE BINARY,
    subject TEXT NOT NULL COLLATE BINARY,
    profile_id TEXT NOT NULL COLLATE BINARY,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > created_at),
    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    client_label TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (issuer, subject) REFERENCES public_profiles (issuer, subject) ON DELETE CASCADE
) STRICT;
CREATE INDEX IF NOT EXISTS product_sessions_by_owner ON product_sessions (issuer, subject);

-- This stores only digests of ephemeral 0x1↔AIS Telegram identity challenges.
-- It cannot create a product session, provider link or BondChain fact.
CREATE TABLE IF NOT EXISTS telegram_provider_proof_challenges (
    nonce_digest BLOB PRIMARY KEY NOT NULL CHECK (length(nonce_digest) = 32),
    context_digest BLOB NOT NULL CHECK (length(context_digest) = 32),
    expires_at INTEGER NOT NULL CHECK (expires_at > 0),
    consumed INTEGER NOT NULL DEFAULT 0 CHECK (consumed IN (0, 1)),
    proof_jti_digest BLOB UNIQUE CHECK (proof_jti_digest IS NULL OR length(proof_jti_digest) = 32)
) STRICT;
CREATE INDEX IF NOT EXISTS telegram_provider_challenges_expiry
    ON telegram_provider_proof_challenges(expires_at);

-- Pending rows contain live PKCE material, valid for five minutes. A callback
-- claim removes it immediately. Any later login start or unmatched callback
-- clears expired material; deduplication receipts stay through the retention
-- window.
CREATE TABLE IF NOT EXISTS telegram_login_transactions (
    workload_subject TEXT NOT NULL,
    actor_id INTEGER NOT NULL CHECK (actor_id > 0),
    chat_id INTEGER NOT NULL CHECK (chat_id = actor_id),
    message_id INTEGER NOT NULL CHECK (message_id > 0),
    state_digest BLOB NOT NULL UNIQUE CHECK (length(state_digest) = 32),
    issuer TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    expires_at INTEGER NOT NULL CHECK (expires_at > 0),
    pending TEXT,
    PRIMARY KEY (workload_subject, actor_id, chat_id, message_id)
) STRICT;
CREATE INDEX IF NOT EXISTS telegram_login_transactions_expiry
    ON telegram_login_transactions(expires_at);

-- A verified Telegram login callback waits here until the bot claims it for
-- the original /login message in the original private chat. The key is the one-use login
-- message; a confirmation deletes its row, so it opens at most one session.
-- The row holds only the AIS claims a product profile is attached by.
CREATE TABLE IF NOT EXISTS telegram_login_confirmations (
    workload_subject TEXT NOT NULL,
    actor_id INTEGER NOT NULL CHECK (actor_id > 0),
    chat_id INTEGER NOT NULL CHECK (chat_id = actor_id),
    message_id INTEGER NOT NULL CHECK (message_id > 0),
    issuer TEXT NOT NULL COLLATE BINARY CHECK (length(issuer) > 0),
    subject TEXT NOT NULL COLLATE BINARY CHECK (length(subject) BETWEEN 1 AND 255),
    pub_dress TEXT COLLATE BINARY,
    auth_time INTEGER NOT NULL CHECK (auth_time >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > 0),
    PRIMARY KEY (workload_subject, actor_id, chat_id, message_id)
) STRICT;
CREATE INDEX IF NOT EXISTS telegram_login_confirmations_expiry
    ON telegram_login_confirmations(expires_at);

-- A /login whose browser step reached the callback and failed: AIS refused,
-- the code exchange failed, or the identity is not this sender's. The bot
-- reads it to say so in the chat instead of waiting for the login to expire.
-- It holds no identity, only the login message it belongs to.
CREATE TABLE IF NOT EXISTS telegram_login_failures (
    workload_subject TEXT NOT NULL,
    actor_id INTEGER NOT NULL CHECK (actor_id > 0),
    chat_id INTEGER NOT NULL CHECK (chat_id = actor_id),
    message_id INTEGER NOT NULL CHECK (message_id > 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > 0),
    PRIMARY KEY (workload_subject, actor_id, chat_id, message_id)
) STRICT;
CREATE INDEX IF NOT EXISTS telegram_login_failures_expiry
    ON telegram_login_failures(expires_at);

-- The product session a confirmed Telegram chat signed in to. The chat
-- client never holds a session token: the bot workload and the verified
-- Telegram sender name the session, and the session row decides the rest.
-- Ending or expiring the session drops its binding.
CREATE TABLE IF NOT EXISTS telegram_chat_sessions (
    workload_subject TEXT NOT NULL,
    actor_id INTEGER NOT NULL CHECK (actor_id > 0),
    token_hash TEXT NOT NULL UNIQUE COLLATE BINARY
        REFERENCES product_sessions (token_hash) ON DELETE CASCADE,
    PRIMARY KEY (workload_subject, actor_id)
) STRICT;
