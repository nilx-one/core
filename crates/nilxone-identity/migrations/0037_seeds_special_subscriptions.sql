-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Identity is the only authority for a Bond's paid 0x1 entitlements and
-- in-game seeds. Neither market, a browser balance nor a client inventory
-- can directly insert into these tables.
--
-- Deliberately no initial balance, automatic credit, purchase endpoint or
-- checkout activation: an authenticated seed-earning/import contract must
-- supply server-verifiable credits before any funded order can be fulfilled.
CREATE TABLE IF NOT EXISTS bond_seeds (
    owner_pub_dress TEXT PRIMARY KEY COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    balance INTEGER NOT NULL DEFAULT 0 CHECK (balance >= 0)
) STRICT;

-- A full Core SubscriptionClock persisted after the atomic issuer debit.
-- Core, not SQL/client UI, defines Exclusive priority and paused Special.
CREATE TABLE IF NOT EXISTS bond_subscription_clocks (
    owner_pub_dress TEXT PRIMARY KEY COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    clock_json TEXT NOT NULL CHECK (json_valid(clock_json))
) STRICT;

-- A market issuer order id is the permanent idempotency key. Retain the
-- amount, SKU, recipient and duration of the verified processing snapshot;
-- these facts cannot be replaced with new caller-supplied terms on retry.
CREATE TABLE IF NOT EXISTS bond_seed_subscription_debits (
    order_id TEXT PRIMARY KEY COLLATE BINARY NOT NULL
        CHECK (length(order_id) BETWEEN 8 AND 128),
    owner_pub_dress TEXT NOT NULL COLLATE BINARY
        REFERENCES identities(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    sku TEXT NOT NULL CHECK (
        sku IN ('0x1_special_1m','0x1_special_3m',
                '0x1_special_6m','0x1_special_12m')
    ),
    price_id INTEGER NOT NULL CHECK (price_id > 0),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    duration_ms INTEGER NOT NULL CHECK (duration_ms > 0),
    committed_at_ms INTEGER NOT NULL CHECK (committed_at_ms >= 0)
) STRICT;

CREATE INDEX IF NOT EXISTS bond_seed_subscription_debits_owner
    ON bond_seed_subscription_debits(owner_pub_dress, committed_at_ms);
