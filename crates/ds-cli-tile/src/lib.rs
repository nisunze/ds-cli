//! `ds tile` — a project's vector-tile outputs: read their state, preflight
//! and plan a run, generate, and manage the catalogue.
//!
//! ## Two deliberately separate authorities
//!
//! A tile run is a governed project output: ds-brain owns preflight,
//! dispatch, the lease and the publish behind one fixed endpoint. Status,
//! preflight, plan and generate restore the native user and use only that
//! user's audience-fenced selected project. Catalogue list/add/remove remain
//! paired because their public contracts have not been extracted yet. `ds`
//! carries no token and no second tiling model.
//!
//! ## Why tiling belongs next to styling
//!
//! The legend of a tiled layer lists only the categorical values the tiles
//! hold, and the tiles record those values (as tilestats) only when they are
//! generated. Re-tiling is therefore how a project re-logs its categoricals
//! after its vocabulary or a style changed — the `ds-tiling` skill spells
//! out the order: catalogs → style → `ds tile generate`.
//!
//! ## What the family is
//!
//! ```text
//!   status → preflight → plan → generate        list → add | remove
//! ```
//!
//! `plan` combines the fixed status and preflight reads without dispatching;
//! `generate` calls only the fixed generation operation after CLI confirmation.

pub mod add;
pub mod generate;
pub mod list;
pub mod plan;
pub mod preflight;
pub mod remove;
pub mod status;

use std::time::Duration;

use ds_cli_auth::{
    HeadlessTileOperation, HeadlessTilePreflight, TileOperationResult, TileOperationStatus,
    TilePreflight, TilePreflightStatus, TileType,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::{Value, json};

pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, SIGNED_OUT,
    UNREACHABLE, UNREADABLE, UNSUPPORTED, classify_signed_out, invoke, paired, paired_availability,
    plural,
};

pub static DOMAIN: Domain = Domain {
    id: "tile",
    summary: "Vector tiles: status, plan, generate, and the catalogue.",
    commands: &[
        &status::COMMAND,
        &preflight::COMMAND,
        &plan::COMMAND,
        &generate::COMMAND,
        &list::COMMAND,
        &add::COMMAND,
        &remove::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The remaining paired wire contract
// ---------------------------------------------------------------------------

pub const TILE_LIST: BridgeOp = BridgeOp {
    operation: "tile.list",
    arguments: &["global", "refresh"],
};
pub const TILE_ADD: BridgeOp = BridgeOp {
    operation: "tile.add",
    arguments: &["type", "source_project", "apply"],
};
pub const TILE_REMOVE: BridgeOp = BridgeOp {
    operation: "tile.remove",
    arguments: &["tile_id", "scope", "apply"],
};

/// Every operation this domain can still send to Desktop, for the parity test
/// to walk. Managed output reads and generation must never be added here.
pub const BRIDGE_OPS: &[&BridgeOp] = &[&TILE_LIST, &TILE_ADD, &TILE_REMOVE];

pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Refusals this domain adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const TILE_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "an unknown tile type or tile id, a blocked preflight, or ds-brain declined the run",
    remedy: "read `ds tile status` and `ds tile preflight`; detail.detail carries the application's message",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that starts a run or changes the tile catalogue",
    remedy: "run `ds tile plan` first, then re-run with --yes once the decision is what you intend",
};

macro_rules! native_refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        pub const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

native_refusal!(
    NATIVE_PROFILE,
    "native_profile_not_configured",
    "the exact packaged native profile is unavailable",
    "install one complete ds release"
);
native_refusal!(
    NATIVE_PROFILE_DIGEST,
    "native_profile_digest_mismatch",
    "the packaged catalogue differs from the build pin",
    "reinstall one complete ds release"
);
native_refusal!(
    NATIVE_PROFILE_UNSAFE,
    "native_profile_unsafe",
    "the packaged native catalogue is unsafe or malformed",
    "reinstall one complete ds release"
);
native_refusal!(
    HEADLESS_SIGNED_OUT,
    "headless_signed_out",
    "the selected lane has no restorable native user",
    "run ds auth login --email <address>"
);
native_refusal!(
    HEADLESS_NO_PROJECT,
    "headless_project_not_selected",
    "the user has no audience-fenced selected project",
    "run ds auth project use --project <exact-id>"
);
native_refusal!(
    PROJECT_CONTEXT_STALE,
    "project_context_stale",
    "the saved project belongs to another identity, lane, or audience",
    "select the project again with ds auth project use"
);
native_refusal!(
    PROJECT_CONTEXT_CHANGED,
    "project_context_changed",
    "the selected project changed between the status and preflight reads",
    "run ds tile plan again against the current selected project"
);
native_refusal!(
    NATIVE_STATE_UNSAFE,
    "native_state_unsafe",
    "protected native state is unsafe or unreadable",
    "repair the owner-only DS config directory"
);
native_refusal!(
    NATIVE_STATE_UNAVAILABLE,
    "native_state_unavailable",
    "protected native state cannot be accessed",
    "repair the owner-only DS config directory"
);
native_refusal!(
    NATIVE_STATE_PROTECTION,
    "native_state_protection_unavailable",
    "this build has no protected-state adapter",
    "install a supported native ds build"
);
native_refusal!(
    NATIVE_STATE_ROOT,
    "native_state_root_invalid",
    "the configured state root is not absolute",
    "unset it or provide an absolute path"
);
native_refusal!(
    NATIVE_STATE_CONFLICT,
    "native_state_conflict",
    "another native operation holds the state lease",
    "retry after that operation finishes"
);
native_refusal!(
    NATIVE_CLEANUP,
    "native_cleanup_required",
    "revoked identity cleanup could not clear context",
    "repair protected state and run auth logout"
);
native_refusal!(
    AUTH_CONTEXT_MISMATCH,
    "auth_context_mismatch",
    "the protected native providers disagree on identity or selected project",
    "sign out or revoke the unintended provider before retrying"
);
native_refusal!(
    AUTH_INPUT,
    "auth_input_invalid",
    "the selected project identity violates the fixed request bound",
    "select a freshly visible project again"
);
native_refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the fixed gateway rejects the verified request",
    "verify the account and its selected-project access"
);
native_refusal!(
    AUTH_REVOKED,
    "auth_revoked",
    "the native session was permanently revoked",
    "sign in again interactively"
);
native_refusal!(
    AUTH_IDENTITY_MISMATCH,
    "auth_identity_mismatch",
    "the restored identity differs from the bound native session",
    "sign in again and report a repeated mismatch"
);
native_refusal!(
    AUTH_TRANSIENT,
    "auth_transient",
    "the fixed native service is temporarily unavailable",
    "retry without changing local state"
);
native_refusal!(
    AUTH_UNREADABLE,
    "auth_response_unreadable",
    "the tile response violates its closed bounded contract",
    "retry once, then update ds if it persists"
);

