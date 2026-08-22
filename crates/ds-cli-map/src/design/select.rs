//! `ds map design select` — how many design features a selector matches, and
//! which.
//!
//! This is the look-before-you-write half of the family, and it takes exactly
//! the same selector `set` takes. Running it first turns "stage a property
//! change on everything that matches" from a leap into a check: the count
//! here is the count `set` will report as matched.
//!
//! Both projections are off by default. The application will return up to
//! five thousand ids and two hundred sampled features, and returning either
//! unasked would make the safe habit — select before you set — the expensive
//! one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::{BBOX_ARG, ID_ARG, LAYER_ARG, TRANSFORMER_ARG, WHERE_ARG};

pub static COMMAND: Command = Command {
    id: "map.design.select",
    path: &["map", "design", "select"],
    contract: 1,
    summary: "Count and sample design features matching a selector.",
    purpose: "\
Counts the features in a transformer's room that match a selector, broken down \
by layer. It takes the same selector `ds map design set` takes, so running it \
first is how a caller learns what a staged change would touch before staging \
it. Ids and sampled properties are explicit projections: by default this \
returns counts only.",
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        LAYER_ARG,
        WHERE_ARG,
        BBOX_ARG,
        ID_ARG,
        Arg::value(
            "sample",
            "<n>",
            "Return this many matches with properties; 0..200.",
        )
        .default("0"),
        Arg::value(
            "ids",
            "<n>",
            "Return this many matched feature ids; 0..5000.",
        )
        .default("0"),
        DESCRIPTOR_ARG,
    ],
    output: "\
How many features matched, the count per layer, and what the selector was. \
With --sample, matched features and their properties; with --ids, their ids. \
`more.omitted` when either projection was cut short.",
    examples: &[
        Example {
            command: "ds map design select --transformer T-1042 --layer lv_lines --where drafting_status=",
            note: "The as-built rows that carry no status at all.",
            runnable: false,
        },
        Example {
            command: "ds map design select --transformer T-1042 --where drafting_status=draft --sample 5 --output json",
            note: "Check what the rows look like before staging a change to them.",
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
        crate::INVALID_NUMBER,
        crate::INVALID_PAIR,
        crate::INVALID_BBOX,
        super::TOO_MANY_IDS,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let sample = crate::integer(
        inputs.require("sample")?,
        "sample",
        0,
        crate::MAX_FEATURE_SAMPLE as i64,
    )?;
    let ids_wanted = crate::integer(
        inputs.require("ids")?,
        "ids",
        0,
        super::MAX_SELECTOR_IDS as i64,
    )? as usize;

    let selector = super::selector(inputs, "")?;
    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));
    arguments.insert("sample".into(), json!(sample));

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_SELECT,
        super::with_selector(arguments, selector.clone()),
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    let empty = Vec::new();
    let ids = result["ids"].as_array().unwrap_or(&empty);
    let shown: Vec<Value> = ids.iter().take(ids_wanted).cloned().collect();
    // The application already bounded `ids` at five thousand, so a caller
    // asking for every id of a larger match set is told the list is short of
    // the count rather than left to infer it.
    let omitted = result["matched"]
        .as_u64()
        .unwrap_or(0)
        .saturating_sub(shown.len() as u64);

    let mut data = json!({
        "transformer": transformer,
        "project": result["project"],
        "source": result["source"],
        "selector": super::describe(&selector),
        "matched": result["matched"].as_u64().unwrap_or(0),
        "matched_by_layer": result["matchedByLayer"],
        "ids": shown,
        "sample": result["sample"],
    });
    if ids_wanted > 0 && omitted > 0 {
        data["more"] = json!({
            "omitted": omitted,
            "remedy": "narrow the selector, or raise --ids",
        });
    }
    Ok(data)
}

pub fn render(data: &Value) -> String {
    let matched = data["matched"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} matched  in {}\n  selector  {}\n",
        crate::plural(matched, "feature"),
        data["transformer"].as_str().unwrap_or(""),
        data["selector"].as_str().unwrap_or(""),
    );
    if matched == 0 {
        out.push_str(
            "  → widen the selector; `ds map design read --property <name>` shows real values\n",
        );
        return out;
    }
    if let Some(layers) = data["matched_by_layer"].as_object() {
        out.push('\n');
        for (layer, count) in layers {
            out.push_str(&format!("  {layer:<28} {count:>7}\n"));
        }
    }
    if let Some(sample) = data["sample"].as_array().filter(|list| !list.is_empty()) {
        out.push_str(&format!("\n{} sampled:\n", sample.len()));
        for feature in sample {
            out.push_str(&format!(
                "  {:<20} {}\n",
                feature["layer"].as_str().unwrap_or(""),
                feature["id"].as_str().unwrap_or(""),
            ));
        }
    }
    if let Some(more) = data["more"].as_object() {
        out.push_str(&format!("\n{} more not listed\n", more["omitted"]));
    }
    out
}
