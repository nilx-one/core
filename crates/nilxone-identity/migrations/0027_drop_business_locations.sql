-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- A business is its stations: many points on the map, one per shop, and no
-- single location of its own. `business_locations` (0026) said which one
-- station a business stood at; it is dropped, and no business declares a
-- `bond_locations` row of its own.
DROP TABLE IF EXISTS business_locations;
