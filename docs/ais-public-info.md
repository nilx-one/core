<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# AIS identity and 0x1 public information

Status: first Core integration slice, not activated in any host.
Owner decision, 2026-11-06: the 0x1 service logic belongs in `nilx-one/core`.
This supersedes the separate, unnamed `aiaiaiai-org` game repository proposed
in Web's `docs/identity-boundary.md`. AIS remains `aiaiaiai-org/4x-ais`.

## Two authorities

| AIS (`4x-ais`) | 0x1 (Core, server-side) |
| --- | --- |
| Shared identity, unique public name, credentials, connected providers | Product profile attached to that identity |
| OIDC issuer and client-specific pairwise subjects | Validates AIS sign-in for client `nilx-one` |
| Identity's own sessions | Product session, owned and checked by its HTTP host |
| Current `pub_dress` claim | Display snapshot of `pub_dress`, never an account key |
| No game concepts | 0x1 public information; other game state remains in its existing modules |

`public_info::PublicInfoService` is Core's product-side service. It composes
`aiaiaiai_sign_in::SignInFlow` with `PublicInfoRepository`, which uses a
**separate 0x1 database**. No native credentials, provider bindings or private
AIS identifier are created there. It is a server library, not an HTTP server
and not part of the Wasm or Swift client graph. For now it lives in the existing
server-only `nilxone-identity` crate alongside the OIDC verifier; that package
location does not make it part of AIS's data authority.

## Sign-in and attachment

1. The host calls `PublicInfoService::start` and applies its redirect and
   transaction cookie. Password entry belongs to `auth.aiaiaiai.org`.
2. The callback passes its cookie and query to `PublicInfoService::finish`.
   `SignInFlow::verify_callback` checks the signed transaction, expiry,
   state, code exchange, PKCE binding, EdDSA signature, issuer, audience,
   nonce and token/authentication times with the existing implementation.
3. The service accepts only the `nilx-one` client configuration. A token for
   Prism or Market does not authorize access to a 0x1 profile.
4. A single atomic upsert finds or creates the local profile by `(iss, sub)`.
   No lookup by `pub_dress` attaches an old Bond. Equal public names from
   different subjects never share product state.
5. The host receives an `AuthenticatedProfile` and a validated same-origin
   return path. It issues its own 0x1 session and clears the transaction
   cookie on success **and** failure. It must bind the authorization result
   to that session, check session validity/revocation on every private
   request, and apply CSRF protection before mutations. The Rust object is
   an in-process authorization result, not a client bearer token.

### Product sessions

The host owns the session cookie, its lifetime, CSRF and rate limits. Core
stores what makes a session trustworthy on a later request or in another
process: `PublicInfoRepository::open_session` records a digest of the host's
token against the exact `(issuer, subject, profile_id)` of the owner a
verified callback returned. `restore_session` is the privileged restore path:
it returns an `AuthenticatedProfile` only while the row is unexpired and still
matches its profile in SQL. Nothing the browser sends, other than the token
whose digest is looked up, takes part. `close_session` is sign-out.

A session is never opened for a profile that no longer exists, a token digest
cannot be opened twice, and deleting a profile deletes its sessions. Opening
a session drops that owner's expired rows. The restored profile carries the
current display name and experience, not a snapshot from sign-in.

A session is active or inactive, and a Bond has at most one active session.
`open_session` takes the client's label and the session the same browser is
replacing, and answers whether the new session is the active client: it is
only when the Bond has no live active session. An inactive session restores
and reads, and `publish_experience_carry` refuses it with `SessionInactive`.
How an inactive session becomes the active one is
[client activation](client-activation.md).

The local `profile_id` is a randomly allocated, stable public product id.
It is neither AIS's private identifier nor an authentication credential.
Display-name changes do not alter it or the experience attached to it. The
newest observed `auth_time` controls the display snapshot; older callbacks
cannot roll it back. A missing `pub_dress` claim preserves the existing
snapshot. Equally timed changes wait for a later authentication or the future
ordered identity-event channel; this slice does not claim real-time rename
synchronization.

A supplied but malformed `pub_dress` claim rejects product sign-in before any
write, even for a known subject or a stale callback. This is an invalid issuer
response, distinct from an absent optional claim. The stored name, XP and last
authentication timestamp remain unchanged.

The product store follows the other Core SQLite stores: WAL, foreign keys,
five connections for a file and one for `:memory:`. A five-second busy timeout
bounds writer contention; WAL permits a writer to commit while another
connection holds a read snapshot, but does not allow concurrent writers.

The legacy `SignInFlow::finish(IdentityRepository, ...)` is retained for
existing hosts. It still has the old first-sight address link. New hosts must
use `PublicInfoService`; changing the legacy method's behavior would activate
an incompatible account migration accidentally.

## Public information and evidence

`read_public(profile_id)` returns only an explicit allowlist:

```json
{
  "profile_id": "<opaque product id>",
  "pub_dress": "0x0sky",
  "experience": {
    "authority": "client",
    "bond_xp": 120,
    "avaia_xp": 45
  }
}
```

`pub_dress` can be null before a display claim is available. Neither `issuer`
nor pairwise `sub` is in this DTO. Passwords, provider subjects, wallet balances,
precise location and internal roles are absent. HTTP hosts should serialize
this DTO only, never `AuthenticatedProfile` or an OIDC result.

`publish_experience_carry` supports the current local `.bnd` carry baseline:
each XP total is monotonic, capped at 1,000,000,000 and retry-safe. Its
authority remains `client`. This does not attest that play occurred, award
seeds, create a completed interaction or produce a BondChain fact. An
`AuthenticatedProfile` is required to mutate the row; knowing its public id
or public name cannot do so. The identity binding is also checked in SQL.

## Delivery and remaining work

This slice creates no running workload and alters no live database. The new
schema is initialized only when a host explicitly opens its product database.
The existing routes, sessions, rewards and issuer worker continue to use their
current storage. No migration or wipe is performed.

Subsequent independently reviewable changes:

1. Implement the actual identity runtime in `4x-ais`. Its `master` currently
   contains a placeholder, stylesheet and image workflow, not an OIDC issuer.
   Keep game state out of that implementation.
2. Add the host adapter for this Core service: its own product database,
   session binding, CSRF/rate limits, public DTO routes and Web sign-in
   redirect. It must not mint sessions for the legacy native identity by
   matching a public address. Core owns shared behavior; the host owns HTTP.
   Sessions go through `open_session` / `restore_session` above; the host
   must not deserialize an `AuthenticatedProfile` supplied by a client.
3. Move the remaining product state behind the same stable product key in
   small changes: committed awards, Avaia, subscriptions, business data and
   game authorization. The existing visit-bound issuer worker remains the
   only seeds debit until its replacement contract is implemented.
4. Add ordered, authenticated identity updates and deletion/session
   invalidation; test renames, deletions and unavailable services across both
   runtimes. Never use a different product's pairwise subject as a 0x1 key.
5. Validate artifacts and deployment configuration, then activate only after
   explicit deployment authorization. Any destructive wipe needs its own
   day-of authorization and approved advance notification.

## Verification

Storage tests cover persistence, concurrent first entry, rename stability,
stale callbacks, conflicting public names, issuer separation, carry retries,
publication caps, missing profiles, the public allowlist, and product sessions
(expiry, sign-out, a refused duplicate token, a mismatched binding, and a
deleted profile). Client activation has its own tests, listed in
[its contract](client-activation.md). A local substitute
issuer exercises HTTP token exchange, JWKS, signature/audience verification
and profile creation through the real Core flow. It is not a live AIS E2E run.
