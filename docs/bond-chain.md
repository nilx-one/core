# BondChain

Status: registered protocol shape; extends `docs/presence-journal.md`. No owning interaction contract exists yet (see README: "Production command/event/effect/projection registries remain empty until an owning interaction contract exists"). No Rust implementation.

A BondChain fact is what a presence-journal visit deliberately is not: bilateral, consensual evidence of an Interaction between two Bonds. Where a visit record is one client's unilateral claim about itself, a BondChain fact requires a counterpart action from a second, independently-operated kernel before it exists at all. This document registers the shape and the invariants that follow from that difference, so a future Core adoption does not silently invent a different one.

## Relationship to the presence journal

`docs/presence-journal.md` already states the boundary from the other side: a visit "contains no counterpart action, no reciprocity, no consent, and therefore cannot complete an Interaction or produce a BondChain fact." This document is the positive definition that boundary implies.

## Canonical fact shape

```text
BondChainFact {
  parties:     (PubDress, PubDress)   // the two Bonds, order-independent
  content:     opaque, local-only     // "the shared part" — never leaves the two devices
  commitment:  Hash                   // deterministic hash of `content`, computed identically by both kernels
  signatures:  (Signature, Signature) // one per party, over `commitment`
  level:       LevelTag               // public; see "Public projection" below
  signedAt:    u64                    // Unix epoch milliseconds
}
```

`content` is the private payload — what the two Bonds actually shared during the Interaction. `commitment` and `signatures` are what makes the fact verifiable without either side trusting the other's copy: both kernels derive the same commitment from the same content and each signs it with their own Bond key, so a later dispute between two independently-held copies is decidable without a third party ever seeing `content`.

## Mirroring is not duplication

The fact is mirrored, not duplicated. Each party persists its own copy of `content`, re-encrypted at rest with a key that is theirs alone (see `nilx-one/web/docs/bnd-file-lifecycle.md` for the client-side key model). The two on-disk ciphertexts are never expected to be identical — only the plaintext `content` and the `(commitment, signatures)` tuple are shared truth. A protocol implementation must not assume byte-identical storage across parties; it must only be able to verify that two independently-decrypted copies hash to the same `commitment`.

## Public projection

Everything above stays on the two devices. A separate, deliberately small public projection may be held wherever the two Bonds' identity records already live:

```text
BondChainEdge {
  parties:  (PubDress, PubDress)
  level:    LevelTag
  signedAt: u64
}
```

No `content`, no `commitment`, no `signatures`. This is the same disclosure pattern `presence-journal.md` already uses for map cells ("cell membership is already disclosed by the local shade itself"): the *existence* and coarse *character* of an Interaction is not hidden, only what was actually exchanged. Its purpose is narrow and specific — see "Recovery" below — not a general-purpose social graph API.

## Minting a fact is not the same right as reading one

A client that can decrypt a Bond's local store is not thereby entitled to mint BondChain facts on that Bond's behalf. Three client postures are distinguished:

1. **decodes and needs** — an official client completing a real Interaction; the only posture that ever calls the minting port.
2. **cannot decode** — a host with no access to the local key (embedded/companion devices); it fails closed on ciphertext exactly as `presence-journal-lifecycle.md`'s "Ciphertext without its key" already requires, and is never handed plaintext or escrow to make up the difference.
3. **decodes but does not need** — a client capable of reading the local store for an unrelated reason, with no product reason to mint. Possessing the decrypt key must not imply the mint right.

The kernel is the only thing that mints. Decode capability lives in bindings/contracts; the mint right is granted only by a completed, bilateral Interaction the kernel itself mediates — never inferred from what a client happens to be able to read.

## Recovery

A Bond's only account-recovery path is BondChain. There is no recovery phrase, code, seed, or server-escrowed secret of any kind — this is a deliberate, load-bearing decision, not a gap to fill in later.

Recovery is scoped, not global:

- A recovering party's new device announces its Bond's public identity. It has no authenticated capability yet.
- The public projection (`BondChainEdge`) tells the recovering party and the protocol who its known counterparties are, since the local list of counterparties was lost along with the device.
- Each contacted counterparty independently decides whether to agree, using the `content` they themselves still hold (no shared secret is transmitted to make this decision).
- Agreement restores exactly the `content` that party shares with the recoverer — not the whole account, and not any other party's `content`.
- There is no fixed quorum or threshold. Any BondChain counterparty may be asked; what is required is the recovering party's request and that one contacted party's agreement, per relationship. Full account state accretes as more counterparties independently agree over time.
- Because recovery is scoped per relationship, one convinced or coerced counterparty can only hand back what was already theirs to know. It cannot grant control over content it was never party to.
- If no counterparty is reachable or willing, the Bond is not recoverable. This is accepted, not treated as a failure mode to patch around with a fallback secret — a fallback secret is exactly what this design forecloses.

## Deliberately unresolved

- **`level`'s role beyond the public projection.** It is at minimum a display attribute. Whether it should ever gate who may be asked, or weight how much of an account a single agreement can restore, is not decided here.
- **Witness staleness.** What an already-recorded `signatures` entry means once the signing party has since lost their own device/key is unspecified. Whether a stale signature still counts toward "one agreeing counterparty" for a future recovery is open.
- **Partial-commit Interactions.** Whether minting a fact is atomic across both kernels, and what a client does if its counterpart's kernel never completes its half, is unspecified.
- **Weighted or repeated agreement.** Whether a counterparty can be asked more than once, or whether an agreement is revocable after the fact, is open.
- **Core ownership.** As with the presence journal, whether Core should ever own this type is unresolved rather than pending. The rule below states today's arrangement only.

## Ownership rule

Until a later normative decision says otherwise, Web/client code owns BondChain fact persistence, the local mirroring transaction, and the recovery flow. Core owns no BondChain API, performs no validation of these facts, and must not expose them as protocol truth beyond what an owning interaction contract explicitly defines.

---

© 2026 aiaiaiai · aiaiaiai.org
