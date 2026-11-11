// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Core's walking graph for a native host (`docs/walk-graph.md`), the same
//! surface the Wasm binding offers.

use std::sync::Arc;

use nilxone_contracts::walk_graph::{self as core, GraphPosition};

/// The pedestrian graph an Avaia walks on, built once.
#[derive(uniffi::Object)]
pub struct WalkGraph {
    graph: core::WalkGraph,
}

/// A walk between two positions.
#[derive(uniffi::Record)]
pub struct WalkRoute {
    /// Its length in metres.
    pub length_m: f64,
    /// Length weighted by surface.
    pub cost: f64,
    /// Node indices passed through, in order.
    pub nodes: Vec<u32>,
    /// Longitude and latitude of every point, interleaved.
    pub points: Vec<f64>,
}

/// Cost and length from one position to every node.
#[derive(uniffi::Record)]
pub struct WalkReach {
    /// Surface-weighted cost to each node; infinite where unreached.
    pub cost: Vec<f64>,
    /// Metres walked to each node; infinite where unreached.
    pub length_m: Vec<f64>,
}

/// Builds the graph from a host's road features, as the Wasm binding does:
/// `features` JSON without coordinates, the number of points of every line,
/// and every point's longitude and latitude interleaved. `None` when the
/// counts do not add up or the JSON is malformed.
#[must_use]
#[uniffi::export]
#[allow(clippy::needless_pass_by_value, reason = "UniFFI inputs are owned")]
pub fn walk_graph_build(
    features: String,
    line_points: Vec<u32>,
    coordinates: Vec<f64>,
) -> Option<Arc<WalkGraph>> {
    core::build_walk_graph_flat(&features, &line_points, &coordinates)
        .map(|graph| Arc::new(WalkGraph { graph }))
}

fn position(edge: u32, t: f64, longitude: f64, latitude: f64) -> GraphPosition {
    GraphPosition {
        edge: edge as usize,
        t,
        point: [longitude, latitude],
        distance_m: 0.0,
    }
}

// Node and edge indices of a real graph fit `u32` and are far below 2^53.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#[uniffi::export]
impl WalkGraph {
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

    /// Every edge's surface code, an index into `footway, track, steps,
    /// street, carriageway`.
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

    /// Places within `max_distance_m`, nearest first: `edge, t, longitude,
    /// latitude, distance_m` each. The first the host's fog lets in is the
    /// snapped position.
    #[must_use]
    pub fn snap_candidates(&self, longitude: f64, latitude: f64, max_distance_m: f64) -> Vec<f64> {
        core::snap_candidates(&self.graph, [longitude, latitude], max_distance_m)
            .into_iter()
            .flat_map(|c| [c.edge as f64, c.t, c.point[0], c.point[1], c.distance_m])
            .collect()
    }

    /// The cheapest walk between two positions; `enterable` has one byte per
    /// node, non-zero where a walk may enter.
    #[must_use]
    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
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
        .map(|route| WalkRoute {
            length_m: route.length_m,
            cost: route.cost,
            nodes: route.nodes.iter().map(|&node| node as u32).collect(),
            points: route.points.iter().flat_map(|p| [p[0], p[1]]).collect(),
        })
    }

    /// Cost and length from one position to every node, bounded by
    /// `max_cost`.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
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

/// The surface a road feature is walked on, or `None` when it is not walked
/// along.
#[must_use]
#[uniffi::export]
#[allow(clippy::needless_pass_by_value, reason = "UniFFI inputs are owned")]
pub fn walk_surface_of(kind: String, kind_detail: Option<String>) -> Option<String> {
    core::surface_of(&kind, kind_detail.as_deref()).map(|surface| surface.as_str().to_owned())
}

/// Cost of one metre on each surface: footway, track, steps, street,
/// carriageway.
#[must_use]
#[uniffi::export]
pub fn walk_surface_weights() -> Vec<f64> {
    core::SURFACE_CODES.map(core::Surface::weight).to_vec()
}

/// Cost of one metre of grass, between a point and the graph.
#[must_use]
#[uniffi::export]
pub fn walk_grass_weight() -> f64 {
    core::GRASS_WEIGHT
}

/// Farthest a point may be from the graph and still be on it.
#[must_use]
#[uniffi::export]
pub fn walk_snap_distance_m() -> f64 {
    core::SNAP_DISTANCE_M
}
