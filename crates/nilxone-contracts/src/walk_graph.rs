// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The pedestrian graph an Avaia walks on, and the router over it.
//!
//! Pure and deterministic: the graph is built from `roads` features a host
//! already read out of its map tiles, and every function gives the same
//! answer for the same input on every client. Fetching tiles, drawing the
//! walk and deciding where to go (the [Avaia drive](../../docs/avaia-drive.md))
//! stay elsewhere; this module only answers "how do I walk from here to
//! there".
//!
//! It is a port of Web's `packages/walk-graph`, bit for bit:
//! `tests/walk_graph_parity.rs` checks it against outputs captured from that
//! implementation. Trigonometry goes through `libm` so a native, an iOS and a
//! Wasm client compute the same bits; the platform's own `sin` and `cos`
//! differ between them in the last place.

// Exact float comparisons are the point here: the router breaks ties the
// same way on every client, so `==` on costs and distances is deliberate.
#![allow(clippy::float_cmp)]

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// A geographic point, longitude first, as tiles and `GeoJSON` carry it.
pub type LonLat = [f64; 2];

const EARTH_RADIUS_M: f64 = 6_371_008.8;
const RAD: f64 = std::f64::consts::PI / 180.0;

/// Coordinates are snapped to 1e-7 degrees (about 1 cm) to name a node.
pub const COORDINATE_PRECISION: f64 = 1e7;

/// How far a loose line end may be from another line and still be joined to
/// it. Covers tile-buffer clipping, which cuts a line at a point the
/// neighbouring tile does not have.
pub const STITCH_TOLERANCE_M: f64 = 1.0;

/// Farthest a point may be from the graph and still be on it.
pub const SNAP_DISTANCE_M: f64 = 30.0;

/// Cost of one metre of grass. Not a graph edge: grass connectors are drawn
/// by the router's caller between a point and the graph.
pub const GRASS_WEIGHT: f64 = 4.0;

const GRID_CELL_DEGREES: f64 = 0.0005;

/// What a walking edge is made of, as far as the map archive can tell.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// Footways, paths, pedestrian streets, corridors.
    Footway,
    /// Tracks, cycleways, bridleways.
    Track,
    /// Steps.
    Steps,
    /// Minor and medium roads: walked as a street with a sidewalk.
    Street,
    /// Major roads: walked as a carriageway.
    Carriageway,
}

impl Surface {
    /// Cost of one metre on this surface.
    #[must_use]
    pub const fn weight(self) -> f64 {
        match self {
            Self::Footway => 1.0,
            Self::Track => 1.3,
            Self::Steps => 2.0,
            Self::Street => 1.5,
            Self::Carriageway => 3.0,
        }
    }

    /// The name, which also orders surfaces of equal weight.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Footway => "footway",
            Self::Track => "track",
            Self::Steps => "steps",
            Self::Street => "street",
            Self::Carriageway => "carriageway",
        }
    }
}

/// Classifies one `roads` feature. `None` means it is not walked along:
/// highways, rail, ferries, and anything the table does not know. An unknown
/// `kind_detail` falls back to `kind`; an unknown `kind` is never walkable.
#[must_use]
pub fn surface_of(kind: &str, kind_detail: Option<&str>) -> Option<Surface> {
    let by_kind = match kind {
        "path" => Surface::Footway,
        "minor_road" | "medium_road" => Surface::Street,
        "major_road" => Surface::Carriageway,
        _ => return None,
    };
    let Some(detail) = kind_detail else {
        return Some(by_kind);
    };
    match detail {
        "footway" | "path" | "pedestrian" | "corridor" => Some(Surface::Footway),
        "track" | "cycleway" | "bridleway" => Some(Surface::Track),
        "steps" => Some(Surface::Steps),
        "living_street" | "residential" | "service" | "unclassified" | "road" | "tertiary"
        | "tertiary_link" => Some(Surface::Street),
        "secondary" | "secondary_link" | "primary" | "primary_link" => Some(Surface::Carriageway),
        "trunk" | "trunk_link" | "motorway" | "motorway_link" | "construction" | "proposed"
        | "raceway" | "bus_guideway" | "busway" | "platform" => None,
        _ => Some(by_kind),
    }
}

/// Great-circle distance in metres.
#[must_use]
pub fn distance_m(a: LonLat, b: LonLat) -> f64 {
    let d_lat = (b[1] - a[1]) * RAD;
    let d_lon = (b[0] - a[0]) * RAD;
    let h = libm::pow(libm::sin(d_lat / 2.0), 2.0)
        + libm::cos(a[1] * RAD) * libm::cos(b[1] * RAD) * libm::pow(libm::sin(d_lon / 2.0), 2.0);
    2.0 * EARTH_RADIUS_M * libm::asin(js_min(1.0, libm::sqrt(h)))
}

