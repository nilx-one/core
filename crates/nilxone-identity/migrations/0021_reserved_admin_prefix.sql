-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Public registration cannot allocate 0x0. Historical role rows stay intact;
-- startup must never re-grant roles based on address prefix.
-- Storage-level defense also covers direct inserts by other service adapters.
CREATE TRIGGER IF NOT EXISTS identities_reserved_admin_prefix_insert
BEFORE INSERT ON identities
WHEN NEW.identity_kind = 'human' AND substr(NEW.pub_dress, 1, 3) = '0x0'
BEGIN
    SELECT RAISE(ABORT, 'reserved_pub_dress_prefix');
END;

-- Human discriminators are immutable; changing only the slug stays valid.
CREATE TRIGGER IF NOT EXISTS identities_human_discriminator_update
BEFORE UPDATE OF pub_dress, identity_kind ON identities
WHEN NEW.identity_kind = 'human'
    AND (OLD.identity_kind <> 'human'
         OR substr(NEW.pub_dress, 1, 3) <> substr(OLD.pub_dress, 1, 3))
BEGIN
    SELECT RAISE(ABORT, 'immutable_pub_dress_discriminator');
END;
