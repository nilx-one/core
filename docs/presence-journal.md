<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Presence journal shape

Status: registered, not adopted  
Owner: none in Core  
Implementation: `nilx-one/web`, Shade / Presence Phase 1  
Peer class: `bond.journal`

The presence journal is a personal, local, append-only record of which H3 cells a person has physically been in. Web consumes it to darken never-visited ground in the Shade shader.

Core does not own this type. This note registers the shape that already exists in TypeScript so that a later adoption starts from the decided form rather than reinventing it. Nothing in Core validates a `VisitRecord` today, and no kernel transition, contract, or binding refers to one.

## Shape

```ts
type CellIndex = string;            // opaque H3 index
const PRESENCE_RESOLUTION = 9;      // H3 resolution; ~174m edge length

interface VisitRecord {
  readonly cell: CellIndex;
  readonly enteredAt: number;       // ms epoch
  readonly leftAt: number | null;   // null while the visit is open
  readonly source: 'self';          // a second value arrives in a later phase
  readonly fixCount: number;        // accepted position fixes during the dwell
  readonly bestAccuracyM: number;   // best reported accuracy, metres
}
```

`CellIndex` is opaque. Core must not parse, compare, or re-derive geography from it.

## Decided

The journal is personal, local, append-only, and never synced. There is no cross-device claim, no counterparty, and nothing to reconcile.

`source` is a closed vocabulary with exactly one value in this phase. A second value is anticipated but not yet defined. Adding one is a change to this note.

`leftAt: null` means the visit is open, not that the departure time is unknown. An open visit is the only visit still accepting fixes.

Records are append-only. A visit is closed by writing `leftAt`, never by deleting or rewriting history.

## Deliberately open

These are unresolved, not settled by silence.

**Key binding.** Web encrypts records with a device-local AES-GCM key that has no relation to `pk_identity`. Rotation, REKEY behaviour, and what happens on a device flip are all undefined. Journals do not survive a device change.

**Ownership.** Whether Core should ever own this type at all is open. A value that is never synced and carries no cross-device claim gives the kernel nothing to enforce, and registration here is not an argument that it should.
