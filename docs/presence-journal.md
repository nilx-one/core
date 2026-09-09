# Presence journal

Status: registered client shape; not owned by 0x1 Core. No Rust implementation.

The presence journal is a personal, device-local, append-only record derived from a client's own location observations. It is never synced and is not protocol evidence. This document exists only so a future Core adoption does not silently invent a different shape.

## Canonical client shape

```text
PRESENCE_RESOLUTION = 9

VisitRecord {
  cell:          CellIndex     // opaque H3 index, string
  enteredAt:     u64           // Unix epoch milliseconds
  leftAt:        u64 | null    // null = dwell open at write time
  source:        "self"        // "avaia" is reserved for a later local phase
  fixCount:      u32
  bestAccuracyM: f64
}
```

`PRESENCE_RESOLUTION` is registered here because it changes the meaning of every stored `cell`. It remains a client-side product constant until Core explicitly adopts this journal.

## Semantics

- **Append-only.** A qualifying dwell may be represented by an opening record (`leftAt: null`) and, when it ends, a second closing record. Readers fold records with the same `(cell, enteredAt)` into one dwell view while the underlying journal remains immutable.
- **Derived from observations, not an observation store.** Raw host geolocation observations remain ephemeral. The journal persists only the minimal derived cell/dwell facts needed by the local shade feature.
- **Local.** Records stay on the device. They are not synchronized, transmitted to a service, analytics pipeline, error collector, or another Bond.
- **Not BondChain.** A visit is one client's local observation about itself. It contains no counterpart action, no reciprocity, no consent, and therefore cannot complete an Interaction or produce a BondChain fact.
- **Not Relationship state.** A collection of visited cells cannot establish proximity, acquaintance, consent, or any other Relationship interpretation.
- **Renderer boundary.** Rendering consumes only the set of lit cell identifiers. Reading journal contents remains a separate explicit path, such as activating a lit cell.
- **Storage visibility.** The cell identifier may remain plaintext when the client stores encrypted record metadata, because cell membership is already disclosed by the local shade itself. Encrypted fields should bind the plaintext cell as authenticated additional data.

## Deliberately unresolved

- **Key binding.** A device-local encryption key has no defined relation to `pk_identity`. Rotation, REKEY behavior, export, recovery, and device transfer are unspecified; the Phase 1 journal does not survive a device change by contract.
- **Region growth.** A renderer may initially use a fixed lightmap region. Travel outside that region needs a separate tiling or region-growth design and does not change this journal shape.
- **Avaia records.** `source: "avaia"` is reserved only so a later local implementation can remain schema-compatible. Its semantics are not defined here, and it must never be interpreted as bilateral evidence or BondChain activity.
- **Training-signal egress.** No journal aggregation or training-data route is authorized by this shape. Any future egress requires a separate explicit governance and data-flow contract.

## Ownership rule

Until a later normative decision says otherwise, Web/client code owns persistence and lifecycle. Core owns no journal API, performs no validation of these records, and must not expose them as protocol truth.

---

© 2026 aiaiaiai · aiaiaiai.org
