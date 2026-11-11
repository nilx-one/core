// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Core's walking graph against Web's TypeScript implementation, bit for bit.
//!
//! `walk_graph/typescript_parity.json` holds inputs and outputs captured from
//! `nilx-one/web` `packages/walk-graph`: the graph built from tile features,
//! snaps, routes and reach, every float as its exact IEEE-754 bits. Fog is a
//! box no walk may enter.

use nilxone_contracts::walk_graph::{
    GraphPosition, LonLat, RoadFeature, SNAP_DISTANCE_M, WalkGraph, build_walk_graph, reach_from,
    route_on_graph, snap_to_graph,
};
use serde_json::Value;

fn float(value: &Value) -> f64 {
    let hex = value.as_str().expect("float bits are a hex string");
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex"))
}

fn point(value: &Value) -> LonLat {
    [float(&value[0]), float(&value[1])]
}

fn index(value: &Value) -> usize {
    usize::try_from(value.as_u64().expect("index")).expect("fits")
}

fn same(actual: f64, expected: f64, what: &str) {
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "{what}: {actual} != {expected}"
    );
}

fn same_point(actual: LonLat, expected: &Value, what: &str) {
    same(actual[0], float(&expected[0]), what);
    same(actual[1], float(&expected[1]), what);
}

fn fog(value: &Value) -> Option<[f64; 4]> {
    value
        .as_array()
        .map(|b| [float(&b[0]), float(&b[1]), float(&b[2]), float(&b[3])])
}

fn outside(box_: [f64; 4]) -> impl Fn(LonLat) -> bool {
    move |p: LonLat| !(p[0] >= box_[0] && p[0] <= box_[2] && p[1] >= box_[1] && p[1] <= box_[3])
}

fn features(case: &Value) -> Vec<RoadFeature> {
    case["features"]
        .as_array()
        .expect("features")
        .iter()
        .map(|f| RoadFeature {
            kind: f["kind"].as_str().expect("kind").to_owned(),
            kind_detail: f["kind_detail"].as_str().map(str::to_owned),
            is_bridge: f["is_bridge"].as_bool().unwrap_or(false),
            lines: f["lines"]
                .as_array()
                .expect("lines")
                .iter()
                .map(|line| line.as_array().expect("line").iter().map(point).collect())
                .collect(),
        })
        .collect()
}

fn check_graph(graph: &WalkGraph, expected: &Value, name: &str) {
    let nodes = expected["nodes"].as_array().expect("nodes");
    assert_eq!(graph.nodes.len(), nodes.len(), "{name}: node count");
    for (i, (actual, expected)) in graph.nodes.iter().zip(nodes).enumerate() {
        same_point(*actual, expected, &format!("{name}: node {i}"));
    }
    let edges = expected["edges"].as_array().expect("edges");
    assert_eq!(graph.edges.len(), edges.len(), "{name}: edge count");
    for (i, (actual, expected)) in graph.edges.iter().zip(edges).enumerate() {
        let what = format!("{name}: edge {i}");
        assert_eq!(
            (actual.a, actual.b),
            (index(&expected["a"]), index(&expected["b"])),
            "{what}"
        );
        assert_eq!(
            actual.surface.as_str(),
            expected["surface"].as_str().expect("surface"),
            "{what}"
        );
        assert_eq!(
            actual.bridge,
            expected["bridge"].as_bool().expect("bridge"),
            "{what}"
        );
        same(actual.length_m, float(&expected["length_m"]), &what);
    }
    let adjacency: Vec<Vec<usize>> = expected["adjacency"]
        .as_array()
        .expect("adjacency")
        .iter()
        .map(|list| list.as_array().expect("list").iter().map(index).collect())
        .collect();
    assert_eq!(graph.adjacency, adjacency, "{name}: adjacency");
}

