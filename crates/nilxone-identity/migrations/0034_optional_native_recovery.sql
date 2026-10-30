-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Preserve issued keys; NULL means no recovery key was issued.
-- Column-level changes keep credentials, sessions and their foreign keys intact.
ALTER TABLE native_credentials ADD COLUMN optional_recovery_key_hash BLOB;
UPDATE native_credentials SET optional_recovery_key_hash = recovery_key_hash;
ALTER TABLE native_credentials DROP COLUMN recovery_key_hash;
ALTER TABLE native_credentials RENAME COLUMN optional_recovery_key_hash TO recovery_key_hash;
