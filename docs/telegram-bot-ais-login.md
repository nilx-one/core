# Telegram AIS login — Gateway wire v1

Status: typed Core contract only, **no public route or production activation**.

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

The host (not the bot) exchanges the authorization code and verifies issuer, EdDSA signature, expected 0x1 audience, nonce, expiry and signed successor subject continuity. It attaches an existing Bond only through Core's authorized mapping, never through public address, username or chat ID. The initiating Telegram sender must explicitly confirm an account-link or account-switch attempt, particularly where the browser authenticated as a different identity. Authorization code, id_token, cookie and product-session bearer values never appear in Telegram messages.

Single Active Client remains enforced. A linked provider does not silently activate Telegram or permit game writes. The Telegram text client is a separate Core-controlled product client from the Web Mini App, with its **own** product session. Switching between these clients requires the canonical confirmed handoff; no implicit sharing, cookie reuse, or activated session inference is permitted. E2E cases must cover expiry, replay, wrong sender, wrong browser account, callback tampering, revocation, conflict, restoration, sign-out and handoff.

## Dependencies

This PR defines portable DTOs and structural validation only. It does **not** expose an HTTP route, persist login transactions, register client_id=0x1, finalize account linking or enable gameplay.

- nilx-one/bot#4 — test Telegram adapter for gateway-issued AIS URL (depends on bot#1)
- nilx-one/core#122 — authenticated Core-owned Gateway runtime
- aiaiaiai-org/4x-ais#13 and #14 — signed subject continuity and scoped AIS entry
- nilx-one/web#621 — controlled legacy Bond migration and callback

No merge, deploy, production token cutover or data migration is implied.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->