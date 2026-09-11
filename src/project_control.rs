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
pub const PROJECT_CONTROL_BULK_PLAN_SCHEMA: &str = "ds.project-control.bulk-plan/v1";
pub const PROJECT_CONTROL_ARTIFACT_DOWNLOAD_SCHEMA: &str =
    "ds.project-control.artifact-download/v1";
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

/// The aggregate/synthetic row names that are not transformers. One home
/// now: `design::row_kind`, which the register, the report lane and
/// `design_status_row` all read.
pub(crate) fn is_special_status_row(name: &str) -> bool {
    crate::design::is_special_row(name)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    presentation: Option<crate::printing::ArtifactPresentation>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    presentation: Option<crate::printing::ArtifactPresentation>,
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
            crate::printing::validate_artifact_presentation(
                &output.format,
                output.paper_size.as_deref(),
                output.presentation.as_ref(),
            )
            .map_err(|message| error(&message))?;
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
                    crate::report_formats::combined_output_filename(
                        kind,
                        &output.output_id,
                        &output.format,
                    )
                },
                output_id: output.output_id,
                format: output.format,
                content_type: output.content_type,
                local_path,
                sha256,
                size_bytes,
                paper_size: output.paper_size,
                presentation: output.presentation,
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
    // An iteration replaces only the selected outputs. Keep every surviving
    // file in its producing batch, with its original sync state and provenance.
    if input.transformer.is_some() {
        let indices = crate::report::current_output_indices(
            &reports
                .iter()
                .map(|report| {
                    report
                        .files
                        .iter()
                        .map(|file| format!("{}\0{}", report.variant, file.output_id))
                        .collect()
                })
                .collect::<Vec<_>>(),
        );
        for (report, kept) in reports.iter_mut().zip(indices) {
            report.files = std::mem::take(&mut report.files)
                .into_iter()
                .enumerate()
                .filter_map(|(index, file)| kept.contains(&index).then_some(file))
                .collect();
        }
        reports.retain(|report| !report.files.is_empty());
    }
    encode(&ProjectedReportCatalog {
        schema: PROJECT_CONTROL_REPORT_CATALOG_SCHEMA,
        project: input.project,
        reports,
    })
}

// ── bulk admissibility ──────────────────────────────────────────────────
//
// "Would this action do anything for this row" is freshness arithmetic over
// published state. The register answered it three times, in three `$derived`
// blocks of a Svelte component, each with a different freshness rule — and a
// fourth time in the toolbar that decided whether to show the counts. A CLI
// caller could not ask it at all: every batch verb took an explicit list and
// performed it, so an agent never learned what would be skipped.
//
// One rule here, over the rows the register is already showing. ds-brain
// re-reads Firestore before dispatching, so this is the operator's own
// denominator, never the authority.

/// The seven bulk verbs the register offers. `combined_and_export` is the
/// compound of the first and the third, and is fresh only when BOTH are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkAction {
    AddToCombined,
    CombinedAndExport,
    GenerateReports,
    RetryProcess,
    Save,
    Delete,
    Version,
}

impl BulkAction {
    /// The capability the register gates the verb on — the SAME grant its own
    /// button is gated on, not a guess. Adding to combined is `design.report`;
    /// exporting reports is `design.download` + `reports.export` (a different
    /// grant, which is why one list for all three verbs hid the Reports counts
    /// from an operator who could export but not combine); the compound verb
    /// needs both legs. Empty for verbs every project member may run on their
    /// own rows.
    fn capabilities(self) -> &'static [&'static str] {
        match self {
            Self::AddToCombined => &["design.report"],
            Self::GenerateReports => &["design.download", "reports.export"],
            Self::CombinedAndExport => &["design.report", "design.download", "reports.export"],
            Self::RetryProcess => &["design.bulk_retry"],
            Self::Delete => &["transformer.delete"],
            Self::Save | Self::Version => &[],
        }
    }

    /// Whether the verb exists at all in this register. The combined mirror
    /// is a project-configuration choice, not a permission.
    fn needs_combined_mirror(self) -> bool {
        matches!(self, Self::AddToCombined | Self::CombinedAndExport)
    }
}

