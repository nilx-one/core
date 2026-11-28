# Telegram AIS login — Gateway authorization v2 (JSON wire v1)

Status: typed Core contract, durable OIDC transactions, the bot's claim of a verified callback and the chat product session; **no public route or production activation**.

The Core-owned product host is the relying party for Telegram bot commands. It authenticates the dedicated bot service, verifies that update metadata came from a genuine private Telegram message, rate-limits and deduplicates the provider/actor/chat/message tuple, and creates a durable, one-time sign-in transaction. A bot transport cannot bind an AIS identity to a Bond.

## Request / response

Route: POST /api/v1/telegram/login/start

Authentication: only `nilx-one/bot` holds the Telegram Bot API token. It derives a domain-separated Ed25519 signing key inside the Bot process. Core receives only the pinned `TELEGRAM_BOT_ID` and `TELEGRAM_BOT_PUBLIC_KEY`. The header `x-0x1-telegram-bot-signature: t=<unix>,v2=<hex>` signs `0x1-telegram-bot-request-v2\n<timestamp>\n<path>\n<body>`. Core verifies it within 60 seconds and keeps the stable workload subject `telegram-bot:<bot id>`. Production does not accept v1 HMAC or a Bot API token in its environment. A valid workload signature does not prove an AIS identity, Bond session or permission to perform a product action. POST `/api/v1/telegram/gateway/health` uses the same middleware and returns 204 without modifying product state. It does not consume the product command quota. Within the 60-second signature window the request may be replayed: login start deduplicates the original message, confirm is one-use, admin decisions recheck current authority and pending status, exact-lease ACKs are no-ops on replay, and health is read-only. Pulls can claim new work and are rate limited; do not treat signatures alone as replay prevention. Product actions retain their own authorization and idempotency contracts.

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

`complete_telegram_login_with_proof` verifies it using the public bot ID
and Telegram's production Ed25519 key (`nilxone_identity::telegram_init_data`):
- Telegram's third-party signature, without HMAC fallback;
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

### Live location

Route: POST /api/v1/telegram/location (`nilxone_api::bot_login::CHAT_LOCATION_PATH`),
behind the same bot-service authentication as login start.

The body is the chat (`ChatActivationQuery`) plus `latitude`, `longitude`
(degrees) and `observed_at` (Unix seconds, Telegram's message time):

~~~json
{"chat":{"version":1,"provider":"telegram","provider_actor_id":42,"chat_id":42},
 "latitude":50.45,"longitude":30.52,"observed_at":1700000000}
~~~

Core applies it as a `live` Bond location only when the chat's session is
active and unexpired, was opened before the observation, and is bound to a
human Bond; the observation is at most 120 seconds old (5 seconds of clock
skew allowed) and newer than the stored one; and the Bond is not in `manual`
mode. The check and the write are one statement. Answers: `204` applied,
`409 location_not_applied` refused, `422 invalid_location` for a coordinate out
of range. Each sender has its own budget of 60 reports a minute, apart from the
chat's other routes. Neither the coordinate nor the request is logged.

### Progress for the page

Route: GET /api/v1/telegram/login/progress?state=… (public, like the callback).

The callback page asks how its sign-in stands, with the state it was given;
the state was spent at the callback and only reads here. The answer names no
account and carries no session value:

~~~json
{"stage":"awaiting_approval","active_client":"Mac · Safari"}
~~~

`stage` is `waiting` (the bot has not claimed yet), `awaiting_approval`,
`signed_in` (every other state of the live chat session this `/login`
opened), `declined` (that session is gone: replaced or signed out) or
`failed`. An
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
  Mini App or a browser is active, the chat signs in inactive and stays so
  until the shared Telegram session exists.
- The chat never asks to take activity by itself. Owner decision of
  2026-11-26: the Telegram chat and the Mini App share one session, so a chat
  asking the Mini App of the same Telegram account would hand activity to
  itself. Until that shared session exists, the chat signs in inactive beside
  another active client, and the page reports it signed in: every state of a
  live chat session but its own open request is `signed_in`. A device that
  asks while the chat is active is approved in the chat
  (`docs/bot-notifications.md`).
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

Core persists login transactions, confirmations and the chat session binding. `tools/telegram-login-host` serves start, callback and confirm with `TELEGRAM_BOT_ID`, `TELEGRAM_BOT_PUBLIC_KEY` and the product `DATABASE_URL`. AIS must allow `telegram_binding` for client `0x1` (aiaiaiai-org/4x-ais#28).

- nilx-one/bot#4 — test Telegram adapter for gateway-issued AIS URL (depends on bot#1)
- nilx-one/core#122 — authenticated Core-owned Gateway runtime
- aiaiaiai-org/4x-ais#13 and #14 — signed subject continuity and scoped AIS entry
- nilx-one/web#621 — controlled legacy Bond migration and callback

No merge, deploy, production token cutover or data migration is implied.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
## Public-authority rollout

Bot signer reference: nilx-one/bot#44, merged `260ff1a629b16aced5831f6a2ffd1b78d8dbc399`; the synthetic key/header/body/path/time vector matches Core's verifier fixture. Telegram production public key is pinned to https://core.telegram.org/bots/webapps#validating-data-for-third-party-use. Real production initData and first-login acceptance remain a live cutover gate; do not log or commit a live user's proof.

Infra#181 disables scheduled pair activation; Infra#182 prepares public metadata, removes the old token from Core's environment, validates both candidates and restores the previous host, poller, and ingress on failure. No production dispatch is implied by merging this PR.
