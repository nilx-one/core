-- © 2026 aiaiaiai · aiaiaiai.org
-- SPDX-License-Identifier: MPL-2.0

-- Telegram delivery presentation state, never Bond or BondChain data.
-- IDs survive restart so a private chat has one Open, one mutable status,
-- and (only for authorized admins) one mutable admin inbox.
CREATE TABLE IF NOT EXISTS telegram_message_slots (
    chat_id INTEGER PRIMARY KEY,
    open_message_id INTEGER,
    status_message_id INTEGER,
    admin_message_id INTEGER
) STRICT;
