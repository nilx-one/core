# Telegram AIS login — Gateway wire v1

Status: typed Core contract, durable OIDC transactions, sender confirmation and the chat product session; **no public route or production activation**.

The Core-owned product host is the relying party for Telegram bot commands. It authenticates the dedicated bot service, verifies that update metadata came from a genuine private Telegram message, rate-limits and deduplicates the provider/actor/chat/message tuple, and creates a durable, one-time sign-in transaction. A bot transport cannot bind an AIS identity to a Bond.

## Request / response

Route: POST /api/v1/telegram/login/start

Authentication: the bot signs each request with the Telegram Bot API token of `@nilxone_bot`. The bot and the host both already hold that token, so no other credential exists (`nilxone_api::bot_login::sign_request`). The header `x-0x1-telegram-bot-signature: t=<unix>,v1=<hex>` carries HMAC-SHA256 under a key derived from the token, over the timestamp, the path and the body. The host accepts it within 60 seconds and names the workload `telegram-bot:<bot id>`. Whoever holds the bot token already controls the chat, so the signature grants no new authority. A replay inside the window reaches only idempotent login start or one-use confirmation.

Request JSON:

~~~json
{"version":1,"provider":"telegram","provider_actor_id":123,"chat_id":123,"message_id":42}
~~~

Response JSON:

~~~json
{"authorization_url":"https://auth.aiaiaiai.org/0x1?...","expires_in_seconds":300}
~~~

The standard API error envelope carries refusal codes from nilxone_api::bot_login.

The host generates unique state, nonce, PKCE S256 verifier/challenge, an exact registered OIDC redirect and a single-use pending-login record. The returned URL is only a request to **start** login, not proof of an identity link or an active product session. The TTL is at most 900 seconds. Retried original Telegram messages must not mint additional independent grants.

## Callback obligations

The host (not the bot) exchanges the authorization code and verifies issuer, EdDSA signature, expected 0x1 audience, nonce and expiry. It attaches an existing Bond only through Core's authorized mapping, never through public address, username or chat ID. Before opening a product session, the host must verify that AIS provider evidence for the initiating Telegram actor matches the callback's exact issuer and 0x1 subject. It must obtain explicit confirmation from the original sender in the private chat. A valid OIDC state does not establish that the person opening the URL owns that Telegram account. After a failed code exchange the bot must prompt for a fresh /login message. Authorization code, id_token, cookie and product-session bearer values never appear in Telegram messages.

Single Active Client remains enforced. A linked provider does not silently activate Telegram or permit game writes. The Telegram text client is a separate Core-controlled product client from the Web Mini App, with its **own** product session. Switching between these clients requires the canonical confirmed handoff; no implicit sharing, cookie reuse, or activated session inference is permitted. E2E cases must cover expiry, replay, wrong sender, wrong browser account, callback tampering, revocation, conflict, restoration, sign-out and handoff.

Login start asks AIS for the scope `openid pub_dress telegram_binding` on the
Telegram callback, the only redirect AIS accepts it on. It also sends
`telegram_binding_hint`, a SHA-256 over the transaction's nonce and the
`/login` sender (`aiaiaiai_sign_in::telegram_binding_hint`). AIS answers with
one bit inside the ID token Core already verifies: `"telegram_binding": true`
if the person's linked Telegram account is that sender and the PKCE code
exchange supplies the same hint, recomputed from the stored sender. AIS
discloses nothing else about the linked account, the host holds no AIS
credential, and no second request is made. A missing or `false` binding stops
the login. A match allows the host to ask for confirmation in the original
chat. The hint travels in the authorization URL, which goes only to the
sender's own chat (4x-ais `docs/runtime.md` states the guarantees).

## Telegram login page (first sign-in without an AIS Telegram link)

AIS confirms `telegram_binding` only for a Telegram account recorded in its
`telegram_links`. That table is filled when 0x1 reports a verified link, so a
first chat sign-in usually has no such link. The login page proves the sender
without one, using a credential the host already holds:

