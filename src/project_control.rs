//! Deterministic Project Control decisions shared by native and browser hosts.
//!
//! The kernel decides scope, cache freshness, dirty-room preservation, bounded
//! download scheduling, report reuse, and local report projection. Hosts keep
//! credentials, HTTP, IndexedDB, filesystem writes, browser downloads, and UI.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

pub const PROJECT_CONTROL_ROOM_PLAN_SCHEMA: &str = "ds.project-control.room-plan/v1";
pub const PROJECT_CONTROL_REPORT_PLAN_SCHEMA: &str = "ds.project-control.report-plan/v1";
pub const PROJECT_CONTROL_REPORT_CATALOG_SCHEMA: &str =
    "ds.project-control.local-report-catalog/v1";
pub const MAX_PROJECT_CONTROL_INPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PROJECT_CONTROL_ROOMS: usize = 2_000;
pub const MAX_PROJECT_CONTROL_OUTBOX_ROWS: usize = 4_096;

const DEFAULT_RESIDENT_CAP: usize = 8;
const MAX_RESIDENT_CAP: usize = 64;
const DEFAULT_PARALLEL_REQUESTS: usize = 4;
const MAX_PARALLEL_REQUESTS: usize = 8;
const MAX_OUTPUTS_PER_ROW: usize = 32;
const MAX_ID_CHARS: usize = 256;
const MAX_PATH_CHARS: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectControlError(String);

