//! Geometry admission: the one rule for what a DS input door accepts as a
//! geometry.
//!
//! Owner ruling, 2026-09-27: "what bigquery rejects should be rejected at our
//! system input boundary", and a polygon covering the globe "should never exist
//! in our system". Storage is increasingly Firestore-only, so the rule is held
//! at the doors, independent of storage. The kernel carries these bytes as a
//! digest-pinned snapshot (`ds-command-kernel/vendor/ds-geo-lite/geo/`).
//!
//! [`admit_geometry`] refuses, with a stable code and a sentence naming the
//! rule and the offending part:
//!
//! 1. `geometry_invalid` — everything `ST_GEOGFROMGEOJSON(json, make_valid =>
//!    FALSE)` refuses. The corpus in this file's tests records BigQuery's own
//!    answer for every case (zero-byte literal queries, 2026-09-27): a
//!    geometry object holding only `type` and `coordinates` (`geometries`);
//!    positions of exactly `[longitude, latitude]` (no altitude) with latitude
//!    in [-90, 90]; a LineString of at least two positions; polygon rings of at
//!    least four positions, closed, with at least three distinct vertices, that
//!    neither cross nor touch themselves; holes inside the exterior ring, not
//!    nested, crossing no other ring and sharing no boundary segment with one.
//! 2. `geometry_invalid`, stricter than BigQuery on three points, each named
//!    in the corpus: a longitude outside [-180, 180] (BigQuery wraps it; every
//!    DS consumer is planar), a type name in the wrong case (BigQuery reads
//!    `point`; RFC 7946 and every DS consumer do not), and a zero-width ring
//!    that doubles back along the equator or a meridian (BigQuery collapses it
//!    into a line; a polygon that encloses nothing is not a polygon).
//! 3. `polygon_covers_hemisphere` — a polygon whose outer ring spans half the
//!    globe or more in both longitude (>= 180°) and latitude (>= 90°): the
//!    inverted or full-globe polygon BigQuery exports for a ring that runs the
//!    wrong way. BigQuery accepts it; DS never holds it. Same rule as
//!    ds-tile-runtime's `covers_a_hemisphere`: lines, points, polar strips and
//!    multipolygon parts spread across the world are admitted.
//!
//! Where BigQuery normalises instead of refusing, this admits too: consecutive
//! duplicate vertices, a LineString whose positions are all equal, a
//! self-crossing LineString, empty Multi* geometries and GeometryCollections,
//! holes touching the exterior ring or each other at single points, and
//! overlapping, touching or nested MultiPolygon parts (BigQuery unions them).
//!
//! BigQuery reads a GeoJSON edge as planar and densifies it into geodesics, so
//! every test here is planar in longitude/latitude, on exact orientation
//! predicates. Self-intersection is a sweep over x with y pruning, not a
//! comparison of every edge pair.

use geo::line_intersection::{LineIntersection, line_intersection};
use geo::{Coord, Kernel, Line, Orientation, kernels::RobustKernel};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fmt;

/// A geometry BigQuery refuses, or one DS refuses on the same grounds.
pub const GEOMETRY_INVALID: &str = "geometry_invalid";
/// A polygon covering half the globe or more: inverted or full-globe.
pub const POLYGON_COVERS_HEMISPHERE: &str = "polygon_covers_hemisphere";
/// How many refused features a [`FeatureRefusals`] names one by one.
pub const FIRST_REFUSALS: usize = 5;

/// Half the globe in longitude and in latitude.
const HEMISPHERE_DEGREES: (f64, f64) = (180.0, 90.0);
const TYPES: [&str; 7] = [
    "Point",
    "MultiPoint",
    "LineString",
    "MultiLineString",
    "Polygon",
    "MultiPolygon",
    "GeometryCollection",
];

/// Why one geometry is not admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// [`GEOMETRY_INVALID`] or [`POLYGON_COVERS_HEMISPHERE`].
    pub code: &'static str,
    /// One sentence naming the rule and the offending part.
    pub reason: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.reason)
    }
}

impl std::error::Error for Refusal {}

