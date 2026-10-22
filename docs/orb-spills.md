# Orb spills

Since v3, **every fog cell that opens for a Bond spills its own orbs**:
five around the cell's centre and five around each landmark in it. The
per-find trail below (v1, v2) stays readable for older hosts until the
hosts move to cells.

## Cell spills (v3)

| Rule | Value |
| --- | --- |
| Orbs per cell | `5 + 5 × landmarks` (`cell_orb_count`), no cap |
| Placement | orbs 0–4 within 25 m of the centre on each axis; then 5 per landmark, within 8 m of it on each axis, in the order the landmarks are given |
| Identity | `orb:cell:<key>:<index>`; `key` is 64 lowercase hex digits |
| Rarity | none: every v3 orb pays 10 (`ORB_EXPERIENCE`) |
| Lifetime | none: a cell's orbs lie until picked up |
| Reach | 15 m or closer (`ORB_PICKUP_METERS`, inclusive) |
| Between Bond and Avaia | an orb within reach of both goes to the nearer, the Bond on a tie |
| Landing | orb `i` lands `70 × (i + 1)` ms after the host first drew the spill |

The identity service answers a cell's spill when the cell opens: an opaque
`key` (its keyed digest of the Bond and the cell), the cell's `center`, and
the cell's landmarks from the published archive. It keeps nothing. Core
needs no secret and no cell geometry: it lays the orbs from that answer, the
`key` seeding the scatter, so the same answer always lays the same orbs.
`cell_orbs(key, center, &[GeoCoordinate])` is the v3 layout.

## Rated cell spills (v4)

A v4 spill names a landmark rarity **registry** version and each landmark by
its stable archive feature **id**. It never carries a rarity: Core reads it.

| Rule | Value |
| --- | --- |
| Landmark rarity | `landmark_rarity(registry, id)`: Core's own versioned registry; a landmark it does not name is `common`; an unpublished version is refused |
| Orb rarity | the cell's own 5 are `common`; each landmark's 5 roll at least rarity `r` with chance `1 / multiplier(r)` (`orb_rarity_roll`), never rarer than the landmark |
| Orb experience | `orb_experience(rarity)`: 10, 20, 40, 80 or 160 |
| Identity | `orb:cell:<key>:<registry>:<landmark id, or ~ for the cell's own>:<n>`, `n` 0–4 |
| Order | landmarks are taken in id order, each id once; an orb's place and rarity depend only on its key, registry, landmark and `n` |

