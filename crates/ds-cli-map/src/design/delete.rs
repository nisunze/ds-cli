//! `ds map design delete` — stage the removal of selected design features.
//!
//! Drafting is not only adding: correcting an imported room means removing
//! duplicated poles, mis-digitized lines, or features that never existed on
//! the ground. Like every other stage command it writes only the operator's
//! local room; `ds map design save` is the separate, confirmed push.
//!
//! The selector is REQUIRED. An empty selector would match the entire room,
//! and "delete everything" must never be the accident default of forgetting a
//! flag — the application refuses it too.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::{BBOX_ARG, ID_ARG, LAYER_ARG, TRANSFORMER_ARG, WHERE_ARG};

pub static COMMAND: Command = Command {
    id: "map.design.delete",
    path: &["map", "design", "delete"],
    contract: 1,
    summary: "Stage the removal of selected design features.",
    purpose: "\
Removes every design feature a selector matches from the transformer's local \
room and marks it dirty; the project is untouched until `ds map design save`. \
The selector is required — deletion is always an explicit selection, never a \
default sweep. Use --dry-run to count what would go first.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        LAYER_ARG,
        WHERE_ARG,
        BBOX_ARG,
        ID_ARG,
        ds_cli_contract::spec::Arg::switch(
            "dry-run",
            "Report what would be removed; stage nothing.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "\
How many features matched and were removed, the removal count per layer, and \
`staged` and `persisted` separately — `persisted` is false here always.",
    examples: &[
        Example {
            command: "ds map design delete --transformer T-1042 --layer lv_poles --where pole_status=duplicate --dry-run",
            note: "Count the duplicates before touching them.",
            runnable: false,
        },
        Example {
            command: "ds map design delete --transformer T-1042 --id lv_poles#41 --output json",
            note: "Stage one removal. Read .data.staged; the project is untouched.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_BBOX,
        super::TOO_MANY_IDS,
        Refusal {
            code: "selector_required",
            when: "no layer, where, bbox, or id narrowed the deletion",
            remedy: "name what to delete; an unselected delete would sweep the whole room",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let dry_run = inputs.switch("dry-run");
    let selector = super::selector(inputs, "")?;
    if selector.is_empty() {
        return Err(Failure::invalid(
            "selector_required",
            "no selector was given; deletion is always an explicit selection",
        )
        .remedy("narrow with --layer, --where, --bbox, or --id"));
    }

    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));
    arguments.insert("dryRun".into(), json!(dry_run));

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_DELETE,
        super::with_selector(arguments, selector.clone()),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "selector": super::describe(&selector),
        "dry_run": result["dryRun"].as_bool().unwrap_or(dry_run),
        "matched": result["matched"].as_u64().unwrap_or(0),
        "removed": result["removed"].as_u64().unwrap_or(0),
        "removed_by_layer": result["removedByLayer"],
        "staged": result["staged"].as_bool().unwrap_or(false),
        "persisted": result["persisted"].as_bool().unwrap_or(false),
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} matched  ·  {} removed\n  selector  {}\n",
        data["matched"],
        data["removed"],
        data["selector"].as_str().unwrap_or(""),
    );
    if let Some(layers) = data["removed_by_layer"].as_object() {
        for (layer, count) in layers {
            out.push_str(&format!("  {layer:<28} {count:>7}\n"));
        }
    }
    if data["dry_run"].as_bool().unwrap_or(false) {
        out.push_str("\ndry run; nothing was staged\n");
        return out;
    }
    out.push('\n');
    out.push_str(super::staging_note(data));
    out
}