fn invalid(reason: String) -> Refusal {
    Refusal {
        code: GEOMETRY_INVALID,
        reason,
    }
}

/// Admit one GeoJSON geometry object, or say why not.
///
/// The value is the geometry itself, never a Feature: a door that holds
/// features passes `feature["geometry"]`, and decides for itself whether a
/// feature without geometry is admissible (see [`admit_features`]).
pub fn admit_geometry(geometry: &Value) -> Result<(), Refusal> {
    judge(geometry, None)
}

/// Every refused feature of one set, counted, with the first
/// [`FIRST_REFUSALS`] named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureRefusals {
    /// How many features were judged.
    pub features: usize,
    /// How many of them were refused.
    pub refused: usize,
    /// The first refusals: 1-based feature number, the feature's `id` when it
    /// has one, and why.
    pub first: Vec<(usize, Option<String>, Refusal)>,
}

impl FeatureRefusals {
    /// [`POLYGON_COVERS_HEMISPHERE`] when that is the only reason named,
    /// otherwise [`GEOMETRY_INVALID`].
    pub fn code(&self) -> &'static str {
        if self
            .first
            .iter()
            .all(|(_, _, refusal)| refusal.code == POLYGON_COVERS_HEMISPHERE)
        {
            POLYGON_COVERS_HEMISPHERE
        } else {
            GEOMETRY_INVALID
        }
    }
}

impl fmt::Display for FeatureRefusals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} of {} features carry a geometry DS does not admit",
            self.refused, self.features
        )?;
        for (position, (number, id, refusal)) in self.first.iter().enumerate() {
            f.write_str(if position == 0 { ": " } else { "; " })?;
            match id {
                Some(id) => write!(f, "feature {number} (id {id}) {refusal}")?,
                None => write!(f, "feature {number} {refusal}")?,
            }
        }
        let unnamed = self.refused - self.first.len();
        if unnamed > 0 {
            write!(f, "; and {unnamed} more")?;
        }
        Ok(())
    }
}

impl std::error::Error for FeatureRefusals {}