pub const NATIVE_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    NATIVE_STATE_UNSAFE,
    NATIVE_STATE_UNAVAILABLE,
    NATIVE_STATE_PROTECTION,
    NATIVE_STATE_ROOT,
    NATIVE_STATE_CONFLICT,
    NATIVE_CLEANUP,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    AUTH_REVOKED,
    AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
];

pub const NATIVE_PLAN_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    PROJECT_CONTEXT_CHANGED,
    NATIVE_STATE_UNSAFE,
    NATIVE_STATE_UNAVAILABLE,
    NATIVE_STATE_PROTECTION,
    NATIVE_STATE_ROOT,
    NATIVE_STATE_CONFLICT,
    NATIVE_CLEANUP,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    AUTH_REVOKED,
    AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
];

pub const NATIVE_WRITE_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    NATIVE_STATE_UNSAFE,
    NATIVE_STATE_UNAVAILABLE,
    NATIVE_STATE_PROTECTION,
    NATIVE_STATE_ROOT,
    NATIVE_STATE_CONFLICT,
    NATIVE_CLEANUP,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    AUTH_REVOKED,
    AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
    CONFIRMATION_REQUIRED,
];

pub fn classify_tile_failure(failure: Failure) -> Failure {
    classify_signed_out(failure)
}

// ---------------------------------------------------------------------------
// Flag shapes shared across the domain
// ---------------------------------------------------------------------------

pub const TYPE_ARG: Arg = Arg {
    name: "type",
    kind: ArgKind::Value,
    value: "<survey|design>",
    required: true,
    default: None,
    choices: &["survey", "design"],
    summary: "Which project output: survey (form entries) or design (transformers and DS Grid models).",
};
pub const OPTIONAL_TYPE_ARG: Arg = Arg {
    name: "type",
    kind: ArgKind::Value,
    value: "<survey|design>",
    required: false,
    default: None,
    choices: &["survey", "design"],
    summary: "One output only; both are reported when absent.",
};
pub const FORCE_ARG: Arg = Arg {
    name: "force",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Run even when the published output is current (a restyle or a vocabulary change needs this).",
};
pub const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

/// The `type` flag, or `null` when the command accepts its absence.
pub fn type_argument(inputs: &ds_cli_contract::Inputs) -> Value {
    inputs.value("type").map_or(Value::Null, |t| json!(t))
}

pub fn tile_type(value: &str) -> TileType {
    match value {
        "survey" => TileType::Survey,
        "design" => TileType::Design,
        _ => unreachable!("the command parser enforces the tile type choices"),
    }
}

