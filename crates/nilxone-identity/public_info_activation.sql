-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Moving the active client (docs/client-activation.md). Applied after the
-- product schema and after `product_sessions.active` exists on an older
-- database. The names carry `product_` because a host may open this store in
-- the same file as the legacy identity store, which has its own
-- `session_activation_requests`.

-- At most one active session per owner. Every statement that makes a session
-- active clears the owner's other active rows first, expired ones included.
CREATE UNIQUE INDEX IF NOT EXISTS product_sessions_one_active
ON product_sessions (issuer, subject)
WHERE active = 1;

-- A request by an inactive session to become the active one. `epoch` is the
-- owner's `session_epoch` when it was made and `approver_token_hash` the
-- session that was active then: a request is void once either no longer
-- holds. `deadline` is when its current open step runs out. There is no
-- background sweep; every read and step applies what is due first.
--
-- The labels name both clients as they were when it was made. `sync_total`
-- is the size the active client declared with its first part,
-- `sync_uploaded` how much of it has arrived, `sync_delivered` how much the
-- asking client has been handed in its current pass, and `sync_digest` the
-- SHA-256 of the whole, set when the upload is whole.
CREATE TABLE IF NOT EXISTS product_activation_requests (
    id TEXT PRIMARY KEY NOT NULL COLLATE BINARY CHECK (length(id) = 32),
    issuer TEXT NOT NULL COLLATE BINARY,
    subject TEXT NOT NULL COLLATE BINARY,
    requester_token_hash TEXT NOT NULL COLLATE BINARY,
    approver_token_hash TEXT NOT NULL COLLATE BINARY,
    requester_label TEXT NOT NULL,
    approver_label TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch >= 0),
    status TEXT NOT NULL CHECK (status IN (
        'pending', 'approved', 'syncing',
        'completed', 'declined', 'objected', 'cancelled', 'superseded', 'expired', 'interrupted'
    )),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    deadline INTEGER NOT NULL,
    resolved_at INTEGER,
    sync_total INTEGER CHECK (sync_total >= 0),
    sync_uploaded INTEGER NOT NULL DEFAULT 0 CHECK (sync_uploaded >= 0),
    sync_delivered INTEGER NOT NULL DEFAULT 0 CHECK (sync_delivered >= 0),
    sync_digest TEXT COLLATE BINARY CHECK (length(sync_digest) = 64),
    FOREIGN KEY (issuer, subject) REFERENCES public_profiles (issuer, subject) ON DELETE CASCADE
) STRICT;

-- One open request per owner.
CREATE UNIQUE INDEX IF NOT EXISTS product_activation_requests_one_open
ON product_activation_requests (issuer, subject)
WHERE status IN ('pending', 'approved', 'syncing');

CREATE INDEX IF NOT EXISTS product_activation_requests_by_owner
ON product_activation_requests (issuer, subject, created_at);

CREATE INDEX IF NOT EXISTS product_activation_requests_by_requester
ON product_activation_requests (requester_token_hash, created_at);

-- The active client's data on its way to the asking client, in the parts it
-- arrived in. The bytes are opaque here. Rows exist only while their request
-- is open and are deleted with every final status.
CREATE TABLE IF NOT EXISTS product_activation_sync (
    request_id TEXT NOT NULL COLLATE BINARY,
    part_offset INTEGER NOT NULL CHECK (part_offset >= 0),
    data BLOB NOT NULL,
    PRIMARY KEY (request_id, part_offset),
    FOREIGN KEY (request_id) REFERENCES product_activation_requests (id) ON DELETE CASCADE
) STRICT;
