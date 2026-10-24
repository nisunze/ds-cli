//! A project's survey data pipeline and published vector tiles, as the
//! Assets & Tools screens use them: the pipeline status, a migration of
//! survey entries between projects, and the project's tile publications
//! (status, list, add a shared one, remove) plus who may use a global layer.
//! Brain authorizes and records; tile generation and survey export are edge
//! compute (`ds tile generate`, `ds data export`) and have no call here.

use ds_gateway_client::Operation;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

pub const INPUT_INVALID: &str = "pipeline_input_invalid";
pub const ANSWER_INVALID: &str = "pipeline_answer_invalid";

const TEXT_MAX_CHARS: usize = 200;
const LIST_MAX: usize = 500;
const STATUS_CAP: u64 = 8 * 1024 * 1024;
const READ_MS: u64 = 60_000;
const MIGRATE_MS: u64 = 600_000;
const READ_RETRIES: u32 = 2;

/// Which survey entries a migration moves.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct MigrationFilter {
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub forms: Option<Vec<String>>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub date_from: Option<String>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub date_to: Option<String>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub surveyors: Option<Vec<String>>,
    /// Administrative units, each `{level, code}` as the boundary picker
    /// names them.
    #[serde(default)]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(optional, type = "Record<string, string>[]")
    )]
    pub admin_boundaries: Option<Vec<Map<String, Value>>>,
    /// A GeoJSON Polygon.
    #[serde(default)]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(optional, type = "Record<string, unknown>")
    )]
    pub boundary: Option<Map<String, Value>>,
}

/// One pipeline or tile action.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineCall {
    /// The project's survey sync and publication status.
    Status { project: String },
    /// Move (or, with `delete_only`, delete) survey entries; `dry_run`
    /// counts what would move.
    Migrate {
        project: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        target_project: Option<String>,
        dry_run: bool,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        delete_source: Option<bool>,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        delete_only: Option<bool>,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        overwrite: Option<bool>,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        filter: Option<MigrationFilter>,
    },
    /// One tile publication's state: `survey` or `design`.
    TileStatus { project: String, tile_type: String },
    /// The project's tile publications, and the global ones it may use.
    TileList {
        project: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        include_global: Option<bool>,
    },
    /// Show another project's published tiles in this one.
    TileAdd {
        project: String,
        tile_type: String,
        source_project: String,
    },
    TileRemove {
        project: String,
        tile_id: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        scope: Option<String>,
    },
    /// The global layers and who may use each, for their administrators.
    GlobalTileList {
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        domain: Option<String>,
    },
    /// Who may use one global layer.
    GlobalTileAccess {
        tile_id: String,
        #[cfg_attr(
            all(feature = "wasm", target_arch = "wasm32"),
            tsify(type = "Record<string, unknown>")
        )]
        access_policy: Map<String, Value>,
    },
}

fn input_invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

fn answer_invalid(what: &str) -> Refusal {
    Refusal::new(
        ANSWER_INVALID,
        format!("The pipeline service answered an invalid {what}."),
    )
}

fn text<'a>(value: &'a str, what: &str) -> Result<&'a str, Refusal> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > TEXT_MAX_CHARS
        || trimmed.chars().any(char::is_control)
    {
        return Err(input_invalid(format!(
            "The {what} is required and bounded."
        )));
    }
    Ok(trimmed)
}

fn texts(values: &[String], what: &str) -> Result<Vec<String>, Refusal> {
    if values.len() > LIST_MAX {
        return Err(input_invalid(format!("Name at most {LIST_MAX} {what}.")));
    }
    values
        .iter()
        .map(|value| text(value, what).map(str::to_owned))
        .collect()
}

fn tile_type(value: &str) -> Result<&str, Refusal> {
    match value {
        "survey" | "design" => Ok(value),
        _ => Err(input_invalid("A tile type is survey or design.")),
    }
}

