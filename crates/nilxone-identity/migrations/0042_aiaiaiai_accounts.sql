-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- 0x1's own record of which Bond an aiaiaiai Identity sign-in belongs to.
-- `subject` is the issuer's pairwise subject for the 0x1 client: opaque,
-- stable for 0x1 and never Identity's private identifier. It is the key; the
-- public address only names the Bond the first time a subject is seen, and
-- follows a rename afterwards. One subject per Bond per issuer, so a second
-- account can never take over a linked Bond.
CREATE TABLE IF NOT EXISTS aiaiaiai_accounts (
    issuer TEXT NOT NULL COLLATE BINARY,
    subject TEXT NOT NULL COLLATE BINARY CHECK (length(subject) BETWEEN 1 AND 255),
    pub_dress TEXT NOT NULL COLLATE BINARY,
    linked_at INTEGER NOT NULL CHECK (linked_at >= 0),
    PRIMARY KEY (issuer, subject),
    UNIQUE (issuer, pub_dress),
    FOREIGN KEY (pub_dress) REFERENCES identities(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE
) STRICT;
