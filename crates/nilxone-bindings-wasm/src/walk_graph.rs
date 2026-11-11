// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Core's walking graph for a browser host (`docs/walk-graph.md`).
//!
//! The graph is built once and stays in Wasm memory; a host routes on it as
//! often as it likes without sending it back. Numbers cross as typed arrays,
//! so every bit survives. Fog is the host's: it passes a mask of the nodes a
//! walk may enter, and takes the first snap candidate its fog lets in.

use nilxone_contracts::walk_graph::{self as core, GraphPosition};
use wasm_bindgen::prelude::wasm_bindgen;

/// The pedestrian graph an Avaia walks on.
#[wasm_bindgen]
pub struct WalkGraph {
    graph: core::WalkGraph,
}

fn position(edge: u32, t: f64, longitude: f64, latitude: f64) -> GraphPosition {
    GraphPosition {
        edge: edge as usize,
        t,
        point: [longitude, latitude],
        distance_m: 0.0,
    }
}

#[wasm_bindgen]
impl WalkGraph {
    /// Builds the graph from a host's road features: `features` is a JSON
    /// array of `{"kind","kind_detail"?,"is_bridge"?,"lines":<count>}`,
    /// `line_points` the number of points of every line, `coordinates`
    /// every point's longitude and latitude interleaved. Numbers never pass
    /// through text, so no bit is lost. `undefined` when the counts do not add
    /// up or the JSON is malformed.
    #[must_use]
    pub fn build(features: &str, line_points: &[u32], coordinates: &[f64]) -> Option<WalkGraph> {
        core::build_walk_graph_flat(features, line_points, coordinates)
            .map(|graph| WalkGraph { graph })
    }

    /// The surface names, in the order of [`Self::edge_surfaces`]' codes.
    #[must_use]
    pub fn surface_names() -> String {
        core::SURFACE_CODES.map(core::Surface::as_str).join(",")
    }

    /// Longitude and latitude of every node, interleaved.
    #[must_use]
    pub fn node_coordinates(&self) -> Vec<f64> {
        self.graph.node_coordinates()
    }

    /// The two node indices of every edge, interleaved.
    #[must_use]
    pub fn edge_nodes(&self) -> Vec<u32> {
        self.graph.edge_nodes()
    }

    /// The length of every edge, in metres.
    #[must_use]
    pub fn edge_lengths(&self) -> Vec<f64> {
        self.graph.edge_lengths()
    }

    /// Every edge's surface code (see [`Self::surface_names`]).
    #[must_use]
    pub fn edge_surfaces(&self) -> Vec<u8> {
        self.graph.edge_surface_codes()
    }

    /// 1 for every bridge edge, 0 otherwise.
    #[must_use]
    pub fn edge_bridges(&self) -> Vec<u8> {
        self.graph.edge_bridges()
    }

    /// Node `i`'s edges are `adjacency_edges[offsets[i]..offsets[i + 1]]`.
    #[must_use]
    pub fn adjacency_offsets(&self) -> Vec<u32> {
        self.graph.adjacency_offsets()
    }

    /// Every node's edge indices, ascending, one node after another.
    #[must_use]
    pub fn adjacency_edges(&self) -> Vec<u32> {
        self.graph.adjacency_edges()
    }

    /// Places on the graph within `max_distance_m` of a point, nearest first:
    /// five numbers each, `edge, t, longitude, latitude, distance_m`. The
    /// first the host's fog lets in is the snapped position.
    #[must_use]
    // Edge indices of a real graph are far below 2^53.
    #[allow(clippy::cast_precision_loss)]
    pub fn snap_candidates(&self, longitude: f64, latitude: f64, max_distance_m: f64) -> Vec<f64> {
        core::snap_candidates(&self.graph, [longitude, latitude], max_distance_m)
            .into_iter()
            .flat_map(|c| [c.edge as f64, c.t, c.point[0], c.point[1], c.distance_m])
            .collect()
    }

    /// The cheapest walk between two positions, or `undefined`. `enterable`
    /// has one byte per node, non-zero where a walk may enter; without it
    /// every node may be entered.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen takes an optional typed array by value"
    )]
    pub fn route(
        &self,
        from_edge: u32,
        from_t: f64,
        from_longitude: f64,
        from_latitude: f64,
        to_edge: u32,
        to_t: f64,
        to_longitude: f64,
        to_latitude: f64,
        enterable: Option<Vec<u8>>,
    ) -> Option<WalkRoute> {
        core::route_on_graph_where(
            &self.graph,
            &position(from_edge, from_t, from_longitude, from_latitude),
            &position(to_edge, to_t, to_longitude, to_latitude),
            &core::mask_enterable(enterable.as_deref()),
        )
        .map(|route| WalkRoute { route })
    }

    /// Cost and length from one position to every node, bounded by
    /// `max_cost` (`Infinity` for none). `enterable` as for [`Self::route`].
    #[must_use]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen takes an optional typed array by value"
    )]
    pub fn reach(
        &self,
        from_edge: u32,
        from_t: f64,
        max_cost: f64,
        enterable: Option<Vec<u8>>,
    ) -> WalkReach {
        let reach = core::reach_from_where(
            &self.graph,
            &position(from_edge, from_t, 0.0, 0.0),
            max_cost,
            &core::mask_enterable(enterable.as_deref()),
        );
        WalkReach {
            cost: reach.cost,
            length_m: reach.length_m,
        }
    }
}

/// A walk between two positions.
#[wasm_bindgen]
pub struct WalkRoute {
    route: core::WalkRoute,
}

#[wasm_bindgen]
impl WalkRoute {
    /// Its length in metres.
    #[must_use]
    pub fn length_m(&self) -> f64 {
        self.route.length_m
    }

    /// Length weighted by surface: what the route minimises.
    #[must_use]
    pub fn cost(&self) -> f64 {
        self.route.cost
    }

    /// Node indices passed through, in order.
    #[must_use]
    // Node indices of a real graph fit `u32`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn nodes(&self) -> Vec<u32> {
        self.route.nodes.iter().map(|&node| node as u32).collect()
    }

    /// Longitude and latitude of every point, start to end, interleaved.
    #[must_use]
    pub fn points(&self) -> Vec<f64> {
        self.route
            .points
            .iter()
            .flat_map(|p| [p[0], p[1]])
            .collect()
    }
}

/// Cost and length from one position to every node.
#[wasm_bindgen]
pub struct WalkReach {
    cost: Vec<f64>,
    length_m: Vec<f64>,
}

#[wasm_bindgen]
impl WalkReach {
    /// Surface-weighted cost to each node; `Infinity` where unreached.
    #[must_use]
    pub fn cost(&self) -> Vec<f64> {
        self.cost.clone()
    }

    /// Metres walked to each node; `Infinity` where unreached.
    #[must_use]
    pub fn length_m(&self) -> Vec<f64> {
        self.length_m.clone()
    }
}