/// Where a point falls on a segment: `t` in `[0, 1]` from `a` to `b`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentProjection {
    /// 0 at `a`, 1 at `b`.
    pub t: f64,
    /// The nearest point.
    pub point: LonLat,
    /// From the original point to `point`.
    pub distance_m: f64,
}

/// Nearest point on segment `a`–`b` to `p`, projected on a local
/// equirectangular plane, which is exact enough at walking scale.
#[must_use]
// The names follow the plane geometry they compute.
#[allow(clippy::many_single_char_names)]
pub fn project_on_segment(p: LonLat, a: LonLat, b: LonLat) -> SegmentProjection {
    let k = libm::cos(a[1] * RAD);
    let bx = (b[0] - a[0]) * k;
    let by = b[1] - a[1];
    let px = (p[0] - a[0]) * k;
    let py = p[1] - a[1];
    let length_sq = bx * bx + by * by;
    let raw = if length_sq == 0.0 {
        0.0
    } else {
        (px * bx + py * by) / length_sq
    };
    let t = js_min(1.0, js_max(0.0, raw));
    let point = lerp(a, b, t);
    SegmentProjection {
        t,
        point,
        distance_m: distance_m(p, point),
    }
}

fn lerp(a: LonLat, b: LonLat, t: f64) -> LonLat {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// `Math.min`: NaN wins, and `-0` is below `+0`.
fn js_min(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN
    } else if x < y || (x == y && x.is_sign_negative()) {
        x
    } else {
        y
    }
}

/// `Math.max`: NaN wins, and `+0` is above `-0`.
fn js_max(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN
    } else if x > y || (x == y && x.is_sign_positive()) {
        x
    } else {
        y
    }
}

/// `Math.round`: the nearest integer, halves toward positive infinity.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// One `roads` feature as a host reads it from a loaded tile. `lines` holds
/// every line of a `LineString` or `MultiLineString`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoadFeature {
    /// The `kind` of the tile's `roads` layer.
    pub kind: String,
    /// The raw OSM highway value, when the tile carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_detail: Option<String>,
    /// Crosses over something, typically water.
    #[serde(default)]
    pub is_bridge: bool,
    /// The feature's lines, as points.
    pub lines: Vec<Vec<LonLat>>,
}

/// One walkable edge. Edges are walkable both ways.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalkEdge {
    /// Node index, below `b`.
    pub a: usize,
    /// Node index, above `a`.
    pub b: usize,
    /// Its length.
    pub length_m: f64,
    /// What it is made of.
    pub surface: Surface,
    /// Crosses over something.
    pub bridge: bool,
}

/// The pedestrian graph. The same features in any order give the same graph,
/// node and edge numbering included.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WalkGraph {
    /// Node positions, ordered by their snapped coordinates.
    pub nodes: Vec<LonLat>,
    /// Edges, ordered by their end nodes.
    pub edges: Vec<WalkEdge>,
    /// Edge indices touching each node, ascending.
    pub adjacency: Vec<Vec<usize>>,
}

/// Surfaces in the order of their codes in [`WalkGraph::edge_surface_codes`].
pub const SURFACE_CODES: [Surface; 5] = [
    Surface::Footway,
    Surface::Track,
    Surface::Steps,
    Surface::Street,
    Surface::Carriageway,
];

// Flat views for bindings, which pass typed arrays rather than objects. Node
// and edge indices of a real graph fit `u32`.
#[allow(clippy::cast_possible_truncation)]
impl WalkGraph {
    /// Longitude and latitude of every node, interleaved.
    #[must_use]
    pub fn node_coordinates(&self) -> Vec<f64> {
        self.nodes
            .iter()
            .flat_map(|node| [node[0], node[1]])
            .collect()
    }

    /// The two node indices of every edge, interleaved.
    #[must_use]
    pub fn edge_nodes(&self) -> Vec<u32> {
        self.edges
            .iter()
            .flat_map(|edge| [edge.a as u32, edge.b as u32])
            .collect()
    }

    /// The length of every edge.
    #[must_use]
    pub fn edge_lengths(&self) -> Vec<f64> {
        self.edges.iter().map(|edge| edge.length_m).collect()
    }

    /// Every edge's surface, as its index in [`SURFACE_CODES`].
    #[must_use]
    pub fn edge_surface_codes(&self) -> Vec<u8> {
        self.edges
            .iter()
            .map(|edge| {
                SURFACE_CODES
                    .iter()
                    .position(|s| *s == edge.surface)
                    .unwrap_or(0) as u8
            })
            .collect()
    }

    /// 1 for every edge that is a bridge, 0 otherwise.
    #[must_use]
    pub fn edge_bridges(&self) -> Vec<u8> {
        self.edges
            .iter()
            .map(|edge| u8::from(edge.bridge))
            .collect()
    }