fn migration(body: &mut Value, filter: &MigrationFilter) -> Result<(), Refusal> {
    if let Some(forms) = filter.forms.as_deref().filter(|forms| !forms.is_empty()) {
        body["forms"] = json!(texts(forms, "forms")?);
    }
    if let Some(from) = filter.date_from.as_deref().filter(|d| !d.trim().is_empty()) {
        body["date_from"] = json!(text(from, "start date")?);
    }
    if let Some(to) = filter.date_to.as_deref().filter(|d| !d.trim().is_empty()) {
        body["date_to"] = json!(text(to, "end date")?);
    }
    if let Some(surveyors) = filter.surveyors.as_deref().filter(|s| !s.is_empty()) {
        body["surveyors"] = json!(texts(surveyors, "surveyors")?);
    }
    if let Some(units) = filter.admin_boundaries.as_deref().filter(|u| !u.is_empty()) {
        if units.len() > LIST_MAX
            || units
                .iter()
                .any(|unit| unit.values().any(|value| !value.is_string()))
        {
            return Err(input_invalid(
                "Administrative units are named by text codes.",
            ));
        }
        body["filter_admin_boundaries"] = json!(units);
    }
    if let Some(boundary) = &filter.boundary {
        if boundary.get("type").and_then(Value::as_str) != Some("Polygon") {
            return Err(input_invalid(
                "A migration boundary is one GeoJSON Polygon.",
            ));
        }
        body["filter_boundary"] = Value::Object(boundary.clone());
    }
    Ok(())
}

impl PipelineCall {
    fn project_id(&self) -> Option<&str> {
        match self {
            Self::Status { project }
            | Self::Migrate { project, .. }
            | Self::TileStatus { project, .. }
            | Self::TileList { project, .. }
            | Self::TileAdd { project, .. }
            | Self::TileRemove { project, .. } => Some(project),
            Self::GlobalTileList { .. } | Self::GlobalTileAccess { .. } => None,
        }
    }

    fn request(&self) -> Result<Plan, Refusal> {
        let project = self.project_id().map(project_reference).transpose()?;
        let body = |action: &str, mut extra: Value| {
            extra["action"] = json!(action);
            if let Some(project) = project {
                extra["eds_project_id"] = json!(project);
            }
            extra
        };
        let read = |operation: Operation, body: Value| -> Result<Plan, Refusal> {
            Ok(Plan {
                request: Request::generated(operation, Some(body))?
                    .timeout_ms(READ_MS)
                    .response_cap(STATUS_CAP),
                retries: READ_RETRIES,
            })
        };
        let write = |operation: Operation, body: Value, timeout_ms: u64| -> Result<Plan, Refusal> {
            Ok(Plan::from(
                Request::generated(operation, Some(body))?
                    .timeout_ms(timeout_ms)
                    .response_cap(STATUS_CAP),
            ))
        };
        Ok(match self {
            Self::Status { .. } => {
                read(Operation::DataPipeline, body("pipeline_status", json!({})))?
            }
            Self::Migrate {
                target_project,
                dry_run,
                delete_source,
                delete_only,
                overwrite,
                filter,
                ..
            } => {
                let delete_only = *delete_only == Some(true);
                let mut extra = json!({"dry_run": dry_run});
                match target_project.as_deref().filter(|t| !t.trim().is_empty()) {
                    Some(target) if !delete_only => {
                        let target = project_reference(target)?;
                        if Some(target) == project {
                            return Err(input_invalid(
                                "A migration moves entries to another project.",
                            ));
                        }
                        extra["target_project_id"] = json!(target);
                    }
                    None if delete_only => {}
                    _ => {
                        return Err(input_invalid(
                            "A migration names its target project, or deletes only.",
                        ))
                    }
                }
                if delete_only {
                    extra["delete_only"] = json!(true);
                } else if *delete_source == Some(true) {
                    extra["delete_source_entries"] = json!(true);
                }
                if *overwrite == Some(true) {
                    extra["overwrite_existing"] = json!(true);
                }
                if let Some(filter) = filter {
                    migration(&mut extra, filter)?;
                }
                write(Operation::DataPipeline, body("migrate", extra), MIGRATE_MS)?
            }
            Self::TileStatus {
                tile_type: kind, ..
            } => read(
                Operation::TilesAction,
                body("status", json!({"tile_type": tile_type(kind)?})),
            )?,
            Self::TileList { include_global, .. } => {
                let mut extra = json!({});
                if *include_global == Some(true) {
                    extra["include_global"] = json!(true);
                }
                read(Operation::TilesAction, body("list", extra))?
            }
            Self::GlobalTileList { domain } => {
                let mut extra = json!({});
                match domain.as_deref().map(str::trim) {
                    None | Some("") => {}
                    Some(domain @ ("network_template" | "solar_template")) => {
                        extra["domain"] = json!(domain)
                    }
                    Some(_) => {
                        return Err(input_invalid(
                            "A global layer domain is network_template or solar_template.",
                        ))
                    }
                }
                read(Operation::TilesAction, body("global_list", extra))?
            }
            Self::TileAdd {
                tile_type: kind,
                source_project,
                ..
            } => write(
                Operation::TilesAction,
                body(
                    "add",
                    json!({"tile_type": tile_type(kind)?,
                        "source_project_id": project_reference(source_project)?}),
                ),
                READ_MS,
            )?,
            Self::TileRemove { tile_id, scope, .. } => {
                let mut extra = json!({"tile_id": text(tile_id, "tile")?});
                match scope.as_deref().map(str::trim) {
                    None | Some("") => {}
                    Some(scope @ ("project" | "global")) => extra["scope"] = json!(scope),
                    Some(_) => return Err(input_invalid("A tile scope is project or global.")),
                }
                write(Operation::TilesAction, body("remove", extra), READ_MS)?
            }
            Self::GlobalTileAccess {
                tile_id,
                access_policy,
            } => {
                if !matches!(
                    access_policy.get("mode").and_then(Value::as_str),
                    Some("all" | "restricted")
                ) {
                    return Err(input_invalid(
                        "An access policy's mode is all or restricted.",
                    ));
                }
                write(
                    Operation::TilesAction,
                    body(
                        "global_access",
                        json!({"tile_id": text(tile_id, "tile")?, "access_policy": access_policy}),
                    ),
                    READ_MS,
                )?
            }
        })
    }

