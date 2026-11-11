# Verified migration of legacy Bonds to AIS

Status: proposed protocol contract; **not activated**. Tracks #114; implementation #115 and Web adapter nilx-one/web#621.

## Authorities and invariants

AIS is authoritative for authentication and issues a verified `(iss, pairwise sub)` for the registered `nilx-one` client. Core is authoritative for Bond ownership and 0x1 product state. `pub_dress` is a mutable display label, never proof of ownership, a migration key, or a fallback lookup.

A migration **attaches an AIS identity to an already existing Bond**; it must not create a replacement Bond, reset XP, Avaia, subscriptions, inventory, wallets, or any completed BondChain interaction. Preserve the legacy Bond's stable domain identifier. The current `public_info` product `profile_id` is a distinct randomly generated identifier: do **not** assume that it equals the legacy Bond identifier. A migration may need an explicit, unique `legacy_bond_id -> product_profile_id` mapping, with existing references kept intact. Verify actual schema and ownership of each state store before implementing this mapping.

## Required proof

A migration transaction is started from a **freshly authenticated legacy Bond session**, or an equivalent server-verified legacy credential. The server resolves the legacy Bond identifier from that authenticated context, never from an untrusted browser field. Separately, the user completes the normal AIS authorization-code + PKCE flow. Core accepts only a validated `nilx-one` OIDC result with correct issuer, audience, state, nonce, token lifetime, and pairwise subject. The host binds both proofs to one short-lived, single-use migration transaction and requires explicit user confirmation.

Neither equal `pub_dress` values, an email match, a public Bond ID, a screenshot, nor possession of an AIS token **alone** authorizes legacy ownership transfer. Do not accept an AIS subject issued to another client.

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

- `attached`: existing Bond remains the domain identity; its state and references remain unchanged; a new product session may be issued after the host rotates the legacy session.
- `already_attached`: retry of the **same verified transaction/identity/Bond** returns the original result without duplicate side effects.
- `identity_conflict` / `bond_conflict`: fail closed; do not detach, reassign, or overwrite either account.
- `proof_invalid` / `expired` / `cancelled`: no mutation.
- `issuer_unavailable`: no mutation; user may start a new authorization attempt.
- `storage_unavailable`: transaction either commits completely or remains recoverable; never report success without a durable binding.

Keep a minimal audit event with transaction ID, stable Bond ID, pseudonymous AIS binding reference, outcome and timestamp; never log raw ID tokens, credentials, PKCE verifier or session tokens. Rate-limit proof attempts and migration starts.

## Test contract and rollout gates

- Existing Bond, XP, Avaia, subscriptions and BondChain references survive sign-in and migration unchanged.
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