    /// Adjacency as offsets into [`Self::adjacency_edges`]: node `i`'s edges
    /// are `adjacency_edges[offsets[i]..offsets[i + 1]]`.
    #[must_use]
    pub fn adjacency_offsets(&self) -> Vec<u32> {
        let mut offsets = Vec::with_capacity(self.adjacency.len() + 1);
        let mut at = 0_u32;
        offsets.push(at);
        for edges in &self.adjacency {
            at += edges.len() as u32;
            offsets.push(at);
        }
        offsets
    }

    /// Every node's edge indices, ascending, one node after another.
    #[must_use]
    pub fn adjacency_edges(&self) -> Vec<u32> {
        self.adjacency
            .iter()
            .flatten()
            .map(|&edge| edge as u32)
            .collect()
    }
}

/// What a road feature is, without its coordinates: they travel separately
/// as numbers, so no text encoding can round them.
#[derive(Clone, Debug, Deserialize)]
struct FeatureShape {
    kind: String,
    #[serde(default)]
    kind_detail: Option<String>,
    #[serde(default)]
    is_bridge: bool,
    /// How many lines the feature has.
    lines: usize,
}

/// Builds the graph from flat input, as a binding passes it: `features` is a
/// JSON array of `{"kind","kind_detail"?,"is_bridge"?,"lines":<count>}`,
/// `line_points` the number of points of every line in order, and
/// `coordinates` every point's longitude and latitude, interleaved. `None`
/// when the JSON is malformed or the counts do not add up.
#[must_use]
pub fn build_walk_graph_flat(
    features: &str,
    line_points: &[u32],
    coordinates: &[f64],
) -> Option<WalkGraph> {
    let shapes: Vec<FeatureShape> = serde_json::from_str(features).ok()?;
    if shapes.iter().map(|shape| shape.lines).sum::<usize>() != line_points.len()
        || line_points.iter().map(|&n| n as usize).sum::<usize>() * 2 != coordinates.len()
    {
        return None;
    }
    let mut lines = line_points.iter();
    let mut points = coordinates.as_chunks::<2>().0.iter().copied();
    let features: Vec<RoadFeature> = shapes
        .into_iter()
        .map(|shape| RoadFeature {
            kind: shape.kind,
            kind_detail: shape.kind_detail,
            is_bridge: shape.is_bridge,
            lines: lines
                .by_ref()
                .take(shape.lines)
                .map(|&n| points.by_ref().take(n as usize).collect())
                .collect(),
        })
        .collect();
    Some(build_walk_graph(&features))
}

/// Applies a binding's mask: one byte per node, non-zero where a walk may
/// enter. Without a mask every node may be entered; nodes past a short mask
/// may not.
pub fn mask_enterable(mask: Option<&[u8]>) -> impl Fn(usize) -> bool + '_ {
    move |node| mask.is_none_or(|mask| mask.get(node).is_some_and(|&byte| byte != 0))
}

/// Cost of walking an edge end to end.
#[must_use]
pub fn edge_cost(edge: &WalkEdge) -> f64 {
    edge.length_m * edge.surface.weight()
}

/// A node, named by its coordinates snapped to [`COORDINATE_PRECISION`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct NodeKey {
    lon: i64,
    lat: i64,
}