/// Why a selected row is not a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Not ticked.
    NotSelected,
    /// The row's kind forbids it (`design::row_allows`).
    NotCombinable,
    NotVersionable,
    NotDeletable,
    /// A synthetic or reserved row is never a report target.
    SpecialRow,
    /// ds-brain published `can_retry: false`.
    RetryRefused,
    /// Already saved: a bulk save would send nothing.
    AlreadySaved,
    /// The combined mirror UI is off in this project.
    CombinedMirrorDisabled,
    /// The operator lacks the capability this verb is gated on.
    CapabilityMissing,
}

#[derive(Debug, Clone, Serialize)]
pub struct BulkTarget {
    pub name: String,
    /// The mirror/report is current; dispatching would re-do finished work.
    pub fresh: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BulkSkip {
    pub name: String,
    pub reason: SkipReason,
}

#[derive(Debug, Serialize)]
struct BulkPlan {
    schema: &'static str,
    action: BulkAction,
    /// Rows the verb would actually dispatch — every target, fresh or stale.
    targets: Vec<BulkTarget>,
    skipped: Vec<BulkSkip>,
    /// Targets whose work is already current.
    fresh: usize,
    /// Targets that would really run.
    stale: usize,
    /// The verb is unavailable for every row: a missing capability, or a
    /// combined mirror this project does not use.
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<SkipReason>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BulkPlanInput {
    action: BulkAction,
    /// The register's own rows, in its order — already filtered, because the
    /// counts have always been over `filtered`, not the render window.
    rows: Vec<serde_json::Value>,
    /// The ticked names. Empty means nothing is ticked, and every verb here
    /// acts on the tick set only.
    #[serde(default)]
    selection: Vec<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    /// The project uses the combined mirror UI.
    #[serde(default)]
    combined_mirror_ui: bool,
    #[serde(default)]
    active_session_transformer: Option<String>,
    #[serde(default)]
    current_user_email: Option<String>,
    #[serde(default)]
    local_edit_lock: Option<String>,
    #[serde(default)]
    now_ms: i64,
}

/// Which of the selected rows each bulk verb may target, and how many would
/// be dispatched rather than skipped as already fresh.
pub fn plan_project_control_bulk_action(input_json: &[u8]) -> Result<String, ProjectControlError> {
    let input: BulkPlanInput = decode(input_json, "bulk-plan")?;
    if input.rows.len() > MAX_PROJECT_CONTROL_ROOMS {
        return Err(error("bulk-plan carries more rows than the register holds"));
    }
    let action = input.action;
    let missing = action
        .capabilities()
        .iter()
        .any(|capability| !input.capabilities.iter().any(|held| held == capability));
    let unavailable = if missing {
        Some(SkipReason::CapabilityMissing)
    } else if action.needs_combined_mirror() && !input.combined_mirror_ui {
        Some(SkipReason::CombinedMirrorDisabled)
    } else {
        None
    };

    let selection: BTreeSet<String> = input
        .selection
        .iter()
        .map(|name| crate::design::canonical_row_name(name))
        .collect();
    let truths = crate::design_status_row::truths(
        &input.rows,
        input.active_session_transformer.as_deref(),
        input.current_user_email.as_deref().unwrap_or_default(),
        input.local_edit_lock.as_deref(),
        input.now_ms,
    );

    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    for truth in &truths {
        let canonical = crate::design::canonical_row_name(&truth.name);
        if !selection.contains(&canonical) {
            skipped.push(BulkSkip {
                name: truth.name.clone(),
                reason: SkipReason::NotSelected,
            });
            continue;
        }
        if let Some(reason) = unavailable {
            skipped.push(BulkSkip {
                name: truth.name.clone(),
                reason,
            });
            continue;
        }
        match bulk_verdict(action, truth) {
            Ok(fresh) => targets.push(BulkTarget {
                name: truth.name.clone(),
                fresh,
            }),
            Err(reason) => skipped.push(BulkSkip {
                name: truth.name.clone(),
                reason,
            }),
        }
    }
    let fresh = targets.iter().filter(|target| target.fresh).count();
    encode(&BulkPlan {
        schema: PROJECT_CONTROL_BULK_PLAN_SCHEMA,
        action,
        stale: targets.len() - fresh,
        fresh,
        targets,
        skipped,
        unavailable,
    })
}

/// One row under one verb: a target (with its freshness) or a named skip.
///
/// Unsaved local state counts as STALE for every published-state verb — the
/// browser holds work the server has not seen, so whatever the server says is
/// current is current about something else.
fn bulk_verdict(
    action: BulkAction,
    truth: &crate::design_status_row::RowReply,
) -> Result<bool, SkipReason> {
    use crate::design_status_row::{CombinedState, SyncState};
    let unsaved = truth.view.sync_state == SyncState::Unsaved;
    let combined_fresh = truth.view.combined_state == CombinedState::Success;
    let report_fresh =
        truth.view.report_state == CombinedState::Success && truth.view.report_has_files;
    match action {
        BulkAction::AddToCombined => {
            if !truth.allows.combinable {
                return Err(SkipReason::NotCombinable);
            }
            Ok(!unsaved && combined_fresh)
        }
        BulkAction::CombinedAndExport => {
            if !truth.allows.combinable {
                return Err(SkipReason::NotCombinable);
            }
            Ok(!unsaved && combined_fresh && report_fresh)
        }
        BulkAction::GenerateReports => {
            if truth.kind != crate::design::RowKind::Individual {
                return Err(SkipReason::SpecialRow);
            }
            Ok(!unsaved && report_fresh)
        }
        BulkAction::RetryProcess => {
            if !truth.retry.allowed {
                return Err(SkipReason::RetryRefused);
            }
            Ok(false)
        }
        BulkAction::Save => {
            if !unsaved {
                return Err(SkipReason::AlreadySaved);
            }
            Ok(false)
        }
        BulkAction::Delete => {
            if !truth.allows.deletable {
                return Err(SkipReason::NotDeletable);
            }
            Ok(false)
        }
        BulkAction::Version => {
            if !truth.allows.versionable {
                return Err(SkipReason::NotVersionable);
            }
            Ok(false)
        }
    }
}

// ── download scope, urls and placement ──────────────────────────────────
//
// Which artifacts a download produces: which rows are in scope, which URLs
// represent them, WHICH COPY of a duplicated artifact wins, and how many are
// fresh, stale or missing.
//
// The placement rule is the one this widening exists for. A record's URL is a
// lazy pointer that is not downloaded by default; a local compute overwrites
// the local copy; the Sync Center uploads the local copy back over the record.
// So whenever a local copy exists it IS the working truth. A positional "last
// wins" shipped once and sent every deduplicated download on a round trip to
// the cloud for bytes already on disk. Here it is an explicit, testable output
// with a `why`, not a map-insertion side effect.

/// Where an artifact URL points. Only explicit local transports and
/// filesystem paths are local: treating every non-HTTP value as local once
/// made `gs://` pointers outrank real cloud URLs.
fn is_remote_artifact_url(url: &str) -> bool {
    let value = url.trim();
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("gs://") || lower.starts_with("compute-artifact://") {
        return true;
    }
    if lower.starts_with("ds-reporter://") {
        return false;
    }
    let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    // The host is everything before the first `/`, `?` or `#`, minus any
    // userinfo and port — `URL.hostname`, spelled out.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or_default();
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_matches(['[', ']']);
    if host.is_empty() {
        return true;
    }
    host != "tauri.localhost" && host != "asset.localhost"
}

/// The artifact NAME two copies share: the last path segment, lowercased,
/// with any query stripped and backslashes normalised.
fn artifact_name(url: &str) -> String {
    let path = url.split('?').next().unwrap_or_default().replace('\\', "/");
    let name = path.rsplit('/').next().unwrap_or_default();
    if name.is_empty() {
        path.to_ascii_lowercase()
    } else {
        name.to_ascii_lowercase()
    }
}

/// Why a URL won its artifact name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementWhy {
    /// Only one copy carried this name.
    Only,
    /// A local copy outranks a remote one, whatever the order.
    LocalWins,
    /// Same placement: the later copy is the newer one.
    LaterWins,
}

#[derive(Debug, Clone, Serialize)]
pub struct Placement {
    /// The row whose artifacts collided.
    pub row: String,
    /// The artifact name the copies collided on.
    pub name: String,
    pub chosen: String,
    pub why: PlacementWhy,
    /// Every copy that carried this name, in the order they were merged.
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadScopeRow {
    name: String,
    /// The row's report artifacts under the placement rule — what the summary
    /// counts, without the print files the delivery adds.
    report_files: Vec<String>,
    /// Report artifacts plus the row's print files: what a download delivers.
    urls: Vec<String>,
    report_status: String,
    /// Special rows are answered (a caller may ask about the combined row)
    /// but are never part of the individual-files scope.
    in_scope: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct DownloadSummary {
    fresh: usize,
    stale: usize,
    missing: usize,
    stale_with_files: usize,
    /// Rows whose artifacts are already listed: fresh plus stale-with-files.
    cached: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
struct SourceUploadSummary {
    draft: usize,
    sketch: usize,
    process: usize,
    total: usize,
    downloadable: usize,
}

#[derive(Debug, Serialize)]
struct ArtifactDownloadPlan {
    schema: &'static str,
    /// Which rows are in scope, in the register's order.
    scope: Vec<String>,
    rows: Vec<DownloadScopeRow>,
    /// Every URL the scope delivers, de-duplicated across rows, in order.
    urls: Vec<String>,
    /// The same list after the operator's format filter.
    filtered_urls: Vec<String>,
    /// URLs the host resolved by another route (a refresh that just ran),
    /// filtered under the same rule so the format checkboxes mean one thing.
    filtered_extra_urls: Vec<String>,
    placement: Vec<Placement>,
    summary: DownloadSummary,
    source_uploads: SourceUploadSummary,
    /// The precedence the scope was built under.
    scope_precedence: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactDownloadInput {
    /// The register's filtered rows, in its order.
    rows: Vec<serde_json::Value>,
    #[serde(default)]
    selection: Vec<String>,
    /// `xlsx` / `gpkg` / `kmz` / `shp`; empty means every format.
    #[serde(default)]
    formats: Vec<String>,
    /// URLs a just-finished mutation produced that the row does not carry
    /// yet, by transformer name.
    #[serde(default)]
    mutation_cache: BTreeMap<String, Vec<String>>,
    /// A list the host already resolved, to be filtered by the same formats.
    #[serde(default)]
    extra_urls: Vec<String>,
}

/// Which artifacts a download produces, and which copy of each wins.
pub fn plan_project_control_artifact_download(
    input_json: &[u8],
) -> Result<String, ProjectControlError> {
    let input: ArtifactDownloadInput = decode(input_json, "artifact-download")?;
    if input.rows.len() > MAX_PROJECT_CONTROL_ROOMS {
        return Err(error(
            "artifact-download carries more rows than the register holds",
        ));
    }
    let selection: BTreeSet<String> = input
        .selection
        .iter()
        .map(|name| crate::design::canonical_row_name(name))
        .collect();

    // Scope precedence (StatusTable): explicit selection > active filter >
    // whole project. The caller has already applied the filter, so the only
    // decision left here is whether a tick narrows it. Special rows are never
    // part of the individual-files scope.
    let mut scope = Vec::new();
    let mut rows = Vec::new();
    let mut urls: Vec<String> = Vec::new();
    let mut summary = DownloadSummary {
        fresh: 0,
        stale: 0,
        missing: 0,
        stale_with_files: 0,
        cached: 0,
    };
    let mut source_uploads = SourceUploadSummary::default();
    let mut placement: Vec<Placement> = Vec::new();

    for row in &input.rows {
        let Some(map) = row.as_object() else { continue };
        let name = map
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let canonical = crate::design::canonical_row_name(&name);
        let in_scope = !crate::design::is_special_row(&canonical)
            && (selection.is_empty() || selection.contains(&canonical));
        let cache = input
            .mutation_cache
            .get(&name)
            .or_else(|| input.mutation_cache.get(&canonical))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let (report, row_placement) = report_files(map, &name, &canonical, cache);
        let delivery = unique_strings(
            report
                .iter()
                .cloned()
                .chain(string_list(map.get("print_files"))),
        );
        let status = map
            .get("report_metadata")
            .and_then(serde_json::Value::as_object)
            .and_then(|meta| meta.get("status"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let has_files = !report.is_empty();
        placement.extend(row_placement);
        if !in_scope {
            rows.push(DownloadScopeRow {
                name,
                report_files: report,
                urls: delivery,
                report_status: status,
                in_scope,
            });
            continue;
        }
        if status == "stale" {
            summary.stale += 1;
            if has_files {
                summary.stale_with_files += 1;
            }
        } else if has_files {
            summary.fresh += 1;
        } else {
            summary.missing += 1;
        }
        for url in &delivery {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
        count_source_uploads(map, &mut source_uploads);
        rows.push(DownloadScopeRow {
            name: name.clone(),
            report_files: report,
            urls: delivery,
            report_status: status,
            in_scope,
        });
        scope.push(name);
    }
    summary.cached = summary.fresh + summary.stale_with_files;

    let filtered_urls = apply_format_filter(&urls, &input.formats);
    let filtered_extra_urls = apply_format_filter(&input.extra_urls, &input.formats);
    encode(&ArtifactDownloadPlan {
        schema: PROJECT_CONTROL_ARTIFACT_DOWNLOAD_SCHEMA,
        scope_precedence: if selection.is_empty() {
            "filtered_rows"
        } else {
            "explicit_selection"
        },
        scope,
        rows,
        urls,
        filtered_urls,
        filtered_extra_urls,
        placement,
        summary,
        source_uploads,
    })
}

/// The dedupe, spelled as a choice: the local copy wins by PLACEMENT; among
/// two copies of the same placement, the later (newer) one wins.
fn choose_placement(candidates: &[String]) -> String {
    let mut chosen = String::new();
    for url in candidates {
        if !chosen.is_empty() && !is_remote_artifact_url(&chosen) && is_remote_artifact_url(url) {
            continue;
        }
        chosen = url.clone();
    }
    chosen
}

/// One row's report artifacts: its listed files, the URLs its report phase
/// cached or emitted, and anything a just-finished mutation produced —
/// de-duplicated by artifact name under the placement rule.
///
/// Two rules ride with it. A combined report with zero rows has nothing to
/// deliver, whatever URLs the document still carries; and the combined row
/// itself never delivers a KMZ.
fn report_files(
    row: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    canonical: &str,
    mutation_cache: &[String],
) -> (Vec<String>, Vec<Placement>) {
    if canonical == "combined_transformer" && combined_report_has_zero_rows(row) {
        return (Vec::new(), Vec::new());
    }
    let merged = unique_strings(
        string_list(row.get("report_files"))
            .into_iter()
            .chain(phase_urls(row.get("report_metadata")))
            .chain(mutation_cache.iter().cloned()),
    );
    let mut byname: Vec<(String, Vec<String>)> = Vec::new();
    for url in merged {
        let key = artifact_name(&url);
        match byname.iter_mut().find(|(name, _)| name == &key) {
            Some((_, candidates)) => candidates.push(url),
            None => byname.push((key, vec![url])),
        }
    }
    let mut placement = Vec::new();
    let mut deduped = Vec::new();
    for (artifact, candidates) in byname {
        let chosen = choose_placement(&candidates);
        let why = if candidates.len() == 1 {
            PlacementWhy::Only
        } else if candidates.iter().any(|url| !is_remote_artifact_url(url))
            && candidates.iter().any(|url| is_remote_artifact_url(url))
        {
            PlacementWhy::LocalWins
        } else {
            PlacementWhy::LaterWins
        };
        deduped.push(chosen.clone());
        placement.push(Placement {
            row: name.to_string(),
            name: artifact,
            chosen,
            why,
            candidates,
        });
    }
    if canonical == "combined_transformer" {
        let kmz = |url: &String| {
            url.to_ascii_lowercase()
                .split('?')
                .next()
                .unwrap_or_default()
                .ends_with(".kmz")
        };
        placement.retain(|entry| !kmz(&entry.chosen));
        deduped.retain(|url| !kmz(url));
    }
    (deduped, placement)
}

fn combined_report_has_zero_rows(row: &serde_json::Map<String, serde_json::Value>) -> bool {
    let details = row
        .get("report_metadata")
        .and_then(serde_json::Value::as_object)
        .and_then(|meta| meta.get("outcomes"))
        .and_then(serde_json::Value::as_object)
        .and_then(|outcomes| outcomes.get("report_export"))
        .and_then(serde_json::Value::as_object)
        .and_then(|export| export.get("details"))
        .and_then(serde_json::Value::as_object);
    match details.and_then(|details| details.get("row_count")) {
        Some(serde_json::Value::Number(number)) => number.as_f64() == Some(0.0),
        Some(serde_json::Value::String(raw)) => raw.trim().parse::<f64>() == Ok(0.0),
        _ => false,
    }
}

/// A report phase's cached and emitted URLs, as strings or `{url}` objects.
fn phase_urls(meta: Option<&serde_json::Value>) -> Vec<String> {
    let Some(meta) = meta.and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    unique_strings(
        string_list(meta.get("cached_urls"))
            .into_iter()
            .chain(string_list(meta.get("output_urls"))),
    )
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    let Some(entries) = value.and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| match entry {
            serde_json::Value::String(raw) => Some(raw.clone()),
            serde_json::Value::Object(map) => map
                .get("url")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .collect()
}

fn unique_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        if value.is_empty() {
            continue;
        }
        if seen.insert(value.clone()) {
            out.push(value);
        }
    }
    out
}

fn apply_format_filter(urls: &[String], formats: &[String]) -> Vec<String> {
    if formats.is_empty() {
        return urls.to_vec();
    }
    urls.iter()
        .filter(|url| {
            let lower = url
                .split('?')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            formats.iter().any(|format| match format.as_str() {
                "xlsx" => lower.ends_with(".xlsx"),
                "gpkg" => lower.ends_with(".gpkg"),
                "kmz" => lower.ends_with(".kmz"),
                "shp" => lower.ends_with(".shp.zip") || lower.contains("_shp.zip"),
                _ => false,
            })
        })
        .cloned()
        .collect()
}

fn count_source_uploads(
    row: &serde_json::Map<String, serde_json::Value>,
    summary: &mut SourceUploadSummary,
) {
    let Some(uploads) = row.get("uploads").and_then(serde_json::Value::as_object) else {
        return;
    };
    for action in ["draft", "sketch", "process"] {
        let sources = uploads
            .get(action)
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let downloadable = sources
            .iter()
            .filter(|source| {
                source
                    .as_object()
                    .and_then(|source| source.get("download_url"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .is_some_and(|url| !url.is_empty())
            })
            .count();
        match action {
            "draft" => summary.draft += sources.len(),
            "sketch" => summary.sketch += sources.len(),
            _ => summary.process += sources.len(),
        }
        summary.total += sources.len();
        summary.downloadable += downloadable;
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

    #[test]
    fn a_bulk_plan_refuses_more_rows_than_the_register_can_hold() {
        let rows: Vec<Value> = (0..=MAX_PROJECT_CONTROL_ROOMS)
            .map(|index| json!({ "name": format!("tx_{index}") }))
            .collect();
        let input = json!({ "action": "save", "rows": rows, "selection": [] });
        let error = plan_project_control_bulk_action(&serde_json::to_vec(&input).unwrap())
            .expect_err("the bound holds");
        assert!(error.to_string().contains("more rows"), "{error}");
    }

    #[test]
    fn an_unknown_bulk_verb_is_refused_rather_than_guessed() {
        let input = json!({ "action": "detonate", "rows": [], "selection": [] });
        assert!(plan_project_control_bulk_action(&serde_json::to_vec(&input).unwrap()).is_err());
    }

    fn bulk_plan(action: &str, capabilities: Value) -> Value {
        let input = json!({
            "action": action,
            "rows": [{
                "name": "tx_1",
                "computed": {
                    "combined_inclusion_state": "success",
                    "report_state": "success",
                    "report_has_files": true
                }
            }],
            "selection": ["tx_1"],
            "capabilities": capabilities,
            "combined_mirror_ui": true,
        });
        serde_json::from_str(
            &plan_project_control_bulk_action(&serde_json::to_vec(&input).unwrap()).unwrap(),
        )
        .unwrap()
    }

    /// Exporting reports and adding to the combined mirror are DIFFERENT
    /// grants in the register that renders these counts: the Export Reports
    /// button reads `design.download` + `reports.export`, the Add-to-combined
    /// button reads `design.report`. One shared list for all three verbs
    /// emptied the Reports counts for an operator who could export but not
    /// combine, and filled them for one who could combine but not export.
    #[test]
    fn each_bulk_verb_asks_for_the_grant_its_own_button_asks_for() {
        let exporter = json!(["design.download", "reports.export"]);
        let combiner = json!(["design.report"]);

        let reports = bulk_plan("generate_reports", exporter.clone());
        assert_eq!(reports["unavailable"], Value::Null);
        assert_eq!(reports["fresh"], json!(1));
        assert_eq!(
            bulk_plan("generate_reports", combiner.clone())["unavailable"],
            json!("capability_missing")
        );

        let combined = bulk_plan("add_to_combined", combiner.clone());
        assert_eq!(combined["unavailable"], Value::Null);
        assert_eq!(combined["fresh"], json!(1));
        assert_eq!(
            bulk_plan("add_to_combined", exporter.clone())["unavailable"],
            json!("capability_missing")
        );

        // The compound verb runs both legs, so it needs both grants.
        for partial in [exporter, combiner] {
            assert_eq!(
                bulk_plan("combined_and_export", partial)["unavailable"],
                json!("capability_missing")
            );
        }
        let both = bulk_plan(
            "combined_and_export",
            json!(["design.report", "design.download", "reports.export"]),
        );
        assert_eq!(both["unavailable"], Value::Null);
        assert_eq!(both["fresh"], json!(1));
    }

    #[test]
    fn a_download_plan_answers_a_special_row_but_never_scopes_it() {
        let input = json!({
            "rows": [
                { "name": "combined_transformer", "report_files": ["gs://b/combined.xlsx"] },
                { "name": "tx_1", "report_files": ["gs://b/tx_1.xlsx"] }
            ]
        });
        let plan: Value = serde_json::from_str(
            &plan_project_control_artifact_download(&serde_json::to_vec(&input).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(plan["scope"], json!(["tx_1"]));
        assert_eq!(plan["rows"][0]["in_scope"], json!(false));
        assert_eq!(
            plan["rows"][0]["report_files"],
            json!(["gs://b/combined.xlsx"])
        );
        assert_eq!(plan["summary"]["fresh"], 1);
        assert_eq!(plan["scope_precedence"], "filtered_rows");
    }

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
        let mut row = row;
        row["outputs"][3]["presentation"] = json!({
            "layout_id":"overview", "layout_name":"Project overview", "paper_name":"A0",
            "orientation":"landscape", "page_mm":[1189,841]
        });
        let mut other_project = row.clone();
        other_project["project_id"] = json!("project-b");
        let mut other_transformer = row.clone();
        other_transformer["operation"] = json!("export-agasharu_1");
        let mut incomplete = row.clone();
        incomplete["outputs"][0]["local_path"] = Value::Null;
        let mut superseded = row.clone();
        superseded["id"] = json!("superseded");
        superseded["created_at"] = json!(0);
        let input = json!({"project":"project-a", "transformer":"agasharu", "rows":[row, superseded, other_project, other_transformer]});
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
        assert_eq!(
            value["reports"][0]["files"][3]["presentation"]["page_mm"],
            json!([1189.0, 841.0])
        );
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
