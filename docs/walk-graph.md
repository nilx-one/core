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

## Bindings

Wasm and UniFFI expose the graph as an object that stays in the runtime's
memory: a host builds it once and snaps, routes and reaches on it without
sending it back.

- **Input.** Road features as JSON *without* coordinates
  (`{"kind","kind_detail"?,"is_bridge"?,"lines":<count>}`), the number of
  points of every line, and every point's longitude and latitude as one
  `Float64Array`. Numbers never pass through text, so no bit is lost and no
  exact float parser is needed.
- **Views.** Node coordinates, edge nodes, lengths, surface codes and bridges,
  and adjacency as offsets into one array: the same graph, as typed arrays.
- **Fog stays the host's.** Core cannot call back into the host, so routing
  and reach take a mask of the nodes a walk may enter, and snapping returns
  every candidate nearest first; the first one the host's fog lets in is the
  snapped place. `snap_candidates`, `route_on_graph_where` and
  `reach_from_where` are the same algorithms the parity test checks.

The binding adds about 107 KB to the Wasm runtime.

Web still uses its TypeScript copy; switching it is a separate change.

---

© 2026 aiaiaiai · aiaiaiai.org
