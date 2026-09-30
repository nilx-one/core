# Spoken line

Status: implementation of an owned contract. The spoken line and earshot are normatively owned by [`nilx-one/0x1`](https://github.com/nilx-one/0x1) — `documents/12-spoken-lines-and-earshot.md`. That document wins wherever this one or the code disagrees with it; this file records only what this crate implements and where the Core boundary sits.

## What this crate implements

`nilxone-contracts` exports the parts 0x1 assigns to Core:

- `SpokenLine` with its canonical JSON encoding (`id`, `speaker`, `text`, `spoken_at`; unknown members rejected).
- `SpokenLineId`: `line_` + 64 lowercase hex, validated as an opaque identifier.
- `SpokenText`: the 0x1 text rules, validated and never repaired.
- `EarshotRadius` (`1..=EARSHOT_MAX_METERS`), `distance_meters`, and `within_earshot`: the 0x1 integer earshot rule, exactly as written there.

## What it deliberately does not do

- **It does not derive `id`.** 0x1 fixes that one utterance has one opaque id and leaves the derivation open. Core validates the shape and never hashes, parses, or reconstructs an id.
- **It does not store or fetch locations.** The caller supplies two coordinates it already legitimately holds, for example two `BondLocation` values.
- **It does not decide who may speak, how long a line is audible, how fresh a location must be, or the default radius.** Those belong to the host service under the bounds 0x1 sets.

---

© 2026 aiaiaiai · aiaiaiai.org
