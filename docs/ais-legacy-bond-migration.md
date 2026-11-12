<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Verified migration of legacy Bonds to AIS

Status: proposed protocol contract; **not activated**. Tracks #114; implementation #115 and Web adapter nilx-one/web#621.

## Authorities and invariants

AIS is authoritative for authentication and issues a verified `(iss, pairwise sub)` for the registered `nilx-one` client. Core is authoritative for Bond ownership and 0x1 product state. `pub_dress` is a mutable display label, never proof of ownership, a migration key, or a fallback lookup.

A migration **attaches an AIS identity to an already existing Bond**; it must not create a replacement Bond, reset XP, Avaia, subscriptions, inventory, wallets, or any completed BondChain interaction. Preserve the legacy Bond's stable domain identifier. The current `public_info` product `profile_id` is a distinct randomly generated identifier: do **not** assume that it equals the legacy Bond identifier. A migration may need an explicit, unique `legacy_bond_id -> product_profile_id` mapping, with existing references kept intact. Verify actual schema and ownership of each state store before implementing this mapping.

## Verified schema inventory (2026-11-12)

Inspected Core master `85031293ef9576b430e5d66b782dc69162df8234` and
Web master `039f585670e19217f0e46121a9cd89a6e150e9df`. These are source
observations, not a production database inventory or deployed-image assertion.
Paths below are relative to `crates/nilxone-identity/` unless qualified.

| Existing storage | Actual identifier / proof | Migration consequence |
| --- | --- | --- |
| `migrations/0001_identities.sql`, `0002_provider_accounts.sql` and later identity alterations | `identities.pub_dress` is the legacy primary key, not an immutable Bond UUID | Preserve the row and existing references; do not invent a pre-existing stable UUID |
| `migrations/0035_private_identity_subjects.sql` | `identity_subjects.subject`: immutable private 32-byte identity value, linked to the mutable address with update/delete cascades | Must stay identity-private; do not copy it into product tables, browser DTOs, Hub or logs as a Bond key |
| `migrations/0003_native_auth.sql`, `0016_session_activation.sql` | Native session token digest, expiration, revocation, active flag and credential-authenticated flag | A cookie value or an unexpired inactive session alone is not fresh authority for a migration mutation |
| `migrations/0042_aiaiaiai_accounts.sql` | `(issuer, subject)` to legacy `pub_dress`; unique `(issuer, pub_dress)`; update/delete cascade | Existing links must be inventoried and checked for conflicts; first-sight address matching is not a migration API |
| `public_info_schema.sql` | `public_profiles`: `(issuer, subject)` primary key; unique random 16-byte hex `profile_id`; optional display name | Preserve an existing profile id; never infer equality with a legacy row or private subject |
| `public_info_schema.sql` | `product_sessions`: digest and exact `(issuer, subject, profile_id)` binding, expiration; profile deletion cascades | Restore through Core and recheck the durable Bond binding before authorizing game operations |
| `migrations/0007_avaia_configuration.sql`, `0013_avaia_location.sql` | Avaia ownership/configuration and location reference legacy addresses | Binding must not recreate Avaia or rewrite its configuration/location |
| `migrations/0020_committed_experience.sql`, `0036_cell_orb_claims.sql`, `0044_find_attestations.sql`, `0045_craft_receipts.sql` | Committed experience and claim/receipt ownership reference legacy identities | Product carry XP is not replacement evidence for committed state; retain receipts and claim idempotency |
| `migrations/0037_seeds_special_subscriptions.sql`, `0038_seeds_credit_provenance.sql`, `0043_paid_special_items.sql` | Seeds, subscription clocks, debit receipts and paid items retain legacy ownership references | No balance move, new grant, debit replay or entitlement reset during attachment |
| `migrations/0039_business_memberships.sql` and role repositories | Memberships and game permissions remain product facts | Identity authentication must not grant roles or rewrite memberships |

`src/repository.rs::find_native_session_with_activity` checks expiration and
revocation and returns the active flag, but it does not establish recent
credential authentication. `find_native_session` does not check active status.
The migration service must perform its own complete proof checks; neither
method alone satisfies the proof contract below.

### Database placement and atomicity

Core's product repository can open its own database. The inspected Web host
actually passes the same `database_url` to the legacy repository and
`product_sign_in_from_environment`, which opens `PublicInfoRepository` there
(`services/identity/src/main.rs`). Logical separation of tables is not proof
of separate physical databases. AIS itself remains a different authority and
must not be queried by joining to its database.

