<!-- © 2026 aiaiaiai · aiaiaiai.org -->
<!-- SPDX-License-Identifier: MPL-2.0 -->

# Vector tile roads

Status: Core implementation, ported from Web's
`packages/map-maplibre/src/mvt-roads.ts` (`decodeTileRoads`)  
Owner: Core

The [walking graph](walk-graph.md) is built from the `roads` layer of the map
archive's tiles. Web reads that layer with its own decoder; a host with no map
renderer, such as a bot, had nothing to read it with. `nilxone_contracts::vector_tile`
reads it in Core, so every host routes on the same network from the same
bytes. A renderer such as MapLibre only draws: routing never needed it.

## What it owns, and what it does not

- It decodes one tile's Mapbox Vector Tile bytes into `RoadFeature`s: line
  features with a string `kind`, `kind_detail` (string) and `is_bridge`
  (boolean `true`) verbatim, every line of at least two points in
  `[longitude, latitude]`. Other layers are skipped undecoded.
- It builds the walking graph from several tiles at once; roads a neighbour
  repeats in its buffer and line ends a tile cut short are merged by the
  graph, as before.
- **Fetching and decompression stay with the host.** The host reads tiles
  from the same self-hosted PMTiles archive the map draws and removes gzip
  before handing them over. Core does no I/O, and has no zlib.
- It decides nothing about where a walk goes, what is fogged, or whether a
  walk is offered: that is the [Avaia drive](avaia-drive.md),
  the host, and contracts such as [Special pickup](special-pickup.md).

## Fails closed

Web's decoder returns what it read so far; Core's returns an error and no
roads, so a host never routes on a network with a silent hole in it.

| Code | When |
| --- | --- |
| `invalid_tile` | `x` or `y` outside zoom `z`, or `z` above 30 |
| `compressed` | the bytes start with the gzip magic `1f 8b` |
| `malformed` | truncated, a wrong protobuf wire type, an extent of 0, or a road point more than one tile width outside its tile |
| `too_coarse` | a graph built from a tile coarser than zoom 14 (`MIN_WALK_TILE_ZOOM`, Web's `ROAD_TILE_ZOOM`) |

The last two guard the graph's spatial index: one segment spanning hundreds
of kilometres would fill it with millions of cells.

## Same bits everywhere

Tile coordinates go to longitude and latitude through `libm`'s `atan` and
`sinh`. `tests/vector_tile_parity.rs` checks every coordinate bit for bit
against the output of Web's TypeScript decoder captured in
`tests/vector_tile/typescript_parity.json`: hand-made edge cases (buffer
outside the tile, `ClosePath`, repeated keys, numeric values, a second
`roads` layer) and seeded random networks in both hemispheres at zoom 0, 12,
14, 15 and 20, with extents 4096 and 512.

## Bindings

- **Rust:** `decode_tile_roads(bytes, TileId { z, x, y }, "roads")` and
  `build_walk_graph_from_tiles(&[RoadTile { tile, bytes }])`.
- **Wasm:** `WalkGraph.from_tiles(tiles, tile_lengths, bytes)`: `tiles` is a
  `Uint32Array` of `z, x, y` per tile, `tile_lengths` each tile's byte
  length, `bytes` the tiles one after another. It throws one of the codes
  above, or `invalid` when the counts do not add up.
- **UniFFI:** `walkGraphFromTiles(tiles: [RoadTileBytes])` throwing
  `WalkTileError`.

Either way the result is the same `WalkGraph` object as `walk_graph_build`:
snap, route and reach on it as in [walk-graph.md](walk-graph.md).

## For a bot

```text
PMTiles archive ── getZxy(14, x, y) ──▶ gzip off ──▶ WalkGraph.from_tiles
   ──▶ snap_candidates(avaia), snap_candidates(point) ──▶ route(...)
   ──▶ walk_route_available = route is not undefined   (special-pickup.md)
```

Web still uses its TypeScript decoder; switching it is a separate change.

---

© 2026 aiaiaiai · aiaiaiai.org
