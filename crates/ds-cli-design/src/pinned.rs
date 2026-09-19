//! `ds design pinned preview` — what a read-only pinned working set costs,
//! answered on a Server with no browser.
//!
//! A pinned transformer is READ-ONLY CONTEXT: dumb GeoJSON an operator glances
//! at while working on something else. It is not editable, not a selection
//! target, and carries no session. The browser used to decide three things
//! about it, and got all three wrong:
//!
//!   - it refetched rooms the machine already held, because the gate that
//!     admitted and cached a room was stricter than the gate that reused it;
//!   - it painted one map source per (transformer, class), so fifty pinned
//!     transformers over six classes were three hundred sources;
//!   - it carried the whole property bag of an EDITABLE feature on data a
//!     preview reads one or two fields of.
//!
//! `ds_command_kernel::pinned_context` owns all three decisions, purely. This
//! command is the headless driver of them: it reads the project's own status
//! rows for the head revisions, plans the fetch against an inventory of what a
//! machine already holds, fetches exactly what the plan named through the
//! restored native user, resolves each design class's PINNED style document out
//! of the published style catalogue, and folds the rooms. Nothing here decides
//! anything — every number in the receipt is the kernel's.
//!
//! It exists so the three numbers can be MEASURED without a browser: how many
//! rooms were reused rather than downloaded, how many map sources the merge
//! removes, and how much of the property bag a preview actually needs.

use std::collections::BTreeMap;

use ds_cli_auth::TransformerSet;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::pinned_context::{
    self, ClassStyle, Merged, PINNED_TRANSFORMER_PROPERTY, Plan, SCHEMA, Styles,
};
use serde_json::{Map, Value, json};

use super::transformer::LANE_ARG;

/// How many pinned transformers one invocation will read. The kernel batches
/// up to 500 for a browser executing one request per chunk; this host fetches
/// one room per call, so the cap is what a single command may spend.
const MAX_PINS: usize = 120;
/// How many merged layer rows the receipt prints. A project has a dozen design
/// classes; a hundred would mean the catalogue, not this project.
const MAX_LAYER_ROWS: usize = 40;
/// How many field names one layer row names before it says how many more.
const MAX_FIELD_NAMES: usize = 24;

const TRANSFORMER: Arg = Arg::repeated(
    "transformer",
    "<name>",
    "A pinned transformer. Repeat for the pin set; at least one is required.",
);
const FOCUS: Arg = Arg::value(
    "focus",
    "<name>",
    "The transformer being edited. It is never pinned context as well, so it is dropped from the plan.",
);
const HIDE: Arg = Arg::repeated(
    "hide",
    "<name>",
    "A pinned transformer the operator has hidden. It contributes no features. Repeat.",
);
const REQUIRE: Arg = Arg::repeated(
    "require",
    "<layer>",
    "A design class a displayable room must carry. Repeat; the default is the transformer's own `tr` anchor.",
);
const HELD: Arg = Arg::value(
    "held",
    "<path>",
    "A JSON inventory of the rooms a machine already holds: [{name, layers:{class:count}, version, complete}].",
);
const FORCE: Arg = Arg::switch(
    "force",
    "Ask for every pinned room again, held or not — the operator's Refresh.",
);
const PLAN_ONLY: Arg = Arg::switch(
    "plan-only",
    "Answer what would be fetched and stop. Nothing is read from the project.",
);

/// Encode one kernel request. The command never hand-builds a kernel struct:
/// it offers what it read as JSON, and the kernel's `deny_unknown_fields` is
/// what stops a second vocabulary growing here.
macro_rules! request {
    ($ty:path, $value:expr) => {
        serde_json::from_value::<$ty>($value).map_err(|error| {
            Failure::invalid(KERNEL_REFUSED.code, error.to_string()).remedy(KERNEL_REFUSED.remedy)
        })
    };
}

macro_rules! refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

