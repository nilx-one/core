-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Administrative workflow, not BondChain evidence. Terminal decisions are
-- durable so each repeated login cannot create another pending request.
CREATE TABLE IF NOT EXISTS admin_role_requests (
    pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    status TEXT NOT NULL CHECK(status IN ('pending', 'approved', 'denied')),
    -- Opaque, never-reused-in-practice callback reference; a deleted rowid
    -- must not be able to authorize a different future registration.
    request_token TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))),
    requested_at INTEGER NOT NULL,
    decided_by TEXT REFERENCES identities(pub_dress)
        ON UPDATE CASCADE ON DELETE SET NULL,
    decided_at INTEGER,
    CHECK((status = 'pending' AND decided_by IS NULL AND decided_at IS NULL)
       OR (status <> 'pending' AND decided_at IS NOT NULL))
) STRICT;
