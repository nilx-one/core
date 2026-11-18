// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Core's vector tile road decoder against Web's TypeScript one, bit for bit.
//!
//! `vector_tile/typescript_parity.json` holds tiles and the roads that
//! `nilx-one/web` `packages/map-maplibre/src/mvt-roads.ts` read from them,
//! every coordinate as its exact IEEE-754 bits: hand-made edge cases (buffer
//! outside the tile, `ClosePath`, repeated keys, numeric values, a second
//! `roads` layer) and seeded random networks in both hemispheres, at zoom 0,
//! 12, 14, 15 and 20, with extents 4096 and 512.

use nilxone_contracts::vector_tile::{
    MIN_WALK_TILE_ZOOM, ROAD_SOURCE_LAYER, RoadTile, TileId, build_walk_graph_from_tiles,
    decode_tile_roads,
};
use nilxone_contracts::walk_graph::build_walk_graph;
use serde_json::Value;

fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex byte"))
        .collect()
}

fn float(value: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(value.as_str().expect("bits"), 16).expect("hex"))
}

fn tile(case: &Value) -> TileId {
    let n = |name: &str| u32::try_from(case["tile"][name].as_u64().expect(name)).expect("u32");
    TileId {
        z: u8::try_from(n("z")).expect("zoom"),
        x: n("x"),
        y: n("y"),
    }
}

fn cases() -> Vec<Value> {
    let json = include_str!("vector_tile/typescript_parity.json");
    let parsed: Value = serde_json::from_str(json).expect("fixture");
    parsed["cases"].as_array().expect("cases").clone()
}

#[test]
fn roads_match_the_typescript_decoder_bit_for_bit() {
    for case in cases() {
        let name = case["name"].as_str().expect("name");
        let roads = decode_tile_roads(
            &bytes(case["bytes"].as_str().expect("bytes")),
            tile(&case),
            ROAD_SOURCE_LAYER,
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        let expected = case["roads"].as_array().expect("roads");
        assert_eq!(roads.len(), expected.len(), "{name}: road count");
        for (at, (road, want)) in roads.iter().zip(expected).enumerate() {
            let what = format!("{name} road {at}");
            assert_eq!(road.kind, want["kind"].as_str().expect("kind"), "{what}");
            assert_eq!(
                road.kind_detail.as_deref(),
                want["kind_detail"].as_str(),
                "{what}"
            );
            assert_eq!(
                road.is_bridge,
                want["is_bridge"].as_bool().expect("bool"),
                "{what}"
            );
            let lines = want["lines"].as_array().expect("lines");
            assert_eq!(road.lines.len(), lines.len(), "{what}: lines");
            for (line, want_line) in road.lines.iter().zip(lines) {
                let points = want_line.as_array().expect("points");
                assert_eq!(line.len(), points.len(), "{what}: points");
                for (point, want_point) in line.iter().zip(points) {
                    for axis in 0..2 {
                        assert_eq!(
                            point[axis].to_bits(),
                            float(&want_point[axis]).to_bits(),
                            "{what}: {} != {}",
                            point[axis],
                            float(&want_point[axis])
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn tiles_build_the_same_graph_as_their_decoded_roads() {
    let cases = cases();
    let tiles: Vec<(TileId, Vec<u8>)> = cases
        .iter()
        .map(|case| (tile(case), bytes(case["bytes"].as_str().expect("bytes"))))
        .filter(|(tile, _)| tile.z >= MIN_WALK_TILE_ZOOM)
        .collect();
    assert_eq!(tiles.len(), 4);
    let borrowed: Vec<RoadTile<'_>> = tiles
        .iter()
        .map(|(tile, bytes)| RoadTile { tile: *tile, bytes })
        .collect();
    let from_tiles = build_walk_graph_from_tiles(&borrowed).expect("graph");
    let features: Vec<_> = tiles
        .iter()
        .flat_map(|(tile, bytes)| {
            decode_tile_roads(bytes, *tile, ROAD_SOURCE_LAYER).expect("roads")
        })
        .collect();
    let direct = build_walk_graph(&features);
    assert_eq!(from_tiles.node_coordinates(), direct.node_coordinates());
    assert_eq!(from_tiles.edge_nodes(), direct.edge_nodes());
    assert!(!direct.edge_nodes().is_empty());
}