pub fn project_receipt(
    lane: &str,
    project_id: &str,
    project_name: &str,
    project_status: &str,
) -> Value {
    json!({
        "lane": lane,
        "project": {
            "ds_project": project_id,
            "project_name": project_name,
            "status": project_status,
        },
    })
}

pub fn operation_project(headless: &HeadlessTileOperation) -> Value {
    project_receipt(
        headless.lane(),
        headless.project_id(),
        headless.project_name(),
        headless.project_status(),
    )
}

pub fn preflight_project(headless: &HeadlessTilePreflight) -> Value {
    project_receipt(
        headless.lane(),
        headless.project_id(),
        headless.project_name(),
        headless.project_status(),
    )
}

pub fn require_same_project(left: &Value, right: &Value) -> Result<(), Failure> {
    if left == right {
        Ok(())
    } else {
        Err(Failure::conflict(
            PROJECT_CONTEXT_CHANGED.code,
            "the selected project changed while the tile decision was being read",
        )
        .remedy(PROJECT_CONTEXT_CHANGED.remedy))
    }
}

pub const fn operation_status(status: TileOperationStatus) -> &'static str {
    match status {
        TileOperationStatus::NotStarted => "not_started",
        TileOperationStatus::InProgress => "in_progress",
        TileOperationStatus::Completed => "completed",
        TileOperationStatus::Failed => "failed",
        TileOperationStatus::Skipped => "skipped",
        TileOperationStatus::Stale => "stale",
        TileOperationStatus::Started => "started",
    }
}

pub const fn preflight_status(status: TilePreflightStatus) -> &'static str {
    match status {
        TilePreflightStatus::Ready => "ready",
        TilePreflightStatus::Empty => "empty",
        TilePreflightStatus::Blocked => "blocked",
    }
}

pub fn operation_json(result: &TileOperationResult) -> Value {
    json!({
        "project": result.project(),
        "type": result.tile_type().token(),
        "status": operation_status(result.status()),
        "phase": result.phase(),
        "message": result.message(),
        "started_at": result.started_at(),
        "failed_at": result.failed_at(),
        "tiled_at": result.tiled_at(),
        "skipped_at": result.skipped_at(),
        "elapsed_seconds": result.elapsed_seconds(),
        "total_features": result.total_features(),
        "triggered_by": result.triggered_by(),
        "last_error": result.last_error(),
        "reason": result.reason(),
        "cache_available": result.cache_available(),
        "cleanup_pending": result.cleanup_pending(),
        "dirty": result.dirty(),
        "in_progress": result.status() == TileOperationStatus::InProgress,
        "force": result.force(),
        "needs_tiling": result.needs_tiling(),
        "will_dispatch": result.will_dispatch(),
        "decision": result.decision(),
    })
}

pub fn preflight_json(result: &TilePreflight) -> Value {
    let layers: Vec<Value> = result
        .layers()
        .iter()
        .map(|layer| {
            json!({
                "table": layer.table(),
                "table_type": layer.table_type(),
                "row_count": layer.row_count(),
                "geometry_count": layer.geometry_count(),
                "geometry_column": layer.geometry_column(),
                "empty": layer.empty(),
                "dashed": layer.dashed(),
                "error": layer.error(),
            })
        })
        .collect();
    json!({
        "project": result.project(),
        "type": result.tile_type().token(),
        "status": preflight_status(result.status()),
        "expected_layer_count": result.expected_layer_count(),
        "will_export_layers": result.will_export_layers(),
        "total_rows": result.total_rows(),
        "total_geometries": result.total_geometries(),
        "layers": layers,
        "empty_layers": result.empty_layers(),
        "errors": result.errors(),
        "warnings": result.warnings(),
        "message": result.message(),
        "repair_required": result.repair_required(),
        "projection_state": result.projection_state(),
    })
}

pub fn plan_decision(
    status: TileOperationStatus,
    tiled_at: Option<u64>,
    dirty: Option<bool>,
    force: bool,
) -> (bool, &'static str) {
    if status == TileOperationStatus::InProgress {
        return (false, "Tile generation is already in progress.");
    }
    if force {
        return (
            true,
            "Force option is enabled — regeneration will run unconditionally.",
        );
    }
    if tiled_at.is_none() {
        return (
            true,
            "No project map output has been published for this type.",
        );
    }
    if status == TileOperationStatus::Stale || dirty == Some(true) {
        return (
            true,
            "Project source data changed since the last published map output.",
        );
    }
    (false, "")
}

