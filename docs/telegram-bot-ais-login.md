# Telegram AIS login — Gateway wire v1

Status: typed Core contract, durable OIDC transactions, the bot's claim of a verified callback and the chat product session; **no public route or production activation**.

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

The host (not the bot) exchanges the authorization code and verifies issuer, EdDSA signature, expected 0x1 audience, nonce and expiry. It attaches an existing Bond only through Core's authorized mapping, never through public address, username or chat ID. Before opening a product session, the host must verify that AIS provider evidence for the initiating Telegram actor matches the callback's exact issuer and 0x1 subject. A valid OIDC state does not establish that the person opening the URL owns that Telegram account; the provider match does, so the bot then claims the callback without asking the person to press anything. After a failed code exchange the bot must prompt for a fresh /login message. Authorization code, id_token, cookie and product-session bearer values never appear in Telegram messages.

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
the login. A match lets the bot claim the callback for the original
`/login` message. The hint travels in the authorization URL, which goes only to the
sender's own chat (4x-ais `docs/runtime.md` states the guarantees).

## First sign-in without an AIS Telegram link

AIS confirms `telegram_binding` only for a Telegram account recorded in its
`telegram_links`. 0x1 fills that table by reporting verified links, so a first
chat sign-in usually finds no link. The sign-in page stays
`auth.aiaiaiai.org/0x1`; the bot opens it as a Telegram Mini App button.
Inside Telegram that page copies `Telegram.WebApp.initData` into its form.
AIS passes the value through unverified as `telegram_init_data` in the token
response (4x-ais `docs/runtime.md`).

`complete_telegram_login` verifies it with the bot token
(`nilxone_identity::telegram_init_data`):
- Telegram's HMAC under `WebAppData`;
- at most 10 minutes old;
- no duplicated fields and no bot user;
- the account must be the `/login` sender.

The sender then counts as proven exactly as with `telegram_binding: true`.
Forwarding the link does not help someone else: their Telegram signs their
own account. Outside Telegram there is no `initData`, so only the AIS link
can bind.

The success page plays the sign-in stages, taps Telegram's haptic engine and
closes the Mini App through the Mini App bridge, with one inline script and
style admitted by their CSP hashes. The player is back in the chat, where the
bot has already claimed the login. The stages say that an account and its
Bond are created only on a first sign-in, when no 0x1 profile exists yet for
the AIS identity (`telegram_login_returns`); a returning sign-in plays the
sign-in stages instead and creates nothing.

### Chat activation

Routes: POST /api/v1/telegram/activation and POST
/api/v1/telegram/activation/answer
(`nilxone_api::bot_login::CHAT_ACTIVATION_PATH`, `CHAT_ACTIVATION_ANSWER_PATH`),
behind the same bot-service authentication as login start.

The first takes the sender and chat (`ChatActivationQuery`) and answers the
chat's own `ActivationState`. When a device asks while the chat is active,
it is `approval_requested`, and the bot shows approve and decline. The second
(`ChatActivationAnswer`) carries the request id and `approve`. The chat keeps
no device state and its data is parked already, so approving is the switch
(`docs/client-activation.md`). A chat without a session is
`provider_authentication_required` (401); other refusals are
`ActivationRefusal` codes. Each sender has its own budget of 60 calls a
minute.

### Progress for the page

Route: GET /api/v1/telegram/login/progress?state=… (public, like the callback).

The callback page asks how its sign-in stands, with the state it was given;
the state was spent at the callback and only reads here. The answer names no
account and carries no session value:

~~~json
{"stage":"awaiting_approval","active_client":"Mac · Safari"}
~~~

`stage` is `waiting` (the bot has not claimed yet), `awaiting_approval`,
`signed_in`, `declined` (refused, ran out or replaced) or `failed`. An
unknown state is `login_not_found` (404). The page plays the sign-in stages
on `signed_in`; on `awaiting_approval` it shows «Запит підтвердження» and the
client that has to agree; on `declined` that stage shows 🚫. The page's CSP
admits `connect-src 'self'` for this one route.

## The bot's claim and the chat session

A verified, provider-matched callback is stored in
`telegram_login_confirmations` for at most 300 seconds. It holds the AIS
issuer, pairwise subject, display snapshot and `auth_time` it will attach by,
keyed by the one-use `/login` message: bot workload, sender, private chat and
message ID.

`PublicInfoRepository::confirm_telegram_login(workload, sender, chat, message,
now, session_expires_at)` is the only way from a callback to a product
session. The host calls it with the workload it authenticated and the sender,
chat and `/login` message the bot sent at login start. The bot asks right
after `/login`, about every 2.5 seconds while the browser step runs, so the
chat shows the result without a press. A different
sender, chat, workload or message, an expired or a used confirmation returns
`None`. Otherwise Core deletes the confirmation, attaches the profile by
`(issuer, subject)` and opens a session labelled `Telegram · Chat`:

- Single Active Client holds across all of the owner's clients in the same
  product database. The chat is active only when no other client is; when the
  Mini App or a browser is active, the chat signs in inactive and becomes
  active through the canonical confirmed handoff.
- When another client is active, the chat signs in inactive and, in the same
  transaction, asks that client (`docs/client-activation.md`, a client
  without device state). The password signed the person in; the active
  client agrees and uploads its data, the back-end parks it for the owner and
  the chat becomes active. A refusal, or a request that runs out, leaves the
  chat signed in and inactive.
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

Request JSON is the login-start shape: the `/login` sender, chat and message id. Response JSON:

~~~json
{"profile_id":"…","pub_dress":"0x0sky","active":false}
~~~

Refusals are `LoginConfirmRefusal`:

- `login_confirmation_not_found` (404): the browser step is unfinished, expired or used, or the sender differs. The bot keeps asking until the login expires.
- `login_failed` (410): the browser step reached the callback and failed. AIS answered with an error, the code exchange failed, or the identity is not this sender's. `telegram_login_failures` records this for 300 seconds, so the bot says so at once and stops asking.

The response never carries a session token.

## Delivery

On a `master` push, Rust CI attests the static `nilxone-telegram-login-host`
binary it verified (`actions/attest-build-provenance`). aiaiaiai-org/infra
(**Deliver Telegram runtime**) takes Core's `master` head, verifies that
attestation for `rust.yml`, the branch and the commit, and replaces the login
host on edge-dev-1. Core holds no credential for Infra.

## Dependencies

Core persists login transactions, confirmations and the chat session binding. `tools/telegram-login-host` serves start, callback and confirm with `TELEGRAM_BOT_TOKEN` and the product `DATABASE_URL`. AIS must allow `telegram_binding` for client `0x1` (aiaiaiai-org/4x-ais#28).

- nilx-one/bot#4 — test Telegram adapter for gateway-issued AIS URL (depends on bot#1)
- nilx-one/core#122 — authenticated Core-owned Gateway runtime
- aiaiaiai-org/4x-ais#13 and #14 — signed subject continuity and scoped AIS entry
- nilx-one/web#621 — controlled legacy Bond migration and callback

No merge, deploy, production token cutover or data migration is implied.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->