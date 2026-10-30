-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- The caption a business names a station by, as written: `xda-market Золоті
-- Ворота`. The station's address is formed from it once, when the station is
-- made; a later caption never renames the address. Stations made before
-- captions have none.
ALTER TABLE business_stations ADD COLUMN caption TEXT
    CHECK (caption IS NULL OR length(caption) BETWEEN 1 AND 256);