/// Keep a human line one line wide without hiding that it was cut.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// One line describing a tile status object as ds-brain reports it.
pub fn status_line(status: &Value) -> String {
    let state = status["status"].as_str().unwrap_or("unknown");
    let mut parts = vec![state.to_string()];
    if let Some(features) = status["total_features"].as_u64() {
        parts.push(plural(features, "feature"));
    }
    if status["dirty"].as_bool() == Some(true) {
        parts.push("sources changed since this output".to_string());
    }
    if status["in_progress"].as_bool() == Some(true) {
        parts.push("running".to_string());
    }
    if let Some(error) = status["last_error"].as_str()
        && !error.is_empty()
    {
        parts.push(format!("last error: {}", truncate(error, 80)));
    }
    parts.join(" · ")
}

/// One line describing a preflight object as ds-brain reports it.
pub fn preflight_line(preflight: &Value) -> String {
    if preflight.is_null() {
        return "preflight not run".to_string();
    }
    let mut parts = vec![
        preflight["status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
    ];
    if let (Some(will), Some(expected)) = (
        preflight["will_export_layers"].as_u64(),
        preflight["expected_layer_count"].as_u64(),
    ) {
        parts.push(format!("{will}/{expected} layers"));
    }
    if let Some(rows) = preflight["total_rows"].as_u64() {
        parts.push(plural(rows, "row"));
    }
    if let Some(empty) = preflight["empty_layers"].as_array()
        && !empty.is_empty()
    {
        let names: Vec<&str> = empty.iter().filter_map(Value::as_str).collect();
        parts.push(format!("empty: {}", truncate(&names.join(", "), 60)));
    }
    if preflight["repair_required"].as_bool() == Some(true) {
        parts.push("projection repair required".to_string());
    }
    for key in ["errors", "warnings"] {
        if let Some(items) = preflight[key].as_array() {
            for item in items.iter().filter_map(Value::as_str) {
                parts.push(format!(
                    "{}: {}",
                    key.trim_end_matches('s'),
                    truncate(item, 80)
                ));
            }
        }
    }
    if let Some(message) = preflight["message"].as_str()
        && !message.is_empty()
    {
        parts.push(truncate(message, 80));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_operation_is_listed_for_the_parity_test_to_walk() {
        let mut names: Vec<&str> = BRIDGE_OPS.iter().map(|op| op.operation).collect();
        names.sort_unstable();
        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(names, unique, "an operation is declared twice");
        assert_eq!(names, ["tile.add", "tile.list", "tile.remove"]);
    }

    #[test]
    fn managed_output_commands_share_the_native_gate() {
        let expected =
            ds_cli_auth::native_availability as fn() -> ds_cli_contract::spec::Availability;
        for command in [
            &status::COMMAND,
            &preflight::COMMAND,
            &plan::COMMAND,
            &generate::COMMAND,
        ] {
            assert!(std::ptr::fn_addr_eq(command.availability, expected));
        }
    }

    #[test]
    fn plan_decision_matches_the_pipeline_staleness_order() {
        assert_eq!(
            plan_decision(TileOperationStatus::InProgress, None, Some(true), true),
            (false, "Tile generation is already in progress.")
        );
        assert!(plan_decision(TileOperationStatus::Completed, Some(1), None, true).0);
        assert!(plan_decision(TileOperationStatus::NotStarted, None, None, false).0);
        assert!(plan_decision(TileOperationStatus::Completed, Some(1), Some(true), false).0);
        assert!(plan_decision(TileOperationStatus::Stale, Some(1), None, false).0);
        assert!(!plan_decision(TileOperationStatus::Completed, Some(1), Some(false), false).0);
    }

    #[test]
    fn plan_never_dispatches_and_generate_uses_only_the_fixed_write() {
        let plan = include_str!("plan.rs");
        assert!(plan.contains("ds_cli_auth::tile_status"));
        assert!(plan.contains("ds_cli_auth::tile_preflight"));
        assert!(!plan.contains("ds_cli_auth::tile_generate"));

        let generate = include_str!("generate.rs");
        assert!(generate.contains("ds_cli_auth::tile_generate"));
        assert!(!generate.contains("ds_cli_auth::tile_status"));
        assert!(!generate.contains("ds_cli_auth::tile_preflight"));
    }

    #[test]
    fn status_and_preflight_lines_read_the_wire_shapes() {
        let line =
            status_line(&json!({"status": "completed", "total_features": 12, "dirty": true}));
        assert_eq!(
            line,
            "completed · 12 features · sources changed since this output"
        );
        let line = preflight_line(
            &json!({"status": "ready", "will_export_layers": 3, "expected_layer_count": 4, "empty_layers": ["spans"]}),
        );
        assert_eq!(line, "ready · 3/4 layers · empty: spans");
        assert_eq!(preflight_line(&Value::Null), "preflight not run");
    }
}