For the initial migration implementation, require the local legacy and product
tables to be accessible through one Core-owned SQLite connection/transaction.
Validate this topology before enabling migration; refuse unsupported placement.
Use one `BEGIN IMMEDIATE` transaction for the binding, request consumption,
legacy-session revocation and audit outcome. Two pools pointed at the same
file do not make two separate transactions atomic. Do not use SQLite `ATTACH`
across WAL databases to claim a crash-atomic commit. A genuinely separate-store
installation needs an independently implemented prepare/reconcile protocol and
must remain disabled until its failure/recovery tests pass.

### Additive product binding proposed for #115

The names below describe new storage, not tables that already exist:

- `legacy_bond_keys`: an independently generated opaque product `bond_id`,
  unique legacy `pub_dress` foreign key with update/delete cascade. Allocate
  once for a human Bond inside the attachment transaction. This product key
  is not the identity-private subject. Existing game rows keep their keys.
- `legacy_bond_bindings`: unique `bond_id` and unique `profile_id`. A validated
  product session resolves its exact profile to this binding, then resolves
  the current legacy row through `legacy_bond_keys`. The display name from
  AIS is never used in that lookup. Foreign-key deletion removes live access;
  recreating an address must not resurrect the previous binding.
- `legacy_bond_migrations`: hashed unpredictable transaction handle, bound
  legacy-session digest, expected issuer/client, verified AIS binding,
  creation/expiry timestamps, confirmation and terminal outcome. Keep raw
  OIDC tokens and PKCE material out of this durable record.
- A minimal audit record retained independently of live binding deletion,
  using opaque product references and outcome, without public names,
  credentials, private identity subjects or session digests in logs.

An existing AIS profile may be reused only if it is not linked to another
Bond and has no incompatible product state. For the initial conservative
implementation, nonzero carried XP or any other pre-existing product state
requires explicit conflict resolution; never sum, replace or silently merge
it with committed legacy XP. A new migration callback must verify the AIS
identity without automatically provisioning a replacement profile first.
Allocate a needed profile and the binding only in the successful transaction.

## Required proof

A migration transaction is started from a **freshly authenticated legacy Bond session**, or an equivalent server-verified legacy credential. The server resolves the legacy Bond identifier from that authenticated context, never from an untrusted browser field. Separately, the user completes the normal AIS authorization-code + PKCE flow. Core accepts only a validated `nilx-one` OIDC result with correct issuer, audience, state, nonce, token lifetime, and pairwise subject. The host binds both proofs to one short-lived, single-use migration transaction and requires explicit user confirmation.

Neither equal `pub_dress` values, an email match, a public Bond ID, a screenshot, nor possession of an AIS token **alone** authorizes legacy ownership transfer. Do not accept an AIS subject issued to another client.

### Proof lifetime and revalidation

The initial implementation uses a maximum ten-minute migration lifetime,
without sliding renewal. Legacy credential authentication and AIS `auth_time`
must be at most ten minutes old at confirmation; reject future legacy times.
OIDC verification retains its existing bounded clock leeway. Lifetime checks
are repeated at commit, not only when the form is opened.

The initial supported legacy proof is a freshly credential-authenticated,
active, unrevoked native session for a human Bond. Recheck its token digest,
creation time, expiry, active flag, credential-authenticated flag and ownership
inside the commit transaction. A provider session, recovered/stale browser
state, or caller-provided `fresh=true` does not qualify; those paths require
fresh credential authentication or a separately reviewed equivalent proof.

The AIS proof is obtained through Core's `SignInFlow::verify_callback` using
the migration's bound OIDC transaction. A browser must not supply an arbitrary
`VerifiedSignIn`, issuer, subject or profile as authority. The host keeps
migration OIDC transactions separate from ordinary sign-in transactions; an
ordinary callback cannot be re-used to confirm migration. Confirmation is a
CSRF-protected POST that repeats neither password nor tokens in its response.

## State machine

`pending_legacy_proof -> pending_ais_proof -> confirmed -> committed`; terminal states: `expired`, `cancelled`, `conflict`, `rejected`. A transaction has a random unguessable ID, expiration, server-side legacy session binding, and one verified AIS identity. Expired or cancelled transactions cannot be revived. On successful commit, consume the transaction atomically with the binding.

### Atomic commit preconditions

1. Both proofs are valid, fresh, bound to the same transaction, and confirmation is recorded.
2. The legacy Bond exists, has not been deleted, and its ownership is not already attached to a different AIS identity.
3. The AIS `(issuer, subject)` is not attached to another legacy Bond or incompatible product profile.
4. The target Bond and any already existing product profile resolve consistently; a prior AIS-only profile with game state requires **explicit conflict resolution**, never silent merging or overwriting.
5. The transaction is not consumed and is not expired at commit time.

Use database uniqueness constraints for both identity and legacy-Bond bindings, and a single transaction for binding, transaction consumption and audit record. If relevant data live in multiple databases, use a durable prepare/reconcile protocol rather than pretending a cross-database write is atomic. Do not delete old records or move balances as part of the first binding.

