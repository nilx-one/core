// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Reads the `roads` layer of a Mapbox Vector Tile into [`RoadFeature`]s
//! (`docs/vector-tile-roads.md`), so a host without a map renderer, such as a
//! bot, can build the [walking graph](crate::walk_graph) from the same archive
//! the map draws. Fetching and decompressing tiles stays with the host.
//!
//! The decoder mirrors Web's `packages/map-maplibre/src/mvt-roads.ts`: only
//! line features with a string `kind` are kept, `kind_detail` and `is_bridge`
//! verbatim, every line of at least two points in `[longitude, latitude]`.
//! Unlike Web it fails closed: a truncated or malformed tile is an error, not
//! a partial answer. It needs no dependency beyond `libm`, whose trigonometry
//! gives the same bits on every platform.

use core::fmt;

use crate::walk_graph::{LonLat, RoadFeature, WalkGraph, build_walk_graph};

/// The archive's road network source layer: what a body walks along.
pub const ROAD_SOURCE_LAYER: &str = "roads";

/// The deepest zoom a tile address may carry; `2^z` stays exact in `f64`.
pub const MAX_TILE_ZOOM: u8 = 30;

/// The coarsest zoom a walking graph is built from: footways and paths are in
/// it (Web's `ROAD_TILE_ZOOM`), and one tile's segments stay a few kilometres
/// long, so the graph's spatial index stays small.
pub const MIN_WALK_TILE_ZOOM: u8 = 14;

/// Where a tile sits in the Web Mercator pyramid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileId {
    /// Zoom.
    pub z: u8,
    /// Column, west to east.
    pub x: u32,
    /// Row, north to south.
    pub y: u32,
}

impl TileId {
    /// Whether `x` and `y` lie inside zoom `z`, which is at most
    /// [`MAX_TILE_ZOOM`].
    #[must_use]
    pub const fn is_valid(self) -> bool {
        if self.z > MAX_TILE_ZOOM {
            return false;
        }
        let size = 1u64 << self.z;
        (self.x as u64) < size && (self.y as u64) < size
    }
}

/// Why a tile could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorTileError {
    /// The tile address is outside its zoom, or the zoom is too deep.
    InvalidTile,
    /// The bytes are gzip: the host must decompress the archive's tile first.
    Compressed,
    /// The bytes are not a well-formed vector tile, or a road reaches further
    /// than one tile's width outside its tile.
    Malformed,
    /// The tile is coarser than [`MIN_WALK_TILE_ZOOM`] for a walking graph.
    TooCoarse,
}

impl VectorTileError {
    /// The stable wire code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidTile => "invalid_tile",
            Self::Compressed => "compressed",
            Self::Malformed => "malformed",
            Self::TooCoarse => "too_coarse",
        }
    }
}

impl fmt::Display for VectorTileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for VectorTileError {}

/// One fetched, decompressed tile of the archive.
#[derive(Clone, Copy, Debug)]
pub struct RoadTile<'a> {
    /// Its address.
    pub tile: TileId,
    /// Its Mapbox Vector Tile bytes.
    pub bytes: &'a [u8],
}

/// The line features of `layer_name` in one tile, as [`RoadFeature`]s.
/// A tile without that layer answers no features.
///
/// # Errors
///
/// [`VectorTileError`] when the address is invalid, the bytes are still gzip,
/// or the tile is truncated or malformed anywhere it is read.
pub fn decode_tile_roads(
    bytes: &[u8],
    tile: TileId,
    layer_name: &str,
) -> Result<Vec<RoadFeature>, VectorTileError> {
    if !tile.is_valid() {
        return Err(VectorTileError::InvalidTile);
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(VectorTileError::Compressed);
    }
    let mut reader = Reader::new(bytes);
    while !reader.done() {
        let (field, wire) = reader.key()?;
        if field != 3 {
            reader.skip(wire)?;
            continue;
        }
        let layer = reader.delimited(wire)?;
        // The name is read first; any other layer is skipped whole, undecoded.
        if name_of_layer(layer)?.as_deref() == Some(layer_name) {
            return roads_of_layer(layer, tile);
        }
    }
    Ok(Vec::new())
}

