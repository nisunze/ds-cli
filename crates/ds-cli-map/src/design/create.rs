//! `ds map design create` — turn supplied geometry into staged design
//! features.
//!
//! This is the machine-facing half of the operator's right-click "create from
//! selection": the same filtering, the same target catalogue, available to a
//! caller that has never seen the design layers.
//!
//! Omitting `--target-layer` is a *discovery* call rather than a refusal. The
//! application answers with the design layers the supplied geometry could
//! become, and how many features each would accept or reject — so a caller
//! with no knowledge of the catalogue learns it by asking, instead of by
//! guessing a layer name and being told no.
//!
//! Like everything before `save`, a real creation stages into the local room.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::design::TRANSFORMER_ARG;
use crate::{DESCRIPTOR_ARG, MAX_CREATE_FEATURES};

const CARRY_ARG: Arg = Arg {
    name: "carry-property",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: false,
    default: None,
    choices: &[],
    summary: "Copy this property from each source feature. Repeat.",
};

const SET_ARG: Arg = Arg {
    name: "set",
    kind: ArgKind::Repeated,
    value: "<key=value>",
    required: false,
    default: None,
    choices: &[],
    summary: "Property to write on every created feature. Repeat.",
};

pub static COMMAND: Command = Command {
    id: "map.design.create",
    path: &["map", "design", "create"],
    contract: 1,
    summary: "Turn GeoJSON or a local layer into staged design features.",
    purpose: "\
Creates design features from either a GeoJSON file or an application-owned \
local layer — the machine-facing half of the operator's create-from-selection \
gesture, with the same filtering. Give exactly one of --features or \
--source-layer. Without \
--target-layer it answers which design layers the supplied geometry could \
become and how many each would accept, so the catalogue can be discovered \
rather than guessed. With one, it stages the features into the local room.",
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        Arg::value("features", "<path>", "GeoJSON file to create from."),
        Arg::value(
            "source-layer",
            "<id>",
            "Local layer id from `ds map view`; rows stay inside the application.",
        ),
        Arg::value(
            "target-layer",
            "<name>",
            "Design layer to create on. Omit to list eligible targets.",
        ),
        CARRY_ARG,
        SET_ARG,
        Arg::switch("dry-run", "Report what would be created; stage nothing."),
        DESCRIPTOR_ARG,
    ],
    output: "\
Without --target-layer, the eligible design layers with how many of the \
supplied features each accepts and rejects. With one, how many were supplied, \
accepted, created and rejected, the layer's new feature total, and `staged` \
and `persisted` separately.",
    examples: &[
        Example {
            command: "ds map design create --transformer T-1042 --features ./poles.geojson --output json",
            note: "Discovery: what could this geometry become?",
            runnable: false,
        },
        Example {
            command: "ds map design create --transformer T-1042 --features ./poles.geojson --target-layer lv_poles --set drafting_status=draft",
            note: "Stage them onto a named layer.",
            runnable: false,
        },
        Example {
            command: "ds map design create --transformer agasharu --source-layer sketch-difference --target-layer lv_lines --set drafting_status=draft --dry-run",
            note: "Preview promotion of a computed local difference into design.",
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
            when: "no such transformer, or the geometry cannot become the target layer",
            remedy: "re-run without --target-layer to list the layers this geometry can become",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::FEATURES_NOT_FOUND,
        crate::FEATURES_NOT_GEOJSON,
        crate::FEATURES_EMPTY,
        Refusal {
            code: "source_choice",
            when: "neither --features nor --source-layer was given, or both were",
            remedy: "give exactly one of --features or --source-layer",
        },
        crate::FEATURES_OVER_BOUND,
        crate::INVALID_PAIR,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let dry_run = inputs.switch("dry-run");
    let features_path = inputs.value("features");
    let source_layer = inputs.value("source-layer");
    if features_path.is_some() == source_layer.is_some() {
        return Err(Failure::invalid(
            "source_choice",
            "give exactly one of --features or --source-layer",
        ));
    }

    // No geometry-type check here, deliberately: which types are acceptable
    // is the target layer's business, and with no target named the whole
    // point of the call is to be told.
    let supplied = features_path
        .map(|path| crate::load_features(path, "features", MAX_CREATE_FEATURES))
        .transpose()?;

    let mut arguments = json!({ "transformer": transformer, "dryRun": dry_run });
    if let Some(supplied) = &supplied {
        arguments["features"] = json!(supplied.features);
    }
    if let Some(layer) = source_layer {
        arguments["sourceLayer"] = json!(layer);
    }
    if let Some(target) = inputs.value("target-layer") {
        arguments["targetLayer"] = json!(target);
    }
    let carry = inputs.repeated("carry-property");
    if !carry.is_empty() {
        arguments["carryProperties"] = json!(carry);
    }
    let properties = crate::pairs(inputs.repeated("set"), "set")?;
    if !properties.is_empty() {
        arguments["properties"] = Value::Object(properties);
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_CREATE,
        arguments,
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    // Discovery answer: no target was named, so the targets are the answer.
    if inputs.value("target-layer").is_none() {
        return Ok(json!({
            "transformer": transformer,
            "supplied": result["supplied"].as_u64().unwrap_or(0),
            "geometry": supplied.as_ref().map(crate::kinds_of).unwrap_or_else(|| vec![
                result["sourceGeometry"].as_str().unwrap_or("Unknown").to_string()
            ]),
            "source_layer": source_layer,
            "targets": result["targets"],
            "created": 0,
            "staged": false,
            "persisted": false,
        }));
    }

    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "target_layer": result["targetLayer"],
        "dry_run": result["dryRun"].as_bool().unwrap_or(dry_run),
        "supplied": result["supplied"].as_u64().unwrap_or(0),
        "source_layer": source_layer,
        "accepted": result["acceptedSources"].as_u64().unwrap_or(0),
        "created": result["created"].as_u64().unwrap_or(0),
        "rejected": result["rejected"],
        "layer_features": result["layerFeatureCount"],
        "staged": result["staged"].as_bool().unwrap_or(false),
        "persisted": result["persisted"].as_bool().unwrap_or(false),
    }))
}

pub fn render(data: &Value) -> String {
    if let Some(targets) = data["targets"].as_array() {
        let mut out = format!(
            "{} supplied ({})\n\neligible design layers:\n",
            crate::plural(data["supplied"].as_u64().unwrap_or(0), "feature"),
            data["geometry"]
                .as_array()
                .map(|kinds| {
                    kinds
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
        );
        if targets.is_empty() {
            out.push_str("  none — this geometry cannot become a design feature here\n");
            return out;
        }
        for target in targets {
            out.push_str(&format!(
                "  {:<24} {:<12} accepts {:>5}  rejects {:>5}\n",
                target["layer"].as_str().unwrap_or(""),
                target["geometryType"].as_str().unwrap_or(""),
                target["accepted"],
                target["rejected"],
            ));
        }
        out.push_str("\n  → re-run with --target-layer <name>\n");
        return out;
    }

    let mut out = format!(
        "{} created on {}\n  {} supplied  ·  {} accepted\n",
        data["created"],
        data["target_layer"].as_str().unwrap_or(""),
        data["supplied"],
        data["accepted"],
    );
    if let Some(rejected) = data["rejected"].as_array().filter(|list| !list.is_empty()) {
        out.push_str(&format!("  {} rejected\n", rejected.len()));
    }
    if data["dry_run"].as_bool().unwrap_or(false) {
        out.push_str("\ndry run; nothing was staged\n");
        return out;
    }
    out.push('\n');
    out.push_str(super::staging_note(data));
    out
}
