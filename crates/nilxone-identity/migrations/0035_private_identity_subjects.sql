-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Immutable 0x1 priv_id (historically named subject in this store).
-- Never expose this to clients or Prism.
-- The public address remains mutable, with ON UPDATE CASCADE preserving the link.
CREATE TABLE IF NOT EXISTS identity_subjects (
    subject BLOB PRIMARY KEY NOT NULL CHECK (length(subject) = 32),
    pub_dress TEXT NOT NULL UNIQUE COLLATE BINARY,
    FOREIGN KEY (pub_dress) REFERENCES identities(pub_dress)
        ON UPDATE CASCADE
        ON DELETE CASCADE
) STRICT;

-- A one-time random private identifier for each existing Bond. Reopening the service
-- must not rotate an existing identifier.
INSERT INTO identity_subjects (subject, pub_dress)
SELECT randomblob(32), i.pub_dress
FROM identities i
WHERE NOT EXISTS (SELECT 1 FROM identity_subjects s WHERE s.pub_dress = i.pub_dress);

-- All native, provider and business registration paths write identities first.
CREATE TRIGGER IF NOT EXISTS identity_subjects_after_human_insert
AFTER INSERT ON identities
WHEN NEW.identity_kind = 'human'
BEGIN
    INSERT INTO identity_subjects (subject, pub_dress)
    VALUES (randomblob(32), NEW.pub_dress);
END;

-- Owned Avaia receive their own private identifier when their row is created.
CREATE TRIGGER IF NOT EXISTS identity_subjects_after_avaia_insert
AFTER INSERT ON identities
WHEN NEW.identity_kind = 'avaia'
BEGIN
    INSERT INTO identity_subjects (subject, pub_dress)
    VALUES (randomblob(32), NEW.pub_dress);
END;

CREATE TRIGGER IF NOT EXISTS identity_subjects_immutable
BEFORE UPDATE OF subject ON identity_subjects
WHEN NEW.subject <> OLD.subject
BEGIN
    SELECT RAISE(ABORT, 'Bond private identifier is immutable');
END;
