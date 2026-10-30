-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Configuration is owner-controlled Avaia identity state. It is deliberately
-- separate from identity existence and from host/device AI runtime availability.
CREATE TABLE IF NOT EXISTS avaia_configuration (
    avaia_pub_dress TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    owner_pub_dress TEXT NOT NULL UNIQUE COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    configuration_state TEXT NOT NULL
        CHECK (configuration_state IN ('unconfigured', 'configured'))
) STRICT;

-- Existing Avaia identities have never crossed the owner-controlled setup save
-- boundary. Backfill only that observable fact; never infer configuration from
-- address shape or local runtime availability.
INSERT OR IGNORE INTO avaia_configuration (
    avaia_pub_dress,
    owner_pub_dress,
    configuration_state
)
SELECT pub_dress, owner_pub_dress, 'unconfigured'
FROM identities
WHERE identity_kind = 'avaia' AND owner_pub_dress IS NOT NULL;

-- Once this migration is active, every newly created/reconciled Avaia begins
-- with explicit persisted state in the same transaction as its identity row.
CREATE TRIGGER IF NOT EXISTS identities_create_avaia_configuration
AFTER INSERT ON identities
WHEN NEW.identity_kind = 'avaia'
BEGIN
    INSERT OR IGNORE INTO avaia_configuration (
        avaia_pub_dress,
        owner_pub_dress,
        configuration_state
    ) VALUES (
        NEW.pub_dress,
        NEW.owner_pub_dress,
        'unconfigured'
    );
END;

-- Preserve backward compatibility for the existing address-only Avaia mutation:
-- a direct owner rename is a profile save. A human Bond rename moves both the
-- Avaia address and owner reference together, so it does not satisfy this WHEN
-- clause and cannot fabricate a configured state.
CREATE TRIGGER IF NOT EXISTS identities_configure_avaia_on_direct_rename
AFTER UPDATE OF pub_dress ON identities
WHEN NEW.identity_kind = 'avaia'
    AND OLD.pub_dress <> NEW.pub_dress
    AND OLD.owner_pub_dress IS NEW.owner_pub_dress
BEGIN
    UPDATE avaia_configuration
    SET configuration_state = 'configured'
    WHERE owner_pub_dress = NEW.owner_pub_dress;
END;
