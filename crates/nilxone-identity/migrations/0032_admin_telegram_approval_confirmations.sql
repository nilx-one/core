-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- A committed admin-role grant queues one result announcement per eligible
-- Telegram-linked administrator. Not queued for rejected requests.
-- Operational notifications; no BondChain or Relationship side effects.
CREATE TABLE IF NOT EXISTS admin_role_telegram_confirmations (
    pub_dress TEXT COLLATE BINARY NOT NULL
        REFERENCES admin_role_requests(pub_dress) ON UPDATE CASCADE ON DELETE CASCADE,
    telegram_user_id INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempt_at INTEGER,
    delivered_at INTEGER,
    PRIMARY KEY(pub_dress, telegram_user_id)
) STRICT;
