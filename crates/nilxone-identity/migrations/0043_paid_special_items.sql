-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

CREATE TABLE IF NOT EXISTS paid_special_items (
    order_id TEXT PRIMARY KEY NOT NULL CHECK(length(order_id) BETWEEN 8 AND 128),
    owner_pub_dress TEXT NOT NULL REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    sku TEXT NOT NULL CHECK(sku IN ('0x1_special_1m','0x1_special_3m','0x1_special_6m','0x1_special_12m')),
    price_id INTEGER NOT NULL CHECK(price_id > 0),
    amount_minor INTEGER NOT NULL CHECK(amount_minor > 0),
    paid_at_ms INTEGER NOT NULL CHECK(paid_at_ms >= 0),
    state TEXT NOT NULL CHECK(state IN ('paid','in_transit','stored','activated')),
    activated_at_ms INTEGER,
    CHECK ((state = 'activated') = (activated_at_ms IS NOT NULL)),
    CHECK (activated_at_ms >= paid_at_ms)
) STRICT;

CREATE INDEX IF NOT EXISTS paid_special_items_owner ON paid_special_items(owner_pub_dress, state);
-- Protect an item receipt even if an older binary is restarted during rollout.
CREATE TRIGGER IF NOT EXISTS prevent_duplicate_direct_grant
BEFORE INSERT ON bond_seed_subscription_debits
WHEN EXISTS (SELECT 1 FROM paid_special_items WHERE order_id = NEW.order_id)
BEGIN SELECT RAISE(ABORT, 'order already issued as item'); END;

CREATE TRIGGER IF NOT EXISTS prevent_duplicate_paid_item
BEFORE INSERT ON paid_special_items
WHEN EXISTS (SELECT 1 FROM bond_seed_subscription_debits WHERE order_id = NEW.order_id)
BEGIN SELECT RAISE(ABORT, 'order already granted'); END;