fn check_reaches(
    graph: &WalkGraph,
    snapped: &[Option<GraphPosition>],
    case: &Value,
    name: &str,
) -> usize {
    let reaches = case["reaches"].as_array().expect("reaches");
    for (i, reach) in reaches.iter().enumerate() {
        let what = format!("{name}: reach {i}");
        let from = snapped[index(&reach["from"])].expect("placed start");
        let max = reach.get("max_cost").map_or(f64::INFINITY, float);
        let fog = fog(&reach["fog"]).map(outside);
        let actual = reach_from(
            graph,
            &from,
            max,
            fog.as_ref().map(|f| f as &dyn Fn(LonLat) -> bool),
        );
        for (field, values) in [("cost", &actual.cost), ("length_m", &actual.length_m)] {
            let expected = reach[field].as_array().expect(field);
            assert_eq!(values.len(), expected.len(), "{what}: {field}");
            for (node, (a, e)) in values.iter().zip(expected).enumerate() {
                same(*a, float(e), &format!("{what}: {field}[{node}]"));
            }
        }
    }
    reaches.len()
}

#[test]
fn the_walking_graph_matches_the_typescript_implementation() {
    let fixture: Value =
        serde_json::from_str(include_str!("walk_graph/typescript_parity.json")).expect("fixture");
    let mut checked = (0, 0, 0, 0);
    for case in fixture["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let graph = build_walk_graph(&features(case));
        check_graph(&graph, &case["graph"], name);
        checked.0 += 1;

        let mut snapped: Vec<Option<GraphPosition>> = Vec::new();
        for (i, snap) in case["snaps"].as_array().expect("snaps").iter().enumerate() {
            let what = format!("{name}: snap {i}");
            let max = snap.get("max_distance_m").map_or(SNAP_DISTANCE_M, float);
            let fog = fog(&snap["fog"]).map(outside);
            let position = snap_to_graph(
                &graph,
                point(&snap["point"]),
                max,
                fog.as_ref().map(|f| f as &dyn Fn(LonLat) -> bool),
            );
            match (&position, &snap["result"]) {
                (None, Value::Null) => {}
                (Some(actual), expected) if !expected.is_null() => {
                    assert_eq!(actual.edge, index(&expected["edge"]), "{what}: edge");
                    same(actual.t, float(&expected["t"]), &what);
                    same_point(actual.point, &expected["point"], &what);
                    same(actual.distance_m, float(&expected["distance_m"]), &what);
                }
                (actual, expected) => panic!("{what}: {actual:?} != {expected}"),
            }
            snapped.push(position);
            checked.1 += 1;
        }

        for (i, route) in case["routes"]
            .as_array()
            .expect("routes")
            .iter()
            .enumerate()
        {
            let what = format!("{name}: route {i}");
            let from = snapped[index(&route["from"])].expect("placed start");
            let to = snapped[index(&route["to"])].expect("placed end");
            let fog = fog(&route["fog"]).map(outside);
            let walk = route_on_graph(
                &graph,
                &from,
                &to,
                fog.as_ref().map(|f| f as &dyn Fn(LonLat) -> bool),
            );
            match (&walk, &route["result"]) {
                (None, Value::Null) => {}
                (Some(actual), expected) if !expected.is_null() => {
                    let nodes: Vec<usize> = expected["nodes"]
                        .as_array()
                        .expect("nodes")
                        .iter()
                        .map(index)
                        .collect();
                    assert_eq!(actual.nodes, nodes, "{what}: nodes");
                    let points = expected["points"].as_array().expect("points");
                    assert_eq!(actual.points.len(), points.len(), "{what}: points");
                    for (p, e) in actual.points.iter().zip(points) {
                        same_point(*p, e, &what);
                    }
                    same(actual.length_m, float(&expected["length_m"]), &what);
                    same(actual.cost, float(&expected["cost"]), &what);
                }
                (actual, expected) => panic!("{what}: {actual:?} != {expected}"),
            }
            checked.2 += 1;
        }

        checked.3 += check_reaches(&graph, &snapped, case, name);
    }
    assert_eq!(checked.0, 6, "every case ran");
    assert!(
        checked.1 >= 80 && checked.2 >= 100 && checked.3 >= 15,
        "{checked:?}"
    );
}
