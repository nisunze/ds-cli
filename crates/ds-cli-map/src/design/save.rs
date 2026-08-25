//! `ds map design save` — push a transformer's staged edits to the project.
//!
//! The only command in this domain that writes anything durable, and the only
//! one dispatch requires `--yes` for. Everything before it stages into the
//! operator's local room; this is the step that makes it the record.
//!
//! Two answers are both successes and must not be confused with each other or
//! with a failure:
//!
//! * nothing staged, or a room with no local changes — `saved: false` with
//!   the reason. The question was answered; there was nothing to push.
//! * the project moved on since the room was loaded — that is a *conflict*,
//!   not a failure, and it is reported with its own exit class so a caller
//!   knows the fix is to reload and re-apply rather than to retry.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

/// What the application says when the project moved on under the room.
/// Matched against its own message; `tests/bridge_parity.rs` holds it to the
/// application's source.
pub const CONFLICT_MARKER: &str = "save refused";

pub static COMMAND: Command = Command {
    id: "map.design.save",
    path: &["map", "design", "save"],
    contract: 1,
    summary: "Push a transformer's staged edits to the project.",
    purpose: "\
Saves the locally staged edits for one transformer to the project. This is the \
only command in this domain that writes durable project data, so dispatch \
requires --yes. A room with nothing staged is a success reporting `saved: \
false`, not a failure. If the transformer changed in the project since the \
room was loaded the save is refused as a conflict, and the fix is to reload \
and re-apply rather than retry.",
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, DESCRIPTOR_ARG],
    output: "\
The transformer, its project, whether it saved, and — when it did not — the \
reason. `persisted` is true only on a save that actually happened.",
    examples: &[Example {
        command: "ds map design save --transformer T-1042 --yes --output json",
        note: "Without --yes, dispatch refuses before anything is sent.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "no such transformer, or the application declined the save",
            remedy: "read detail.detail for the application's own message",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        Refusal {
            code: "transformer_changed",
            when: "the transformer changed in the project since this room was loaded",
            remedy: "reload it in DS GridDesign and re-apply the change; retrying will not help",
        },
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that writes to the project",
            remedy: "re-run with --yes once you intend the write",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;

    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_SAVE,
        json!({ "transformer": transformer }),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
    .map_err(classify_conflict)?;

    let saved = result["saved"].as_bool().unwrap_or(false);
    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "saved": saved,
        "reason": result["reason"],
        "design_version": result["designVersion"],
        "concurrency_generation": result["concurrencyGeneration"],
        "staged": false,
        "persisted": saved,
    }))
}

/// A stale room is a conflict, and conflict is a class of its own.
///
/// Letting it through as `desktop_refused` would give it the `failed` exit
/// class, which tells a caller the work did not happen — true, but not the
/// part that matters. `conflict` says the caller's view of the world is
/// stale, which is the thing they have to fix.
fn classify_conflict(failure: Failure) -> Failure {
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !detail.contains(CONFLICT_MARKER) {
        return failure;
    }
    Failure::conflict(
        "transformer_changed",
        "the transformer changed in the project since this room was loaded",
    )
    .remedy("reload it in DS GridDesign and re-apply the change; retrying will not help")
    .next("ds map design read --transformer <name>")
}

pub fn render(data: &Value) -> String {
    let transformer = data["transformer"].as_str().unwrap_or("");
    if data["saved"].as_bool().unwrap_or(false) {
        return format!(
            "saved  {transformer}  to {}\n  design v{}  -  concurrency generation {}\n",
            data["project"].as_str().unwrap_or("?"),
            data["design_version"].as_u64().unwrap_or(0),
            data["concurrency_generation"].as_u64().unwrap_or(0),
        );
    }
    format!(
        "not saved  {transformer}\n  {}\n",
        data["reason"]
            .as_str()
            .unwrap_or("nothing staged for this transformer"),
    )
}
