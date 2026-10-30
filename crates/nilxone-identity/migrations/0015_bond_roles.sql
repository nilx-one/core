-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- The application role of a human Bond. The role is optional: no row means
-- `user`, the only role public registration creates. Other roles are assigned
-- out of band. The role follows the Bond through a pub_dress rename.
CREATE TABLE IF NOT EXISTS bond_roles (
    pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'admin', 'business'))
) STRICT;

-- Admin rights were previously derived from these two pub_dress values. Carry
-- the Bonds that already hold them over once; after this, holding one of these
-- names grants nothing by itself.
INSERT OR IGNORE INTO bond_roles (pub_dress, role)
SELECT pub_dress, 'admin'
FROM identities
WHERE identity_kind = 'human' AND pub_dress IN ('0x0sky', '0x0небо');
