use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use ds_elevation_host::{RwandaSampler, SourceCrs};
use ds_geo::layer::{Coordinates, Layer};
use ds_geo::projection::GridModelCrs;
use ds_geo_lite::projected_tin::{
    MAX_PROJECTED_TIN_POINTS, ProjectedTerrainPoint, ProjectedTerrainTin,
    ProjectedTerrainTinOptions,
};
use ds_terrain_sampling::{MAX_ROUTE_VERTICES, MAX_SURFACE_QUERIES, RoutePoint};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    InputEvidence, MAX_INPUT_BYTES, MAX_METADATA_PATH_BYTES, SamplingFileError,
    SamplingFileRequest, SamplingSource,
};

pub(crate) struct InputRoute {
    pub route_id: String,
    pub feature_id: Option<Value>,
    pub feature_index: usize,
    pub part_index: usize,
    pub points: Vec<RoutePoint>,
    pub planned_queries: usize,
}

pub(crate) enum PreparedSurface {
    Rwanda,
    Survey(ProjectedTerrainTin),
}

pub(crate) struct Prepared {
    pub request: SamplingFileRequest,
    pub out: PathBuf,
    pub projection: GridModelCrs,
    pub routes: Vec<InputRoute>,
    pub planned_queries: usize,
    pub inputs: Vec<InputEvidence>,
    pub surface: PreparedSurface,
    pub source_evidence: Value,
    pub survey_bytes: Option<Vec<u8>>,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bounded_read(path: &Path, role: &str) -> Result<(Vec<u8>, InputEvidence), SamplingFileError> {
    let canonical =
        fs::canonicalize(path).map_err(|error| SamplingFileError::Io(error.to_string()))?;
    if canonical.as_os_str().len() > MAX_METADATA_PATH_BYTES {
        return Err(SamplingFileError::InputTooLarge(format!(
            "{role} path exceeds {MAX_METADATA_PATH_BYTES} bytes"
        )));
    }
    let file = File::open(&canonical).map_err(|error| SamplingFileError::Io(error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| SamplingFileError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(SamplingFileError::Io(format!(
            "{role} must be a regular file"
        )));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(SamplingFileError::InputTooLarge(format!(
            "{role} exceeds {MAX_INPUT_BYTES} bytes"
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| SamplingFileError::Io(error.to_string()))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(SamplingFileError::InputTooLarge(format!(
            "{role} grew beyond {MAX_INPUT_BYTES} bytes"
        )));
    }
    let evidence = InputEvidence {
        role: role.into(),
        path: canonical.to_string_lossy().into_owned(),
        bytes: bytes.len(),
        sha256: digest(&bytes),
    };
    Ok((bytes, evidence))
}

pub(crate) fn fresh_output(out: &Path) -> Result<PathBuf, SamplingFileError> {
    match fs::symlink_metadata(out) {
        Ok(_) => {
            return Err(SamplingFileError::OutputExists(
                out.to_string_lossy().into_owned(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(SamplingFileError::Io(error.to_string())),
    }
    let name = out.file_name().ok_or_else(|| {
        SamplingFileError::RequestInvalid("output must name a fresh directory".into())
    })?;
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent =
        fs::canonicalize(parent).map_err(|error| SamplingFileError::Io(error.to_string()))?;
    if !parent.is_dir() {
        return Err(SamplingFileError::Io(
            "output parent must be an existing directory".into(),
        ));
    }
    let canonical = parent.join(name);
    if canonical.as_os_str().len() > MAX_METADATA_PATH_BYTES {
        return Err(SamplingFileError::InputTooLarge(
            "output path exceeds metadata limit".into(),
        ));
    }
    match fs::symlink_metadata(&canonical) {
        Ok(_) => Err(SamplingFileError::OutputExists(
            canonical.to_string_lossy().into_owned(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(canonical),
        Err(error) => Err(SamplingFileError::Io(error.to_string())),
    }
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn lonlat(point: [f64; 2]) -> Result<[f64; 2], SamplingFileError> {
    if !point[0].is_finite()
        || !point[1].is_finite()
        || !(-180.0..=180.0).contains(&point[0])
        || !(-90.0..90.0).contains(&point[1])
    {
        return Err(SamplingFileError::SourceInvalid(
            "coordinates must be finite WGS84 longitude/latitude".into(),
        ));
    }
    Ok(point)
}

fn route_points(
    coordinates: &Coordinates,
    projection: GridModelCrs,
) -> Result<Vec<RoutePoint>, SamplingFileError> {
    let coordinates = coordinates.as_array().ok_or_else(|| {
        SamplingFileError::RequestInvalid("route line coordinates must be an array".into())
    })?;
    if coordinates.len() > MAX_ROUTE_VERTICES {
        return Err(SamplingFileError::InputTooLarge(
            "route exceeds vertex limit".into(),
        ));
    }
    coordinates
        .iter()
        .map(|coordinate| {
            let values = coordinate.as_array().ok_or_else(|| {
                SamplingFileError::RequestInvalid("route vertex must be an array".into())
            })?;
            if !(2..=3).contains(&values.len())
                || values.iter().any(|value| value.as_f64().is_none())
            {
                return Err(SamplingFileError::RequestInvalid(
                    "route vertex must contain two or three finite numbers".into(),
                ));
            }
            let xy = lonlat(coordinate.pair().ok_or_else(|| {
                SamplingFileError::RequestInvalid("invalid route coordinate".into())
            })?)?;
            let xy = projection
                .from_map(xy)
                .map_err(|error| SamplingFileError::RequestInvalid(error.to_string()))?;
            Ok(RoutePoint {
                x_m: xy[0],
                y_m: xy[1],
            })
        })
        .collect()
}

fn routes(bytes: &[u8], projection: GridModelCrs) -> Result<Vec<InputRoute>, SamplingFileError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| SamplingFileError::RequestInvalid(error.to_string()))?;
    if value.get("crs").is_some_and(|crs| !crs.is_null()) {
        return Err(SamplingFileError::RequestInvalid(
            "route must use RFC 7946 WGS84 with no legacy crs member".into(),
        ));
    }
    if !matches!(
        value["type"].as_str(),
        Some("FeatureCollection" | "Feature" | "LineString" | "MultiLineString")
    ) {
        return Err(SamplingFileError::RequestInvalid(
            "route must be GeoJSON line geometry, Feature, or FeatureCollection".into(),
        ));
    }
    let layer = Layer::import_geojson(&value).map_err(SamplingFileError::RequestInvalid)?;
    let mut routes = Vec::new();
    let mut total_vertices = 0usize;
    for (feature_index, feature) in layer.features.into_iter().enumerate() {
        let route_id = feature
            .identity()
            .unwrap_or_else(|| format!("feature-{feature_index}"));
        if route_id.len() > 128 {
            return Err(SamplingFileError::InputTooLarge(
                "route ID exceeds 128 bytes".into(),
            ));
        }
        if feature
            .id
            .as_ref()
            .is_some_and(|id| !id.is_string() && !id.is_number())
        {
            return Err(SamplingFileError::RequestInvalid(
                "GeoJSON feature id must be a string or number".into(),
            ));
        }
        let geometry = feature.geometry.ok_or_else(|| {
            SamplingFileError::RequestInvalid(
                "route feature has invalid or missing geometry".into(),
            )
        })?;
        let parts: Vec<&Coordinates> = match geometry.kind.as_str() {
            "LineString" => vec![&geometry.coordinates],
            "MultiLineString" => geometry
                .coordinates
                .as_array()
                .ok_or_else(|| {
                    SamplingFileError::RequestInvalid(
                        "MultiLineString coordinates must be an array".into(),
                    )
                })?
                .iter()
                .collect(),
            _ => {
                return Err(SamplingFileError::RequestInvalid(
                    "every route feature must be a LineString or MultiLineString".into(),
                ));
            }
        };
        if parts.is_empty() {
            return Err(SamplingFileError::RequestInvalid(
                "route geometry has no line parts".into(),
            ));
        }
        for (part_index, part) in parts.into_iter().enumerate() {
            let points = route_points(part, projection)?;
            total_vertices = total_vertices.checked_add(points.len()).ok_or_else(|| {
                SamplingFileError::InputTooLarge("route vertex count overflow".into())
            })?;
            if total_vertices > MAX_ROUTE_VERTICES {
                return Err(SamplingFileError::InputTooLarge(format!(
                    "aggregate route exceeds {MAX_ROUTE_VERTICES} vertices"
                )));
            }
            routes.push(InputRoute {
                route_id: route_id.clone(),
                feature_id: feature.id.clone(),
                feature_index,
                part_index,
                points,
                planned_queries: 0,
            });
        }
    }
    if routes.is_empty() {
        return Err(SamplingFileError::RequestInvalid(
            "route document is empty".into(),
        ));
    }
    Ok(routes)
}

fn survey_points(
    bytes: &[u8],
    source: SourceCrs,
    target: GridModelCrs,
    columns: [&str; 3],
) -> Result<Vec<ProjectedTerrainPoint>, SamplingFileError> {
    let mut reader = csv::ReaderBuilder::new().from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|error| SamplingFileError::SourceInvalid(error.to_string()))?
        .clone();
    let mut indices = [0usize; 3];
    for (i, column) in columns.iter().enumerate() {
        if column.is_empty()
            || column.len() > 128
            || columns[..i].contains(column)
            || headers.iter().filter(|header| header == column).count() != 1
        {
            return Err(SamplingFileError::SourceInvalid(format!(
                "column {column:?} must name one unique, distinct CSV header"
            )));
        }
        indices[i] = headers.iter().position(|header| header == *column).unwrap();
    }
    let mut points = Vec::new();
    for (index, row) in reader.records().enumerate() {
        if index >= MAX_PROJECTED_TIN_POINTS {
            return Err(SamplingFileError::InputTooLarge(format!(
                "survey exceeds {MAX_PROJECTED_TIN_POINTS} rows"
            )));
        }
        let row = row.map_err(|error| SamplingFileError::SourceInvalid(error.to_string()))?;
        let mut xyz = [0.0; 3];
        for (ordinate, &column) in indices.iter().enumerate() {
            xyz[ordinate] = row
                .get(column)
                .unwrap_or("")
                .trim()
                .parse::<f64>()
                .map_err(|_| {
                    SamplingFileError::SourceInvalid(format!(
                        "survey row {index} has invalid {}",
                        columns[ordinate]
                    ))
                })?;
            if !xyz[ordinate].is_finite() {
                return Err(SamplingFileError::SourceInvalid(format!(
                    "survey row {index} has nonfinite XYZ"
                )));
            }
        }
        let xy = match source {
            SourceCrs::Projected(crs) if crs == target => [xyz[0], xyz[1]],
            SourceCrs::Projected(crs) => target
                .from_map(lonlat(crs.to_map([xyz[0], xyz[1]]).map_err(|error| {
                    SamplingFileError::SourceInvalid(error.to_string())
                })?)?)
                .map_err(|error| SamplingFileError::SourceInvalid(error.to_string()))?,
            SourceCrs::Wgs84Lonlat => target
                .from_map(lonlat([xyz[0], xyz[1]])?)
                .map_err(|error| SamplingFileError::SourceInvalid(error.to_string()))?,
        };
        points.push(ProjectedTerrainPoint {
            x_m: xy[0],
            y_m: xy[1],
            z_m: xyz[2],
        });
    }
    Ok(points)
}

pub(crate) fn prepare(request_path: &Path, out: &Path) -> Result<Prepared, SamplingFileError> {
    let out = fresh_output(out)?;
    let (request_bytes, request_evidence) = bounded_read(request_path, "request")?;
    let request: SamplingFileRequest = serde_json::from_slice(&request_bytes)
        .map_err(|error| SamplingFileError::RequestInvalid(error.to_string()))?;
    let projection = GridModelCrs::parse(&request.projected_crs)
        .map_err(|error| SamplingFileError::RequestInvalid(error.to_string()))?;
    let source_crs = match &request.source {
        SamplingSource::RwandaTiff {} => None,
        SamplingSource::SurveyTin {
            source_crs,
            max_triangle_edge_m,
            breaklines,
            ..
        } => {
            if !breaklines.is_empty() {
                return Err(SamplingFileError::BreaklinesUnsupported {
                    count: breaklines.len(),
                });
            }
            if !max_triangle_edge_m.is_finite() || *max_triangle_edge_m <= 1e-6 {
                return Err(SamplingFileError::SourceInvalid(
                    "max_triangle_edge_m must be finite and greater than one micrometre".into(),
                ));
            }
            Some(
                SourceCrs::parse(source_crs)
                    .map_err(|error| SamplingFileError::SourceInvalid(error.to_string()))?,
            )
        }
    };
    let base = Path::new(&request_evidence.path).parent().unwrap();
    let (route_bytes, route_evidence) = bounded_read(&resolve(base, &request.route_file), "route")?;
    let mut routes = routes(&route_bytes, projection)?;
    let mut planned_queries = 0usize;
    for route in &mut routes {
        ds_terrain_sampling::validate_request(&route.points, &request.settings)?;
        route.planned_queries =
            ds_terrain_sampling::planned_query_count(&route.points, &request.settings)?;
        planned_queries = planned_queries
            .checked_add(route.planned_queries)
            .filter(|total| *total <= MAX_SURFACE_QUERIES)
            .ok_or_else(|| {
                SamplingFileError::InputTooLarge(format!(
                    "aggregate sampling exceeds {MAX_SURFACE_QUERIES} queries"
                ))
            })?;
    }
    let mut inputs = vec![request_evidence.clone(), route_evidence];
    let (surface, source_evidence, survey_bytes) = match &request.source {
        SamplingSource::RwandaTiff {} => (
            PreparedSurface::Rwanda,
            serde_json::to_value(RwandaSampler::source_evidence()).unwrap(),
            None,
        ),
        SamplingSource::SurveyTin {
            csv_file,
            x_column,
            y_column,
            z_column,
            max_triangle_edge_m,
            ..
        } => {
            let (bytes, evidence) = bounded_read(&resolve(base, csv_file), "declared_survey")?;
            let source = source_crs.unwrap();
            let source_definition = match source {
                SourceCrs::Wgs84Lonlat => "EPSG:4326".to_string(),
                SourceCrs::Projected(crs) => crs.canonical_definition(),
            };
            let points = survey_points(&bytes, source, projection, [x_column, y_column, z_column])?;
            let tin = ProjectedTerrainTin::build(
                &points,
                ProjectedTerrainTinOptions {
                    max_triangle_edge_m: *max_triangle_edge_m,
                },
                &[],
            )
            .map_err(|error| SamplingFileError::SourceInvalid(error.to_string()))?;
            let source_evidence = json!({
                "id": format!("declared_survey_sha256_{}", &evidence.sha256[..16]),
                "dataset": "declared local CSV terrain observations",
                "interpolation": "delaunay_piecewise_planar_barycentric_actual_xy",
                "vertical_datum": "unknown", "nominal_resolution_m": null,
                "measurement_authenticity": "unverified_declared_input",
                "source_crs": source_definition, "projected_crs": projection.canonical_definition(),
                "source_point_count": points.len(), "unique_point_count": tin.unique_point_count(),
                "supported_triangle_count": tin.triangles().len(),
                "discarded_long_edge_triangle_count": tin.discarded_long_edge_triangle_count(),
                "max_triangle_edge_m": max_triangle_edge_m,
                "raw_bytes_retained_in": "declared-survey-input.csv"
            });
            inputs.push(evidence);
            (PreparedSurface::Survey(tin), source_evidence, Some(bytes))
        }
    };
    Ok(Prepared {
        request,
        out,
        projection,
        routes,
        planned_queries,
        inputs,
        surface,
        source_evidence,
        survey_bytes,
    })
}