    fn admitted(&self, body: Value) -> Result<Value, Refusal> {
        // A tile action reports failure in its own `status: error` member.
        if body.get("status").and_then(Value::as_str) == Some("error") {
            let message = body
                .get("error")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .unwrap_or("The tile service refused the request.");
            return Err(Refusal::new(
                ANSWER_INVALID,
                message.chars().take(500).collect::<String>(),
            ));
        }
        match self {
            Self::TileList { .. } | Self::GlobalTileList { .. } => {
                let tiles = data(&body)
                    .as_array()
                    .or_else(|| body.get("tiles").and_then(Value::as_array));
                match tiles {
                    Some(tiles) if tiles.iter().all(Value::is_object) => Ok(json!(tiles)),
                    None if body.get("tiles").is_some_and(Value::is_null) => Ok(json!([])),
                    _ => Err(answer_invalid("tile list")),
                }
            }
            _ => data(&body)
                .as_object()
                .cloned()
                .map(Value::Object)
                .ok_or_else(|| answer_invalid("answer")),
        }
    }
}

impl Exchange for PipelineCall {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        self.request()
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        self.admitted(body).map(Admitted::Done)
    }

    fn project(&self) -> Option<&str> {
        self.project_id()
    }
}

/// The typed browser export Svelte and JSON windows call; it runs the shared
/// executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod exports {
    use super::PipelineCall;
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// One pipeline or tile action; the answer is its admitted JSON document.
    #[wasm_bindgen(js_name = pipelineCall, unchecked_return_type = "any")]
    pub async fn pipeline_call(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "PipelineCall")] call: JsValue,
    ) -> Result<JsValue, JsValue> {
        let call: PipelineCall = tsify::serde_wasm_bindgen::from_value(call)
            .map_err(|error| js_sys::Error::new(&format!("Invalid pipeline call: {error}")))?;
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