refusal!(
    NO_PINS,
    "pinned_set_empty",
    "no --transformer was named, so there is no pinned working set to answer for",
    "name the pinned transformers with --transformer <name>, repeated"
);
refusal!(
    TOO_MANY_PINS,
    "pinned_set_too_large",
    "more pinned transformers were named than one command reads",
    "pin fewer than 120 transformers, or split the set across two runs"
);
refusal!(
    HELD_UNREADABLE,
    "pinned_held_inventory_unreadable",
    "the --held file cannot be read, is not JSON, or is not the inventory shape",
    "pass a readable JSON array of {name, layers:{class:count}, version, complete}"
);
refusal!(
    KERNEL_REFUSED,
    "pinned_context_refused",
    "the kernel declined the plan, the style resolution or the merge",
    "read the message: it names the request that was malformed"
);
refusal!(
    NO_STYLES,
    "pinned_style_catalog_empty",
    "the project publishes no style catalogue, so no keep-set can be computed",
    "publish the project's styles, or run with --plan-only"
);

const REFUSALS: &[Refusal] = &[
    super::transformer::NATIVE_PROFILE,
    super::transformer::NATIVE_PROFILE_DIGEST,
    super::transformer::NATIVE_PROFILE_UNSAFE,
    super::transformer::HEADLESS_SIGNED_OUT,
    super::transformer::HEADLESS_NO_PROJECT,
    super::transformer::PROJECT_CONTEXT_STALE,
    NO_PINS,
    TOO_MANY_PINS,
    HELD_UNREADABLE,
    NO_STYLES,
    KERNEL_REFUSED,
];

