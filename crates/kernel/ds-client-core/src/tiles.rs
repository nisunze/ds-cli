use serde::{Deserialize, Serialize};

use crate::{ClientError, ErrorKind, TransportResponse};

const MAX_TILE_LAYERS: usize = 10_000;
const MAX_TEXT_CHARS: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TileType {
    Survey,
    Design,
}

impl TileType {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Survey => "survey",
            Self::Design => "design",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileOperationStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed,
    Skipped,
    Stale,
    Started,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TileOperationResult {
    project: String,
    tile_type: TileType,
    status: TileOperationStatus,
    phase: Option<String>,
    message: String,
    started_at: Option<u64>,
    failed_at: Option<u64>,
    tiled_at: Option<u64>,
    skipped_at: Option<u64>,
    elapsed_seconds: Option<f64>,
    total_features: Option<u64>,
    triggered_by: Option<String>,
    last_error: Option<String>,
    reason: Option<String>,
    cache_available: Option<bool>,
    cleanup_pending: Option<bool>,
    dirty: Option<bool>,
    force: Option<bool>,
    needs_tiling: Option<bool>,
    will_dispatch: Option<bool>,
    decision: Option<String>,
}

impl TileOperationResult {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub const fn tile_type(&self) -> TileType {
        self.tile_type
    }
    pub const fn status(&self) -> TileOperationStatus {
        self.status
    }
    pub fn phase(&self) -> Option<&str> {
        self.phase.as_deref()
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub const fn started_at(&self) -> Option<u64> {
        self.started_at
    }
    pub const fn failed_at(&self) -> Option<u64> {
        self.failed_at
    }
    pub const fn tiled_at(&self) -> Option<u64> {
        self.tiled_at
    }
    pub const fn skipped_at(&self) -> Option<u64> {
        self.skipped_at
    }
    pub const fn elapsed_seconds(&self) -> Option<f64> {
        self.elapsed_seconds
    }
    pub const fn total_features(&self) -> Option<u64> {
        self.total_features
    }
    pub fn triggered_by(&self) -> Option<&str> {
        self.triggered_by.as_deref()
    }
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
    pub const fn cache_available(&self) -> Option<bool> {
        self.cache_available
    }
    pub const fn cleanup_pending(&self) -> Option<bool> {
        self.cleanup_pending
    }
    pub const fn dirty(&self) -> Option<bool> {
        self.dirty
    }
    pub const fn force(&self) -> Option<bool> {
        self.force
    }
    pub const fn needs_tiling(&self) -> Option<bool> {
        self.needs_tiling
    }
    pub const fn will_dispatch(&self) -> Option<bool> {
        self.will_dispatch
    }
    pub fn decision(&self) -> Option<&str> {
        self.decision.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilePreflightStatus {
    Ready,
    Empty,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TilePreflightLayer {
    table: String,
    #[serde(default)]
    table_type: Option<String>,
    row_count: u64,
    geometry_count: u64,
    #[serde(default)]
    geometry_column: Option<String>,
    empty: bool,
    dashed: bool,
    #[serde(default)]
    error: Option<String>,
}

impl TilePreflightLayer {
    pub fn table(&self) -> &str {
        &self.table
    }
    pub fn table_type(&self) -> Option<&str> {
        self.table_type.as_deref()
    }
    pub const fn row_count(&self) -> u64 {
        self.row_count
    }
    pub const fn geometry_count(&self) -> u64 {
        self.geometry_count
    }
    pub fn geometry_column(&self) -> Option<&str> {
        self.geometry_column.as_deref()
    }
    pub const fn empty(&self) -> bool {
        self.empty
    }
    pub const fn dashed(&self) -> bool {
        self.dashed
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilePreflight {
    project: String,
    tile_type: TileType,
    status: TilePreflightStatus,
    expected_layer_count: usize,
    will_export_layers: usize,
    total_rows: u64,
    total_geometries: u64,
    layers: Vec<TilePreflightLayer>,
    empty_layers: Vec<String>,
    errors: Vec<String>,
    warnings: Vec<String>,
    message: String,
    repair_required: bool,
    projection_state: Option<String>,
}

impl TilePreflight {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub const fn tile_type(&self) -> TileType {
        self.tile_type
    }
    pub const fn status(&self) -> TilePreflightStatus {
        self.status
    }
    pub const fn expected_layer_count(&self) -> usize {
        self.expected_layer_count
    }
    pub const fn will_export_layers(&self) -> usize {
        self.will_export_layers
    }
    pub const fn total_rows(&self) -> u64 {
        self.total_rows
    }
    pub const fn total_geometries(&self) -> u64 {
        self.total_geometries
    }
    pub fn layers(&self) -> &[TilePreflightLayer] {
        &self.layers
    }
    pub fn empty_layers(&self) -> &[String] {
        &self.empty_layers
    }
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub const fn repair_required(&self) -> bool {
        self.repair_required
    }
    pub fn projection_state(&self) -> Option<&str> {
        self.projection_state.as_deref()
    }
}

pub(crate) enum TileOperation<'a> {
    Status(TileType),
    Preflight(TileType),
    Generate {
        tile_type: TileType,
        force: bool,
    },
    List {
        include_global: bool,
    },
    Add {
        tile_type: TileType,
        source_project: &'a str,
    },
    Remove {
        tile_id: &'a str,
        scope: TileScope,
    },
}

impl TileOperation<'_> {
    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
        use serde_json::json;
        let mut body = json!({"action": "", "eds_project_id": project});
        match self {
            Self::Status(kind) | Self::Preflight(kind) => {
                body["action"] = json!(if matches!(self, Self::Status(_)) {
                    "status"
                } else {
                    "preflight"
                });
                body["tile_type"] = json!(kind.token());
            }
            Self::Generate { tile_type, force } => {
                body["action"] = json!("generate");
                body["tile_type"] = json!(tile_type.token());
                body["force"] = json!(force);
            }
            Self::List { include_global } => {
                body["action"] = json!("list");
                body["include_global"] = json!(include_global);
            }
            Self::Add {
                tile_type,
                source_project,
            } => {
                body["action"] = json!("add");
                body["tile_type"] = json!(tile_type.token());
                body["source_project_id"] = json!(source_project);
            }
            Self::Remove { tile_id, scope } => {
                body["action"] = json!("remove");
                body["tile_id"] = json!(tile_id);
                body["scope"] = json!(scope);
            }
        }
        serde_json::to_vec(&body).expect("a closed tile request always serializes")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileScope {
    Project,
    Global,
}

/// Safe catalogue projection: storage locations and signed URLs remain private.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileCatalogEntry {
    pub id: String,
    pub name: String,
    pub scope: TileScope,
    #[serde(default)]
    pub tile_type: Option<String>,
    #[serde(default)]
    pub source_project_id: Option<String>,
    #[serde(default)]
    pub total_features: Option<u64>,
    #[serde(default)]
    #[serde(deserialize_with = "catalog_timestamp")]
    pub tiled_at: Option<u64>,
}

// Legacy tile catalogues store RFC3339 dates; newer runs store epoch
// milliseconds. Both represent the same public timestamp, never credentials.
fn catalog_timestamp<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    use serde::de::Error;
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => n.as_u64().map(Some).ok_or_else(|| {
            D::Error::custom("tile timestamp must be nonnegative epoch milliseconds")
        }),
        Some(serde_json::Value::String(s)) => {
            let date =
                time::OffsetDateTime::parse(&s, &time::format_description::well_known::Rfc3339)
                    .map_err(|_| D::Error::custom("tile timestamp must be RFC3339"))?;
            u64::try_from(date.unix_timestamp_nanos() / 1_000_000)
                .map(Some)
                .map_err(|_| D::Error::custom("tile timestamp predates the epoch"))
        }
        Some(_) => Err(D::Error::custom("invalid tile timestamp")),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TileCatalog {
    pub tiles: Vec<TileCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileMutation {
    pub tile_id: String,
    pub scope: TileScope,
}

pub(crate) fn validate_catalog_id(value: &str) -> Result<(), ClientError> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > 200
        || value
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "invalid tile catalogue identity",
        ));
    }
    Ok(())
}

fn catalog_wire(
    response: TransportResponse,
    limit: usize,
) -> Result<serde_json::Value, ClientError> {
    let bad = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "invalid tile catalogue response",
        )
    };
    if response.body.is_empty() || response.body.len() > limit {
        return Err(bad());
    }
    let wire: serde_json::Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if wire["status"] != "success" {
        return Err(bad());
    }
    Ok(wire)
}

