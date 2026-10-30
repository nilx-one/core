-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Durable per-Telegram-recipient delivery attempts; role requests are NOT BondChain.
CREATE TABLE IF NOT EXISTS admin_role_telegram_deliveries (
    pub_dress TEXT COLLATE BINARY NOT NULL
        REFERENCES admin_role_requests(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    telegram_user_id INTEGER NOT NULL,
    last_attempt_at INTEGER NOT NULL,
    delivered_at INTEGER,
    attempts INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(pub_dress, telegram_user_id)
) STRICT;