1. After `/login`, the bot shows a Telegram Mini App button for
   `GET /api/v1/telegram/login/app?m=<login message id>`
   (`nilxone_api::bot_login::APP_PATH`). This is the login page, not the game
   Mini App.
2. Inside Telegram the page posts `Telegram.WebApp.initData` to the same path.
   The host verifies Telegram's signature with `TELEGRAM_BOT_TOKEN`, checks
   that it is fresh (5 minutes) and that it is not a bot. It reads the sender
   from it.
3. `bind_telegram_login_browser` binds the open transaction of that workload,
   sender and message to this browser: Core stores the SHA-256 of a random
   token. The host sets the token as an HttpOnly, Secure, `SameSite=Lax`
   `__Host-` cookie, and the page goes on to AIS in the same browser.
4. On the callback, a bound transaction requires that cookie. Without it, or
   with another browser's, the callback is refused before the code is spent.
   An authorization link handed to somebody else therefore cannot finish the
   sender's login. The sender is proven when AIS says `telegram_binding: true`
   or the bound browser returns.
5. The success page clears the cookie and closes the Mini App. The player is
   back in the chat and confirms there. Only the original sender can confirm.

## Sender confirmation and the chat session

A verified, provider-matched callback is stored in
`telegram_login_confirmations` for at most 300 seconds. It holds the AIS
issuer, pairwise subject, display snapshot and `auth_time` it will attach by,
keyed by the one-use `/login` message: bot workload, sender, private chat and
message ID.

`PublicInfoRepository::confirm_telegram_login(workload, sender, chat, message,
now, session_expires_at)` is the only way from a callback to a product
session. The host calls it with the workload it authenticated and the sender,
chat and `/login` message from a verified Telegram confirm press. A different
sender, chat, workload or message, an expired or a used confirmation returns
`None`. Otherwise Core deletes the confirmation, attaches the profile by
`(issuer, subject)` and opens a session labelled `Telegram · Chat`:

- Single Active Client holds across all of the owner's clients in the same
  product database. The chat is active only when no other client is; when the
  Mini App or a browser is active, the chat signs in inactive and becomes
  active through the canonical confirmed handoff.
- A new confirmation in the same chat replaces that chat's previous session
  in the same transaction, so signing in again on the active chat keeps it
  active.
- The session token is random and discarded: the chat never holds a bearer
  value. `telegram_chat_sessions` binds (workload, sender) to the session
  digest, and `restore_telegram_chat_session` re-proves the owner in SQL for a
  later update. `close_telegram_chat_session` signs the chat out. Ending or
  expiring the session drops the binding.

### Confirm route

Route: POST /api/v1/telegram/login/confirm (`nilxone_api::bot_login::CONFIRM_PATH`), behind the same bot-service authentication as login start.

Request JSON is the login-start shape, carrying the confirm press's sender and chat and the original `/login` message id. Response JSON:

~~~json
{"profile_id":"…","pub_dress":"0x0sky","active":false}
~~~

Refusals are `LoginConfirmRefusal`; `login_confirmation_not_found` (404) covers an unfinished, expired or used browser step and a different sender alike. The response never carries a session token.

## Dependencies

Core persists login transactions, confirmations and the chat session binding. `tools/telegram-login-host` serves start, callback and confirm with `TELEGRAM_BOT_TOKEN` and the product `DATABASE_URL`. AIS must allow `telegram_binding` for client `0x1` (aiaiaiai-org/4x-ais#28).

- nilx-one/bot#4 — test Telegram adapter for gateway-issued AIS URL (depends on bot#1)
- nilx-one/core#122 — authenticated Core-owned Gateway runtime
- aiaiaiai-org/4x-ais#13 and #14 — signed subject continuity and scoped AIS entry
- nilx-one/web#621 — controlled legacy Bond migration and callback

No merge, deploy, production token cutover or data migration is implied.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->