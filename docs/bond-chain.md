# BondChain

Status: registered pointer only. The BondChain (`bch`) model, its cryptography, and its recovery lifecycle are normatively owned by [`nilx-one/0x1`](https://github.com/nilx-one/0x1) — `documents/04-bondchain-interaction-model.md`, `documents/06-cryptography-and-wire-protocol.md`, and `documents/15-devices-and-recovery.md`. This document does not redefine them; it exists only to record this crate's implementation status and to correct an earlier draft of this file that drifted from canon before it merged.

## What changed from the first draft

An earlier version of this document invented a model that conflicts with 0x1 in two places, worth naming so nobody rediscovers the same dead ends:

- **Key derivation.** It assumed each party holds an independent, non-extractable local key and that a BondChain fact is "mirrored, not duplicated." The canonical model is simpler and stronger: `k = HKDF(ECDH || H(head))` — a shared key both legitimate parties independently derive from their own long-term key material via X25519 ECDH, bound to the current `bond.chain` head. It is not a local-only secret; it is pairwise by construction. See 0x1's Pairwise Key Derivation.
- **Public discovery.** It proposed a public projection of counterparty edges (existence, level, timestamp) so a recovering device could find its counterparties. 0x1's Relationship projection is explicit that no such thing exists: "not a new shared protocol object, not a global social-graph edge, and not operator-owned truth." Recovery instead relies on the human supplying `counterpart_hint` from their own memory or records — there is no queryable registry to fall back on. A product surface that wants to help someone remember who to ask must do so from data the person themselves authorized to keep, never from a protocol-level graph.

## What this crate owns today

Nothing yet. `bch` command/event/effect/projection registries remain empty until an owning interaction contract exists, per this repository's README. `nilxone-kernel` performs no BondChain validation, minting, or recovery today.

## Relationship to `bond.journal`

`docs/presence-journal.md` in this repository predates the canonical glossary term `bond.journal` ("a single-owner local store of observations, priors, and adaptive state... never becomes relationship truth" — 0x1 `documents/02-glossary.md`). The two describe the same boundary; `presence-journal.md`'s content stands, under that canonical name.

## Client posture, restated correctly

0x1 already draws the boundary this document's first draft tried to invent, using its own key roles rather than an invented "decode vs. mint" table:

- `sk_bond` — human-gated signing authority for commitment-bearing `bond.chain` records (`INIT`, `CONSENT`, `ACCEPT`, `ATTEST`, `REKEY`, `REVOKE`, `CONTINUE`).
- `sk_ack` — derived engine authority for acknowledgements and bounded automation; it "has no path to independently create a human commitment."

Holding decrypt access to a `bond.chain` entry (the derived `k`) is not itself either key role. A client posture table belongs, if anywhere, in the Web implementation document (`nilx-one/web/docs/bnd-file-lifecycle.md`), scoped to how a device holds and guards `sk_bond`/`sk_ack` locally — not here, and not redefining what 0x1 already owns.

## Recovery

Fully owned by 0x1 `documents/15-devices-and-recovery.md`: `REC-REQ` (a required `bch_id`, verified against the assisting Bond's own held genesis before the out-of-band code is even checked), the live-device objection window for an `active` `old_device_pk`, and `CONTINUE` scoped to non-terminal histories. This document does not restate that mechanism; see the source.

---

© 2026 aiaiaiai · aiaiaiai.org
