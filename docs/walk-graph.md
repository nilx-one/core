<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Walking graph

Status: Core implementation, moved from Web's `packages/walk-graph` under
[Core as the engine and the API](core-api-boundary.md). Web still uses its
TypeScript copy until it switches to Core's.

`nilxone_contracts::walk_graph` answers one question for every client: how
does an Avaia walk from here to there. It is pure and deterministic. A host
reads the `roads` features of the map tiles it loaded, builds the graph,
places points on it and routes between them. Fetching tiles, drawing the walk
and deciding where to go (the [Avaia drive](avaia-drive.md)) stay with the
host and the drive. The drive still never sees a coordinate.

## What it does

- **Builds the graph** from `RoadFeature`s: each feature's `kind` and
  `kind_detail` give a `Surface`, or none for what is not walked along
  (highways, rail, ferries, anything unknown). Coordinates are snapped to
  1e-7 degrees. Overlapping copies keep the cheapest surface. Line ends that a
  tile cut a hair short are joined back within 1 m. The same features in any
  order give the same graph, numbering included.
- **Snaps** a point to the nearest enterable place on the graph within 30 m
  by default.
- **Routes** between two positions: Dijkstra over metres weighted by surface,
  ties broken by node index. Nodes the caller rules out (fog) are never
  entered.
- **Reaches** every node from one position at once, optionally bounded by
  cost.

Surface weights per metre: footway 1.0, track 1.3, steps 2.0, street 1.5,
carriageway 3.0; grass, which a caller draws between a point and the graph,
4.0.

## Same bits everywhere

Clients on different platforms must walk the same way. Trigonometry goes
through the `libm` crate, not the platform's `sin` and `cos`, which differ in
the last place between Linux, iOS and Wasm. Rounding follows JavaScript's
`Math.round` (halves toward positive infinity), as the implementation it
replaces did.

`tests/walk_graph_parity.rs` checks the result bit for bit against outputs
captured from Web's TypeScript implementation in
`tests/walk_graph/typescript_parity.json`: six sets of roads (grids in three
hemispheres with clipped tiles, duplicates, bridges and broken lines, two
random networks and an empty one), their graphs, snaps, routes and reach,
with and without fog.

## Not here yet

No binding exports it yet; a host calls it through Wasm or UniFFI once the
bindings do. Web keeps its TypeScript copy until it switches.

---

© 2026 aiaiaiai · aiaiaiai.org
