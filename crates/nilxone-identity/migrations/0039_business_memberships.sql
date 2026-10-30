-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

CREATE TABLE IF NOT EXISTS business_memberships (
    business_pub_dress TEXT NOT NULL COLLATE BINARY,
    member_pub_dress TEXT NOT NULL COLLATE BINARY,
    role TEXT NOT NULL CHECK (role IN ('member', 'special_member', 'maintainer', 'administrator', 'co_owner', 'owner')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (business_pub_dress, member_pub_dress),
    CHECK (business_pub_dress != member_pub_dress),
    FOREIGN KEY (business_pub_dress) REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (member_pub_dress) REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE
) STRICT;
CREATE INDEX IF NOT EXISTS business_memberships_by_member
ON business_memberships(member_pub_dress, business_pub_dress);
