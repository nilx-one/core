# Spoken line

Status: registered ahead of the canonical specification. A spoken line is not yet specified in [`nilx-one/0x1`](https://github.com/nilx-one/0x1); this document and `nilxone-contracts::SpokenLine` exist so the first product surface has one shared, deterministic shape instead of several private ones. When 0x1 specifies an owning contract, that contract wins and this shape is reconciled to it.

## What it is

A spoken line is one short text a Bond says aloud on the map. Clients present it over the speaker's avatar for the Bonds within earshot.

```text
SpokenLine {
  id:         SpokenLineId   // "line_" + 64 lowercase hex; one utterance, one id
  speaker:    PubDress
  text:       SpokenText     // 1..=280 scalars, NFC, no controls except LF, no surrounding whitespace
  spoken_at:  DecimalU64     // Unix epoch seconds
}
```

The id is derived by the producer from the utterance's origin (for example `sha256(source_id "\n" external_id)`), so a re-delivered utterance is recognisably the same line. Core does not hash and does not choose the derivation.

## What it is not

- **Not an Interaction.** A line has no counterpart action, no reciprocity, and no consent. It cannot complete a `BondChain` entry.
- **Not Relationship state.** Hearing a line establishes no acquaintance, proximity fact, or edge between speaker and listener.
- **Not presence evidence.** The line carries no coordinate and proves nothing about where anyone was.
- **Not a channel.** Who is allowed to speak, and where the text comes from, are product and delivery policy owned outside Core.

## Earshot

`within_earshot(speaker, listener, radius)` answers whether two coordinates are at most `radius` meters apart. `distance_meters` is an equirectangular approximation in integer arithmetic (E7 degrees, a whole-degree cosine table with linear interpolation, `u128::isqrt`), so Rust, WebAssembly, and FFI callers agree on every input.

- The bound is inclusive and symmetric.
- Longitude wraps across the antimeridian.
- It is specified for distances up to `EARSHOT_MAX_METERS` (5 000 m) away from the poles; `EarshotRadius` refuses larger radii. It is not a geodesic and must not be used for navigation.
- Core neither stores nor fetches locations. The caller supplies the two coordinates it already legitimately holds, for example two `BondLocation` values.

## Deliberately unresolved

- **Canonical ownership.** Whether 0x1 adopts this shape, replaces it, or folds it into a broadcast class.
- **Radius policy.** The default reach is a product decision made by the host service; Core only bounds it.
- **Expiry.** How long a line stays audible is presentation policy. Core does not define a TTL.
- **Speaker authorisation.** Core does not know who may speak.

---

© 2026 aiaiaiai · aiaiaiai.org