Rarity names and multipliers are `RarityV3` ([five-tier rarity](rarity-v3.md)).
The orb **chance** and orb **experience** above are orb rules of their own
([0x1 Experience Orb rarity](https://github.com/nilx-one/0x1/blob/master/documents/product-direction/rarity-coefficient.md)):
they are not find encounter rates and not `base_find_xp`, and neither table
prices the other. Orbs are not gated by a subscription.

A rated orb's id names everything its rarity depends on, and a published
registry version never changes (a revision is a new version), so
`rated_orb_award(id)` gives the same rarity and experience for the same id on
every retry and every runtime. The identity service prices a pick-up with it
after checking what Core cannot: that the `key` is its own keyed digest for
that Bond and cell, and that the named landmark lies in that cell in the
archive it answered from. Until the service does both, a rated orb is
presentation only, and its rarity is not an award.

The host passes the answer as one entry of `cells` in the `orb_world` input:

```json
{
  "spills": [],
  "cells": [
    {
      "key": "5f1c0a7e9b2d4c6e8a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f",
      "center": { "longitude_e7": "304461000", "latitude_e7": "504650000" },
      "registry": 1,
      "rated_landmarks": [
        {
          "id": "n1",
          "at": { "longitude_e7": "304470000", "latitude_e7": "504665000" }
        }
      ],
      "appeared_at": "1000"
    }
  ],
  "picked": [],
  "bond": null,
  "avaia": null
}
```

A landmark carrying a `rarity`, or any other unknown field, makes the input
invalid. A spill that gives both v3 `landmarks` and a `registry`, or
`rated_landmarks` without one, lies nowhere, as does a bad key or registry.
Rated orbs are drawn with `kind` `orb` and their `rarity` code; v3 cell orbs
and a find's orbs carry no `rarity`, so their output is unchanged. A cell has
no `goal` and no expiry, so it never moves `next_expiry`. Registry version 1
names no landmark yet: its curation is a later revision.

## Find spills (v1, v2)

When a fog cell opens, each find in it spills a short trail of **orbs** that
leads to the find, optionally visiting a real landmark in that same cell on
the way. The optional landmark is a navigation waypoint, not a collectible,
not an artifact placement, and not proof that either Bond visited it.
Core owns every rule about them; a host only draws what
Core answers, reports where its Bond and Avaia are, and asks its claiming
service who was first.

| Rule | Value |
| --- | --- |
| Orbs per find | 5 to 30, `orb_count(artifact_id)`, FNV-1a of the id |
| Clumps | 3 to 6 orbs each, evenly from the trail's start to 88% of the way to the find |
| Scatter | up to 3 m from the clump's middle on each axis |
| Shortest trail | 30 m: a start closer to the first destination (waypoint or find) is moved back along the same line |
| Optional landmark `via` (v2) | First 75% of the route heads to the waypoint, then the route bends toward the unchanged find goal |
| Lifetime | 30 minutes from the spill (`ORB_LIFETIME_MS`) |
| Reach | strictly closer than 15 m (`ORB_PICKUP_METERS`), by `distance_meters` |
| Pays | 10 experience (`ORB_EXPERIENCE`) to whoever picks it up, Avaia or Bond |
| Landing | orb `i` lands `70 × (i + 1)` ms after the host first drew the spill; the find's ring after the last |

Everything is integer arithmetic over E7 coordinates, so every runtime lays
the same orb in the same place **for the same full wire input**. A host must
select waypoints deterministically from the same published landmark source;
when no eligible landmark is known, the straight path remains correct. No
new artifact or orb identity is minted for the waypoint.

## `orb_world(world, now_ms)`

Core keeps no state. The host hands in what it knows, as JSON (decimal
strings for times and coordinates):

```json
{
  "spills": [
    {
      "artifact_id": "art:seg:312346:298243:e2908:1:0",
      "from": { "longitude_e7": "304461000", "latitude_e7": "504650000" },
      "via": { "longitude_e7": "304470000", "latitude_e7": "504665000" },
      "to": { "longitude_e7": "304469000", "latitude_e7": "504655000" },
      "appeared_at": "1000",
      "expires_at": "1801000",
      "count": 20,
      "taken": [1]
    }
  ],
  "picked": ["orb:art:seg:312346:298243:e2908:1:0:2"],
  "bond": null,
  "avaia": { "longitude_e7": "304469000", "latitude_e7": "504655000" }
}
```

- `from` is where the trail starts (the middle of the cell the find lies in),
  `via` is an optional verified-local-landmark coordinate, and `to` always
  remains where the actual find lies. Omitting `via` retains the exact v1
  geometry. A legacy Core rejecting unknown fields must receive no `via`
  until its v2 runtime is deployed. `count` and `taken` remain the
  claiming service's answer; `picked` records what the device already picked
  up. Neither a waypoint nor a new Core runtime creates a new orb identity.
- `bond` is the Bond's own observation, given only while it may pick up; a
  declared point is never given. `avaia` is the Avaia's body while it walks.

Core answers `{"ok":true,"view":{"orbs":[…],"bond_reach":[…],"avaia_reach":[…],"next_expiry":"…"}}`:
each drawn orb (`kind` `orb`, or `goal` for the find) with its point and
`lands_at`, the orb ids each of them reaches now, nearest first, and when the
soonest live spill is gone. A spill that is gone, an orb taken or picked, and
a spill whose `count` is not its find's are not drawn.

## Runtime compatibility and rollout

- **Core v1:** omitted `via` is mandatory. Legacy orb positions, IDs, counts,
  stagger, reach, and the unchanged Artifact goal must remain identical.
- **Core v2:** optional `via` guides the geometry without changing identity,
  reward, or goal.
- **Core v3:** optional `cells` carries per-cell spills. Find spills, their
  reach (strictly closer than 15 m) and their ids are unchanged.
- **Core v4:** `ORB_SPILL_VERSION = 4`; a cell spill may name a `registry`
  with `rated_landmarks`, and each rated orb reports its rarity. v3 spills,
  their ids and placement are unchanged, and so is `cell_orbs`.
- Both `orb_spill_version()` and `orb_world(...)` are exported through
  **Wasm and UniFFI**. The latter uses canonical decimal-string `now_ms` and
  the same JSON success/error envelope.
- **Web and iOS:** discover the *actually loaded* Core runtime version before
  serializing `via` (2 or more), `cells` (3 or more) or a rated spill
  (4 or more). A newer Web bundle
  running older Core must omit them: older runtimes reject unknown fields.
  No deployment status is inferred from merging a Core PR, and a newer Core
  library is not proof that all installed hosts have upgraded.
- **Evidence:** compare the same v2 and v3 fixtures through native Core,
  Wasm, and UniFFI, and verify that absent versus explicitly null `via` preserves the
  legacy output. The host still obtains real same-cell waypoints from verified
  map archives; Core does not invent landmarks or authenticate their source.

## Claims

A cell's orbs are the Bond's own: nobody else sees them, and no service
claims them first-wins. Between the Bond and its Avaia, the device picks up
what Core reports in each one's reach, and records it in `picked`.

For a find's orbs, who picked an orb up first is not Core's to decide: the host's service claims
it, first come first served, and answers a later pick-up as taken ("crap!").
An orb is a pick-up intent and presentation, never a `BondChain` interaction,
a Relationship, or proof of presence.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
