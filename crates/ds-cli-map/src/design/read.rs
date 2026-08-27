//! `ds map design read` — what is in a transformer's room, and where it came
//! from.
//!
//! The answer names its own source: `local` when the operator has the
//! transformer open with unsaved edits, `cloud` when it was fetched from the
//! project. That distinction is the first thing a caller needs, because every
//! staging command in this family writes into the local room — reading
//! `cloud` and then staging means the room was created by this call.
//!
//! `--property` adds a value histogram over the room. It exists so "what
//! governance state are these rows actually in?" costs one call and a few
//! hundred bytes, rather than pulling every feature across the bridge to
//! count them here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

/// Enough distinct values to see the shape of a governance field. A property
/// with more distinct values than this is an identifier, not a state.
const DEFAULT_LIMIT: &str = "20";

pub static COMMAND: Command = Command {
    id: "map.design.read",
    path: &["map", "design", "read"],
    contract: 1,
    summary: "Summarise a transformer's design layers.",
    purpose: "\
Reports a transformer's design layers and their feature counts, whether the \
room is the operator's local one with unsaved edits or a fresh read from the \
project, and which project it belongs to. Pass --property for a value \
histogram across the room — the cheap way to ask what governance state the \
rows are actually in before staging a change to them.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        Arg::value(
            "property",
            "<name>",
            "Also count the values of this property.",
        ),
        // Not `design::LAYER_ARG`: here the layers narrow the histogram only,
        // and the room summary is always the whole room. Reusing the family's
        // wording would promise a filter this command does not apply.
        Arg::repeated("layer", "<name>", "Narrow --property to these layers."),
        Arg::value(
            "limit",
            "<n>",
            "Report at most this many property values; 1..200.",
        )
        .default(DEFAULT_LIMIT),
        DESCRIPTOR_ARG,
    ],
    output: "\
The transformer, its project, whether the room is `local` or `cloud`, whether \
it holds unsaved edits, the per-layer feature counts and the total. With \
--property, the value counts, commonest first, with `more.omitted` when cut.",
    examples: &[
        Example {
            command: "ds map design read --transformer T-1042 --output json",
            note: "Read .data.dirty before staging anything.",
            runnable: false,
        },
        Example {
            command: "ds map design read --transformer T-1042 --property drafting_status --layer lv_lines",
            note: "How many rows are approved, draft, or carry no status at all.",
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
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 200)? as usize;
    let property = inputs.value("property").unwrap_or_default();
    let layers = inputs.repeated("layer");

    let mut arguments = json!({ "transformer": transformer });
    if !property.is_empty() {
        arguments["property"] = json!(property);
        // The application narrows the histogram by `layers` only when the
        // property was asked for; sending them otherwise would declare a
        // filter the read does not apply.
        if !layers.is_empty() {
            arguments["layers"] = json!(layers);
        }
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let room = crate::invoke(
        &descriptor,
        &crate::DESIGN_READ,
        arguments,
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    let mut data = json!({
        "transformer": transformer,
        "project": room["project"],
        "source": room["source"],
        "dirty": room["dirty"].as_bool().unwrap_or(false),
        "server_known": room["serverKnown"].as_bool().unwrap_or(false),
        "server_version": room["serverVersion"],
        "layers": room["layerFeatureCounts"],
        "features": room["totalFeatures"].as_u64().unwrap_or(0),
    });

    if !property.is_empty() {
        let (values, omitted) = histogram(&room["propertyValues"], limit);
        data["property"] = json!(property);
        data["property_values"] = values;
        if omitted > 0 {
            data["more"] = json!({
                "omitted": omitted,
                "remedy": "re-run with a larger --limit, or narrow with --layer",
            });
        }
    }
    Ok(data)
}

/// The commonest values first, bounded. Ordering by count is what makes a
/// truncated histogram still answer the question that was asked.
fn histogram(values: &Value, limit: usize) -> (Value, usize) {
    let Some(object) = values.as_object() else {
        return (json!([]), 0);
    };
    let mut counted: Vec<(&String, u64)> = object
        .iter()
        .map(|(key, value)| (key, value.as_u64().unwrap_or(0)))
        .collect();
    counted.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
    let omitted = counted.len().saturating_sub(limit);
    let shown: Vec<Value> = counted
        .into_iter()
        .take(limit)
        .map(|(value, count)| json!({ "value": value, "features": count }))
        .collect();
    (json!(shown), omitted)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}  in {}\n  {} room{}  ·  {}\n",
        data["transformer"].as_str().unwrap_or(""),
        data["project"].as_str().unwrap_or("?"),
        data["source"].as_str().unwrap_or("?"),
        if data["dirty"].as_bool().unwrap_or(false) {
            ", unsaved edits"
        } else {
            ""
        },
        crate::plural(data["features"].as_u64().unwrap_or(0), "feature"),
    );

    if let Some(layers) = data["layers"].as_object() {
        out.push('\n');
        for (name, count) in layers {
            out.push_str(&format!("  {name:<28} {count:>7}\n"));
        }
    }

    if let Some(values) = data["property_values"].as_array() {
        out.push_str(&format!(
            "\n{}:\n",
            data["property"].as_str().unwrap_or("property")
        ));
        for value in values {
            out.push_str(&format!(
                "  {:<28} {:>7}\n",
                value["value"].as_str().unwrap_or(""),
                value["features"],
            ));
        }
    }
    if let Some(more) = data["more"].as_object() {
        out.push_str(&format!(
            "\n{} more not shown\n  → {}\n",
            more["omitted"],
            more["remedy"].as_str().unwrap_or(""),
        ));
    }
    out
}