/// The walking graph of several tiles' `roads` layers together. Features a
/// neighbouring tile repeats in its buffer are merged by the graph, and line
/// ends a tile cut short are joined back, as [`build_walk_graph`] does.
///
/// # Errors
///
/// The first tile's [`VectorTileError`], including [`VectorTileError::TooCoarse`]
/// below [`MIN_WALK_TILE_ZOOM`]; one bad tile fails the whole graph, so a host
/// never routes on a network with a silent hole in it.
pub fn build_walk_graph_from_tiles(tiles: &[RoadTile<'_>]) -> Result<WalkGraph, VectorTileError> {
    let mut features = Vec::new();
    for tile in tiles {
        if tile.tile.z < MIN_WALK_TILE_ZOOM {
            return Err(VectorTileError::TooCoarse);
        }
        features.extend(decode_tile_roads(tile.bytes, tile.tile, ROAD_SOURCE_LAYER)?);
    }
    Ok(build_walk_graph(&features))
}

const LINESTRING: u64 = 2;
const DEFAULT_EXTENT: u64 = 4096;

const WIRE_VARINT: u8 = 0;
const WIRE_FIXED64: u8 = 1;
const WIRE_DELIMITED: u8 = 2;
const WIRE_FIXED32: u8 = 5;

/// An attribute value, as far as a road needs it.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Text(String),
    Bool(bool),
    Other,
}

/// A layer's name, read without decoding its features, keys or values.
fn name_of_layer(bytes: &[u8]) -> Result<Option<String>, VectorTileError> {
    let mut reader = Reader::new(bytes);
    while !reader.done() {
        let (field, wire) = reader.key()?;
        if field == 1 {
            return Ok(Some(text(reader.delimited(wire)?)));
        }
        reader.skip(wire)?;
    }
    Ok(None)
}

fn roads_of_layer(bytes: &[u8], tile: TileId) -> Result<Vec<RoadFeature>, VectorTileError> {
    let mut reader = Reader::new(bytes);
    let mut keys = Vec::new();
    let mut values = Vec::new();
    let mut features = Vec::new();
    let mut extent = DEFAULT_EXTENT;
    while !reader.done() {
        let (field, wire) = reader.key()?;
        match field {
            2 => features.push(reader.delimited(wire)?),
            3 => keys.push(text(reader.delimited(wire)?)),
            // A value with no known field is no value, as in Web: its tags are
            // skipped rather than overriding an earlier one.
            4 => values.push(decode_value(reader.delimited(wire)?)?),
            5 => extent = reader.varint_field(wire)?,
            _ => reader.skip(wire)?,
        }
    }
    if extent == 0 || extent > u64::from(u32::MAX) {
        return Err(VectorTileError::Malformed);
    }
    // A real tile's buffer is a small fraction of its extent; a point a whole
    // tile outside it is broken or hostile, and would stretch one segment
    // across the graph's spatial index.
    #[allow(clippy::cast_possible_wrap, reason = "extent fits u32")]
    let reach = (-(extent as i64))..=(2 * extent as i64);
    #[allow(clippy::cast_precision_loss, reason = "extent fits u32, exact in f64")]
    let extent = extent as f64;

    let mut roads = Vec::new();
    for feature in features {
        let feature = decode_feature(feature, &keys, &values)?;
        if feature.kind_type != LINESTRING {
            continue;
        }
        let Some(Value::Text(kind)) = feature.property("kind") else {
            continue;
        };
        let lines = lines_of(&feature.geometry);
        if lines
            .iter()
            .flatten()
            .any(|(x, y)| !reach.contains(x) || !reach.contains(y))
        {
            return Err(VectorTileError::Malformed);
        }
        let lines: Vec<Vec<LonLat>> = lines
            .into_iter()
            .filter(|line| line.len() >= 2)
            .map(|line| {
                line.into_iter()
                    .map(|(px, py)| {
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "tile units are far below 2^53"
                        )]
                        to_lon_lat(tile, px as f64 / extent, py as f64 / extent)
                    })
                    .collect()
            })
            .collect();
        if lines.is_empty() {
            continue;
        }
        let kind_detail = match feature.property("kind_detail") {
            Some(Value::Text(detail)) => Some(detail.clone()),
            _ => None,
        };
        let is_bridge = feature.property("is_bridge") == Some(&Value::Bool(true));
        roads.push(RoadFeature {
            kind: kind.clone(),
            kind_detail,
            is_bridge,
            lines,
        });
    }
    Ok(roads)
}