impl NodeKey {
    // Snapped coordinates are within ±1.8e9 and exactly representable.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn of(point: LonLat) -> Self {
        Self {
            lon: js_round(point[0] * COORDINATE_PRECISION) as i64,
            lat: js_round(point[1] * COORDINATE_PRECISION) as i64,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn point(self) -> LonLat {
        [
            self.lon as f64 / COORDINATE_PRECISION,
            self.lat as f64 / COORDINATE_PRECISION,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    a: NodeKey,
    b: NodeKey,
    surface: Surface,
    bridge: bool,
}

fn segment(a: NodeKey, b: NodeKey, surface: Surface, bridge: bool) -> Segment {
    if a <= b {
        Segment {
            a,
            b,
            surface,
            bridge,
        }
    } else {
        Segment {
            a: b,
            b: a,
            surface,
            bridge,
        }
    }
}

/// Builds the walking graph from a tile's `roads` features.
#[must_use]
pub fn build_walk_graph(features: &[RoadFeature]) -> WalkGraph {
    let mut raw = Vec::new();
    for feature in features {
        let Some(surface) = surface_of(&feature.kind, feature.kind_detail.as_deref()) else {
            continue;
        };
        for line in &feature.lines {
            let mut previous: Option<NodeKey> = None;
            for point in line {
                if !point[0].is_finite() || !point[1].is_finite() {
                    previous = None;
                    continue;
                }
                let node = NodeKey::of(*point);
                if let Some(previous) = previous
                    && previous != node
                {
                    raw.push(segment(previous, node, surface, feature.is_bridge));
                }
                previous = Some(node);
            }
        }
    }
    assemble(&dedupe(stitch(&dedupe(raw))))
}

/// Overlapping copies of one stretch collapse to the cheapest surface.
fn dedupe(segments: Vec<Segment>) -> Vec<Segment> {
    let mut by_pair: BTreeMap<(NodeKey, NodeKey), Segment> = BTreeMap::new();
    for s in segments {
        let Some(kept) = by_pair.get(&(s.a, s.b)) else {
            by_pair.insert((s.a, s.b), s);
            continue;
        };
        let weight = s.surface.weight() - kept.surface.weight();
        let order = if weight == 0.0 {
            match s.surface.as_str().cmp(kept.surface.as_str()) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            }
        } else {
            weight
        };
        if order < 0.0 || (order == 0.0 && s.bridge && !kept.bridge) {
            by_pair.insert((s.a, s.b), s);
        }
    }
    // A BTreeMap keyed by the ordered pair iterates in segment order.
    by_pair.into_values().collect()
}

/// Coarse bucket index so stitching only looks at segments near a loose end.
#[derive(Default)]
struct SegmentGrid {
    cells: HashMap<(i64, i64), Vec<usize>>,
}

// Cell indices of real coordinates fit easily in `i64`.
#[allow(clippy::cast_possible_truncation)]
fn cell(degrees: f64) -> i64 {
    (degrees / GRID_CELL_DEGREES).floor() as i64
}

impl SegmentGrid {
    fn add(&mut self, s: &Segment, index: usize) {
        let (a, b) = (s.a.point(), s.b.point());
        let pad_lat = STITCH_TOLERANCE_M / 111_000.0;
        let cos = js_max(0.01, libm::cos((a[1] * std::f64::consts::PI) / 180.0));
        let pad_lon = pad_lat / cos;
        let (x0, x1) = (
            cell(js_min(a[0], b[0]) - pad_lon),
            cell(js_max(a[0], b[0]) + pad_lon),
        );
        let (y0, y1) = (
            cell(js_min(a[1], b[1]) - pad_lat),
            cell(js_max(a[1], b[1]) + pad_lat),
        );
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(index);
            }
        }
    }

    fn near(&self, point: LonLat) -> &[usize] {
        self.cells
            .get(&(cell(point[0]), cell(point[1])))
            .map_or(&[], Vec::as_slice)
    }
}

/// Joins loose line ends to the line they were cut from.
///
/// Every node used by exactly one segment is visited in coordinate order. If
/// another node lies within the tolerance it is merged into it; otherwise, if
/// a segment passes within the tolerance, that segment is split at the loose
/// end. A real dead end farther than the tolerance from anything stays.
fn stitch(input: &[Segment]) -> Vec<Segment> {
    let mut segments: Vec<Option<Segment>> = input.iter().copied().map(Some).collect();
    let mut degree: HashMap<NodeKey, u32> = HashMap::new();
    for s in input {
        *degree.entry(s.a).or_default() += 1;
        *degree.entry(s.b).or_default() += 1;
    }
    let mut grid = SegmentGrid::default();
    for (index, s) in input.iter().enumerate() {
        grid.add(s, index);
    }
    let mut loose: Vec<NodeKey> = degree
        .iter()
        .filter(|&(_, &count)| count == 1)
        .map(|(&node, _)| node)
        .collect();
    loose.sort_unstable();

    for end in loose {
        if degree.get(&end) != Some(&1) {
            continue;
        }
        let nearby: Vec<(usize, Segment)> = grid
            .near(end.point())
            .iter()
            .filter_map(|&index| segments[index].map(|s| (index, s)))
            .collect();
        let Some(&(own_index, own)) = nearby.iter().find(|(_, s)| s.a == end || s.b == end) else {
            continue;
        };
        let neighbour = if own.a == end { own.b } else { own.a };

        if let Some(target) = merge_target(end, neighbour, &nearby) {
            segments[own_index] = None;
            let joined = segment(neighbour, target, own.surface, own.bridge);
            segments.push(Some(joined));
            grid.add(&joined, segments.len() - 1);
            degree.insert(end, 0);
            *degree.entry(target).or_default() += 1;
            continue;
        }

        let mut split: Option<(usize, Segment, f64)> = None;
        for &(index, s) in &nearby {
            if s.a == end || s.b == end {
                continue;
            }
            let distance = project_on_segment(end.point(), s.a.point(), s.b.point()).distance_m;
            if distance > STITCH_TOLERANCE_M {
                continue;
            }
            if split.is_none_or(|(_, _, best)| distance < best) {
                split = Some((index, s, distance));
            }
        }
        if let Some((index, s, _)) = split {
            segments[index] = None;
            for piece in [
                segment(s.a, end, s.surface, s.bridge),
                segment(end, s.b, s.surface, s.bridge),
            ] {
                segments.push(Some(piece));
                grid.add(&piece, segments.len() - 1);
            }
            *degree.entry(end).or_default() += 2;
        }
    }
    segments.into_iter().flatten().collect()
}

