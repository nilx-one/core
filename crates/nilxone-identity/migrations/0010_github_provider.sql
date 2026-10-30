-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

CREATE TABLE identity_providers_v3 (
    provider TEXT NOT NULL,
    provider_subject TEXT NOT NULL,
    pub_dress TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (provider, provider_subject),
    FOREIGN KEY (pub_dress) REFERENCES identities(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE,
    CHECK (provider IN ('telegram', 'discord', 'github'))
) STRICT;

INSERT INTO identity_providers_v3 (provider, provider_subject, pub_dress, created_at)
SELECT provider, provider_subject, pub_dress, created_at
FROM identity_providers;

DROP TABLE identity_providers;
ALTER TABLE identity_providers_v3 RENAME TO identity_providers;