struct Feature<'a> {
    kind_type: u64,
    geometry: Vec<u32>,
    /// Key and value indices, later pairs overriding earlier ones.
    properties: Vec<(&'a str, &'a Value)>,
}

impl Feature<'_> {
    fn property(&self, name: &str) -> Option<&Value> {
        self.properties
            .iter()
            .rev()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    }
}

fn decode_feature<'a>(
    bytes: &[u8],
    keys: &'a [String],
    values: &'a [Option<Value>],
) -> Result<Feature<'a>, VectorTileError> {
    let mut reader = Reader::new(bytes);
    let mut feature = Feature {
        kind_type: 0,
        geometry: Vec::new(),
        properties: Vec::new(),
    };
    while !reader.done() {
        let (field, wire) = reader.key()?;
        match field {
            2 => {
                let mut tags = Reader::new(reader.delimited(wire)?);
                while !tags.done() {
                    let key = tags.varint()?;
                    let value = tags.varint()?;
                    let key = usize::try_from(key).ok().and_then(|at| keys.get(at));
                    let value = usize::try_from(value)
                        .ok()
                        .and_then(|at| values.get(at))
                        .and_then(Option::as_ref);
                    if let (Some(key), Some(value)) = (key, value) {
                        feature.properties.push((key.as_str(), value));
                    }
                }
            }
            3 => feature.kind_type = reader.varint_field(wire)?,
            4 => {
                let mut packed = Reader::new(reader.delimited(wire)?);
                feature.geometry.clear();
                while !packed.done() {
                    let command =
                        u32::try_from(packed.varint()?).map_err(|_| VectorTileError::Malformed)?;
                    feature.geometry.push(command);
                }
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(feature)
}

/// Geometry commands to lines in tile units: `MoveTo` starts one, `LineTo`
/// extends it, `ClosePath` adds nothing, and an unknown command ends the read.
fn lines_of(commands: &[u32]) -> Vec<Vec<(i64, i64)>> {
    let mut lines: Vec<Vec<(i64, i64)>> = Vec::new();
    let (mut x, mut y) = (0i64, 0i64);
    let mut i = 0;
    while i < commands.len() {
        let header = commands[i];
        i += 1;
        let id = header & 0x7;
        let count = header >> 3;
        match id {
            1 | 2 => {
                let mut n = 0;
                while n < count && i + 1 < commands.len() {
                    x += zigzag(commands[i]);
                    y += zigzag(commands[i + 1]);
                    i += 2;
                    n += 1;
                    if id == 1 {
                        lines.push(Vec::new());
                    }
                    if let Some(line) = lines.last_mut() {
                        line.push((x, y));
                    }
                }
            }
            7 => {}
            _ => break,
        }
    }
    lines
}

const fn zigzag(value: u32) -> i64 {
    (value >> 1) as i64 ^ -((value & 1) as i64)
}

/// A point of tile `tile` at fractions `fx`, `fy` of its width and height.
fn to_lon_lat(tile: TileId, fx: f64, fy: f64) -> LonLat {
    let n = f64::from(1u32 << tile.z.min(MAX_TILE_ZOOM));
    let longitude = ((f64::from(tile.x) + fx) / n) * 360.0 - 180.0;
    let latitude = (libm::atan(libm::sinh(
        core::f64::consts::PI * (1.0 - (2.0 * (f64::from(tile.y) + fy)) / n),
    )) * 180.0)
        / core::f64::consts::PI;
    [longitude, latitude]
}

fn decode_value(bytes: &[u8]) -> Result<Option<Value>, VectorTileError> {
    let mut reader = Reader::new(bytes);
    let mut value = None;
    while !reader.done() {
        let (field, wire) = reader.key()?;
        match field {
            1 => value = Some(Value::Text(text(reader.delimited(wire)?))),
            7 => value = Some(Value::Bool(reader.varint_field(wire)? != 0)),
            2..=6 => {
                reader.skip(wire)?;
                value = Some(Value::Other);
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(value)
}

/// Text as `TextDecoder` reads it: invalid UTF-8 becomes U+FFFD.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A protobuf reader that never reads past its slice.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    const fn done(&self) -> bool {
        self.position >= self.bytes.len()
    }

    fn varint(&mut self) -> Result<u64, VectorTileError> {
        let mut result = 0u64;
        for shift in (0..64).step_by(7) {
            let &byte = self
                .bytes
                .get(self.position)
                .ok_or(VectorTileError::Malformed)?;
            self.position += 1;
            result |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Ok(result);
            }
        }
        Err(VectorTileError::Malformed)
    }

    fn key(&mut self) -> Result<(u64, u8), VectorTileError> {
        let key = self.varint()?;
        #[allow(clippy::cast_possible_truncation, reason = "masked to three bits")]
        Ok((key >> 3, (key & 0x7) as u8))
    }

    fn varint_field(&mut self, wire: u8) -> Result<u64, VectorTileError> {
        if wire != WIRE_VARINT {
            return Err(VectorTileError::Malformed);
        }
        self.varint()
    }

    fn delimited(&mut self, wire: u8) -> Result<&'a [u8], VectorTileError> {
        if wire != WIRE_DELIMITED {
            return Err(VectorTileError::Malformed);
        }
        let length = usize::try_from(self.varint()?).map_err(|_| VectorTileError::Malformed)?;
        self.advance(length)
    }

    fn advance(&mut self, length: usize) -> Result<&'a [u8], VectorTileError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(VectorTileError::Malformed)?;
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn skip(&mut self, wire: u8) -> Result<(), VectorTileError> {
        match wire {
            WIRE_VARINT => self.varint().map(|_| ()),
            WIRE_FIXED64 => self.advance(8).map(|_| ()),
            WIRE_DELIMITED => self.delimited(wire).map(|_| ()),
            WIRE_FIXED32 => self.advance(4).map(|_| ()),
            _ => Err(VectorTileError::Malformed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A minimal vector tile writer, the same as Web's test fixture writer.
    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        while value >= 0x80 {
            #[allow(clippy::cast_possible_truncation)]
            out.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        #[allow(clippy::cast_possible_truncation)]
        out.push(value as u8);
        out
    }
    fn key(field: u64, wire: u64) -> Vec<u8> {
        varint(field * 8 + wire)
    }
    fn delimited(field: u64, bytes: &[u8]) -> Vec<u8> {
        [key(field, 2), varint(bytes.len() as u64), bytes.to_vec()].concat()
    }
    fn zz(n: i64) -> u64 {
        #[allow(clippy::cast_sign_loss)]
        if n >= 0 {
            (n * 2) as u64
        } else {
            (-n * 2 - 1) as u64
        }
    }
    fn geometry(lines: &[&[(i64, i64)]]) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut x, mut y) = (0, 0);
        for line in lines {
            let (first, rest) = line.split_first().unwrap();
            out.extend([9, zz(first.0 - x), zz(first.1 - y)]);
            (x, y) = *first;
            out.push(2 | ((rest.len() as u64) << 3));
            for &(px, py) in rest {
                out.extend([zz(px - x), zz(py - y)]);
                (x, y) = (px, py);
            }
        }
        out.into_iter().flat_map(varint).collect()
    }

    enum V {
        S(&'static str),
        B(bool),
        D(f64),
    }

    struct F {
        kind_type: u64,
        props: Vec<(&'static str, V)>,
        lines: Vec<&'static [(i64, i64)]>,
    }

    fn layer(name: &str, features: &[F]) -> Vec<u8> {
        let mut keys: Vec<&str> = Vec::new();
        let mut values: Vec<Vec<u8>> = Vec::new();
        let mut out = [key(15, 0), varint(2), delimited(1, name.as_bytes())].concat();
        for feature in features {
            let mut tags = Vec::new();
            for (k, v) in &feature.props {
                let at = keys.iter().position(|x| x == k).unwrap_or_else(|| {
                    keys.push(k);
                    keys.len() - 1
                });
                let encoded = match v {
                    V::S(s) => delimited(1, s.as_bytes()),
                    V::B(b) => [key(7, 0), varint(u64::from(*b))].concat(),
                    V::D(d) => [key(3, 1), d.to_le_bytes().to_vec()].concat(),
                };
                values.push(encoded);
                tags.extend(varint(at as u64));
                tags.extend(varint(values.len() as u64 - 1));
            }
            let body = [
                delimited(2, &tags),
                key(3, 0),
                varint(feature.kind_type),
                delimited(4, &geometry(&feature.lines)),
            ]
            .concat();
            out.extend(delimited(2, &body));
        }
        for k in keys {
            out.extend(delimited(3, k.as_bytes()));
        }
        for v in values {
            out.extend(delimited(4, &v));
        }
        out.extend([key(5, 0), varint(4096)].concat());
        delimited(3, &out)
    }

    const TILE: TileId = TileId {
        z: 14,
        x: 9581,
        y: 5528,
    };

    fn fixture() -> Vec<u8> {
        [
            layer(
                "pois",
                &[F {
                    kind_type: 1,
                    props: vec![("kind", V::S("monument"))],
                    lines: vec![&[(10, 10)]],
                }],
            ),
            layer(
                "roads",
                &[
                    F {
                        kind_type: 2,
                        props: vec![("kind", V::S("path")), ("kind_detail", V::S("footway"))],
                        lines: vec![&[(0, 0), (4096, 0)]],
                    },
                    F {
                        kind_type: 2,
                        props: vec![
                            ("kind", V::S("minor_road")),
                            ("is_bridge", V::B(true)),
                            ("min_zoom", V::D(13.5)),
                        ],
                        lines: vec![
                            &[(0, 2048), (2048, 2048)],
                            &[(100, 100), (200, 300), (300, 300)],
                        ],
                    },
                    // No kind: nothing to say how it is walked.
                    F {
                        kind_type: 2,
                        props: vec![("kind_detail", V::S("footway"))],
                        lines: vec![&[(0, 0), (10, 10)]],
                    },
                    // A polygon is not a line.
                    F {
                        kind_type: 3,
                        props: vec![("kind", V::S("path"))],
                        lines: vec![&[(0, 0), (10, 0), (10, 10)]],
                    },
                    // A one-point line is dropped, and so is a feature left empty.
                    F {
                        kind_type: 2,
                        props: vec![("kind", V::S("path")), ("is_bridge", V::S("yes"))],
                        lines: vec![&[(5, 5)]],
                    },
                ],
            ),
        ]
        .concat()
    }

    #[test]
    fn reads_the_roads_layers_lines_with_their_attributes_verbatim() {
        let roads = decode_tile_roads(&fixture(), TILE, ROAD_SOURCE_LAYER).unwrap();
        assert_eq!(roads.len(), 2);
        assert_eq!(roads[0].kind, "path");
        assert_eq!(roads[0].kind_detail.as_deref(), Some("footway"));
        assert!(!roads[0].is_bridge);
        assert_eq!(roads[0].lines.len(), 1);
        assert_eq!(roads[1].kind, "minor_road");
        assert_eq!(roads[1].kind_detail, None);
        assert!(roads[1].is_bridge);
        assert_eq!(
            roads[1].lines.iter().map(Vec::len).collect::<Vec<_>>(),
            [2, 3]
        );
    }

    #[test]
    fn places_tile_coordinates_on_the_tiles_own_ground() {
        let roads = decode_tile_roads(&fixture(), TILE, ROAD_SOURCE_LAYER).unwrap();
        let [west, east] = [roads[0].lines[0][0], roads[0].lines[0][1]];
        let n = f64::from(1u32 << 14);
        assert_eq!(
            west[0].to_bits(),
            (f64::from(9581) / n * 360.0 - 180.0).to_bits()
        );
        assert_eq!(
            east[0].to_bits(),
            (f64::from(9582) / n * 360.0 - 180.0).to_bits()
        );
        assert_eq!(west[1].to_bits(), east[1].to_bits());
        // The tile's north edge, in Kyiv's tile row.
        assert!((west[1] - 50.401_515).abs() < 1e-6, "{}", west[1]);
    }

    #[test]
    fn a_missing_layer_answers_nothing() {
        assert!(
            decode_tile_roads(&fixture(), TILE, "water")
                .unwrap()
                .is_empty()
        );
        assert!(
            decode_tile_roads(&[], TILE, ROAD_SOURCE_LAYER)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn bad_tiles_fail_closed() {
        let tile = fixture();
        for cut in [1, tile.len() / 2, tile.len() - 1] {
            assert_eq!(
                decode_tile_roads(&tile[..cut], TILE, ROAD_SOURCE_LAYER),
                Err(VectorTileError::Malformed),
                "cut at {cut}"
            );
        }
        assert_eq!(
            decode_tile_roads(&[0x1f, 0x8b, 8, 0], TILE, ROAD_SOURCE_LAYER),
            Err(VectorTileError::Compressed)
        );
        for tile in [
            TileId {
                z: 14,
                x: 1 << 14,
                y: 0,
            },
            TileId { z: 31, x: 0, y: 0 },
        ] {
            assert_eq!(
                decode_tile_roads(&[], tile, ROAD_SOURCE_LAYER),
                Err(VectorTileError::InvalidTile)
            );
        }
        // A layer that is a varint, not a message.
        assert_eq!(
            decode_tile_roads(&[0x18, 0x01], TILE, ROAD_SOURCE_LAYER),
            Err(VectorTileError::Malformed)
        );
        // A road a whole tile outside its tile.
        let far = layer(
            "roads",
            &[F {
                kind_type: 2,
                props: vec![("kind", V::S("path"))],
                lines: vec![&[(0, 0), (8193, 0)]],
            }],
        );
        assert_eq!(
            decode_tile_roads(&far, TILE, ROAD_SOURCE_LAYER),
            Err(VectorTileError::Malformed)
        );
        // An extent of zero would put every point at infinity.
        let zero_extent = delimited(3, &[delimited(1, b"roads"), key(5, 0), varint(0)].concat());
        assert_eq!(
            decode_tile_roads(&zero_extent, TILE, ROAD_SOURCE_LAYER),
            Err(VectorTileError::Malformed)
        );
    }

    #[test]
    fn tiles_build_one_graph_and_one_bad_tile_fails_it() {
        let tile = fixture();
        let graph = build_walk_graph_from_tiles(&[RoadTile {
            tile: TILE,
            bytes: &tile,
        }])
        .unwrap();
        assert!(!graph.edges.is_empty());
        assert_eq!(
            build_walk_graph_from_tiles(&[
                RoadTile {
                    tile: TILE,
                    bytes: &tile,
                },
                RoadTile {
                    tile: TILE,
                    bytes: &tile[..3],
                },
            ])
            .err(),
            Some(VectorTileError::Malformed)
        );
        let coarse = TileId {
            z: 13,
            x: 4790,
            y: 2764,
        };
        assert_eq!(
            build_walk_graph_from_tiles(&[RoadTile {
                tile: coarse,
                bytes: &tile,
            }])
            .err(),
            Some(VectorTileError::TooCoarse)
        );
    }
}
