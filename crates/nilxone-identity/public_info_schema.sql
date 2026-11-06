-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Separate 0x1 product database. Neither public name nor a subject received
-- by a different OIDC client can attach an existing profile.
CREATE TABLE IF NOT EXISTS public_profiles (
    issuer TEXT NOT NULL COLLATE BINARY,
    subject TEXT NOT NULL COLLATE BINARY CHECK (length(subject) BETWEEN 1 AND 255),
    profile_id TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))),
    pub_dress TEXT COLLATE BINARY,
    last_auth_time INTEGER NOT NULL CHECK (last_auth_time >= 0),
    bond_xp INTEGER NOT NULL DEFAULT 0 CHECK (bond_xp BETWEEN 0 AND 1000000000),
    avaia_xp INTEGER NOT NULL DEFAULT 0 CHECK (avaia_xp BETWEEN 0 AND 1000000000),
    PRIMARY KEY (issuer, subject)
) STRICT;
