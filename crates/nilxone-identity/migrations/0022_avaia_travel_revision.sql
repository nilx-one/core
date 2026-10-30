-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Distinguishes an explicit admin travel from ordinary location publication.
ALTER TABLE avaia_locations
ADD COLUMN travel_revision INTEGER NOT NULL DEFAULT 0
    CHECK (travel_revision >= 0);
