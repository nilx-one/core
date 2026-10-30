-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

ALTER TABLE identities ADD COLUMN pub_dress_label TEXT;
ALTER TABLE identities ADD COLUMN pub_dress_label_suffix TEXT NOT NULL DEFAULT '';

CREATE UNIQUE INDEX identities_pub_dress_label_unique
ON identities(pub_dress_label COLLATE NOCASE)
WHERE pub_dress_label IS NOT NULL;