impl fmt::Display for ProjectControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ProjectControlError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomPlanInput {
    project: String,
    heads: Vec<RoomHead>,
    rooms: Vec<LocalRoom>,
    #[serde(default)]
    requested: Option<Vec<String>>,
    #[serde(default)]
    force: bool,
    #[serde(default = "default_resident_cap")]
    resident_cap: usize,
    #[serde(default = "default_parallel_requests")]
    max_parallel_requests: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomHead {
    name: String,
    #[serde(default)]
    version: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalRoom {
    name: String,
    server_known: bool,
    #[serde(default)]
    local_dirty: bool,
    #[serde(default)]
    server_version: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RoomProvenance {
    Cache,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReusedRoom {
    name: String,
    provenance: RoomProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoomAvailability {
    total: usize,
    local: usize,
    current: usize,
    dirty: usize,
    stale: usize,
    missing: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct RoomMaterializationPlan {
    schema: &'static str,
    project: String,
    scope: &'static str,
    names: Vec<String>,
    force: bool,
    availability: RoomAvailability,
    reuse: Vec<ReusedRoom>,
    fetch: Vec<String>,
    fetch_batches: Vec<Vec<String>>,
    request_concurrency: usize,
    batch_width: usize,
    resident_cap: usize,
}

fn default_resident_cap() -> usize {
    DEFAULT_RESIDENT_CAP
}

fn default_parallel_requests() -> usize {
    DEFAULT_PARALLEL_REQUESTS
}

/// Decide which transformer rooms are current, dirty, stale, missing, or need
/// transfer. The returned batches keep the maximum simultaneously resident
/// room count at or below `resident_cap`, even when the host runs every
/// admitted request concurrently.
pub fn plan_project_control_rooms(input_json: &[u8]) -> Result<String, ProjectControlError> {
    let input: RoomPlanInput = decode(input_json, "room-plan")?;
    bounded_text(&input.project, "project", MAX_ID_CHARS)?;
    if input.heads.len() > MAX_PROJECT_CONTROL_ROOMS
        || input.rooms.len() > MAX_PROJECT_CONTROL_ROOMS
    {
        return Err(error("room-plan exceeds the 2000-room bound"));
    }
    if !(1..=MAX_RESIDENT_CAP).contains(&input.resident_cap) {
        return Err(error("resident_cap must be in 1..=64"));
    }
    if !(1..=MAX_PARALLEL_REQUESTS).contains(&input.max_parallel_requests) {
        return Err(error("max_parallel_requests must be in 1..=8"));
    }

    let mut all_heads = BTreeMap::<String, RoomHead>::new();
    let mut roster = Vec::new();
    for head in input.heads {
        bounded_text(&head.name, "head.name", MAX_ID_CHARS)?;
        if head.version == Some(0) {
            return Err(error("head.version must be positive when supplied"));
        }
        if all_heads.contains_key(&head.name) {
            return Err(error("room-plan contains a duplicate server head"));
        }
        if !is_special_status_row(&head.name) {
            roster.push(head.name.clone());
        }
        all_heads.insert(head.name.clone(), head);
    }

    let mut rooms = BTreeMap::<String, LocalRoom>::new();
    for room in input.rooms {
        bounded_text(&room.name, "room.name", MAX_ID_CHARS)?;
        if room.server_version == Some(0) {
            return Err(error("room.server_version must be positive when supplied"));
        }
        if rooms.insert(room.name.clone(), room).is_some() {
            return Err(error("room-plan contains duplicate local room metadata"));
        }
    }

    let availability = availability(&roster, &all_heads, &rooms);
    let (scope, names) = match input.requested {
        None => ("all_active", roster),
        Some(requested) => {
            if requested.is_empty() || requested.len() > MAX_PROJECT_CONTROL_ROOMS {
                return Err(error("requested must contain 1..=2000 room names"));
            }
            let active = roster.iter().map(String::as_str).collect::<BTreeSet<_>>();
            let mut seen = BTreeSet::new();
            let mut names = Vec::new();
            for name in requested {
                bounded_text(&name, "requested room", MAX_ID_CHARS)?;
                if !active.contains(name.as_str()) {
                    return Err(error("a requested room is not an active transformer"));
                }
                if seen.insert(name.clone()) {
                    names.push(name);
                }
            }
            ("explicit", names)
        }
    };

    let mut reuse = Vec::new();
    let mut fetch = Vec::new();
    for name in &names {
        let room = rooms.get(name);
        if room.is_some_and(|room| room.local_dirty) {
            reuse.push(ReusedRoom {
                name: name.clone(),
                provenance: RoomProvenance::Local,
            });
        } else if input.force || !room_is_current(all_heads.get(name), room) {
            fetch.push(name.clone());
        } else {
            reuse.push(ReusedRoom {
                name: name.clone(),
                provenance: RoomProvenance::Cache,
            });
        }
    }

    let request_concurrency = fetch
        .len()
        .min(input.max_parallel_requests)
        .min(input.resident_cap);
    let batch_width = input
        .resident_cap
        .checked_div(request_concurrency)
        .unwrap_or(0);
    let fetch_batches = if batch_width == 0 {
        Vec::new()
    } else {
        fetch.chunks(batch_width).map(<[String]>::to_vec).collect()
    };

    encode(&RoomMaterializationPlan {
        schema: PROJECT_CONTROL_ROOM_PLAN_SCHEMA,
        project: input.project,
        scope,
        names,
        force: input.force,
        availability,
        reuse,
        fetch,
        fetch_batches,
        request_concurrency,
        batch_width,
        resident_cap: input.resident_cap,
    })
}

fn is_special_status_row(name: &str) -> bool {
    let canonical = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    matches!(
        canonical.as_str(),
        "compounded_report"
            | "combined_transformer"
            | "all_transformers"
            | "mv_data"
            | "collisions"
    )
}

fn availability(
    roster: &[String],
    heads: &BTreeMap<String, RoomHead>,
    rooms: &BTreeMap<String, LocalRoom>,
) -> RoomAvailability {
    let mut local = 0;
    let mut current = 0;
    let mut dirty = 0;
    for name in roster {
        let Some(room) = rooms.get(name) else {
            continue;
        };
        local += 1;
        if room.local_dirty {
            dirty += 1;
        } else if room_is_current(heads.get(name), Some(room)) {
            current += 1;
        }
    }
    RoomAvailability {
        total: roster.len(),
        local,
        current,
        dirty,
        stale: local - current - dirty,
        missing: roster.len() - local,
    }
}

fn room_is_current(head: Option<&RoomHead>, room: Option<&LocalRoom>) -> bool {
    let (Some(head), Some(room)) = (head, room) else {
        return false;
    };
    room.server_known
        && head.version.is_some()
        && head.version == room.server_version
        && !room.local_dirty
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportPlanInput {
    existing: Vec<String>,
    #[serde(default)]
    report_state: Option<String>,
    report_has_files: bool,
    #[serde(default)]
    refresh_if_stale: bool,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Serialize)]
struct ReportDownloadPlan {
    schema: &'static str,
    action: &'static str,
    reason: &'static str,
    existing: Vec<String>,
    fresh: bool,
}

/// Decide whether a report download may reuse already-listed artifacts or must
/// refresh them. The host performs the chosen I/O and never reinterprets it.
pub fn plan_project_control_report_download(
    input_json: &[u8],
) -> Result<String, ProjectControlError> {
    let input: ReportPlanInput = decode(input_json, "report-plan")?;
    if input.existing.len() > 128 {
        return Err(error("report-plan contains more than 128 existing files"));
    }
    let mut seen = BTreeSet::new();
    let mut existing = Vec::new();
    for value in input.existing {
        bounded_text(&value, "existing report file", MAX_PATH_CHARS)?;
        if seen.insert(value.clone()) {
            existing.push(value);
        }
    }
    let fresh = input.report_state.as_deref() == Some("success") && input.report_has_files;
    let (action, reason) = if input.force {
        ("refresh", "forced")
    } else if existing.is_empty() {
        ("refresh", "missing")
    } else if !input.refresh_if_stale {
        ("reuse", "existing_allowed")
    } else if fresh {
        ("reuse", "server_head_current")
    } else {
        ("refresh", "server_head_stale")
    };
    encode(&ReportDownloadPlan {
        schema: PROJECT_CONTROL_REPORT_PLAN_SCHEMA,
        action,
        reason,
        existing,
        fresh,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportCatalogInput {
    project: String,
    #[serde(default)]
    transformer: Option<String>,
    rows: Vec<OutboxRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutboxRow {
    id: String,
    project_id: String,
    engine: String,
    operation: String,
    variant: String,
    client_run_id: String,
    created_at: u64,
    phase: String,
    status: String,
    #[serde(default)]
    error: Option<String>,
    outputs: Vec<OutboxOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutboxOutput {
    output_id: String,
    format: String,
    content_type: String,
    #[serde(default)]
    local_path: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    expected_sha256: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    expected_size_bytes: Option<u64>,
    #[serde(default)]
    paper_size: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectedReportFile {
    output_id: String,
    filename: String,
    format: String,
    content_type: String,
    local_path: String,
    sha256: String,
    size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    paper_size: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectedReport {
    id: String,
    kind: &'static str,
    operation: String,
    variant: String,
    client_run_id: String,
    created_at: u64,
    phase: String,
    status: String,
    error: Option<String>,
    files: Vec<ProjectedReportFile>,
}

#[derive(Debug, Serialize)]
struct ProjectedReportCatalog {
    schema: &'static str,
    project: String,
    reports: Vec<ProjectedReport>,
}

/// Project the local combined/compounded report rows which Project Control may
/// offer. Artifact URLs remain a host concern; this returns only verified
/// locators and content identity.
pub fn project_project_control_local_reports(
    input_json: &[u8],
) -> Result<String, ProjectControlError> {
    let input: ReportCatalogInput = decode(input_json, "local-report-catalog")?;
    bounded_text(&input.project, "project", MAX_ID_CHARS)?;
    if input.rows.len() > MAX_PROJECT_CONTROL_OUTBOX_ROWS {
        return Err(error("local-report-catalog exceeds the 4096-row bound"));
    }
    if let Some(name) = &input.transformer {
        bounded_text(name, "transformer", MAX_ID_CHARS)?;
        if !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || is_special_status_row(name)
        {
            return Err(error(
                "local-report-catalog requires one canonical transformer",
            ));
        }
    }
    let requested_operation = input
        .transformer
        .as_ref()
        .map(|name| format!("export-{name}"));
    let mut reports = Vec::new();
    for row in input.rows {
        if row.project_id != input.project || row.engine != "network_reporter" {
            continue;
        }
        let kind = if let Some(operation) = &requested_operation {
            if &row.operation != operation {
                continue;
            }
            "transformer"
        } else {
            match row.operation.as_str() {
                "export-combined_transformer" => "combined",
                "export-compounded_report" => "compounded",
                _ => continue,
            }
        };
        for value in [
            row.id.as_str(),
            row.variant.as_str(),
            row.client_run_id.as_str(),
            row.phase.as_str(),
            row.status.as_str(),
        ] {
            bounded_text(value, "local report identity", MAX_ID_CHARS)?;
        }
        if row.outputs.len() > MAX_OUTPUTS_PER_ROW {
            return Err(error("a local report row exceeds the 32-output bound"));
        }
        let mut files = Vec::new();
        for output in row.outputs {
            if output.paper_size.as_deref().is_some_and(|paper| {
                !matches!(paper, "A0" | "A1" | "A2" | "A3" | "A4" | "A5" | "Custom")
                    || (output.format != "pdf_a0"
                        && output.format != "pdf_a3"
                        && crate::report_formats::named_print_output(&output.format).is_none())
                    || (output.format == "pdf_a0" && paper != "A0")
                    || (output.format == "pdf_a3" && paper != "A3")
            }) {
                return Err(error("local report paper metadata is invalid"));
            }
            let sha256 = output
                .sha256
                .filter(|value| !value.is_empty())
                .or(output.expected_sha256)
                .unwrap_or_default();
            let size_bytes = output
                .size_bytes
                .filter(|size| *size > 0)
                .or(output.expected_size_bytes)
                .unwrap_or(0);
            let Some(local_path) = output.local_path else {
                continue;
            };
            if !valid_sha256(&sha256) || size_bytes == 0 {
                continue;
            }
            bounded_text(&output.output_id, "output_id", MAX_ID_CHARS)?;
            bounded_text(&output.format, "output format", MAX_ID_CHARS)?;
            bounded_text(&output.content_type, "output content_type", MAX_ID_CHARS)?;
            bounded_text(&local_path, "local report locator", MAX_PATH_CHARS)?;
            files.push(ProjectedReportFile {
                filename: if let Some(transformer) = &input.transformer {
                    let definition = crate::report_formats::output(&output.output_id)
                        .map_err(|_| error("unknown transformer report output"))?;
                    if definition.content_type != output.content_type
                        || definition.format != output.format
                    {
                        return Err(error("transformer report output metadata mismatch"));
                    }
                    format!("{transformer}{}", definition.suffix)
                } else {
                    output_filename(kind, &output.output_id, &output.format)
                },
                output_id: output.output_id,
                format: output.format,
                content_type: output.content_type,
                local_path,
                sha256,
                size_bytes,
                paper_size: output.paper_size,
            });
        }
        if !files.is_empty() {
            reports.push(ProjectedReport {
                id: row.id,
                kind,
                operation: row.operation,
                variant: row.variant,
                client_run_id: row.client_run_id,
                created_at: row.created_at,
                phase: row.phase,
                status: row.status,
                error: row.error,
                files,
            });
        }
    }
    reports.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    encode(&ProjectedReportCatalog {
        schema: PROJECT_CONTROL_REPORT_CATALOG_SCHEMA,
        project: input.project,
        reports,
    })
}

fn output_filename(kind: &str, output_id: &str, format: &str) -> String {
    if kind == "compounded" {
        return "compounded_reports.zip".to_string();
    }
    if let Ok(output) = crate::report_formats::output(output_id) {
        if output.format == format {
            return format!("combined_transformer{}", output.suffix);
        }
    }
    match output_id {
        "shp" => "combined_transformer.shp.zip".to_string(),
        "xlsx" => "combined_transformer.xlsx".to_string(),
        "geojsonseq" => "combined_transformer_geojsonl.zip".to_string(),
        _ => format!("combined_transformer.{format}"),
    }
}

fn decode<T: for<'de> Deserialize<'de>>(
    input_json: &[u8],
    label: &str,
) -> Result<T, ProjectControlError> {
    if input_json.is_empty() || input_json.len() > MAX_PROJECT_CONTROL_INPUT_BYTES {
        return Err(error(&format!(
            "{label} input must be 1..={MAX_PROJECT_CONTROL_INPUT_BYTES} bytes"
        )));
    }
    serde_json::from_slice(input_json).map_err(|_| error(&format!("{label} input is invalid")))
}

fn encode(value: &impl Serialize) -> Result<String, ProjectControlError> {
    serde_json::to_string(value).map_err(|_| error("Project Control output cannot be encoded"))
}

fn bounded_text(value: &str, field: &str, max_chars: usize) -> Result<(), ProjectControlError> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(error(&format!("{field} is invalid")));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn error(message: &str) -> ProjectControlError {
    ProjectControlError(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn room_plan(overrides: Value) -> Vec<u8> {
        let mut value = json!({
            "project": "project-a",
            "heads": [
                { "name": "t1", "version": 7 },
                { "name": "t2", "version": 8 },
                { "name": "t3", "version": 9 },
                { "name": "combined_transformer" }
            ],
            "rooms": [
                { "name": "t1", "server_known": true, "server_version": 7 },
                { "name": "t2", "server_known": true, "server_version": 7 },
                { "name": "t3", "server_known": true, "server_version": 9, "local_dirty": true }
            ],
            "resident_cap": 4,
            "max_parallel_requests": 2
        });
        if let (Some(target), Some(source)) = (value.as_object_mut(), overrides.as_object()) {
            target.extend(source.clone());
        }
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn room_plan_preserves_dirty_work_and_bounds_parallel_residency() {
        let value: Value =
            serde_json::from_str(&plan_project_control_rooms(&room_plan(json!({}))).unwrap())
                .unwrap();
        assert_eq!(value["names"], json!(["t1", "t2", "t3"]));
        assert_eq!(
            value["availability"],
            json!({
                "total": 3, "local": 3, "current": 1, "dirty": 1, "stale": 1, "missing": 0
            })
        );
        assert_eq!(
            value["reuse"],
            json!([
                { "name": "t1", "provenance": "cache" },
                { "name": "t3", "provenance": "local" }
            ])
        );
        assert_eq!(value["fetch"], json!(["t2"]));
        assert_eq!(value["request_concurrency"], 1);
        assert_eq!(value["batch_width"], 4);
    }

    #[test]
    fn force_refreshes_clean_rooms_but_never_dirty_rooms() {
        let value: Value = serde_json::from_str(
            &plan_project_control_rooms(&room_plan(json!({
                "requested": ["t1", "t3"], "force": true
            })))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["scope"], "explicit");
        assert_eq!(value["fetch"], json!(["t1"]));
        assert_eq!(
            value["reuse"],
            json!([{ "name": "t3", "provenance": "local" }])
        );
    }

    #[test]
    fn report_plan_reuses_only_when_the_requested_freshness_policy_allows_it() {
        let plan = |input: Value| -> Value {
            serde_json::from_str(
                &plan_project_control_report_download(&serde_json::to_vec(&input).unwrap())
                    .unwrap(),
            )
            .unwrap()
        };
        assert_eq!(
            plan(json!({
                "existing": ["a", "a"], "report_state": "success",
                "report_has_files": true, "refresh_if_stale": true
            }))["action"],
            "reuse"
        );
        assert_eq!(
            plan(json!({
                "existing": ["a"], "report_state": "warning",
                "report_has_files": true, "refresh_if_stale": true
            }))["reason"],
            "server_head_stale"
        );
        assert_eq!(
            plan(json!({
                "existing": ["a"], "report_state": "success",
                "report_has_files": true, "force": true
            }))["reason"],
            "forced"
        );
    }

    #[test]
    fn transformer_catalog_is_exact_scoped_and_names_all_produced_formats() {
        let outputs: Vec<Value> = ["xlsx", "shp", "gpkg", "pdf_a0", "geojson"].iter().map(|format| {
            let output = crate::report_formats::output(format).unwrap();
            json!({"output_id":output.output_id,"format":output.format,"content_type":output.content_type,
                "local_path":format!("ds-reporter:v1:{}", "a".repeat(64)),"sha256":"", "expected_sha256":"b".repeat(64),"size_bytes":0,"expected_size_bytes":42,
                "paper_size": if *format == "pdf_a0" { json!("A0") } else { Value::Null }})
        }).collect();
        let row = json!({"id":"held", "project_id":"project-a", "engine":"network_reporter",
            "operation":"export-agasharu", "variant":"default", "client_run_id":"run", "created_at":1,
            "phase":"pending", "status":"pending", "outputs":outputs});
        let mut other_project = row.clone();
        other_project["project_id"] = json!("project-b");
        let mut other_transformer = row.clone();
        other_transformer["operation"] = json!("export-agasharu_1");
        let mut incomplete = row.clone();
        incomplete["outputs"][0]["local_path"] = Value::Null;
        let input = json!({"project":"project-a", "transformer":"agasharu", "rows":[row, other_project, other_transformer]});
        let value: Value = serde_json::from_str(
            &project_project_control_local_reports(&serde_json::to_vec(&input).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(value["reports"].as_array().unwrap().len(), 1);
        let names: Vec<&str> = value["reports"][0]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["filename"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "agasharu.xlsx",
                "agasharu.shp.zip",
                "agasharu.gpkg",
                "agasharu.a0.pdf",
                "agasharu_geojson.zip"
            ]
        );
        assert_eq!(value["reports"][0]["files"][3]["paper_size"], "A0");
        let input = json!({"project":"project-a", "transformer":"agasharu", "rows":[incomplete]});
        let value: Value = serde_json::from_str(
            &project_project_control_local_reports(&serde_json::to_vec(&input).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(value["reports"][0]["files"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn local_report_catalog_keeps_only_complete_project_reporter_artifacts() {
        let input = json!({
            "project": "project-a",
            "rows": [
                {
                    "id": "new", "project_id": "project-a", "engine": "network_reporter",
                    "operation": "export-compounded_report", "variant": "default",
                    "client_run_id": "run-2", "created_at": 2, "phase": "stored",
                    "status": "pending", "error": null,
                    "outputs": [{
                        "output_id": "zip", "format": "zip", "content_type": "application/zip",
                        "local_path": "opaque/new", "sha256": format!("sha256:{}", "a".repeat(64)),
                        "size_bytes": 20
                    }]
                },
                {
                    "id": "old", "project_id": "project-a", "engine": "network_reporter",
                    "operation": "export-combined_transformer", "variant": "default",
                    "client_run_id": "run-1", "created_at": 1, "phase": "stored",
                    "status": "pending", "error": null,
                    "outputs": [{
                        "output_id": "xlsx", "format": "xlsx",
                        "content_type": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                        "expected_sha256": "b".repeat(64), "expected_size_bytes": 10
                    }]
                },
                {
                    "id": "other", "project_id": "project-b", "engine": "network_reporter",
                    "operation": "export-combined_transformer", "variant": "default",
                    "client_run_id": "run-3", "created_at": 3, "phase": "stored",
                    "status": "pending", "outputs": []
                }
            ]
        });
        let value: Value = serde_json::from_str(
            &project_project_control_local_reports(&serde_json::to_vec(&input).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(value["reports"][0]["id"], "new");
        assert_eq!(
            value["reports"][0]["files"][0]["filename"],
            "compounded_reports.zip"
        );
        assert_eq!(value["reports"].as_array().unwrap().len(), 1);
    }
}
