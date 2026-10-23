//! Closed caller decoding and result projection for the existing data commands.
//! The terminal and native UI bind these named operations, not a raw dispatcher.

use crate::{elevation, point_cloud, RequestContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const REFUSED_CODE: &str = "native_data_refused";
pub const FAILED_CODE: &str = "native_data_failed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    InvalidInput,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug)]
pub struct CommandFailure {
    pub class: FailureClass,
    pub code: &'static str,
    pub message: String,
    pub remedy: String,
    pub detail: Value,
}
impl CommandFailure {
    fn invalid(message: String) -> Self {
        Self {
            class: FailureClass::InvalidInput,
            code: REFUSED_CODE,
            message,
            remedy: "Retry with the documented local source, output and sampling contract.".into(),
            detail: json!({}),
        }
    }
    fn invalid_code(code: &'static str, message: impl Into<String>) -> Self {
        let mut failure = Self::invalid(message.into());
        failure.code = code;
        failure
    }
    fn sampling(message: String) -> Self {
        Self::invalid_code("sampling_incomplete", message)
    }
    fn area(message: String) -> Self {
        Self::invalid_code("area_required", message)
    }
    pub(crate) fn failed(message: String) -> Self {
        Self { class: FailureClass::Failed, code: FAILED_CODE, message, remedy: "Resolve the named native resource or file failure, then retry the same explicit request.".into(), detail: json!({}) }
    }
}
impl From<String> for CommandFailure {
    fn from(message: String) -> Self {
        Self::invalid(message)
    }
}
impl std::fmt::Display for CommandFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for CommandFailure {}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    FullLocalDemRequired,
    MaterializedFormatBound,
    StreamGroupBound,
    PointBudgetExceeded,
    AreaTooLargeToPlan,
    Cancelled,
}
impl RefusalCode {
    fn failure(self) -> (FailureClass, &'static str) {
        match self {
            Self::FullLocalDemRequired => (FailureClass::Unavailable, "full_local_dem_required"),
            Self::MaterializedFormatBound => {
                (FailureClass::InvalidInput, "materialized_format_bound")
            }
            Self::StreamGroupBound => (FailureClass::InvalidInput, "stream_group_bound"),
            Self::PointBudgetExceeded => (FailureClass::InvalidInput, "point_budget_exceeded"),
            Self::AreaTooLargeToPlan => (FailureClass::InvalidInput, "area_too_large_to_plan"),
            Self::Cancelled => (FailureClass::Failed, "extraction_cancelled"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommandRefusal {
    pub code: RefusalCode,
    pub message: String,
    pub remedy: String,
    #[serde(flatten)]
    pub detail: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CommandOutcome {
    Completed { receipt: Value },
    Planned { receipt: Value },
    Refused { refusal: CommandRefusal },
}
impl CommandOutcome {
    /// The native command's terminal result, with its refusal vocabulary and
    /// classification owned here rather than re-decided by each CLI adapter.
    pub fn into_receipt(self) -> Result<Value, CommandFailure> {
        match self {
            Self::Completed { receipt } | Self::Planned { receipt } => Ok(receipt),
            Self::Refused { refusal } => {
                let (class, code) = refusal.code.failure();
                Err(CommandFailure {
                    class,
                    code,
                    message: refusal.message,
                    remedy: refusal.remedy,
                    detail: Value::Object(refusal.detail),
                })
            }
        }
    }
}

fn projected(outcome: impl Serialize) -> Result<CommandOutcome, CommandFailure> {
    let value = serde_json::to_value(outcome).map_err(|e| CommandFailure::failed(e.to_string()))?;
    serde_json::from_value(command_projection(value))
        .map_err(|e| CommandFailure::failed(e.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachArguments {
    source: String,
    out: String,
    #[serde(default = "x")]
    x_column: String,
    #[serde(default = "y")]
    y_column: String,
    #[serde(default = "crs")]
    source_crs: String,
    separator: Option<String>,
    common_column: Option<String>,
    #[serde(default = "none")]
    fallback: String,
}
fn x() -> String {
    "x".into()
}
fn y() -> String {
    "y".into()
}
fn crs() -> String {
    "nix_itrf2005".into()
}
fn none() -> String {
    "none".into()
}

/// The one fallback an attach may name: the provider's own source id, as its
/// evidence records it (`fallbackSource`).
pub const TERRARIUM_FALLBACK: &str = "aws_terrarium_30m";

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("native data arguments exceed the one MiB bound".into());
    }
    serde_json::from_value(value).map_err(|e| format!("invalid native data arguments: {e}"))
}

pub fn elevation_request(
    arguments: Value,
) -> Result<elevation::NativeElevationRequest, CommandFailure> {
    let input: AttachArguments = decode(arguments)?;
    if !std::path::Path::new(&input.source).is_absolute()
        || !std::path::Path::new(&input.out).is_absolute()
    {
        return Err(CommandFailure::invalid_code(
            "absolute_path_required",
            "source and out must be absolute local paths",
        ));
    }
    // The Rwanda DEM wherever it covers a whole surface group, else AWS
    // Terrarium for that group — only when asked for by name. By default a
    // Rwanda hole stays explicit and unresolved.
    let use_terrarium_fallback = match input.fallback.as_str() {
        "none" => false,
        TERRARIUM_FALLBACK => true,
        _ => {
            return Err(CommandFailure::invalid(format!(
                "fallback must be none or {TERRARIUM_FALLBACK}"
            )));
        }
    };
    let request: elevation::NativeElevationRequest = decode(json!({
        "sourcePath": input.source, "outputPath": input.out,
        "xColumn": input.x_column, "yColumn": input.y_column, "sourceCrs": input.source_crs,
        "separator": input.separator, "commonColumn": input.common_column,
        "useTerrariumFallback": use_terrarium_fallback,
    }))?;
    request.validate()?;
    Ok(request)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Numeric {
    Text(String),
    Number(serde_json::Number),
}
impl Numeric {
    fn text(self) -> String {
        match self {
            Self::Text(v) => v,
            Self::Number(v) => v.to_string(),
        }
    }
    fn finite(self, name: &str) -> Result<f64, String> {
        let v = self
            .text()
            .parse::<f64>()
            .map_err(|_| format!("{name} must be finite"))?;
        if !v.is_finite() {
            return Err(format!("{name} must be finite"));
        }
        Ok(v)
    }
    fn integer(self, name: &str) -> Result<u64, String> {
        self.text()
            .parse()
            .map_err(|_| format!("{name} must be an unsigned integer"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanArguments {
    area: Option<String>,
    bbox: Option<String>,
    mode: String,
    spacing_m: Option<Numeric>,
    seed: Option<Numeric>,
    count: Option<Numeric>,
    density_per_km2: Option<Numeric>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractArguments {
    area: Option<String>,
    bbox: Option<String>,
    mode: String,
    spacing_m: Option<Numeric>,
    seed: Option<Numeric>,
    count: Option<Numeric>,
    density_per_km2: Option<Numeric>,
    out: String,
    #[serde(default = "none")]
    fallback: String,
}

fn cloud_request(
    input: PlanArguments,
    out: Option<String>,
) -> Result<point_cloud::NativePointCloudRequest, CommandFailure> {
    if input.area.is_none() && input.bbox.is_none() {
        return Err(CommandFailure::area(
            "An elevation point cloud needs an area: pass --area or --bbox.".into(),
        ));
    }
    for path in input.area.iter().chain(out.iter()) {
        if !std::path::Path::new(path).is_absolute() {
            return Err(CommandFailure::invalid_code(
                "absolute_path_required",
                "area and out must be absolute local paths",
            ));
        }
    }
    let bbox = input
        .bbox
        .map(|raw| {
            let values = raw
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|v| !v.is_empty())
                .map(|v| {
                    v.parse::<f64>()
                        .map_err(|_| "bbox must contain four finite numbers".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            if values.len() != 4 || values.iter().any(|v| !v.is_finite()) {
                return Err("bbox must contain four finite numbers".to_string());
            }
            Ok([values[0], values[1], values[2], values[3]])
        })
        .transpose()
        .map_err(CommandFailure::area)?;
    let spacing = input
        .spacing_m
        .map(|v| v.finite("spacing_m"))
        .transpose()
        .map_err(CommandFailure::sampling)?;
    let seed = input
        .seed
        .map(|v| v.integer("seed"))
        .transpose()
        .map_err(CommandFailure::sampling)?;
    let count = input
        .count
        .map(|v| v.integer("count"))
        .transpose()
        .map_err(CommandFailure::sampling)?;
    let density = input
        .density_per_km2
        .map(|v| v.finite("density_per_km2"))
        .transpose()
        .map_err(CommandFailure::sampling)?;
    let request: point_cloud::NativePointCloudRequest = decode(json!({
        "areaPath": input.area, "bbox": bbox, "outputPath": out,
        "sampling": { "mode": input.mode, "spacingM": spacing, "seed": seed, "count": count, "densityPerKm2": density },
        "useTerrariumFallback": false,
    }))?;
    request.validate().map_err(CommandFailure::sampling)?;
    Ok(request)
}

pub fn point_cloud_plan_request(
    arguments: Value,
) -> Result<point_cloud::NativePointCloudRequest, CommandFailure> {
    cloud_request(decode(arguments)?, None)
}

pub fn point_cloud_extract_request(
    arguments: Value,
) -> Result<point_cloud::NativePointCloudRequest, CommandFailure> {
    let input: ExtractArguments = decode(arguments)?;
    if input.fallback != "none" {
        return Err(CommandFailure::invalid(
            "fallback must be none; Rwanda coverage holes remain explicit".into(),
        ));
    }
    cloud_request(
        PlanArguments {
            area: input.area,
            bbox: input.bbox,
            mode: input.mode,
            spacing_m: input.spacing_m,
            seed: input.seed,
            count: input.count,
            density_per_km2: input.density_per_km2,
        },
        Some(input.out),
    )
}

/// Command output has historically used snake_case; UI DTOs use camelCase.
/// Both projections are made here from the same typed operation outcome.
fn command_projection(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(command_projection).collect()),
        Value::Object(items) => Value::Object(
            items
                .into_iter()
                .map(|(key, value)| {
                    let mut snake = String::new();
                    for c in key.chars() {
                        if c.is_ascii_uppercase() {
                            snake.push('_');
                            snake.push(c.to_ascii_lowercase());
                        } else {
                            snake.push(c);
                        }
                    }
                    let value = if key == "renamedAttributes" {
                        value
                    } else {
                        command_projection(value)
                    };
                    (
                        if key == "evidence" {
                            "receipt".into()
                        } else {
                            snake
                        },
                        value,
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

pub fn attach(
    context: &RequestContext,
    arguments: Value,
) -> Result<CommandOutcome, CommandFailure> {
    let request = elevation_request(arguments)?;
    projected(elevation::interpolate_typed(context, request)?)
}

pub fn plan(context: &RequestContext, arguments: Value) -> Result<CommandOutcome, CommandFailure> {
    let request = point_cloud_plan_request(arguments)?;
    projected(point_cloud::plan_typed(context, request)?)
}

pub fn extract(
    context: &RequestContext,
    state: &point_cloud::NativePointCloudState,
    arguments: Value,
) -> Result<CommandOutcome, CommandFailure> {
    let request = point_cloud_extract_request(arguments)?;
    projected(point_cloud::extract_typed(context, state, request)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caller_decoders_refuse_unknown_inputs_and_fractional_integer_choices() {
        assert!(
            elevation_request(json!({"source":"/a", "out":"/b", "project":"ambient"})).is_err()
        );
        assert!(point_cloud_plan_request(
            json!({"bbox":"30,-2,31,-1", "mode":"seeded_random", "seed":"1.5", "count":"5"})
        )
        .is_err());
        assert!(point_cloud_extract_request(json!({"bbox":"30,-2,31,-1", "mode":"grid", "spacing_m":"25", "out":"/b", "fallback":"terrarium"})).is_err());
    }
    #[test]
    fn headless_bbox_plan_needs_no_dem_network_or_project() {
        fn offline() -> Result<(), String> {
            Err("network must not run".into())
        }
        let result = plan(
            &RequestContext::local_files(crate::ExecutionHost::Headless, offline),
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"grid", "spacing_m":"100"}),
        )
        .unwrap();
        let result = serde_json::to_value(result).unwrap();
        assert_eq!(result["status"], "planned");
        assert!(result["receipt"]["point_count"].as_u64().unwrap() > 0);
    }
    #[test]
    fn receipt_projection_preserves_authored_attribute_names() {
        let v = command_projection(
            json!({"evidence":{"pointCount":1,"renamedAttributes":{"PoleID":"source_PoleID"}}}),
        );
        assert_eq!(v["receipt"]["point_count"], 1);
        assert_eq!(
            v["receipt"]["renamed_attributes"]["PoleID"],
            "source_PoleID"
        );
    }
    #[test]
    fn shared_plan_admission_preserves_all_displaced_ui_sampling_refusals() {
        fn offline() -> Result<(), String> {
            panic!("invalid plan must not acquire data")
        }
        let context = RequestContext::local_files(crate::ExecutionHost::Headless, offline);
        for arguments in [
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"grid"}),
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"seeded_random", "seed":"1"}),
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"seeded_random", "seed":"1", "count":"5", "density_per_km2":"5"}),
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"seeded_random", "count":"5"}),
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"voronoi"}),
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"grid", "spacing_m":"0.1"}),
            json!({"mode":"grid", "spacing_m":"50"}),
            json!({"bbox":"30.06,-1.95,30.05,-1.94", "mode":"grid", "spacing_m":"50"}),
            json!({"bbox":"30.05,-1.95,30.06,-1.94", "mode":"grid", "spacing_m":"50", "source":"/x.csv"}),
        ] {
            assert!(
                plan(&context, arguments.clone()).is_err(),
                "accepted {arguments}"
            );
        }
    }

    #[test]
    fn flat_elevation_decoder_preserves_exact_reading_and_rwanda_only_policy() {
        let request = elevation_request(json!({"source":"/data/a.csv", "out":"/data/b.geojson", "x_column":"longitude", "y_column":"latitude", "source_crs":"wgs84_lonlat", "separator":",", "common_column":"alignment", "fallback":"none"})).unwrap();
        let expected: elevation::NativeElevationRequest = serde_json::from_value(json!({"sourcePath":"/data/a.csv", "outputPath":"/data/b.geojson", "xColumn":"longitude", "yColumn":"latitude", "sourceCrs":"wgs84_lonlat", "separator":",", "commonColumn":"alignment", "useTerrariumFallback":false})).unwrap();
        assert_eq!(request, expected);
        assert!(
            elevation_request(json!({"source":"/a", "out":"/b", "fallback":"terrarium"})).is_err()
        );
    }
    #[test]
    fn command_refusal_keeps_one_typed_classification_and_bounded_facts() {
        let outcome: CommandOutcome = serde_json::from_value(json!({"status":"refused", "refusal":{"code":"full_local_dem_required", "message":"large request", "point_count":5000, "point_limit":4000, "remedy":"Repair the component."}})).unwrap();
        let failure = outcome.into_receipt().unwrap_err();
        assert_eq!(failure.class, FailureClass::Unavailable);
        assert_eq!(failure.code, "full_local_dem_required");
        assert_eq!(failure.detail["point_count"], 5000);
        assert_eq!(failure.detail["point_limit"], 4000);
        assert_eq!(failure.remedy, "Repair the component.");
    }
}

#[cfg(test)]
#[path = "bridge_fallback_tests.rs"]
mod fallback_tests;