## Result and failure semantics

- `attached`: existing Bond remains the domain identity; its state and references remain unchanged; the legacy proof session is revoked atomically with attachment; the host clears its cookie and opens a fresh product session only after durable commit.
- `already_attached`: retry of the **same verified transaction/identity/Bond** returns the original result without duplicate side effects.
- `identity_conflict` / `bond_conflict`: fail closed; do not detach, reassign, or overwrite either account.
- `proof_invalid` / `expired` / `cancelled`: no mutation.
- `issuer_unavailable`: no mutation; user may start a new authorization attempt.
- `storage_unavailable`: transaction either commits completely or remains recoverable; never report success without a durable binding.

Keep a minimal audit event with transaction ID, stable Bond ID, pseudonymous AIS binding reference, outcome and timestamp; never log raw ID tokens, credentials, PKCE verifier or session tokens. Rate-limit proof attempts and migration starts.

## Recovery and game-session authorization

The successful transaction invalidates pre-existing product sessions for the
target profile as well as the legacy proof session. A new product session is
opened only after commit. If its creation or HTTP delivery fails, keep the
committed binding: a fresh AIS sign-in restores the same account. Do not undo
attachment or restore revoked sessions just because the response was lost.
A committed retry requires an authenticated recovery context bound to the
same principal; possession of a transaction id alone must neither disclose
the outcome nor create a session. It never reruns attachment side effects.

For game requests, restore the product session, resolve the durable binding,
then apply existing product authorization/active-client rules. A valid AIS
profile with no binding is not an existing Bond. Never fall back to its public
name, mint a native credential session, or treat product authentication as a
`gm` role. An expired or signed-out product session cannot use the binding.
Migration must not grant a second active writer; #115/#621 must preserve the
existing single-active-client policy before enabling game mutations.

## Client-id transition is a separate identity migration

Current AIS registration and Core/Web validation use `nilx-one`; `0x1` is the
agreed target. Because the pairwise subject changes with client id, do not
replace the audience string and reattach by name. Before transition, inventory
issued subjects and local bindings without exporting private AIS identifiers.
A transition must use AIS-authoritative continuity evidence for the same
identity, bound to the old and new client, or fresh verified control of both
bindings. Define that proof contract before implementing aliases.

The account's `profile_id`, product Bond key and all game references must remain
stable. Old/new subject aliases must be unique and scoped to the exact issuer
and client; a token for another product cannot use them. Do not globally relax
`PublicInfoService`'s client allowlist. Plan session invalidation, coexistence,
retirement and rollback without rotating AIS secrets. #115 initially targets
the current client; it must not silently activate the rename.

## Test contract and rollout gates

- Existing Bond, XP, Avaia, subscriptions and BondChain references survive sign-in and migration unchanged.
- Renaming a legacy address preserves its opaque product key and access;
  deleting and recreating that address never inherits a prior binding.
- Session revocation, deactivation or expiry between proof and commit prevents
  attachment; a stale credential flag alone cannot authorize it.
- Failure before commit rolls back binding, consumption, revocation and audit
  together; response loss after commit recovers without a second attachment.
- Product-session restore resolves the intended Bond and preserves existing
  game authorization; an unbound or foreign profile cannot act for that Bond.
- Logout revokes product access; replay of a migration handle cannot reopen it.
- Client-id transition tests preserve the same profile and Bond while rejecting
  collisions, foreign-client tokens and unverified continuity claims.
- Matching public address with a different verified AIS subject **never** grants legacy access.
- A forged or stale legacy session, wrong OIDC client, replayed callback, expired transaction and missing confirmation fail without writes.
- Concurrent attempts for the same Bond or AIS identity cannot create two bindings.
- Existing AIS-only product profile with state cannot be silently merged into a legacy Bond.
- Retries after commit are idempotent; injected write failures do not leave half-migrated state.
- Web, Telegram and future native hosts call the same Core service; host-specific cookies and CSRF stay in hosts.
- Migration remains off by default until schema inventory, test fixtures, full CI, controlled E2E with real AIS, recovery plan and explicit deploy authorization.

## Delivery boundaries

1. **#114:** this contract and repository/schema inventory, without activating migration.
2. **#115:** Core verified migration service, durable state transitions, unique constraints and adversarial tests.
3. **nilx-one/web#621:** opt-in legacy-session + AIS callback UX, explicit confirmation and session rotation; default disabled.
4. Separate release validation, legacy data backup, migration monitoring and authorized rollout.

This document defines desired behavior, not proof that existing storage already satisfies it.

---

© 2026 aiaiaiai · aiaiaiai.org