/// The nearest other node within the tolerance of a loose end; ties go to the
/// lower node.
fn merge_target(end: NodeKey, neighbour: NodeKey, nearby: &[(usize, Segment)]) -> Option<NodeKey> {
    let mut merge: Option<(NodeKey, f64)> = None;
    for (_, s) in nearby {
        for node in [s.a, s.b] {
            if node == end || node == neighbour {
                continue;
            }
            let distance = distance_m(end.point(), node.point());
            if distance > STITCH_TOLERANCE_M {
                continue;
            }
            if merge.is_none_or(|(best, best_distance)| {
                distance < best_distance || (distance == best_distance && node < best)
            }) {
                merge = Some((node, distance));
            }
        }
    }
    merge.map(|(node, _)| node)
}

fn assemble(segments: &[Segment]) -> WalkGraph {
    let mut ordered: Vec<NodeKey> = segments.iter().flat_map(|s| [s.a, s.b]).collect();
    ordered.sort_unstable();
    ordered.dedup();
    let index_of: HashMap<NodeKey, usize> = ordered
        .iter()
        .enumerate()
        .map(|(index, &node)| (node, index))
        .collect();
    let edges: Vec<WalkEdge> = segments
        .iter()
        .map(|s| WalkEdge {
            a: index_of[&s.a],
            b: index_of[&s.b],
            length_m: distance_m(s.a.point(), s.b.point()),
            surface: s.surface,
            bridge: s.bridge,
        })
        .collect();
    let mut adjacency = vec![Vec::new(); ordered.len()];
    for (index, edge) in edges.iter().enumerate() {
        adjacency[edge.a].push(index);
        adjacency[edge.b].push(index);
    }
    WalkGraph {
        nodes: ordered.into_iter().map(NodeKey::point).collect(),
        edges,
        adjacency,
    }
}

/// Ground a walk may not enter: fog, or anything else the caller rules out.
pub type CanEnter<'a> = Option<&'a dyn Fn(LonLat) -> bool>;

fn enters(can_enter: CanEnter<'_>, point: LonLat) -> bool {
    can_enter.is_none_or(|can_enter| can_enter(point))
}

/// A point placed on the graph: somewhere along one edge.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphPosition {
    /// The edge.
    pub edge: usize,
    /// 0 at the edge's `a` node, 1 at its `b` node.
    pub t: f64,
    /// The position.
    pub point: LonLat,
    /// From the original point to `point`.
    pub distance_m: f64,
}

/// The nearest enterable place on the graph within `max_distance_m` of
/// `point` (use [`SNAP_DISTANCE_M`] by default). Ties go to the lower edge.
#[must_use]
pub fn snap_to_graph(
    graph: &WalkGraph,
    point: LonLat,
    max_distance_m: f64,
    can_enter: CanEnter<'_>,
) -> Option<GraphPosition> {
    let mut best: Option<GraphPosition> = None;
    for (index, edge) in graph.edges.iter().enumerate() {
        let (Some(&a), Some(&b)) = (graph.nodes.get(edge.a), graph.nodes.get(edge.b)) else {
            continue;
        };
        let projection = project_on_segment(point, a, b);
        if projection.distance_m > max_distance_m {
            continue;
        }
        if best.is_some_and(|best| projection.distance_m >= best.distance_m) {
            continue;
        }
        if !enters(can_enter, projection.point) {
            continue;
        }
        best = Some(GraphPosition {
            edge: index,
            t: projection.t,
            point: projection.point,
            distance_m: projection.distance_m,
        });
    }
    best
}

/// Every place on the graph within `max_distance_m` of `point`, nearest
/// first, ties by edge index: one per edge. The first a caller may enter is
/// what [`snap_to_graph`] answers with that caller's `can_enter`, so a host
/// that cannot hand Core a callback (a binding) applies its own fog here. A
/// point that is not finite is on no graph.
#[must_use]
pub fn snap_candidates(
    graph: &WalkGraph,
    point: LonLat,
    max_distance_m: f64,
) -> Vec<GraphPosition> {
    if !point[0].is_finite() || !point[1].is_finite() {
        return Vec::new();
    }
    let mut candidates: Vec<GraphPosition> = graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(index, edge)| {
            let (a, b) = (*graph.nodes.get(edge.a)?, *graph.nodes.get(edge.b)?);
            let projection = project_on_segment(point, a, b);
            (projection.distance_m <= max_distance_m).then_some(GraphPosition {
                edge: index,
                t: projection.t,
                point: projection.point,
                distance_m: projection.distance_m,
            })
        })
        .collect();
    candidates.sort_by(|x, y| {
        x.distance_m
            .partial_cmp(&y.distance_m)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(x.edge.cmp(&y.edge))
    });
    candidates
}

