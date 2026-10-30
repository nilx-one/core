-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Run inside the repository's transaction. Replacing only this column keeps
-- the identity table, its indexes and all referencing relationships intact.
-- Keep 0005 unchanged so already deployed databases are upgraded as well.
ALTER TABLE identities ADD COLUMN avatar_model_next TEXT
    CHECK (avatar_model_next IS NULL OR avatar_model_next IN
        ('sky-study', 'dasha-study', 'kai-study', 'dasha-v2-study'));
UPDATE identities SET avatar_model_next = avatar_model;
ALTER TABLE identities DROP COLUMN avatar_model;
ALTER TABLE identities RENAME COLUMN avatar_model_next TO avatar_model;
