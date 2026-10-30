-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Reopen 0x0 allocation after applying legacy 0021 on existing databases.
DROP TRIGGER IF EXISTS identities_reserved_admin_prefix_insert;