pub static COMMAND: Command = Command {
    id: "design.pinned.preview",
    path: &["design", "pinned", "preview"],
    contract: 1,
    summary: "Plan, fetch and fold a read-only pinned working set, with no browser.",
    purpose: "\
Answers what a pinned working set costs, with the kernel decision the map uses. \
A room already held is reused; a room missing ONE design class asks for that \
class, not the room; only a moved head revision forces a whole room. Rooms of \
the same class fold into ONE collection carrying the fields that class's pinned \
style document reads, plus the transformer each feature came from. It writes \
nothing and opens no edit context; --plan-only reads nothing at all. \
docs/reference/design.md#pinned-context has the rest.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER,
        FOCUS,
        HIDE,
        REQUIRE,
        HELD,
        FORCE,
        PLAN_ONLY,
        LANE_ARG,
    ],
    output: "\
Lane and selected-project identity; `plan` (reused and fetched room counts, the \
layer-level fetches, the batched requests a browser would make, and the read \
projection); and unless --plan-only, `merge` \u{2014} sources and layers before and \
after the fold, the property keys the rooms carried and how many survive, and \
one bounded row per merged class. Read failures are listed, never fatal.",
    examples: &[
        Example {
            command: "ds design pinned preview --transformer AGASHARU --transformer GITEGA --plan-only --output json",
            note: "`.data.plan.counts.rooms_fetched` is what a cold machine downloads.",
            runnable: false,
        },
        Example {
            command: "ds design pinned preview --transformer AGASHARU --held ./held.json --output json",
            note: "`.data.merge.sources_before` vs `.sources_after` is the map-source saving.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    requires: Requires::Server,
    search: &["context", "neighbouring", "reference", "cost", "rooms"],
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let pins: Vec<String> = inputs.repeated("transformer").to_vec();
    if pins.is_empty() {
        return Err(Failure::invalid(NO_PINS.code, NO_PINS.when).remedy(NO_PINS.remedy));
    }
    if pins.len() > MAX_PINS {
        return Err(Failure::invalid(
            TOO_MANY_PINS.code,
            format!(
                "{} pinned transformers were named; {MAX_PINS} is the cap",
                pins.len()
            ),
        )
        .remedy(TOO_MANY_PINS.remedy));
    }
    let held = held_inventory(inputs.value("held"))?;

    // The heads. `list_transformers_status` is the project's own register, and
    // a head that has MOVED is the one reason a held room is refetched whole.
    let status = ds_cli_auth::transformer_status(
        lane,
        &TransformerSet::new(std::iter::empty::<String>())
            .map_err(|error| Failure::invalid("invalid_transformer_scope", error.to_string()))?,
    )?;
    let heads: Vec<Value> = status
        .result()
        .rows()
        .iter()
        .map(|row| json!({"name": row.name(), "version": head_version(row.row())}))
        .collect();

    let plan = pinned_context::plan(request!(
        pinned_context::PlanRequest,
        json!({
            "schema": SCHEMA,
            "pins": pins,
            "focus": inputs.value("focus"),
            "force": inputs.switch("force"),
            "require": inputs.repeated("require"),
            "held": held,
            "heads": heads,
        })
    )?)
    .map_err(refused)?;

    let mut output = super::transformer::project_receipt(&status);
    let object = output.as_object_mut().expect("receipt is an object");
    object.insert("plan".into(), plan_projection(&plan));
    if inputs.switch("plan-only") {
        return Ok(output);
    }

    // Read exactly what the plan named. A per-name failure is partitioned: one
    // unreadable room never costs the operator the rest of the set.
    let mut rooms: Vec<Value> = Vec::new();
    let mut failures: Vec<Value> = Vec::new();
    for name in plan
        .reuse
        .iter()
        .map(|item| &item.name)
        .chain(plan.fetch.iter().map(|item| &item.name))
    {
        match ds_cli_auth::transformer_context(lane, name) {
            Ok(context) => rooms.push(json!({
                "name": context.snapshot().transformer_name(),
                "layers": context.snapshot().layers(),
            })),
            Err(failure) => failures.push(json!({"name": name, "message": failure.message()})),
        }
    }

    let classes: Vec<String> = rooms
        .iter()
        .filter_map(|room| room["layers"].as_object())
        .flat_map(Map::keys)
        .cloned()
        .collect();
    let catalog = ds_cli_auth::style_catalog(lane)?;
    if catalog.result().document().get("styles").is_none() {
        return Err(Failure::conflict(NO_STYLES.code, NO_STYLES.when).remedy(NO_STYLES.remedy));
    }
    let resolved = pinned_context::styles(request!(
        pinned_context::StylesRequest,
        json!({
            "schema": SCHEMA,
            "catalog": catalog.result().document(),
            "classes": classes,
        })
    )?)
    .map_err(refused)?;
    let styles: BTreeMap<String, Value> = resolved
        .classes
        .iter()
        .map(|class| (class.class_name.clone(), class.document.clone()))
        .collect();

    let merged = pinned_context::merge(request!(
        pinned_context::MergeRequest,
        json!({
            "schema": SCHEMA,
            "rooms": rooms,
            "hidden": inputs.repeated("hide"),
            "styles": styles,
        })
    )?)
    .map_err(refused)?;

    let object = output.as_object_mut().expect("receipt is an object");
    object.insert("merge".into(), merge_projection(&merged, &resolved));
    object.insert("failures".into(), json!(failures));
    Ok(output)
}

/// The plan, as a receipt: the counts, and the layer-level fetches named. The
/// whole-room fetches are counted, not listed — a cold machine fetches them
/// all, and printing a hundred names says nothing the count does not.
fn plan_projection(plan: &Plan) -> Value {
    let partial: Vec<Value> = plan
        .fetch
        .iter()
        .filter(|item| !item.whole_room)
        .map(|item| json!({"name": item.name, "layers": item.layers}))
        .collect();
    json!({
        "counts": {
            "pinned": plan.counts.pinned,
            "reused": plan.counts.reused,
            "rooms_fetched": plan.counts.rooms_fetched,
            "layers_fetched": plan.counts.layers_fetched,
            "requests": plan.counts.requests,
        },
        "layer_fetches": partial,
        "projection": plan.projection,
    })
}

/// The fold, as a receipt: the three numbers, then one bounded row per class.
fn merge_projection(merged: &Merged, resolved: &Styles) -> Value {
    let rows: Vec<Value> = merged
        .layers
        .iter()
        .take(MAX_LAYER_ROWS)
        .map(|layer| {
            let style: Option<&ClassStyle> = resolved
                .classes
                .iter()
                .find(|class| class.class_name == layer.class_name);
            let named: Vec<&String> = layer.fields.iter().take(MAX_FIELD_NAMES).collect();
            json!({
                "class": layer.class_name,
                "geometry_type": layer.geometry_type,
                "features": layer.feature_count,
                "transformers": layer.transformers.len(),
                "style_ref": style.and_then(|class| class.style_ref.clone()),
                "pinned_variant": style.is_some_and(|class| class.pinned_variant),
                "fields": named,
                "fields_omitted": layer.fields.len().saturating_sub(MAX_FIELD_NAMES),
            })
        })
        .collect();
    json!({
        "source": merged.source,
        "rooms": merged.totals.rooms,
        "hidden": merged.totals.hidden,
        "sources_before": merged.totals.sources_before,
        "sources_after": merged.totals.sources_after,
        "layers_before": merged.totals.layers_before,
        "layers_after": merged.totals.layers_after,
        "features": merged.totals.features,
        "properties_before": merged.totals.properties_before,
        "properties_after": merged.totals.properties_after,
        "properties_dropped": merged.totals.properties_dropped,
        "identity_property": PINNED_TRANSFORMER_PROPERTY,
        "layers": rows,
        "layers_omitted": merged.layers.len().saturating_sub(MAX_LAYER_ROWS),
    })
}

/// The head revision the register carries for one transformer, or null. The
/// kernel treats an unknown revision as absence of evidence, never staleness.
fn head_version(row: &Value) -> Value {
    row.get("metadata")
        .and_then(|metadata| metadata.get("version"))
        .and_then(|version| {
            version
                .as_i64()
                .or_else(|| version.as_f64().map(|n| n as i64))
        })
        .map_or(Value::Null, Value::from)
}

fn held_inventory(path: Option<&str>) -> Result<Value, Failure> {
    let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(json!([]));
    };
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::invalid(HELD_UNREADABLE.code, format!("{path}: {error}"))
            .remedy(HELD_UNREADABLE.remedy)
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid(HELD_UNREADABLE.code, format!("{path}: {error}"))
            .remedy(HELD_UNREADABLE.remedy)
    })?;
    if !value.is_array() {
        return Err(Failure::invalid(
            HELD_UNREADABLE.code,
            format!("{path}: the held inventory is a JSON array of rooms"),
        )
        .remedy(HELD_UNREADABLE.remedy));
    }
    Ok(value)
}

