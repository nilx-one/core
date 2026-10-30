-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

CREATE TABLE IF NOT EXISTS identities (
    pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL,
    tg_id INTEGER NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;
