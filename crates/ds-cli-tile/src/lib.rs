//! `ds tile` — a project's vector-tile outputs: read their state, preflight
//! and plan a run, generate, and manage the catalogue.
//!
//! ## Why this domain is a bridge domain
//!
//! A tile run is a governed project output: ds-brain owns preflight,
//! dispatch, the lease and the publish behind one endpoint, and the paired
//! application drives it from its Pipeline panel. `ds` reuses exactly that
//! client under the application's own session — the same staleness rule
//! decides whether a run is needed, the same preflight reads the sources, the
//! same dispatch starts the job. `ds` carries no token and no second tiling
//! model.
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
//! `plan` and `generate` are one operation with `apply` false or true, so
//! what was reviewed is what is dispatched.

pub mod add;
pub mod generate;
pub mod list;
pub mod plan;
pub mod preflight;
pub mod remove;
pub mod status;

use std::time::Duration;

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
// The declared wire contract
// ---------------------------------------------------------------------------

pub const TILE_STATUS: BridgeOp = BridgeOp {
    operation: "tile.status",
    arguments: &["type"],
};
pub const TILE_PREFLIGHT: BridgeOp = BridgeOp {
    operation: "tile.preflight",
    arguments: &["type"],
};
pub const TILE_GENERATE: BridgeOp = BridgeOp {
    operation: "tile.generate",
    arguments: &["type", "force", "apply"],
};
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

/// Every operation this domain can send, for the parity test to walk. `plan`
/// and `generate` are one operation — `apply` false or true — so the list is
/// one shorter than the command list.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &TILE_STATUS,
    &TILE_PREFLIGHT,
    &TILE_GENERATE,
    &TILE_LIST,
    &TILE_ADD,
    &TILE_REMOVE,
];

pub const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// A plan reads the pipeline document and runs the preflight; a generate
/// additionally dispatches. Both return before the job finishes.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(3 * 60);

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

/// The `type` flag, or `null` when the command accepts its absence.
pub fn type_argument(inputs: &ds_cli_contract::Inputs) -> Value {
    inputs.value("type").map_or(Value::Null, |t| json!(t))
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
        // plan and generate share one operation; every other command owns one.
        assert_eq!(names.len(), DOMAIN.commands.len() - 1);
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
