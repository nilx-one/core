<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Bot notifications: what the chat hears

Status: Core contract and storage; the bot and the producers adopt it separately. Wire in `nilxone-api::bot_notifications`, authority in `nilxone-identity` (`PublicInfoRepository`). Owner decisions of 2026-11-26.

The back-end is the single source of what happened to a Bond. While the person plays in the Mini App or a browser, the back-end queues what the Telegram chat should hear, and the bot, the only writer to Telegram, fetches it, words it and delivers it. Nothing is pushed to Telegram from anywhere else.

## What is queued

| `kind` | When | Carries |
| --- | --- | --- |
| `activation_requested` | A device asks to take over while the chat is the active client. | `label` (the device), `request_id` to answer with |
| `find` | The Bond picked up a find of a notable rarity. | `rarity`, `label` (the item, if named) |
| `orb` | The Bond picked up an experience orb of a notable rarity. | `rarity` |
| `landmark` | The Bond reached a landmark of a notable rarity. | `rarity`, `label` (the landmark, if named) |

Notable means Rare, Special and Exclusive. A rarity is told by the scale its object lives on now: finds still roll on the legacy catalog (`rare`, `legendary`) until it migrates (`docs/rarity-v3.md`); orbs and landmarks use `RarityV3` (`rare`, `special`, `exclusive`). A legacy `legendary` is never told as `special` or `exclusive`.

Only `activation_requested` is produced by Core itself (`request_activation`, when the session that has to agree is the chat's). Finds, orbs and landmarks are queued by the back-end where it learns of them, with `enqueue_bot_notification`.

## Rules

- A notification is stored only while the owner has a live signed-in chat. Nobody would hear it otherwise.
- `dedupe` keeps one row per event (the request, attestation, orb or landmark); the same event again changes nothing.
- The chat does not need to be the active client to hear: it only shows.
- A notification lives until the bot acknowledges it, or a day.
- `label` is at most 80 bytes, without control characters. Nothing sealed travels: a find is told by its rarity and display name, never by its history or keys.

## Routes

Behind the same bot-service signature as login start (`docs/telegram-bot-ais-login.md`), with a budget of 240 calls a minute per bot.

- `POST /api/v1/telegram/notifications/pull` `{"version":1,"limit":1..=50}` answers `{"notifications":[…]}`: the oldest undelivered notifications of the chats this bot signed in, each with the `provider_actor_id` whose private chat it goes to. Pulling marks nothing.
- `POST /api/v1/telegram/notifications/ack` `{"version":1,"ids":[…]}` (1 to 50) deletes the delivered ones of this bot's chats and answers `{"acknowledged": n}`.

Delivery is at least once: a bot that delivered and then lost the acknowledgement delivers again on the next pull. The bot answers an `activation_requested` with `POST /api/v1/telegram/activation/answer`.