pub(crate) fn decode_catalog(
    response: TransportResponse,
    limit: usize,
    project: &str,
    include_global: bool,
) -> Result<TileCatalog, ClientError> {
    let bad = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "invalid tile catalogue response",
        )
    };
    let wire = catalog_wire(response, limit)?;
    if wire["eds_project_id"] != project || wire.get("tiles").is_none() {
        return Err(bad());
    }
    // Go emits null for the empty catalogue.
    let rows = if wire["tiles"].is_null() {
        Vec::new()
    } else {
        serde_json::from_value::<Vec<TileCatalogEntry>>(wire["tiles"].clone()).map_err(|_| bad())?
    };
    if rows.len() > MAX_TILE_LAYERS || wire["count"].as_u64() != Some(rows.len() as u64) {
        return Err(bad());
    }
    let mut seen = std::collections::HashSet::new();
    for row in &rows {
        if validate_catalog_id(&row.id).is_err()
            || row.name.chars().count() > MAX_TEXT_CHARS
            || (!include_global && row.scope == TileScope::Global)
            || !seen.insert((row.id.clone(), format!("{:?}", row.scope)))
            || row
                .source_project_id
                .as_deref()
                .is_some_and(|id| validate_catalog_id(id).is_err())
            || row.tile_type.as_ref().is_some_and(|v| v.len() > 200)
        {
            return Err(bad());
        }
    }
    Ok(TileCatalog { tiles: rows })
}

