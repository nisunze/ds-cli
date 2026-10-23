//! Typed geometry operations hosts run through the Kernel: interactive line
//! edits on the main thread and bulk profile sampling in the compute pool.
//!
//! The algorithms stay with their owners — the portable `ds-geo-lite`
//! primitives and `ds-geo`'s metric oriented extent. This module owns the
//! typed request and reply, request admission and the reply decoder; a host
//! never selects an engine export or forwards packed arguments by name.
use ds_geo_lite::measurement::{measure_frame, MeasureFrameInput};
use ds_geo_lite::operations::{self, profile_stations};
use ds_geo_lite::types::Coord;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `[longitude, latitude]` in WGS84 degrees.
pub type Position = [f64; 2];

/// The most positions one request may carry across all of its geometries.
pub const MAX_GEOMETRY_POSITIONS: usize = 1_000_000;
/// Arc segments per quarter circle a buffer may request.
pub const MAX_BUFFER_QUAD_SEGMENTS: u32 = 256;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "input", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum GeometryOperationRequest {
    /// Extend a line to meet a target polyline (CAD EXTEND).
    ExtendLine(ExtendLineInput),
    /// Trim a line where a cutting edge crosses it, keeping one side (CAD TRIM).
    TrimLine(TrimLineInput),
    /// Split a line at the point nearest to `point`.
    SplitLineAtPoint(SplitLineAtPointInput),
    /// Split a line at every crossing of a cutting line.
    SplitLineWithCutter(SplitLineWithCutterInput),
    /// Buffer a Point, LineString or Polygon (outer ring) into one ring.
    BufferGeometry(BufferGeometryInput),
    /// Offset a polyline to one side (CAD OFFSET).
    OffsetLine(OffsetLineInput),
    /// Geodesic length of a polyline.
    LineLength(LineLengthInput),
    /// Stations every `interval_m` that also keep every authored vertex.
    PointsAlongLineWithVertices(PointsAlongLineInput),
    /// Start/end/angle-point/tap stations of independently stationed lines.
    ClassifyProfileStations(ProfileStationsInput),
    /// Minimum-area metric rectangle around positions, expanded on every side.
    OrientedExtentBuffered(OrientedExtentInput),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct ExtendLineInput {
    pub line: Vec<Position>,
    pub target: Vec<Position>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct TrimLineInput {
    pub line: Vec<Position>,
    pub cutter: Vec<Position>,
    /// A position on the side of the line to keep.
    pub keep: Position,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct SplitLineAtPointInput {
    pub line: Vec<Position>,
    pub point: Position,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct SplitLineWithCutterInput {
    pub line: Vec<Position>,
    pub cutter: Vec<Position>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct BufferGeometryInput {
    /// A GeoJSON geometry. Point, LineString and Polygon (outer ring) are
    /// buffered; any other type has no buffer.
    #[cfg_attr(feature = "wasm", tsify(type = "GeoJSON.Geometry"))]
    pub geometry: Value,
    /// Buffer radius in metres; a non-positive radius has no buffer.
    pub distance_m: f64,
    /// Arc segments per quarter circle (a circle has four times as many).
    pub quad_segments: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct OffsetLineInput {
    pub line: Vec<Position>,
    /// Offset in metres; a non-positive offset has no result.
    pub distance_m: f64,
    /// Right of the line's travel direction; otherwise left.
    pub right_side: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct LineLengthInput {
    pub line: Vec<Position>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct PointsAlongLineInput {
    pub line: Vec<Position>,
    /// Spacing between regular stations in metres; finite and positive.
    pub interval_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct ProfileStationsInput {
    pub lines: Vec<Vec<Position>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct OrientedExtentInput {
    pub positions: Vec<Position>,
    /// Expansion of every side in metres.
    pub buffer_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "result", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum GeometryOperationResponse {
    ExtendLine(LineResult),
    TrimLine(LineResult),
    SplitLineAtPoint(SegmentsResult),
    SplitLineWithCutter(SegmentsResult),
    BufferGeometry(RingResult),
    OffsetLine(LineResult),
    LineLength(LineLengthResult),
    PointsAlongLineWithVertices(StationedPointsResult),
    ClassifyProfileStations(ProfileStationsResult),
    OrientedExtentBuffered(RingResult),
}

/// A resulting line, or `None` when the operation has no answer (no
/// intersection, nothing to offset).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct LineResult {
    pub line: Option<Vec<Position>>,
}

/// Two or more resulting lines, or `None` when the line cannot be split.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct SegmentsResult {
    pub segments: Option<Vec<Vec<Position>>>,
}

/// One closed ring of at least four positions, or `None` when there is none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct RingResult {
    pub ring: Option<Vec<Position>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct LineLengthResult {
    pub length_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct StationedPoint {
    pub lng: f64,
    pub lat: f64,
    /// Cumulative distance from the line start in metres.
    pub distance_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct StationedPointsResult {
    pub points: Vec<StationedPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum ProfileStationRole {
    Start,
    End,
    AnglePoint,
    Tap,
}

/// One classified station. Field names are the profile consumers' contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct ClassifiedProfileStation {
    #[serde(rename = "lineIndex")]
    pub line_index: u32,
    /// `None` for a tap projected into the middle of a receiving segment.
    #[serde(rename = "vertexIndex")]
    pub vertex_index: Option<u32>,
    pub role: ProfileStationRole,
    /// Alignment-local station; never a multi-band atlas chain distance.
    pub station_m: f64,
    pub lng: f64,
    pub lat: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct ProfileStationsResult {
    pub stations: Vec<ClassifiedProfileStation>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeometryOperationError {
    InvalidRequest(String),
    InvalidResponse(String),
}

impl std::fmt::Display for GeometryOperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "geometry_request_invalid: {message}"),
            Self::InvalidResponse(message) => write!(f, "geometry_response_invalid: {message}"),
        }
    }
}

impl std::error::Error for GeometryOperationError {}

fn invalid_request(message: impl Into<String>) -> GeometryOperationError {
    GeometryOperationError::InvalidRequest(message.into())
}

fn invalid_response(message: impl Into<String>) -> GeometryOperationError {
    GeometryOperationError::InvalidResponse(message.into())
}

fn finite_position(position: &Position) -> bool {
    position[0].is_finite() && position[1].is_finite()
}

fn admit_line(name: &str, line: &[Position]) -> Result<(), GeometryOperationError> {
    match line.iter().position(|position| !finite_position(position)) {
        Some(index) => Err(invalid_request(format!(
            "{name} position {index} is not a finite longitude/latitude pair"
        ))),
        None => Ok(()),
    }
}

fn admit_number(name: &str, value: f64) -> Result<(), GeometryOperationError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(invalid_request(format!("{name} must be a finite number")))
    }
}

/// The supported geometry's buffer kind and its positions, or `None` for a
/// type that has no buffer.
fn buffer_source(geometry: &Value) -> Result<Option<(u32, Vec<Position>)>, GeometryOperationError> {
    let kind = match geometry.get("type").and_then(Value::as_str) {
        Some("Point") => 0,
        Some("LineString") => 1,
        Some("Polygon") => 2,
        Some(_) => return Ok(None),
        None => return Err(invalid_request("geometry must be a GeoJSON geometry")),
    };
    let coordinates = geometry
        .get("coordinates")
        .ok_or_else(|| invalid_request("geometry has no coordinates"))?;
    let position = |value: &Value| -> Result<Position, GeometryOperationError> {
        let pair = value
            .as_array()
            .filter(|pair| pair.len() >= 2)
            .ok_or_else(|| invalid_request("geometry position must be an array of numbers"))?;
        let lng = pair[0].as_f64();
        let lat = pair[1].as_f64();
        match (lng, lat) {
            (Some(lng), Some(lat)) => Ok([lng, lat]),
            _ => Err(invalid_request(
                "geometry position must be an array of numbers",
            )),
        }
    };
    let positions = |value: &Value| -> Result<Vec<Position>, GeometryOperationError> {
        value
            .as_array()
            .ok_or_else(|| invalid_request("geometry coordinates must be an array"))?
            .iter()
            .map(position)
            .collect()
    };
    let positions = match kind {
        0 => vec![position(coordinates)?],
        1 => positions(coordinates)?,
        _ => match coordinates
            .as_array()
            .ok_or_else(|| invalid_request("geometry coordinates must be an array"))?
            .first()
        {
            Some(outer) => positions(outer)?,
            None => Vec::new(),
        },
    };
    Ok(Some((kind, positions)))
}

fn position_count(request: &GeometryOperationRequest) -> usize {
    use GeometryOperationRequest as Request;
    match request {
        Request::ExtendLine(input) => input.line.len() + input.target.len(),
        Request::TrimLine(input) => input.line.len() + input.cutter.len() + 1,
        Request::SplitLineAtPoint(input) => input.line.len() + 1,
        Request::SplitLineWithCutter(input) => input.line.len() + input.cutter.len(),
        Request::BufferGeometry(_) => 0,
        Request::OffsetLine(input) => input.line.len(),
        Request::LineLength(input) => input.line.len(),
        Request::PointsAlongLineWithVertices(input) => input.line.len(),
        Request::ClassifyProfileStations(input) => input.lines.iter().map(Vec::len).sum(),
        Request::OrientedExtentBuffered(input) => input.positions.len(),
    }
}

/// Request admission: finite inputs, bounded size, positive sampling interval.
pub fn validate_request(request: &GeometryOperationRequest) -> Result<(), GeometryOperationError> {
    use GeometryOperationRequest as Request;
    let count = position_count(request);
    if count > MAX_GEOMETRY_POSITIONS {
        return Err(invalid_request(format!(
            "{count} positions exceed the {MAX_GEOMETRY_POSITIONS} position limit"
        )));
    }
    match request {
        Request::ExtendLine(input) => {
            admit_line("line", &input.line)?;
            admit_line("target", &input.target)
        }
        Request::TrimLine(input) => {
            admit_line("line", &input.line)?;
            admit_line("cutter", &input.cutter)?;
            admit_line("keep", &[input.keep])
        }
        Request::SplitLineAtPoint(input) => {
            admit_line("line", &input.line)?;
            admit_line("point", &[input.point])
        }
        Request::SplitLineWithCutter(input) => {
            admit_line("line", &input.line)?;
            admit_line("cutter", &input.cutter)
        }
        Request::BufferGeometry(input) => {
            admit_number("distance_m", input.distance_m)?;
            if !(1..=MAX_BUFFER_QUAD_SEGMENTS).contains(&input.quad_segments) {
                return Err(invalid_request(format!(
                    "quad_segments must be between 1 and {MAX_BUFFER_QUAD_SEGMENTS}"
                )));
            }
            if let Some((_, positions)) = buffer_source(&input.geometry)? {
                if positions.len() > MAX_GEOMETRY_POSITIONS {
                    return Err(invalid_request(format!(
                        "{} positions exceed the {MAX_GEOMETRY_POSITIONS} position limit",
                        positions.len()
                    )));
                }
                admit_line("geometry", &positions)?;
            }
            Ok(())
        }
        Request::OffsetLine(input) => {
            admit_number("distance_m", input.distance_m)?;
            admit_line("line", &input.line)
        }
        Request::LineLength(input) => admit_line("line", &input.line),
        Request::PointsAlongLineWithVertices(input) => {
            if !(input.interval_m.is_finite() && input.interval_m > 0.0) {
                return Err(invalid_request("interval_m must be finite and positive"));
            }
            admit_line("line", &input.line)
        }
        Request::ClassifyProfileStations(input) => input
            .lines
            .iter()
            .enumerate()
            .try_for_each(|(index, line)| admit_line(&format!("lines[{index}]"), line)),
        Request::OrientedExtentBuffered(input) => {
            admit_number("buffer_m", input.buffer_m)?;
            admit_line("positions", &input.positions)
        }
    }
}

fn coords(line: &[Position]) -> Vec<Coord> {
    line.iter()
        .map(|[lng, lat]| Coord::new(*lng, *lat))
        .collect()
}

fn positions(coords: &[Coord]) -> Vec<Position> {
    coords.iter().map(|coord| [coord.lng, coord.lat]).collect()
}

fn line_result(line: Option<Vec<Coord>>, minimum: usize) -> LineResult {
    LineResult {
        line: line
            .filter(|line| line.len() >= minimum)
            .map(|line| positions(&line)),
    }
}

fn segments_result(segments: Option<Vec<Vec<Coord>>>) -> SegmentsResult {
    SegmentsResult {
        segments: segments
            .filter(|segments| segments.len() >= 2)
            .map(|segments| segments.iter().map(|segment| positions(segment)).collect()),
    }
}

fn ring_result(ring: Vec<Position>) -> RingResult {
    RingResult {
        ring: (ring.len() >= 4).then_some(ring),
    }
}

fn role(role: profile_stations::ProfileStationRole) -> ProfileStationRole {
    use profile_stations::ProfileStationRole as Engine;
    match role {
        Engine::Start => ProfileStationRole::Start,
        Engine::End => ProfileStationRole::End,
        Engine::AnglePoint => ProfileStationRole::AnglePoint,
        Engine::Tap => ProfileStationRole::Tap,
    }
}

fn index(value: usize, name: &str) -> Result<u32, GeometryOperationError> {
    u32::try_from(value).map_err(|_| invalid_request(format!("{name} exceeds the supported range")))
}

/// Run one admitted geometry operation through its owning algorithm.
pub fn execute(
    request: &GeometryOperationRequest,
) -> Result<GeometryOperationResponse, GeometryOperationError> {
    use GeometryOperationRequest as Request;
    use GeometryOperationResponse as Response;
    validate_request(request)?;
    Ok(match request {
        Request::ExtendLine(input) => Response::ExtendLine(line_result(
            operations::extend::extend_to(&coords(&input.line), &coords(&input.target)),
            1,
        )),
        Request::TrimLine(input) => Response::TrimLine(line_result(
            operations::trim::trim_at(
                &coords(&input.line),
                &coords(&input.cutter),
                Coord::new(input.keep[0], input.keep[1]),
            ),
            1,
        )),
        Request::SplitLineAtPoint(input) => Response::SplitLineAtPoint(segments_result(
            operations::split::split_at_point(
                &coords(&input.line),
                Coord::new(input.point[0], input.point[1]),
            )
            .map(|(first, second)| vec![first, second]),
        )),
        Request::SplitLineWithCutter(input) => Response::SplitLineWithCutter(segments_result(
            operations::split::split_with_line(&coords(&input.line), &coords(&input.cutter)),
        )),
        Request::BufferGeometry(input) => {
            let ring = match buffer_source(&input.geometry)? {
                Some((kind, source)) if input.distance_m > 0.0 && !source.is_empty() => {
                    let source = coords(&source);
                    match kind {
                        0 if source.len() == 1 => operations::buffer::buffer_point(
                            source[0],
                            input.distance_m,
                            input.quad_segments,
                        ),
                        0 => Vec::new(),
                        1 => operations::buffer::buffer_line(
                            &source,
                            input.distance_m,
                            input.quad_segments,
                        ),
                        _ => operations::buffer::buffer_polygon(
                            &source,
                            input.distance_m,
                            input.quad_segments,
                        ),
                    }
                }
                _ => Vec::new(),
            };
            Response::BufferGeometry(ring_result(positions(&ring)))
        }
        Request::OffsetLine(input) => {
            Response::OffsetLine(if input.distance_m > 0.0 && input.line.len() >= 2 {
                line_result(
                    Some(operations::offset::offset_line(
                        &coords(&input.line),
                        input.distance_m,
                        input.right_side,
                    )),
                    2,
                )
            } else {
                LineResult { line: None }
            })
        }
        Request::LineLength(input) => Response::LineLength(LineLengthResult {
            length_m: measure_frame(MeasureFrameInput {
                vertices: input.line.clone(),
                mode: "distance".into(),
            })
            .total_length_m,
        }),
        Request::PointsAlongLineWithVertices(input) => {
            Response::PointsAlongLineWithVertices(StationedPointsResult {
                points: operations::points_along::points_along_with_vertices(
                    &coords(&input.line),
                    input.interval_m,
                )
                .into_iter()
                .map(|point| StationedPoint {
                    lng: point.coord.lng,
                    lat: point.coord.lat,
                    distance_m: point.distance_m,
                })
                .collect(),
            })
        }
        Request::ClassifyProfileStations(input) => {
            let lines: Vec<Vec<Coord>> = input.lines.iter().map(|line| coords(line)).collect();
            let stations = profile_stations::classify_profile_stations(&lines)
                .into_iter()
                .map(|station| {
                    Ok(ClassifiedProfileStation {
                        line_index: index(station.line_index, "line index")?,
                        vertex_index: station
                            .vertex_index
                            .map(|vertex| index(vertex, "vertex index"))
                            .transpose()?,
                        role: role(station.role),
                        station_m: station.station_m,
                        lng: station.coord.lng,
                        lat: station.coord.lat,
                    })
                })
                .collect::<Result<_, GeometryOperationError>>()?;
            Response::ClassifyProfileStations(ProfileStationsResult { stations })
        }
        Request::OrientedExtentBuffered(input) => {
            let flat: Vec<f64> = input.positions.iter().flatten().copied().collect();
            let ring = ds_geo::oriented_extent::oriented_extent_buffered(&flat, input.buffer_m)
                .map_err(invalid_request)?;
            Response::OrientedExtentBuffered(ring_result(
                ring.chunks_exact(2)
                    .map(|pair| [pair[0], pair[1]])
                    .filter(finite_position)
                    .collect(),
            ))
        }
    })
}

fn check_positions(name: &str, line: &[Position]) -> Result<(), GeometryOperationError> {
    match line.iter().position(|position| !finite_position(position)) {
        Some(index) => Err(invalid_response(format!(
            "{name} position {index} is not finite"
        ))),
        None => Ok(()),
    }
}

fn check_line(line: &LineResult, minimum: usize) -> Result<(), GeometryOperationError> {
    match &line.line {
        Some(line) if line.len() < minimum => Err(invalid_response(format!(
            "line has {} positions; at least {minimum} are required",
            line.len()
        ))),
        Some(line) => check_positions("line", line),
        None => Ok(()),
    }
}

fn check_segments(result: &SegmentsResult) -> Result<(), GeometryOperationError> {
    let Some(segments) = &result.segments else {
        return Ok(());
    };
    if segments.len() < 2 {
        return Err(invalid_response("a split has at least two segments"));
    }
    for (index, segment) in segments.iter().enumerate() {
        if segment.len() < 2 {
            return Err(invalid_response(format!(
                "segment {index} has fewer than two positions"
            )));
        }
        check_positions(&format!("segment {index}"), segment)?;
    }
    Ok(())
}

fn check_ring(result: &RingResult) -> Result<(), GeometryOperationError> {
    match &result.ring {
        Some(ring) if ring.len() < 4 => Err(invalid_response("a ring has at least four positions")),
        Some(ring) => check_positions("ring", ring),
        None => Ok(()),
    }
}

fn check_points(result: &StationedPointsResult) -> Result<(), GeometryOperationError> {
    let mut previous = 0.0;
    for (index, point) in result.points.iter().enumerate() {
        if ![point.lng, point.lat, point.distance_m]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(invalid_response(format!("point {index} is not finite")));
        }
        if point.distance_m < previous {
            return Err(invalid_response(format!(
                "point {index} decreases the cumulative distance"
            )));
        }
        previous = point.distance_m;
    }
    Ok(())
}

fn check_stations(
    result: &ProfileStationsResult,
    lines: &[Vec<Position>],
) -> Result<(), GeometryOperationError> {
    for (index, station) in result.stations.iter().enumerate() {
        let Some(line) = lines.get(station.line_index as usize) else {
            return Err(invalid_response(format!(
                "station {index} names line {} of {}",
                station.line_index,
                lines.len()
            )));
        };
        if station
            .vertex_index
            .is_some_and(|vertex| vertex as usize >= line.len())
        {
            return Err(invalid_response(format!(
                "station {index} names a vertex outside its line"
            )));
        }
        if ![station.station_m, station.lng, station.lat]
            .iter()
            .all(|value| value.is_finite())
            || station.station_m < 0.0
        {
            return Err(invalid_response(format!(
                "station {index} has a non-finite or negative value"
            )));
        }
    }
    Ok(())
}

/// Accept a reply only when it answers this request with a well-formed result.
pub fn validate_response(
    request: &GeometryOperationRequest,
    response: GeometryOperationResponse,
) -> Result<GeometryOperationResponse, GeometryOperationError> {
    use GeometryOperationRequest as Request;
    use GeometryOperationResponse as Response;
    validate_request(request)?;
    match (request, &response) {
        (Request::ExtendLine(_), Response::ExtendLine(line))
        | (Request::TrimLine(_), Response::TrimLine(line)) => check_line(line, 1),
        (Request::OffsetLine(_), Response::OffsetLine(line)) => check_line(line, 2),
        (Request::SplitLineAtPoint(_), Response::SplitLineAtPoint(segments))
        | (Request::SplitLineWithCutter(_), Response::SplitLineWithCutter(segments)) => {
            check_segments(segments)
        }
        (Request::BufferGeometry(_), Response::BufferGeometry(ring))
        | (Request::OrientedExtentBuffered(_), Response::OrientedExtentBuffered(ring)) => {
            check_ring(ring)
        }
        (Request::LineLength(_), Response::LineLength(length)) => {
            if length.length_m.is_finite() && length.length_m >= 0.0 {
                Ok(())
            } else {
                Err(invalid_response("length_m must be finite and non-negative"))
            }
        }
        (
            Request::PointsAlongLineWithVertices(_),
            Response::PointsAlongLineWithVertices(points),
        ) => check_points(points),
        (Request::ClassifyProfileStations(input), Response::ClassifyProfileStations(stations)) => {
            check_stations(stations, &input.lines)
        }
        _ => Err(invalid_response(
            "reply operation does not match the request",
        )),
    }?;
    Ok(response)
}

pub fn decode_request(bytes: &[u8]) -> Result<GeometryOperationRequest, GeometryOperationError> {
    let request: GeometryOperationRequest =
        serde_json::from_slice(bytes).map_err(|error| invalid_request(error.to_string()))?;
    validate_request(&request)?;
    Ok(request)
}

pub fn decode_response(
    request: &GeometryOperationRequest,
    bytes: &[u8],
) -> Result<GeometryOperationResponse, GeometryOperationError> {
    validate_response(
        request,
        serde_json::from_slice(bytes).map_err(|error| invalid_response(error.to_string()))?,
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn request(
        input: tsify::Ts<GeometryOperationRequest>,
    ) -> Result<GeometryOperationRequest, JsError> {
        input
            .to_rust()
            .map_err(|error| JsError::new(&invalid_request(error.to_string()).to_string()))
    }

    #[wasm_bindgen(js_name = geometryOperation)]
    pub fn run(
        input: tsify::Ts<GeometryOperationRequest>,
    ) -> Result<tsify::Ts<GeometryOperationResponse>, JsError> {
        execute(&request(input)?)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = decodeGeometryOperation)]
    pub fn decode(
        input: tsify::Ts<GeometryOperationRequest>,
        result: tsify::Ts<GeometryOperationResponse>,
    ) -> Result<tsify::Ts<GeometryOperationResponse>, JsError> {
        let request = request(input)?;
        let response = result
            .to_rust()
            .map_err(|error| JsError::new(&invalid_response(error.to_string()).to_string()))?;
        validate_response(&request, response)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
