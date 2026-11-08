-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Crafts and repairs paid from the Identity wallet (docs/find-attestation.md,
-- "Crafting"). The device's craft id is the idempotency key: a retry with the
-- same recipe and inputs answers the first receipt and never debits twice.
CREATE TABLE IF NOT EXISTS craft_receipts (
    craft_id TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        CHECK (length(craft_id) BETWEEN 8 AND 64),
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    recipe_id TEXT NOT NULL CHECK (length(recipe_id) BETWEEN 1 AND 64),
    seeds_debited INTEGER NOT NULL CHECK (seeds_debited >= 0),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0)
) STRICT;

-- Each attested find a craft used up. Used up, it is never sold or used again.
CREATE TABLE IF NOT EXISTS craft_inputs (
    attestation_id TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        REFERENCES find_attestations(attestation_id) ON DELETE CASCADE,
    craft_id TEXT NOT NULL COLLATE BINARY
        REFERENCES craft_receipts(craft_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX IF NOT EXISTS craft_inputs_by_craft ON craft_inputs(craft_id);

-- A thing used up in a craft is not sold, whatever path asks.
CREATE TRIGGER IF NOT EXISTS prevent_sale_of_crafted_input
BEFORE UPDATE OF sold_at_ms ON find_attestations
WHEN NEW.sold_at_ms IS NOT NULL
 AND EXISTS (SELECT 1 FROM craft_inputs WHERE attestation_id = NEW.attestation_id)
BEGIN SELECT RAISE(ABORT, 'attested find was used up in a craft'); END;
