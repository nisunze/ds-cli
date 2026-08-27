//! `ds map design process` — run the Fast LV process on one transformer.
//!
//! This is the step that turns staged geometry into a network: customers
//! pulled from the configured source, poles, spans and service cables
//! generated. The engineering is entirely the application's — the same
//! kernel, through the same edit session the operator's own run uses.
//!
//! ## The differential is the interesting half
//!
//! A full run recalculates everything, including the as-built network that
//! was just marked approved. `--differential-where drafting_status=draft`
//! narrows the run to the LV lines that matched, freezing the rest for that
//! one run. The freeze is transient: the kernel treats the frozen rows as
//! approved for the run and strips the flag from every output, so the
//! operator's real `drafting_status` is untouched.
//!
//! Two things about that are reported rather than hidden. A differential
//! selector that matches nothing is a refusal from the application, not a
//! silent widening into a full run — because a full run is exactly what the
//! caller was trying to avoid. And a blocking diagnostic makes the kernel run
//! full regardless, which arrives here as `blocked_from_differential: true`
//! alongside `mode`, so a caller can tell that the freeze did not hold.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

const DIFFERENTIAL_WHERE: Arg = Arg {
    name: "differential-where",
    kind: ArgKind::Repeated,
    value: "<key=value>",
    required: false,
    default: None,
    choices: &[],
    summary: "Recalculate only lv_lines matching this. Repeat to AND.",
};

const DIFFERENTIAL_ID: Arg = Arg {
    name: "differential-id",
    kind: ArgKind::Repeated,
    value: "<feature-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Recalculate only these lv_lines. Repeat.",
};

const DIFFERENTIAL_BBOX: Arg = Arg {
    name: "differential-bbox",
    kind: ArgKind::Value,
    value: "<w,s,e,n>",
    required: false,
    default: None,
    choices: &[],
    summary: "Recalculate only lv_lines meeting this box, in degrees.",
};

/// Enough warnings to see what went wrong; the application caps its own list
/// at fifty.
const DEFAULT_LIMIT: &str = "10";

pub static COMMAND: Command = Command {
    id: "map.design.process",
    path: &["map", "design", "process"],
    contract: 1,
    summary: "Run the Fast LV process on a staged transformer.",
    purpose: "\
Generates the LV network for one transformer — customers, poles, spans and \
service cables — through the same kernel and the same edit session the \
operator's own run uses. With no differential flags it runs FULL, \
recalculating everything. Any differential flag narrows it to the matching \
lv_lines and freezes the rest for that run only. It stages; nothing reaches \
the project until `ds map design save`.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        DIFFERENTIAL_WHERE,
        DIFFERENTIAL_ID,
        DIFFERENTIAL_BBOX,
        Arg::value("limit", "<n>", "Report at most this many warnings; 0..50.")
            .default(DEFAULT_LIMIT),
        DESCRIPTOR_ARG,
    ],
    output: "\
Whether the run was `full` or `differential` and how many lv_lines it \
selected, whether a blocking diagnostic forced it full anyway, the layer and \
feature counts it produced, per-layer totals, bounded warnings, and `staged` \
and `persisted` separately.",
    examples: &[
        Example {
            command: "ds map design process --transformer T-1042 --differential-where drafting_status=draft --output json",
            note: "Only the new feeders move; the approved as-built network is frozen.",
            runnable: false,
        },
        Example {
            command: "ds map design process --transformer T-1042",
            note: "No differential flags: a full run, recalculating everything.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the differential matched no lv_lines, those lines carry no stable id, or an edit is already open",
            remedy: "run `ds map design select --layer lv_lines` with the same selector to see what it matches",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_PAIR,
        crate::INVALID_BBOX,
        crate::INVALID_NUMBER,
        super::TOO_MANY_IDS,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let limit = crate::integer(inputs.require("limit")?, "limit", 0, 50)? as usize;

    let differential = super::selector(inputs, "differential")?;
    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));
    // Absent rather than empty. An empty differential object would ask the
    // application to narrow to nothing, and it refuses that rather than
    // widening — which would turn "no flags given" into a hard failure
    // instead of the full run it means.
    if !differential.is_empty() {
        arguments.insert("differential".into(), Value::Object(differential.clone()));
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
        "selector": if differential.is_empty() {
            Value::Null
        } else {
            json!(super::describe(&differential))
        },
        "differential_selected": result["differentialSelected"].as_u64().unwrap_or(0),
        // True means a blocking diagnostic made the kernel run full even
        // though a differential was asked for. Reported because the freeze
        // silently not holding is the failure a caller cannot see otherwise.
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
    if let Some(selector) = data["selector"].as_str() {
        out.push_str(&format!(
            "  selector  {selector}  ·  {} lv_line(s)\n",
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