pub(crate) fn decode_mutation(
    response: TransportResponse,
    limit: usize,
    expected_id: Option<&str>,
    scope: TileScope,
) -> Result<TileMutation, ClientError> {
    let bad = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "invalid tile mutation receipt",
        )
    };
    let wire = catalog_wire(response, limit)?;
    let result: TileMutation = serde_json::from_value(wire).map_err(|_| bad())?;
    if validate_catalog_id(&result.tile_id).is_err()
        || result.scope != scope
        || expected_id.is_some_and(|id| id != result.tile_id)
    {
        return Err(bad());
    }
    Ok(result)
}

#[derive(Deserialize)]
struct OperationWire {
    status: String,
    eds_project_id: String,
    tile_type: String,
    message: String,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    failed_at: Option<u64>,
    #[serde(default)]
    tiled_at: Option<u64>,
    #[serde(default)]
    skipped_at: Option<u64>,
    #[serde(default)]
    elapsed_seconds: Option<f64>,
    #[serde(default)]
    total_features: Option<u64>,
    #[serde(default)]
    triggered_by: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    cache_available: Option<bool>,
    #[serde(default)]
    cleanup_pending: Option<bool>,
    #[serde(default)]
    dirty: Option<bool>,
    #[serde(default)]
    force: Option<bool>,
    #[serde(default)]
    needs_tiling: Option<bool>,
    #[serde(default)]
    will_dispatch: Option<bool>,
    #[serde(default)]
    decision: Option<String>,
}

#[derive(Deserialize)]
struct PreflightWire {
    action: String,
    status: String,
    eds_project_id: String,
    tile_type: String,
    expected_layer_count: usize,
    will_export_layers: usize,
    total_rows: u64,
    total_geometries: u64,
    layers: Vec<TilePreflightLayer>,
    empty_layers: Vec<String>,
    errors: Vec<String>,
    warnings: Vec<String>,
    message: String,
    repair_required: bool,
    #[serde(default)]
    projection_state: Option<String>,
}

pub(crate) fn validate_project(project: &str) -> Result<(), ClientError> {
    crate::validate_project_id(project)
}

pub(crate) fn decode_operation(
    response: TransportResponse,
    limit: usize,
    project: &str,
    tile_type: TileType,
    allow_started: bool,
) -> Result<TileOperationResult, ClientError> {
    let wire: OperationWire = decode(response, limit)?;
    if wire.eds_project_id != project || wire.tile_type != tile_type.token() {
        return Err(unreadable());
    }
    let status = match wire.status.as_str() {
        "not_started" => TileOperationStatus::NotStarted,
        "in_progress" => TileOperationStatus::InProgress,
        "completed" => TileOperationStatus::Completed,
        "failed" => TileOperationStatus::Failed,
        "skipped" => TileOperationStatus::Skipped,
        "stale" => TileOperationStatus::Stale,
        "started" if allow_started => TileOperationStatus::Started,
        _ => return Err(unreadable()),
    };
    if wire
        .elapsed_seconds
        .is_some_and(|value| !value.is_finite() || value < 0.0)
        || !valid_texts([
            wire.phase.as_deref(),
            wire.triggered_by.as_deref(),
            wire.last_error.as_deref(),
            wire.reason.as_deref(),
            wire.decision.as_deref(),
            Some(&wire.message),
        ])
    {
        return Err(unreadable());
    }
    Ok(TileOperationResult {
        project: wire.eds_project_id,
        tile_type,
        status,
        phase: nonempty(wire.phase),
        message: wire.message,
        started_at: wire.started_at,
        failed_at: wire.failed_at,
        tiled_at: wire.tiled_at,
        skipped_at: wire.skipped_at,
        elapsed_seconds: wire.elapsed_seconds,
        total_features: wire.total_features,
        triggered_by: nonempty(wire.triggered_by),
        last_error: nonempty(wire.last_error),
        reason: nonempty(wire.reason),
        cache_available: wire.cache_available,
        cleanup_pending: wire.cleanup_pending,
        dirty: wire.dirty,
        force: wire.force,
        needs_tiling: wire.needs_tiling,
        will_dispatch: wire.will_dispatch,
        decision: nonempty(wire.decision),
    })
}

