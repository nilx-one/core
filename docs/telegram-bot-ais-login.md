# Telegram AIS login — Gateway wire v1

Status: typed Core contract, durable OIDC transactions, sender confirmation and the chat product session; **no public route or production activation**.

The Core-owned product host is the relying party for Telegram bot commands. It authenticates the dedicated bot service, verifies that update metadata came from a genuine private Telegram message, rate-limits and deduplicates the provider/actor/chat/message tuple, and creates a durable, one-time sign-in transaction. A bot transport cannot bind an AIS identity to a Bond.

## Request / response

Route: POST /api/v1/telegram/login/start

Authentication: dedicated service credential scoped to the official test bot, never the Telegram Bot API token, anonymous cookie, or public user identifier.

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

The Core callback verifier uses the product host’s direct AIS access token
with audience `ais` and scope `telegram.identity.resolve`. It looks up the
Telegram sender recorded in the login transaction. It verifies AIS’s signed
response against a one-use nonce and compares its issuer and 0x1 pairwise
subject with the verified browser callback. A match allows the host to ask
for confirmation in the original chat.

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

## Dependencies

Core persists login transactions, confirmations and the chat session binding. The host still needs to expose the callback and confirm routes, register client_id=0x1 in AIS, and hold the direct AIS token for provider verification.

- nilx-one/bot#4 — test Telegram adapter for gateway-issued AIS URL (depends on bot#1)
- nilx-one/core#122 — authenticated Core-owned Gateway runtime
- aiaiaiai-org/4x-ais#13 and #14 — signed subject continuity and scoped AIS entry
- nilx-one/web#621 — controlled legacy Bond migration and callback

No merge, deploy, production token cutover or data migration is implied.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->