/// A walk between two graph positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalkRoute {
    /// From the start position to the end position, graph nodes in between.
    pub points: Vec<LonLat>,
    /// Node indices passed through, in order; empty when both ends share an
    /// edge and no node is passed.
    pub nodes: Vec<usize>,
    /// Its length.
    pub length_m: f64,
    /// Length weighted by surface: what the route minimises.
    pub cost: f64,
}

/// Binary heap on (cost, node), the node breaking ties.
#[derive(Default)]
struct MinHeap {
    items: Vec<(f64, usize)>,
}

fn less(x: (f64, usize), y: (f64, usize)) -> bool {
    x.0 < y.0 || (x.0 == y.0 && x.1 < y.1)
}

impl MinHeap {
    fn push(&mut self, cost: f64, node: usize) {
        self.items.push((cost, node));
        let mut i = self.items.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 2;
            if !less(self.items[i], self.items[parent]) {
                break;
            }
            self.items.swap(i, parent);
            i = parent;
        }
    }

    fn pop(&mut self) -> Option<(f64, usize)> {
        let top = *self.items.first()?;
        let last = self.items.pop()?;
        if !self.items.is_empty() {
            self.items[0] = last;
            let mut i = 0;
            loop {
                let left = 2 * i + 1;
                let right = left + 1;
                let mut smallest = i;
                if left < self.items.len() && less(self.items[left], self.items[smallest]) {
                    smallest = left;
                }
                if right < self.items.len() && less(self.items[right], self.items[smallest]) {
                    smallest = right;
                }
                if smallest == i {
                    break;
                }
                self.items.swap(i, smallest);
                i = smallest;
            }
        }
        Some(top)
    }
}

/// The cheapest walk between two graph positions, or `None` when none exists.
///
/// Dijkstra over surface-weighted metres; nodes `can_enter` refuses are never
/// entered (checked per node, not along edges). Ties go to the lower node.
#[must_use]
pub fn route_on_graph(
    graph: &WalkGraph,
    from: &GraphPosition,
    to: &GraphPosition,
    can_enter: CanEnter<'_>,
) -> Option<WalkRoute> {
    route_on_graph_where(graph, from, to, &|node| enters_node(graph, can_enter, node))
}

fn enters_node(graph: &WalkGraph, can_enter: CanEnter<'_>, node: usize) -> bool {
    graph
        .nodes
        .get(node)
        .is_some_and(|&point| enters(can_enter, point))
}

/// [`route_on_graph`] with the nodes a walk may enter named by index: a host
/// that cannot hand Core a callback (a binding) passes a mask instead. Nodes
/// `enterable` refuses, and indices the graph does not hold, are never
/// entered.
#[must_use]
pub fn route_on_graph_where(
    graph: &WalkGraph,
    from: &GraphPosition,
    to: &GraphPosition,
    enterable: &dyn Fn(usize) -> bool,
) -> Option<WalkRoute> {
    let start_edge = graph.edges.get(from.edge)?;
    let end_edge = graph.edges.get(to.edge)?;
    let enterable = |node: usize| node < graph.nodes.len() && enterable(node);

    let n = graph.nodes.len();
    let mut cost = vec![f64::INFINITY; n];
    let mut length = vec![f64::INFINITY; n];
    let mut previous: Vec<Option<usize>> = vec![None; n];
    let mut done = vec![false; n];
    let mut heap = MinHeap::default();

    let start_cost = edge_cost(start_edge);
    for (node, fraction) in [(start_edge.a, from.t), (start_edge.b, 1.0 - from.t)] {
        if !enterable(node) {
            continue;
        }
        let c = start_cost * fraction;
        if c < cost[node] {
            cost[node] = c;
            length[node] = start_edge.length_m * fraction;
            heap.push(c, node);
        }
    }

    // (cost, length, the last node before the end edge, if any).
    let mut best: Option<(f64, f64, Option<usize>)> = None;
    if from.edge == to.edge {
        let fraction = (from.t - to.t).abs();
        best = Some((start_cost * fraction, start_edge.length_m * fraction, None));
    }

    let end_cost = edge_cost(end_edge);
    let tail = |node: usize| {
        if node == end_edge.a {
            Some(to.t)
        } else if node == end_edge.b {
            Some(1.0 - to.t)
        } else {
            None
        }
    };

    while let Some((c, node)) = heap.pop() {
        if done[node] || c > cost[node] {
            continue;
        }
        if best.is_some_and(|(best_cost, _, _)| c >= best_cost) {
            break;
        }
        done[node] = true;

        if let Some(fraction) = tail(node) {
            let total = c + end_cost * fraction;
            if best.is_none_or(|(best_cost, _, _)| total < best_cost) {
                best = Some((
                    total,
                    length[node] + end_edge.length_m * fraction,
                    Some(node),
                ));
            }
        }

        // A graph not built by `build_walk_graph` may be inconsistent: an
        // index it does not hold leads nowhere, it never panics.
        for edge in graph
            .adjacency
            .get(node)
            .into_iter()
            .flatten()
            .filter_map(|&i| graph.edges.get(i))
        {
            let next = if edge.a == node { edge.b } else { edge.a };
            if !enterable(next) || done[next] {
                continue;
            }
            let next_cost = c + edge_cost(edge);
            let ties_lower = next_cost == cost[next] && previous[next].is_some_and(|p| node < p);
            if next_cost < cost[next] || ties_lower {
                cost[next] = next_cost;
                length[next] = length[node] + edge.length_m;
                previous[next] = Some(node);
                heap.push(next_cost, next);
            }
        }
    }

    let (cost, length_m, via) = best?;
    let mut nodes = Vec::new();
    let mut at = via;
    while let Some(node) = at {
        nodes.push(node);
        at = previous[node];
    }
    nodes.reverse();
    let mut points = Vec::with_capacity(nodes.len() + 2);
    points.push(from.point);
    points.extend(nodes.iter().map(|&node| graph.nodes[node]));
    points.push(to.point);
    Some(WalkRoute {
        points,
        nodes,
        length_m,
        cost,
    })
}

