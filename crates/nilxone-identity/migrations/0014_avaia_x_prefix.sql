-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- An Avaia address is its owner's pub_dress without the leading `0`:
-- `0x0sky` owns `x0skai`, no longer `0skai`. Stored addresses predate that
-- shape and gain the literal `x`; every referencing column follows through
-- ON UPDATE CASCADE.
--
-- This is a format change, not an owner's rename, so the trigger that marks a
-- renamed Avaia configured must not see it. Repository code re-applies
-- 0007_avaia_configuration.sql in the same transaction, which recreates it.
DROP TRIGGER IF EXISTS identities_configure_avaia_on_direct_rename;

UPDATE identities
SET pub_dress = 'x' || pub_dress
WHERE identity_kind = 'avaia' AND substr(pub_dress, 1, 1) <> 'x'
    -- A business station (0024) is ownerless and keeps its business's prefix.
    AND owner_pub_dress IS NOT NULL;