pub(crate) fn decode_preflight(
    response: TransportResponse,
    limit: usize,
    project: &str,
    tile_type: TileType,
) -> Result<TilePreflight, ClientError> {
    let wire: PreflightWire = decode(response, limit)?;
    let status = match wire.status.as_str() {
        "ready" => TilePreflightStatus::Ready,
        "empty" => TilePreflightStatus::Empty,
        "blocked" => TilePreflightStatus::Blocked,
        _ => return Err(unreadable()),
    };
    if wire.action != "preflight"
        || wire.eds_project_id != project
        || wire.tile_type != tile_type.token()
        || wire.layers.len() > MAX_TILE_LAYERS
        || wire.empty_layers.len() > MAX_TILE_LAYERS
        || wire.errors.len() > MAX_TILE_LAYERS
        || wire.warnings.len() > MAX_TILE_LAYERS
        || !bounded(&wire.message, MAX_TEXT_CHARS, true)
        || !wire.layers.iter().all(valid_layer)
        || !wire
            .empty_layers
            .iter()
            .all(|value| bounded(value, 1_024, false))
        || !wire
            .errors
            .iter()
            .chain(&wire.warnings)
            .all(|value| bounded(value, MAX_TEXT_CHARS, true))
        || !wire
            .projection_state
            .as_deref()
            .is_none_or(|value| bounded(value, 128, true))
    {
        return Err(unreadable());
    }
    Ok(TilePreflight {
        project: wire.eds_project_id,
        tile_type,
        status,
        expected_layer_count: wire.expected_layer_count,
        will_export_layers: wire.will_export_layers,
        total_rows: wire.total_rows,
        total_geometries: wire.total_geometries,
        layers: wire.layers,
        empty_layers: wire.empty_layers,
        errors: wire.errors,
        warnings: wire.warnings,
        message: wire.message,
        repair_required: wire.repair_required,
        projection_state: nonempty(wire.projection_state),
    })
}

fn decode<T: for<'de> Deserialize<'de>>(
    response: TransportResponse,
    limit: usize,
) -> Result<T, ClientError> {
    if response.body.len() > limit {
        return Err(unreadable());
    }
    serde_json::from_slice(&response.body).map_err(|_| unreadable())
}

fn valid_layer(layer: &TilePreflightLayer) -> bool {
    bounded(&layer.table, 1_024, false)
        && layer
            .table_type
            .as_deref()
            .is_none_or(|value| bounded(value, 128, false))
        && layer
            .geometry_column
            .as_deref()
            .is_none_or(|value| bounded(value, 1_024, false))
        && layer
            .error
            .as_deref()
            .is_none_or(|value| bounded(value, MAX_TEXT_CHARS, true))
}

fn valid_texts<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> bool {
    values
        .into_iter()
        .flatten()
        .all(|value| bounded(value, MAX_TEXT_CHARS, true))
}

fn bounded(value: &str, max: usize, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty())
        && value.trim() == value
        && value.chars().count() <= max
        && !value
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the response is outside the closed tile contract",
    )
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_optional_projection_state_is_not_an_auth_failure() {
        let wire = json!({"action":"preflight","status":"ready","eds_project_id":"p","tile_type":"design","expected_layer_count":0,"will_export_layers":0,"total_rows":0,"total_geometries":0,"layers":[],"empty_layers":[],"errors":[],"warnings":[],"message":"Ready","repair_required":false,"projection_state":""});
        let response = TransportResponse {
            status: 200,
            body: serde_json::to_vec(&wire).unwrap(),
        };
        assert_eq!(
            decode_preflight(response, 10000, "p", TileType::Design)
                .unwrap()
                .projection_state(),
            None
        );
        let mut wrong = wire;
        wrong["eds_project_id"] = json!("foreign");
        assert!(decode_preflight(
            TransportResponse {
                status: 200,
                body: serde_json::to_vec(&wrong).unwrap()
            },
            10000,
            "p",
            TileType::Design
        )
        .is_err());
    }

    #[test]
    fn catalogue_accepts_both_authored_timestamp_representations() {
        let mut row = json!({"id":"p.survey","name":"Survey","scope":"project","tiled_at":"2026-01-03T11:57:36.954Z"});
        let dated: TileCatalogEntry = serde_json::from_value(row.clone()).unwrap();
        row["tiled_at"] = json!(1767441456954_u64);
        assert_eq!(
            dated.tiled_at,
            serde_json::from_value::<TileCatalogEntry>(row.clone())
                .unwrap()
                .tiled_at
        );
        for bad in [
            json!(-1),
            json!(true),
            json!("yesterday"),
            json!(1.5),
            json!("1960-01-01T00:00:00Z"),
        ] {
            row["tiled_at"] = bad;
            assert!(serde_json::from_value::<TileCatalogEntry>(row.clone()).is_err());
        }
        row.as_object_mut().unwrap().remove("tiled_at");
        assert!(serde_json::from_value::<TileCatalogEntry>(row)
            .unwrap()
            .tiled_at
            .is_none());
    }
}
