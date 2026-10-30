-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Avaia is an AI Bond identity, not a UI-only companion row. Keep human and
-- Avaia public addresses in the same global namespace so the primary key
-- continues to enforce pub_dress uniqueness across identity kinds.
ALTER TABLE identities
    ADD COLUMN identity_kind TEXT NOT NULL DEFAULT 'human'
    CHECK (identity_kind IN ('human', 'avaia'));

ALTER TABLE identities
    ADD COLUMN owner_pub_dress TEXT REFERENCES identities(pub_dress) ON DELETE CASCADE;

-- `identities.created_at` already exists in the historical schema. Preserve
-- that observed timestamp for pre-amendment humans; all new human/Avaia rows
-- write the current registration or reconciliation time through repository code.
CREATE UNIQUE INDEX IF NOT EXISTS identities_one_owned_avaia_per_owner
    ON identities(owner_pub_dress)
    WHERE identity_kind = 'avaia';

CREATE INDEX IF NOT EXISTS identities_kind_owner
    ON identities(identity_kind, owner_pub_dress);