fn refused(message: String) -> Failure {
    Failure::invalid(KERNEL_REFUSED.code, message).remedy(KERNEL_REFUSED.remedy)
}

pub fn render(data: &Value) -> String {
    let plan = &data["plan"]["counts"];
    let mut out = format!(
        "pinned context in {} ({}) · {} pinned · {} reused · {} rooms + {} layers fetched in {} requests\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        plan["pinned"].as_u64().unwrap_or(0),
        plan["reused"].as_u64().unwrap_or(0),
        plan["rooms_fetched"].as_u64().unwrap_or(0),
        plan["layers_fetched"].as_u64().unwrap_or(0),
        plan["requests"].as_u64().unwrap_or(0),
    );
    for row in data["plan"]["layer_fetches"]
        .as_array()
        .into_iter()
        .flatten()
    {
        out.push_str(&format!(
            "  {} needs only {}\n",
            row["name"].as_str().unwrap_or("?"),
            row["layers"]
                .as_array()
                .map(|layers| layers
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default(),
        ));
    }
    let Some(merge) = data.get("merge").filter(|merge| !merge.is_null()) else {
        return out;
    };
    out.push_str(&format!(
        "  {} sources → {} · {} layers → {} · {} properties → {} ({} dropped)\n",
        merge["sources_before"].as_u64().unwrap_or(0),
        merge["sources_after"].as_u64().unwrap_or(0),
        merge["layers_before"].as_u64().unwrap_or(0),
        merge["layers_after"].as_u64().unwrap_or(0),
        merge["properties_before"].as_u64().unwrap_or(0),
        merge["properties_after"].as_u64().unwrap_or(0),
        merge["properties_dropped"].as_u64().unwrap_or(0),
    ));
    for row in merge["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<24} {:>7} features · {} transformers · keeps {}\n",
            row["class"].as_str().unwrap_or("?"),
            row["features"].as_u64().unwrap_or(0),
            row["transformers"].as_u64().unwrap_or(0),
            row["fields"]
                .as_array()
                .map(|fields| fields
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default(),
        ));
    }
    for row in data["failures"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {} · {}\n",
            row["name"].as_str().unwrap_or("?"),
            row["message"].as_str().unwrap_or("unreadable"),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::output::{Format, Output};

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    /// The command may not grow a second opinion about what to fetch. Every
    /// number it prints is read out of the kernel's own answer.
    #[test]
    fn the_plan_receipt_is_the_kernels_answer_unchanged() {
        let plan = pinned_context::plan(
            request!(
                pinned_context::PlanRequest,
                json!({
                    "schema": SCHEMA,
                    "pins": ["T1", "T2"],
                    "held": [{"name": "T1", "layers": {"tr": 1, "customers": 9}, "version": 4}],
                    "heads": [{"name": "T1", "version": 4}, {"name": "T2", "version": 2}],
                })
            )
            .expect("the request encodes"),
        )
        .expect("the kernel plans");
        let receipt = plan_projection(&plan);
        assert_eq!(receipt["counts"]["reused"], 1);
        assert_eq!(receipt["counts"]["rooms_fetched"], 1);
        assert_eq!(receipt["projection"], pinned_context::CONTEXT_PROJECTION);
    }

    /// A room the machine holds but which is missing ONE class must ask for
    /// that class, and the receipt must SAY which — that is the whole point.
    #[test]
    fn a_partial_room_names_the_one_layer_it_still_needs() {
        let plan = pinned_context::plan(
            request!(
                pinned_context::PlanRequest,
                json!({
                    "schema": SCHEMA,
                    "pins": ["T1"],
                    "require": ["tr", "lv_lines"],
                    "held": [{"name": "T1", "layers": {"tr": 1}}],
                })
            )
            .expect("the request encodes"),
        )
        .expect("the kernel plans");
        let receipt = plan_projection(&plan);
        assert_eq!(receipt["counts"]["rooms_fetched"], 0);
        assert_eq!(receipt["counts"]["layers_fetched"], 1);
        assert_eq!(receipt["layer_fetches"][0]["layers"], json!(["lv_lines"]));
    }

    #[test]
    fn an_empty_pin_set_refuses_with_a_remedy() {
        let inputs = ds_cli_contract::args::parse(&COMMAND, &[]).expect("defaults parse");
        let failure = run(&inputs, &context()).expect_err("no pins is a refusal");
        assert_eq!(failure.code(), NO_PINS.code);
        assert!(
            failure
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("--transformer")),
            "the refusal must name the flag that fixes it",
        );
    }

    #[test]
    fn a_held_inventory_that_is_not_an_inventory_refuses_with_a_remedy() {
        let failure = held_inventory(Some("/nonexistent/held.json")).expect_err("unreadable");
        assert_eq!(failure.code(), HELD_UNREADABLE.code);
        assert!(failure.remedy_text().is_some());
    }
}
