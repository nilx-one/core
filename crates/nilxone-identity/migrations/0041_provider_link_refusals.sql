-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- The last time a provider account could not be linked because the Bond that
-- tried already holds another account of that provider (Core's
-- `ProviderTypeAlreadyLinked`). A partner that sent the person to link, such
-- as 0xda-market's bot, reads it to say why the account is still unlinked.
-- It names no Bond. A successful link of the account removes it.
CREATE TABLE IF NOT EXISTS provider_link_refusals (
    provider TEXT NOT NULL COLLATE BINARY,
    provider_subject TEXT NOT NULL COLLATE BINARY,
    reason TEXT NOT NULL CHECK (reason IN ('provider_type_already_linked')),
    refused_at INTEGER NOT NULL CHECK (refused_at >= 0),
    PRIMARY KEY (provider, provider_subject)
) STRICT;