/// Admit every feature's geometry. A feature whose `geometry` is null or
/// absent carries no geometry and is admitted; that is the door's question,
/// not this one. Returns how many features were judged.
pub fn admit_features<'a, I>(features: I) -> Result<usize, FeatureRefusals>
where
    I: IntoIterator<Item = &'a Value>,
{
    let mut refusals = FeatureRefusals {
        features: 0,
        refused: 0,
        first: Vec::new(),
    };
    for feature in features {
        refusals.features += 1;
        let Some(geometry) = feature.get("geometry").filter(|value| !value.is_null()) else {
            continue;
        };
        if let Err(refusal) = admit_geometry(geometry) {
            refusals.refused += 1;
            if refusals.first.len() < FIRST_REFUSALS {
                let id = match feature.get("id") {
                    Some(Value::String(id)) => Some(format!("{id:?}")),
                    Some(Value::Number(id)) => Some(id.to_string()),
                    _ => None,
                };
                refusals.first.push((refusals.features, id, refusal));
            }
        }
    }
    if refusals.refused == 0 {
        Ok(refusals.features)
    } else {
        Err(refusals)
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// `within` names the enclosing collection member, e.g.
/// `GeometryCollection geometry 2`.
fn judge(value: &Value, within: Option<&str>) -> Result<(), Refusal> {
    let label = within.unwrap_or("the geometry");
    let Some(object) = value.as_object() else {
        return Err(invalid(format!(
            "{label} is {}, not a GeoJSON geometry object",
            json_kind(value)
        )));
    };
    let kind = match object.get("type") {
        Some(Value::String(kind)) => kind.as_str(),
        Some(other) => {
            return Err(invalid(format!(
                "{label} has a `type` that is {}, not a string",
                json_kind(other)
            )));
        }
        None => return Err(invalid(format!("{label} has no `type` member"))),
    };
    let name = match within {
        Some(within) => format!("{within} ({kind})"),
        None => kind.to_string(),
    };
    match kind {
        "Point" => {
            position(coordinates(object, &name)?, &name, None)?;
        }
        "MultiPoint" => {
            for (index, value) in array(coordinates(object, &name)?, &name, "positions")?
                .iter()
                .enumerate()
            {
                position(value, &name, Some(index))?;
            }
        }
        "LineString" => line(coordinates(object, &name)?, &name)?,
        "MultiLineString" => {
            for (index, value) in array(coordinates(object, &name)?, &name, "lines")?
                .iter()
                .enumerate()
            {
                line(value, &format!("{name} line {index}"))?;
            }
        }
        "Polygon" => polygon(coordinates(object, &name)?, &name)?,
        "MultiPolygon" => {
            for (index, value) in array(coordinates(object, &name)?, &name, "polygons")?
                .iter()
                .enumerate()
            {
                polygon(value, &format!("{name} polygon {index}"))?;
            }
        }
        "GeometryCollection" => {
            only_members(object, &name, "geometries")?;
            let geometries = object
                .get("geometries")
                .ok_or_else(|| invalid(format!("{name} has no `geometries` member")))?;
            for (index, member) in array(geometries, &name, "geometries")?.iter().enumerate() {
                judge(member, Some(&format!("{name} geometry {index}")))?;
            }
        }
        "Feature" | "FeatureCollection" => {
            return Err(invalid(format!(
                "{label} is a {kind}; only a geometry object is admitted here"
            )));
        }
        other => {
            return Err(invalid(
                match TYPES.iter().find(|known| known.eq_ignore_ascii_case(other)) {
                    Some(known) => format!(
                        "{label} has type `{other}`; GeoJSON type names are case-sensitive \
                         and this one is spelled `{known}`"
                    ),
                    None => {
                        format!("{label} has type `{other}`, which is not a GeoJSON geometry type")
                    }
                },
            ));
        }
    }
    Ok(())
}

/// A geometry holds `type` and exactly one data member; BigQuery refuses any
/// other member (`bbox`, `crs`, foreign members).
fn only_members(object: &Map<String, Value>, name: &str, data: &str) -> Result<(), Refusal> {
    match object.keys().find(|key| *key != "type" && *key != data) {
        Some(key) => Err(invalid(format!(
            "{name} carries the member `{key}`; a geometry holds only `type` and `{data}`"
        ))),
        None => Ok(()),
    }
}

fn coordinates<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a Value, Refusal> {
    only_members(object, name, "coordinates")?;
    object
        .get("coordinates")
        .ok_or_else(|| invalid(format!("{name} has no `coordinates` member")))
}

fn array<'a>(value: &'a Value, name: &str, of: &str) -> Result<&'a Vec<Value>, Refusal> {
    value.as_array().ok_or_else(|| {
        invalid(format!(
            "{name} coordinates are {}, not an array of {of}",
            json_kind(value)
        ))
    })
}

fn position(value: &Value, owner: &str, index: Option<usize>) -> Result<Coord<f64>, Refusal> {
    let label = match index {
        Some(index) => format!("{owner} position {index}"),
        None => format!("{owner} position"),
    };
    let Some(pair) = value.as_array() else {
        return Err(invalid(format!(
            "{label} is {}, not a [longitude, latitude] array",
            json_kind(value)
        )));
    };
    if pair.len() != 2 {
        return Err(invalid(format!(
            "{label} has {} value{}; a position is exactly [longitude, latitude]{}",
            pair.len(),
            if pair.len() == 1 { "" } else { "s" },
            if pair.len() > 2 {
                " and carries no altitude"
            } else {
                ""
            }
        )));
    }
    let (Some(x), Some(y)) = (pair[0].as_f64(), pair[1].as_f64()) else {
        return Err(invalid(format!("{label} is not two numbers")));
    };
    if !x.is_finite() || !y.is_finite() {
        return Err(invalid(format!("{label} is not finite")));
    }
    if !(-90.0..=90.0).contains(&y) {
        return Err(invalid(format!(
            "{label} has latitude {}, outside [-90, 90]",
            number(y)
        )));
    }
    if !(-180.0..=180.0).contains(&x) {
        return Err(invalid(format!(
            "{label} has longitude {}, outside [-180, 180]",
            number(x)
        )));
    }
    Ok(Coord { x, y })
}

