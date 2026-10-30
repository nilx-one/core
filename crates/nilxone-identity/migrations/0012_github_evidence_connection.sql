-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Repository-access credentials are a capability owned by a Bond, not Bond identity
-- authority. The stable GitHub numeric subject is retained only to reconcile the
-- external account and to prevent one provider account from spanning Bonds.
CREATE TABLE IF NOT EXISTS github_evidence_connections (
    pub_dress TEXT PRIMARY KEY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    github_user_id TEXT NOT NULL UNIQUE,
    login TEXT NOT NULL,
    profile_url TEXT NOT NULL,
    avatar_url TEXT NOT NULL,
    encrypted_access_token BLOB NOT NULL,
    connection_state TEXT NOT NULL DEFAULT 'connected'
        CHECK (connection_state IN ('connected', 'degraded')),
    diagnostic TEXT,
    connected_at INTEGER NOT NULL,
    refreshed_at INTEGER NOT NULL
);
