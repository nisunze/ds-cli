//! Host capture and route coverage for an exact read-only native MV projection.
//! Network owns package decode, layer projection and CRS transformation.
use ds_command_kernel::project_dataset_cache::Extent;
use ds_network_client::{CapturedNetworkClient, NetworkFailure};
use ds_network_contracts::{
    engine::json_object_order, grid::NativeGridQuerySourcePin, mv_projection::*,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::{fmt, io::Read};

#[derive(Debug)]
pub enum ProjectionError {
    Input(String),
    Network(NetworkFailure),
}
impl ProjectionError {
    pub fn code(&self) -> &str {
        match self {
            Self::Input(_) => "mv_projection_input_invalid",
            Self::Network(error) => error.owner_code().unwrap_or(error.code()),
        }
    }
    pub fn unavailable(&self) -> bool {
        matches!(self, Self::Network(error) if matches!(error.code(),
            "network_engine_missing" | "network_engine_invalid" | "network_engine_incompatible" | "network_engine_unqualified"))
    }
}
impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => error.fmt(f),
            Self::Network(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ProjectionError {}
impl From<String> for ProjectionError {
    fn from(error: String) -> Self {
        Self::Input(error)
    }
}
impl From<&str> for ProjectionError {
    fn from(error: &str) -> Self {
        Self::Input(error.into())
    }
}
impl From<NetworkFailure> for ProjectionError {
    fn from(error: NetworkFailure) -> Self {
        Self::Network(error)
    }
}

/// One intentional consumer request retains one captured executable across its models.
/// Construct only after a consumer has admitted MV bytes; unused MV data probes nothing.
pub struct ProjectionSession {
    client: CapturedNetworkClient,
    stage: tempfile::TempDir,
}
impl ProjectionSession {
    /// Capture the Network engine of the request's own installed lane. A
    /// Canary print never projects its MV context with the packaged Stable
    /// engine (or the reverse); an explicit development override stays explicit.
    pub fn capture_for_lane(lane: &str) -> Result<Self, ProjectionError> {
        let client = CapturedNetworkClient::capture_for_lane(lane, 1, &|| false)?;
        let stage = ds_network_client::private_disk_stage()?;
        Ok(Self { client, stage })
    }
    pub fn project(&self, bytes: &[u8], revision: &str) -> Result<Value, ProjectionError> {
        let request = MvProjectionRequestV1 {
            schema: MV_PROJECTION_REQUEST_SCHEMA_V1.into(),
            source: NativeGridQuerySourcePin {
                package_sha256: format!("{:x}", Sha256::digest(bytes)),
                expected_revision: None,
                engineering_input_root: None,
            },
            json_object_order: json_object_order(),
            revision_id: revision.into(),
            layers: MV_PROJECTION_LAYERS.to_vec(),
        };
        Ok(self
            .client
            .mv_projection(&request, bytes, self.stage.path(), &|| false)?
            .result
            .projection)
    }
    pub fn route_extents(
        &self,
        bytes: &[u8],
        revision: &str,
        model_id: &str,
    ) -> Result<Vec<Extent>, ProjectionError> {
        let projection = self.project(bytes, revision)?;
        route_extents_in(&projection, model_id).map_err(ProjectionError::Input)
    }
}

/// One exact local draft projected once for both map capture and geographic
/// acquisition. The caller supplies and authorizes the DS project separately.
pub struct LocalMvProjection {
    pub model_id: String,
    pub revision_id: String,
    pub sha256: String,
    pub projection: Value,
    pub route_extents: Vec<Extent>,
}

pub fn load_local(path: &Path, lane: &str) -> Result<LocalMvProjection, ProjectionError> {
    if !path.is_absolute() || path.extension().and_then(|v| v.to_str()) != Some("dsgrid") {
        return Err("local MV model must be an absolute .dsgrid file".into());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 256_000_000 {
        return Err("local MV model must be a regular file of at most 256 MB".into());
    }
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || opened.len() != meta.len() {
        return Err("local MV model changed while opening".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != meta.dev() || opened.ino() != meta.ino() {
            return Err("local MV model changed while opening".into());
        }
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_MV_PROJECTION_PACKAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != opened.len() {
        return Err("local MV model changed or exceeded its byte bound during capture".into());
    }
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let model_id = format!("local-{}", &sha256[..16]);
    let revision_id = format!("local:sha256:{sha256}");
    let projection = project(&bytes, &revision_id, lane)?;
    let route_extents = route_extents_in(&projection, &model_id)?;
    Ok(LocalMvProjection {
        model_id,
        revision_id,
        sha256,
        projection,
        route_extents,
    })
}

pub fn project(bytes: &[u8], revision: &str, lane: &str) -> Result<Value, ProjectionError> {
    ProjectionSession::capture_for_lane(lane)?.project(bytes, revision)
}

/// Small route-segment footprints for project data acquisition. One long MV
/// line must not turn into a district-sized rectangular provider request.
pub fn route_extents(
    bytes: &[u8],
    revision: &str,
    model_id: &str,
    lane: &str,
) -> Result<Vec<Extent>, ProjectionError> {
    ProjectionSession::capture_for_lane(lane)?.route_extents(bytes, revision, model_id)
}

fn route_extents_in(projection: &Value, model_id: &str) -> Result<Vec<Extent>, String> {
    let features = projection["features"]
        .as_array()
        .ok_or("MV projection has no features")?;
    let mut extents = Vec::new();
    for (feature_index, feature) in features.iter().enumerate() {
        if feature["properties"]["_layer"] != "alignments" {
            continue;
        }
        let lines = match feature["geometry"]["type"].as_str() {
            Some("LineString") => vec![&feature["geometry"]["coordinates"]],
            Some("MultiLineString") => feature["geometry"]["coordinates"]
                .as_array()
                .ok_or("MV route has no coordinate array")?
                .iter()
                .collect(),
            _ => continue,
        };
        for (line_index, line) in lines.into_iter().enumerate() {
            let points = line.as_array().ok_or("MV route has invalid coordinates")?;
            for (segment_index, segment) in points.windows(2).enumerate() {
                let point = |value: &Value| -> Result<[f64; 2], String> {
                    let coords = value.as_array().ok_or("MV route point is not an array")?;
                    let lon = coords
                        .first()
                        .and_then(Value::as_f64)
                        .ok_or("MV longitude is invalid")?;
                    let lat = coords
                        .get(1)
                        .and_then(Value::as_f64)
                        .ok_or("MV latitude is invalid")?;
                    if !lon.is_finite()
                        || !lat.is_finite()
                        || !(-180.0..=180.0).contains(&lon)
                        || !(-85.0..=85.0).contains(&lat)
                    {
                        return Err("MV route coordinate is outside geographic bounds".into());
                    }
                    Ok([lon, lat])
                };
                let a = point(&segment[0])?;
                let b = point(&segment[1])?;
                if a == b {
                    continue;
                }
                extents.push(Extent {
                    name: format!("mv:{model_id}:{feature_index}:{line_index}:{segment_index}"),
                    bounds: Some([
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[0].max(b[0]),
                        a[1].max(b[1]),
                    ]),
                });
                if extents.len() > ds_command_kernel::project_dataset_cache::MAX_EXTENTS {
                    return Err("MV route has more than 5,000 acquisition segments; split the model into governed heads".into());
                }
            }
        }
    }
    Ok(extents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn mv_route_coverage_uses_segments_instead_of_one_district_envelope() {
        let projection = json!({"features":[
            {"properties":{"_layer":"alignments"},"geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.001,-2.0],[30.001,-2.001]]}},
            {"properties":{"_layer":"structures"},"geometry":{"type":"Point","coordinates":[30.0,-2.0]}}
        ]});
        let extents = route_extents_in(&projection, "model-1").unwrap();
        assert_eq!(extents.len(), 2);
        assert_eq!(extents[0].bounds, Some([30.0, -2.0, 30.001, -2.0]));
        assert_eq!(extents[1].bounds, Some([30.001, -2.001, 30.001, -2.0]));
        assert!(extents
            .iter()
            .all(|extent| extent.name.starts_with("mv:model-1:")));
    }
}
