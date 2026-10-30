-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE identities_v2 (
    pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;

INSERT INTO identities_v2 (pub_dress, created_at)
SELECT pub_dress, created_at
FROM identities;

CREATE TABLE identity_providers (
    provider TEXT NOT NULL,
    provider_subject TEXT NOT NULL,
    pub_dress TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (provider, provider_subject),
    FOREIGN KEY (pub_dress) REFERENCES identities_v2(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE,
    CHECK (provider IN ('telegram', 'discord'))
) STRICT;

INSERT INTO identity_providers (provider, provider_subject, pub_dress, created_at)
SELECT 'telegram', CAST(tg_id AS TEXT), pub_dress, created_at
FROM identities;

DROP TABLE identities;
ALTER TABLE identities_v2 RENAME TO identities;

COMMIT;
PRAGMA foreign_keys = ON;
