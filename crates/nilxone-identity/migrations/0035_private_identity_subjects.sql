-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Identity-only immutable subject. Never expose this to clients or Prism.
-- The public address remains mutable, with ON UPDATE CASCADE preserving the link.
CREATE TABLE IF NOT EXISTS identity_subjects (
    subject BLOB PRIMARY KEY NOT NULL CHECK (length(subject) = 32),
    pub_dress TEXT NOT NULL UNIQUE COLLATE BINARY,
    FOREIGN KEY (pub_dress) REFERENCES identities(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE
) STRICT;

-- A one-time random subject for each existing human. Reopening the service
-- must not rotate an already allocated subject.
INSERT OR IGNORE INTO identity_subjects (subject, pub_dress)
SELECT randomblob(32), pub_dress
FROM identities
WHERE identity_kind = 'human';

-- All future native or provider registration paths write identities first.
-- Owned Avaia rows are not human accounts and must not receive a subject.
CREATE TRIGGER IF NOT EXISTS identity_subjects_after_human_insert
AFTER INSERT ON identities
WHEN NEW.identity_kind = 'human'
BEGIN
    INSERT INTO identity_subjects (subject, pub_dress)
    VALUES (randomblob(32), NEW.pub_dress);
END;