/// How far every node is from one graph position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reach {
    /// Surface-weighted cost to each node; infinite where it cannot be reached.
    pub cost: Vec<f64>,
    /// Metres walked to each node along the cheapest way; infinite likewise.
    pub length_m: Vec<f64>,
}

/// The cheapest way from one position to every node at once. Nodes costing
/// more than `max_cost` (use `f64::INFINITY` for no bound) are left unreached,
/// which also bounds the work.
#[must_use]
pub fn reach_from(
    graph: &WalkGraph,
    from: &GraphPosition,
    max_cost: f64,
    can_enter: CanEnter<'_>,
) -> Reach {
    reach_from_where(graph, from, max_cost, &|node| {
        enters_node(graph, can_enter, node)
    })
}

/// [`reach_from`] with the nodes a walk may enter named by index, as in
/// [`route_on_graph_where`].
#[must_use]
pub fn reach_from_where(
    graph: &WalkGraph,
    from: &GraphPosition,
    max_cost: f64,
    enterable: &dyn Fn(usize) -> bool,
) -> Reach {
    let n = graph.nodes.len();
    let mut cost = vec![f64::INFINITY; n];
    let mut length_m = vec![f64::INFINITY; n];
    let Some(start_edge) = graph.edges.get(from.edge) else {
        return Reach { cost, length_m };
    };
    let enterable = |node: usize| node < n && enterable(node);
    let mut done = vec![false; n];
    let mut heap = MinHeap::default();
    let start_cost = edge_cost(start_edge);
    for (node, fraction) in [(start_edge.a, from.t), (start_edge.b, 1.0 - from.t)] {
        let c = start_cost * fraction;
        if !enterable(node) || c > max_cost || c >= cost[node] {
            continue;
        }
        cost[node] = c;
        length_m[node] = start_edge.length_m * fraction;
        heap.push(c, node);
    }
    while let Some((c, node)) = heap.pop() {
        if done[node] || c > cost[node] {
            continue;
        }
        done[node] = true;
        // A graph not built by `build_walk_graph` may be inconsistent: an
        // index it does not hold leads nowhere, it never panics.
        for edge in graph
            .adjacency
            .get(node)
            .into_iter()
            .flatten()
            .filter_map(|&i| graph.edges.get(i))
        {
            let next = if edge.a == node { edge.b } else { edge.a };
            if !enterable(next) || done[next] {
                continue;
            }
            let next_cost = c + edge_cost(edge);
            if next_cost > max_cost || next_cost >= cost[next] {
                continue;
            }
            cost[next] = next_cost;
            length_m[next] = length_m[node] + edge.length_m;
            heap.push(next_cost, next);
        }
    }
    Reach { cost, length_m }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_follows_javascript() {
        for (x, rounded) in [
            (2.5, 3.0),
            (-2.5, -2.0),
            (-0.5, 0.0),
            (0.49, 0.0),
            (-1.51, -2.0),
            (1.5, 2.0),
            (-3.0, -3.0),
        ] {
            assert_eq!(js_round(x), rounded, "{x}");
        }
    }

    #[test]
    fn surfaces_follow_the_table() {
        assert_eq!(surface_of("path", Some("steps")), Some(Surface::Steps));
        assert_eq!(surface_of("minor_road", None), Some(Surface::Street));
        assert_eq!(
            surface_of("medium_road", Some("unknown_detail")),
            Some(Surface::Street)
        );
        assert_eq!(surface_of("major_road", Some("trunk")), None);
        assert_eq!(surface_of("highway", Some("footway")), None);
        assert_eq!(surface_of("rail", None), None);
    }

    #[test]
    fn an_inconsistent_graph_routes_nowhere_instead_of_panicking() {
        let edge = |a, b| WalkEdge {
            a,
            b,
            length_m: 10.0,
            surface: Surface::Footway,
            bridge: false,
        };
        let graph = WalkGraph {
            nodes: vec![[30.5, 50.4], [30.5001, 50.4]],
            edges: vec![edge(0, 1), edge(1, 7)],
            adjacency: vec![vec![0, 9], vec![0, 1]],
        };
        let at = |edge, t| GraphPosition {
            edge,
            t,
            point: [30.5, 50.4],
            distance_m: 0.0,
        };
        let _ = route_on_graph(&graph, &at(0, 0.2), &at(1, 0.5), None);
        let _ = route_on_graph(&graph, &at(0, 0.2), &at(5, 0.5), None);
        let reach = reach_from(&graph, &at(1, 0.5), f64::INFINITY, None);
        assert_eq!(reach.cost.len(), 2);
        let short = WalkGraph {
            nodes: vec![[30.5, 50.4]],
            edges: vec![edge(0, 1)],
            adjacency: vec![],
        };
        let _ = route_on_graph(&short, &at(0, 0.0), &at(0, 1.0), None);
        let _ = reach_from(&short, &at(0, 0.0), f64::INFINITY, None);
    }

    #[test]
    fn flat_input_and_views_describe_the_same_graph() {
        let shapes = r#"[{"kind":"path","lines":1},
            {"kind":"major_road","kind_detail":"primary","is_bridge":true,"lines":1}]"#;
        let coordinates = [
            30.5, 50.4, 30.501, 50.4, 30.501, 50.401, 30.501, 50.4, 30.502, 50.4,
        ];
        let graph = build_walk_graph_flat(shapes, &[3, 2], &coordinates).expect("features");
        assert_eq!(
            graph,
            build_walk_graph(&[
                RoadFeature {
                    kind: "path".to_owned(),
                    kind_detail: None,
                    is_bridge: false,
                    lines: vec![vec![[30.5, 50.4], [30.501, 50.4], [30.501, 50.401]]],
                },
                RoadFeature {
                    kind: "major_road".to_owned(),
                    kind_detail: Some("primary".to_owned()),
                    is_bridge: true,
                    lines: vec![vec![[30.501, 50.4], [30.502, 50.4]]],
                },
            ])
        );
        assert_eq!(graph.node_coordinates().len(), graph.nodes.len() * 2);
        assert_eq!(graph.edge_nodes().len(), graph.edges.len() * 2);
        let codes = graph.edge_surface_codes();
        for (edge, code) in graph.edges.iter().zip(&codes) {
            assert_eq!(SURFACE_CODES[usize::from(*code)], edge.surface);
        }
        assert_eq!(
            graph
                .edge_bridges()
                .iter()
                .map(|&b| usize::from(b))
                .sum::<usize>(),
            1
        );
        let (offsets, edges) = (graph.adjacency_offsets(), graph.adjacency_edges());
        for (node, adjacent) in graph.adjacency.iter().enumerate() {
            let range = offsets[node] as usize..offsets[node + 1] as usize;
            let flat: Vec<usize> = edges[range].iter().map(|&e| e as usize).collect();
            assert_eq!(&flat, adjacent);
        }
        assert!(build_walk_graph_flat("{", &[], &[]).is_none());
        assert!(build_walk_graph_flat(r#"[{"kind":"path"}]"#, &[], &[]).is_none());
        // Counts that do not add up are refused, never read past.
        assert!(build_walk_graph_flat(shapes, &[3], &coordinates).is_none());
        assert!(build_walk_graph_flat(shapes, &[3, 2], &coordinates[..9]).is_none());
        assert!(build_walk_graph_flat(shapes, &[3, 3], &coordinates).is_none());
    }

    #[test]
    fn a_mask_names_the_nodes_a_walk_may_enter() {
        let open = mask_enterable(None);
        assert!(open(0) && open(99));
        let mask = [1_u8, 0, 2];
        let masked = mask_enterable(Some(&mask));
        assert_eq!(
            (masked(0), masked(1), masked(2), masked(3)),
            (true, false, true, false)
        );
    }

    #[test]
    fn a_route_on_one_edge_passes_no_node() {
        let graph = build_walk_graph(&[RoadFeature {
            kind: "path".to_owned(),
            kind_detail: None,
            is_bridge: false,
            lines: vec![vec![[30.5, 50.4], [30.501, 50.4]]],
        }]);
        let from = snap_to_graph(&graph, [30.5002, 50.4], SNAP_DISTANCE_M, None).expect("on graph");
        let to = snap_to_graph(&graph, [30.5008, 50.4], SNAP_DISTANCE_M, None).expect("on graph");
        let route = route_on_graph(&graph, &from, &to, None).expect("route");
        assert!(route.nodes.is_empty());
        assert_eq!(route.points.len(), 2);
        assert!(snap_to_graph(&graph, [30.5, 50.41], SNAP_DISTANCE_M, None).is_none());
    }
}