fn line(value: &Value, name: &str) -> Result<(), Refusal> {
    let positions = array(value, name, "positions")?;
    for (index, value) in positions.iter().enumerate() {
        position(value, name, Some(index))?;
    }
    if positions.len() < 2 {
        return Err(invalid(format!(
            "{name} has {} position{}; a line needs at least 2",
            positions.len(),
            if positions.len() == 1 { "" } else { "s" }
        )));
    }
    Ok(())
}

/// A coordinate as it would be written, without 300 digits for 1e308.
fn number(value: f64) -> String {
    if value.abs() >= 1e7 {
        format!("{value:e}")
    } else {
        format!("{value}")
    }
}

fn show(point: Coord<f64>) -> String {
    format!("[{}, {}]", number(point.x), number(point.y))
}

/// A computed crossing point, to a nanodegree (under a millimetre): the
/// intersection of two edges is rounded, so its last digits say nothing.
fn near(point: Coord<f64>) -> String {
    let round = |value: f64| {
        let text = format!("{value:.9}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        if text == "-0" {
            "0".to_string()
        } else {
            text.to_string()
        }
    };
    format!("[{}, {}]", round(point.x), round(point.y))
}

fn polygon(value: &Value, name: &str) -> Result<(), Refusal> {
    let rings = array(value, name, "rings")?;
    if rings.is_empty() {
        return Err(invalid(format!("{name} has no exterior ring")));
    }
    let mut parsed = Vec::with_capacity(rings.len());
    for (ring, value) in rings.iter().enumerate() {
        let label = format!("{name} ring {ring}");
        let positions = array(value, &label, "positions")?;
        let mut points = Vec::with_capacity(positions.len());
        for (index, value) in positions.iter().enumerate() {
            points.push(position(value, &label, Some(index))?);
        }
        if points.len() < 4 {
            return Err(invalid(format!(
                "{label} has {} positions; a ring needs at least 4 (three distinct vertices \
                 and the closing repeat of the first)",
                points.len()
            )));
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        if first != last {
            return Err(invalid(format!(
                "{label} is not closed: it ends at {} instead of its first position {}",
                show(last),
                show(first)
            )));
        }
        parsed.push(points);
    }
    covers_a_hemisphere(&parsed[0], name)?;
    let rings = parsed
        .into_iter()
        .enumerate()
        .map(|(ring, points)| vertices(points, name, ring))
        .collect::<Result<Vec<_>, _>>()?;
    topology(&rings, name)?;
    containment(&rings, name)
}

fn covers_a_hemisphere(outer: &[Coord<f64>], name: &str) -> Result<(), Refusal> {
    let (mut west, mut south) = (f64::INFINITY, f64::INFINITY);
    let (mut east, mut north) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in outer {
        (west, south) = (west.min(point.x), south.min(point.y));
        (east, north) = (east.max(point.x), north.max(point.y));
    }
    if east - west >= HEMISPHERE_DEGREES.0 && north - south >= HEMISPHERE_DEGREES.1 {
        return Err(Refusal {
            code: POLYGON_COVERS_HEMISPHERE,
            reason: format!(
                "{name} ring 0 spans {}° of longitude and {}° of latitude: an inverted or \
                 full-globe polygon, which BigQuery exports for a ring that runs the wrong way. \
                 DS never holds a polygon covering half the globe or more; repair the source \
                 geometry (in BigQuery: ST_GEOGFROMTEXT(ST_ASTEXT(geom), make_valid => TRUE))",
                east - west,
                north - south
            ),
        });
    }
    Ok(())
}

fn key(point: Coord<f64>) -> (u64, u64) {
    // `+ 0.0` folds -0.0 into 0.0, which `==` already treats as equal.
    ((point.x + 0.0).to_bits(), (point.y + 0.0).to_bits())
}

/// The ring's cycle of vertices: the closing repeat dropped and consecutive
/// duplicates collapsed, as BigQuery normalises them.
fn vertices(points: Vec<Coord<f64>>, name: &str, ring: usize) -> Result<Vec<Coord<f64>>, Refusal> {
    let mut cycle: Vec<Coord<f64>> = Vec::with_capacity(points.len());
    for point in &points[..points.len() - 1] {
        if cycle.last() != Some(point) {
            cycle.push(*point);
        }
    }
    while cycle.len() > 1 && cycle.first() == cycle.last() {
        cycle.pop();
    }
    let distinct = cycle
        .iter()
        .map(|point| key(*point))
        .collect::<HashSet<_>>()
        .len();
    if distinct < 3 {
        return Err(invalid(format!(
            "{name} ring {ring} has {distinct} distinct vertices; a ring needs at least 3"
        )));
    }
    Ok(cycle)
}

struct Edge {
    line: Line<f64>,
    ring: usize,
    index: usize,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

/// No ring crosses or touches itself; no two rings cross or share a boundary
/// segment. Rings of one polygon may touch at single points.
fn topology(rings: &[Vec<Coord<f64>>], name: &str) -> Result<(), Refusal> {
    let mut edges = Vec::with_capacity(rings.iter().map(Vec::len).sum());
    for (ring, cycle) in rings.iter().enumerate() {
        for index in 0..cycle.len() {
            let (start, end) = (cycle[index], cycle[(index + 1) % cycle.len()]);
            edges.push(Edge {
                line: Line::new(start, end),
                ring,
                index,
                min_x: start.x.min(end.x),
                max_x: start.x.max(end.x),
                min_y: start.y.min(end.y),
                max_y: start.y.max(end.y),
            });
        }
    }
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_unstable_by(|a, b| edges[*a].min_x.total_cmp(&edges[*b].min_x));
    for (position, &a) in order.iter().enumerate() {
        let first = &edges[a];
        for &b in &order[position + 1..] {
            let second = &edges[b];
            if second.min_x > first.max_x {
                break;
            }
            if second.max_y < first.min_y || second.min_y > first.max_y {
                continue;
            }
            if let Some(hit) = line_intersection(first.line, second.line) {
                let (e, f) = if (first.ring, first.index) < (second.ring, second.index) {
                    (first, second)
                } else {
                    (second, first)
                };
                meeting(e, f, hit, rings, name)?;
            }
        }
    }
    Ok(())
}

fn meeting(
    e: &Edge,
    f: &Edge,
    hit: LineIntersection<f64>,
    rings: &[Vec<Coord<f64>>],
    name: &str,
) -> Result<(), Refusal> {
    let overlap = match hit {
        LineIntersection::Collinear { intersection } if intersection.start != intersection.end => {
            Some(intersection)
        }
        _ => None,
    };
    let point = match hit {
        LineIntersection::SinglePoint { intersection, .. } => intersection,
        LineIntersection::Collinear { intersection } => intersection.start,
    };
    if e.ring != f.ring {
        if let Some(shared) = overlap {
            return Err(invalid(format!(
                "{name} rings {} and {} share the boundary segment from {} to {}",
                e.ring,
                f.ring,
                show(shared.start),
                show(shared.end)
            )));
        }
        if hit.is_proper() {
            return Err(invalid(format!(
                "{name} ring {} crosses ring {} at {}",
                f.ring,
                e.ring,
                near(point)
            )));
        }
        return Ok(());
    }
    let ring = e.ring;
    if let Some(shared) = overlap {
        return Err(invalid(format!(
            "{name} ring {ring} doubles back on itself: edges {} and {} overlap from {} to {}",
            e.index,
            f.index,
            show(shared.start),
            show(shared.end)
        )));
    }
    if hit.is_proper() {
        return Err(invalid(format!(
            "{name} ring {ring} crosses itself: edges {} and {} cross at {}",
            e.index,
            f.index,
            near(point)
        )));
    }
    let count = rings[ring].len();
    if (e.index + 1) % count == f.index || (f.index + 1) % count == e.index {
        // Neighbouring edges meet at the vertex they share.
        return Ok(());
    }
    Err(invalid(format!(
        "{name} ring {ring} touches itself at {}: edges {} and {} meet there",
        show(point),
        e.index,
        f.index
    )))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Location {
    Inside,
    Outside,
    Boundary,
}

/// A ring's edges bucketed into horizontal bands, so locating a point reads
/// the edges of one band rather than the whole ring.
struct RingIndex<'a> {
    cycle: &'a [Coord<f64>],
    south: f64,
    north: f64,
    band_height: f64,
    bands: Vec<Vec<u32>>,
}

impl<'a> RingIndex<'a> {
    fn new(cycle: &'a [Coord<f64>]) -> Self {
        let south = cycle.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let north = cycle.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        let count = ((cycle.len() as f64).sqrt().ceil() as usize).max(1);
        let band_height = (north - south) / count as f64;
        let mut index = Self {
            cycle,
            south,
            north,
            band_height,
            bands: vec![Vec::new(); count],
        };
        for edge in 0..cycle.len() {
            let (a, b) = index.edge(edge);
            let (low, high) = (index.band(a.y.min(b.y)), index.band(a.y.max(b.y)));
            for band in &mut index.bands[low..=high] {
                band.push(edge as u32);
            }
        }
        index
    }

    fn edge(&self, edge: usize) -> (Coord<f64>, Coord<f64>) {
        (self.cycle[edge], self.cycle[(edge + 1) % self.cycle.len()])
    }

    fn band(&self, y: f64) -> usize {
        if self.band_height > 0.0 {
            (((y - self.south) / self.band_height) as usize).min(self.bands.len() - 1)
        } else {
            0
        }
    }

    fn locate(&self, point: Coord<f64>) -> Location {
        if point.y < self.south || point.y > self.north {
            return Location::Outside;
        }
        let mut crossings = 0_usize;
        for &edge in &self.bands[self.band(point.y)] {
            let (a, b) = self.edge(edge as usize);
            let side = RobustKernel::orient2d(a, b, point);
            if side == Orientation::Collinear
                && point.x >= a.x.min(b.x)
                && point.x <= a.x.max(b.x)
                && point.y >= a.y.min(b.y)
                && point.y <= a.y.max(b.y)
            {
                return Location::Boundary;
            }
            let upward = a.y <= point.y && b.y > point.y;
            let downward = b.y <= point.y && a.y > point.y;
            if (upward && side == Orientation::CounterClockwise)
                || (downward && side == Orientation::Clockwise)
            {
                crossings += 1;
            }
        }
        if crossings % 2 == 1 {
            Location::Inside
        } else {
            Location::Outside
        }
    }

    /// Where `cycle` lies relative to this ring, judged at its first vertex
    /// (or edge midpoint) that is not on this ring's boundary.
    fn side_of(&self, cycle: &[Coord<f64>]) -> Option<Location> {
        let midpoints = (0..cycle.len()).map(|edge| {
            let (a, b) = (cycle[edge], cycle[(edge + 1) % cycle.len()]);
            Coord {
                x: (a.x + b.x) / 2.0,
                y: (a.y + b.y) / 2.0,
            }
        });
        cycle
            .iter()
            .copied()
            .chain(midpoints)
            .map(|point| self.locate(point))
            .find(|location| *location != Location::Boundary)
    }
}

fn bounds(cycle: &[Coord<f64>]) -> [f64; 4] {
    cycle.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[w, s, e, n], p| [w.min(p.x), s.min(p.y), e.max(p.x), n.max(p.y)],
    )
}

/// Every hole lies inside the exterior ring, and no hole inside another.
/// Rings neither cross nor share a segment by now (see [`topology`]), so one
/// point off a ring's boundary says which side of it the whole ring is on.
fn containment(rings: &[Vec<Coord<f64>>], name: &str) -> Result<(), Refusal> {
    if rings.len() < 2 {
        return Ok(());
    }
    let shell = RingIndex::new(&rings[0]);
    for (hole, cycle) in rings.iter().enumerate().skip(1) {
        if shell.side_of(cycle) == Some(Location::Outside) {
            return Err(invalid(format!(
                "{name} ring {hole} is not inside the exterior ring 0; a hole must lie inside it"
            )));
        }
    }
    let mut holes: Vec<(usize, [f64; 4])> = (1..rings.len())
        .map(|ring| (ring, bounds(&rings[ring])))
        .collect();
    holes.sort_unstable_by(|a, b| a.1[0].total_cmp(&b.1[0]));
    for &(outer, [west, south, east, north]) in &holes {
        let mut index = None;
        let start = holes.partition_point(|(_, bounds)| bounds[0] < west);
        for &(inner, [w, s, e, n]) in &holes[start..] {
            if w > east {
                break;
            }
            if inner == outer || w < west || e > east || s < south || n > north {
                continue;
            }
            let index = index.get_or_insert_with(|| RingIndex::new(&rings[outer]));
            if index.side_of(&rings[inner]) == Some(Location::Inside) {
                return Err(invalid(format!(
                    "{name} ring {inner} lies inside ring {outer}; holes may not nest"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn code(geometry: &Value) -> &'static str {
        match admit_geometry(geometry) {
            Ok(()) => "admits",
            Err(refusal) => refusal.code,
        }
    }

    fn reason(geometry: Value) -> String {
        admit_geometry(&geometry).expect_err("refused").to_string()
    }

    /// BigQuery's own answers. Every geometry below was sent as a zero-byte
    /// literal query, `SELECT SAFE.ST_GEOGFROMGEOJSON('<geometry>') IS NULL`,
    /// on 2026-09-27 (project engineering-data-solutions); `bigquery` records
    /// the answer. `ds` is this module's answer. Where BigQuery accepts and DS
    /// refuses, `on_top` says why.
    const CORPUS: &str = include_str!("admission_corpus.json");

    #[test]
    fn every_corpus_case_gets_the_recorded_answer() {
        let corpus: Vec<Value> = serde_json::from_str(CORPUS).expect("corpus is JSON");
        assert!(corpus.len() >= 120, "corpus shrank to {}", corpus.len());
        let mut wrong = Vec::new();
        for case in &corpus {
            let name = case["name"].as_str().expect("name");
            let bigquery = case["bigquery"].as_str().expect("bigquery answer");
            let expected = case["ds"].as_str().expect("ds answer");
            let got = code(&case["geometry"]);
            if got != expected {
                wrong.push(format!("{name}: expected {expected}, got {got}"));
            }
            // BigQuery's refusals are the floor: DS refuses every one of them.
            if bigquery == "refuses" {
                assert_ne!(expected, "admits", "{name}: BigQuery refuses it");
            }
            // DS refuses what BigQuery accepts only for a named reason.
            let stricter = bigquery == "accepts" && expected != "admits";
            assert_eq!(
                stricter,
                case.get("on_top").is_some(),
                "{name}: `on_top` names exactly the cases DS refuses and BigQuery accepts"
            );
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn the_inverted_world_ring_bigquery_exports_is_refused() {
        let inverted = json!({"type":"Polygon","coordinates":[
            [[180,90],[-180,90],[-180,-90],[180,-90],[180,90]],
            [[30,-2],[30.1,-2],[30.1,-2.1],[30,-2.1],[30,-2]]]});
        let refusal = admit_geometry(&inverted).expect_err("refused");
        assert_eq!(refusal.code, POLYGON_COVERS_HEMISPHERE);
        assert!(
            refusal
                .reason
                .starts_with("Polygon ring 0 spans 360° of longitude and 180° of latitude"),
            "{refusal}"
        );
    }

    #[test]
    fn refusals_name_the_rule_and_the_part() {
        assert_eq!(
            reason(json!({"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1]]]})),
            "geometry_invalid: Polygon ring 0 is not closed: it ends at [0, 1] instead of its \
             first position [0, 0]"
        );
        assert_eq!(
            reason(json!({"type":"Polygon","coordinates":[[[0,0],[2,2],[2,0],[0,2],[0,0]]]})),
            "geometry_invalid: Polygon ring 0 crosses itself: edges 0 and 2 cross at [1, 1]"
        );
        assert_eq!(
            reason(json!({"type":"MultiPolygon","coordinates":[
                [[[10,10],[11,10],[11,11],[10,11],[10,10]]],
                [[[0,0],[1,0],[1,1],[0,1],[0,0]],[[2,2],[3,2],[3,3],[2,3],[2,2]]]]})),
            "geometry_invalid: MultiPolygon polygon 1 ring 1 is not inside the exterior ring 0; \
             a hole must lie inside it"
        );
        assert_eq!(
            reason(json!({"type":"GeometryCollection","geometries":[
                {"type":"Point","coordinates":[30,-1]},
                {"type":"LineString","coordinates":[[30,-1,1500],[31,-1,1500]]}]})),
            "geometry_invalid: GeometryCollection geometry 1 (LineString) position 0 has 3 \
             values; a position is exactly [longitude, latitude] and carries no altitude"
        );
        // A computed crossing is shown to a nanodegree, not to its float noise.
        assert_eq!(
            reason(json!({"type":"Polygon","coordinates":[
                [[30.0,-2.0],[30.2,-1.8],[30.2,-2.0],[30.0,-1.8],[30.0,-2.0]]]})),
            "geometry_invalid: Polygon ring 0 crosses itself: edges 0 and 2 cross at [30.1, -1.9]"
        );
        assert_eq!(
            reason(json!({"type":"Point","coordinates":[181,0]})),
            "geometry_invalid: Point position has longitude 181, outside [-180, 180]"
        );
        assert_eq!(
            reason(json!({"type":"Polygon","coordinates":[
                [[0,0],[10,0],[10,10],[0,10],[0,0]],
                [[1,1],[9,1],[9,9],[1,9],[1,1]],
                [[3,3],[5,3],[5,5],[3,5],[3,3]]]})),
            "geometry_invalid: Polygon ring 2 lies inside ring 1; holes may not nest"
        );
    }

    #[test]
    fn feature_sets_are_refused_with_a_count_and_the_first_reasons() {
        let bowtie = json!({"type":"Polygon","coordinates":[[[0,0],[2,2],[2,0],[0,2],[0,0]]]});
        let mut features = vec![
            json!({"type":"Feature","properties":{},"geometry":null}),
            json!({"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[30,-1]}}),
        ];
        for id in 0..7 {
            features.push(
                json!({"type":"Feature","id":format!("f{id}"),"properties":{},"geometry":bowtie}),
            );
        }
        assert_eq!(admit_features(&features[..2]), Ok(2));
        let refused = admit_features(&features).expect_err("refused");
        assert_eq!((refused.features, refused.refused), (9, 7));
        assert_eq!(refused.first.len(), FIRST_REFUSALS);
        assert_eq!(refused.code(), GEOMETRY_INVALID);
        let text = refused.to_string();
        assert!(
            text.starts_with(
                "7 of 9 features carry a geometry DS does not admit: feature 3 (id \"f0\") \
                 geometry_invalid: Polygon ring 0 crosses itself"
            ),
            "{text}"
        );
        assert!(text.ends_with("; and 2 more"), "{text}");
    }

    /// Self-intersection is a sweep, not a comparison of every edge pair: a
    /// 100,000-vertex boundary with a hole is judged in well under a second in
    /// release and a few seconds in debug; the pairwise test would take hours.
    #[test]
    fn a_hundred_thousand_vertex_boundary_is_judged_quickly() {
        let ring = |vertices: usize, radius: f64, clockwise: bool| -> Vec<Value> {
            let mut ring: Vec<Value> = (0..vertices)
                .map(|step| {
                    let mut angle = step as f64 / vertices as f64 * std::f64::consts::TAU;
                    if clockwise {
                        angle = -angle;
                    }
                    json!([30.0 + radius * angle.cos(), -2.0 + radius * angle.sin()])
                })
                .collect();
            ring.push(ring[0].clone());
            ring
        };
        let boundary = json!({"type":"Polygon","coordinates":[
            ring(100_000, 1.0, false), ring(10_000, 0.5, true)]});
        let started = std::time::Instant::now();
        assert_eq!(admit_geometry(&boundary), Ok(()));
        assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    }
}
