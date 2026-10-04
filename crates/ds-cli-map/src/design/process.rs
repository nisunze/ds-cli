//! `ds map design process` — run the LV process on one transformer.
//!
//! This is the step that turns staged geometry into a network: customers
//! pulled from the configured source, poles, spans and service cables
//! generated. The engineering is entirely the application's — the same
//! kernel, through the same edit session the operator's own run uses.
//!
//! A JSON differential request is forwarded unchanged to the LV owner. The
//! same selection/edit intent is used by the browser and native processors.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::design::TRANSFORMER_ARG;
use crate::{DESCRIPTOR_ARG, TARGET_ARG};

const REQUEST: Arg = Arg::value(
    "request",
    "<json-file>",
    "The web differential request: {selected_feeders:[id,...], auto_process?:{differential_enabled,is_mv_session,accumulator_bound,force_full,changed_features:[{layer_name,feature_id}]}}; at most 1 MiB. Omit for a full run.",
);

/// Enough warnings to see what went wrong; the application caps its own list
/// at fifty.
const DEFAULT_LIMIT: &str = "10";

pub static COMMAND: Command = Command {
    id: "map.design.process",
    path: &["map", "design", "process"],
    contract: 2,
    summary: "Run the LV process on a staged transformer.",
    purpose: "Generates the LV network for one transformer through the Rust LV pipeline. Pass --request with the same JSON differential selection or AutoProcess edits the web sends; Rust decides the scope and freeze. Omit it for a full run. It stages; nothing reaches the project until `ds map design save`.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        REQUEST,
        Arg::value("limit", "<n>", "Report at most this many warnings; 0..50.")
            .default(DEFAULT_LIMIT),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
Whether the run was `full` or `differential` and how many lv_lines it \
selected, whether captured edits and blocking diagnostics forced a full AutoProcess run, the layer and \
feature counts it produced, per-layer totals, bounded warnings, and `staged` \
and `persisted` separately.",
    examples: &[
        Example {
            command: "ds map design process --transformer T-1042 --request feeders.json --output json",
            note: "feeders.json contains a selected_feeders array. Rust freezes everything outside the selection.",
            runnable: false,
        },
        Example {
            command: "ds map design process --transformer T-1042",
            note: "No request: a full run, recalculating everything.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::PROJECT_NOT_OPEN,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the differential matched no lv_lines, those lines carry no stable id, or an edit is already open",
            remedy: "run `ds map design read --transformer <name>` to inspect feature ids and the open edit context",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_NUMBER,
        Refusal {
            code: "differential_request_invalid",
            when: "the request file cannot be read, exceeds 1 MiB, or is not JSON",
            remedy: "pass --request with the JSON body described by this command; Rust validates its fields",
        },
    ],
    reference: Some("docs/reference/map.md"),
    search: &[],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let limit = crate::integer(inputs.require("limit")?, "limit", 0, 50)? as usize;

    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));
    if let Some(path) = inputs.value("request") {
        use std::io::Read;
        let invalid = |error: String| {
            Failure::invalid("differential_request_invalid", error)
                .remedy("pass --request with the JSON body described by this command")
        };
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| invalid(e.to_string()))?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(invalid("request exceeds 1 MiB".into()));
        }
        let differential: Value =
            serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        arguments.insert("differential".into(), differential);
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_PROCESS,
        Value::Object(arguments),
        crate::DESIGN_PROCESS_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    let empty = Vec::new();
    let warnings = result["warnings"].as_array().unwrap_or(&empty);
    let shown: Vec<Value> = warnings.iter().take(limit).cloned().collect();
    let omitted = warnings.len().saturating_sub(shown.len());

    let mut data = json!({
        "transformer": transformer,
        "project": result["project"],
        "mode": result["mode"],
        "differential_selected": result["differentialSelected"].as_u64().unwrap_or(0),
        // The Rust preview owns this diagnostic receipt; a manual selection
        // can remain differential even when retry diagnostics exist.
        "blocked_from_differential": result["blockedFromDifferential"].as_bool().unwrap_or(false),
        "layers": result["layerCount"],
        "features": result["featureCount"],
        "layer_features": result["layerFeatureCounts"],
        "warning_count": warnings.len(),
        "warnings": shown,
        "staged": result["staged"].as_bool().unwrap_or(false),
        "persisted": result["persisted"].as_bool().unwrap_or(false),
    });
    if omitted > 0 {
        data["more"] = json!({
            "omitted": omitted,
            "remedy": format!("re-run with --limit {}", warnings.len().min(50)),
        });
    }
    Ok(data)
}

pub fn render(data: &Value) -> String {
    let mode = data["mode"].as_str().unwrap_or("?");
    let mut out = format!(
        "{} run  on {}\n",
        mode,
        data["transformer"].as_str().unwrap_or("")
    );
    if mode == "differential" {
        out.push_str(&format!(
            "  {} lv_line(s) selected\n",
            data["differential_selected"]
        ));
    }
    if data["blocked_from_differential"].as_bool().unwrap_or(false) {
        out.push_str("  a blocking diagnostic forced a full run; the freeze did not hold\n");
    }
    out.push_str(&format!(
        "  {} feature(s) across {} layer(s)\n",
        data["features"], data["layers"],
    ));
    if let Some(warnings) = data["warnings"].as_array().filter(|list| !list.is_empty()) {
        out.push_str(&format!(
            "\n{} warning(s):\n",
            data["warning_count"].as_u64().unwrap_or(0)
        ));
        for warning in warnings {
            out.push_str(&format!("  {}\n", warning.as_str().unwrap_or("")));
        }
    }
    if let Some(more) = data["more"].as_object() {
        out.push_str(&format!("  {} more not shown\n", more["omitted"]));
    }
    out.push('\n');
    out.push_str(super::staging_note(data));
    out
}
