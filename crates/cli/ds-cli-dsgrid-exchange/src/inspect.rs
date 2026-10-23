//! `ds dsgrid-exchange inspect` — what are these files, and what can be made
//! from them?
//!
//! This is the first question anyone has about a pile of engineering data,
//! and until now the only way to answer it was to try a conversion and read
//! the failure. It classifies each source, records its exact digest, and
//! returns the engine's own capability matrix: which conversions are
//! available from this set, which are blocked, and why.
//!
//! It is read-only and writes nothing. Nothing is converted, and no output
//! path is touched — the point is to decide *whether* to convert before doing
//! it.
//!
//! Note the deliberate adjacency with `ds dsgrid inspect`, which
//! `ds capabilities --search inspect` returns alongside this one. That
//! command takes `--model` and answers **model identity**: which `.dsgrid` is
//! this, what is in it. This one takes `--source` and answers **source
//! classification**: what format are these files, what can they become. The
//! opening words of each summary carry that distinction, because that is all
//! a caller reads before choosing.
//!
//! The classification and the capability matrix are both the exchange
//! owner's, in the Network addon. `ds` reads bytes, stages them, and shapes
//! the answer.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_dsgrid::native;
use ds_network_contracts::grid_exchange::ExchangeInspectParamsV1;
use ds_network_contracts::grid_operation::GridOperationV1;
use serde_json::{Value, json};

use crate::sources;

pub static COMMAND: Command = Command {
    id: "dsgrid-exchange.inspect",
    path: &["dsgrid-exchange", "inspect"],
    contract: 2,
    summary: "Classify sources or read retained native material-history evidence.",
    purpose: "\
Answers what a pile of engineering files actually is, and what the engine can \
do with it. Each source is classified and digested; the result carries the \
capability matrix — every conversion this set supports, every one it does not, \
and the reason. Nothing is converted and nothing is written: this is the call \
that decides whether a conversion is worth attempting. With --material-history, \
read only retained original/round-trip backups or explicit CRI sources: exact \
byte hashes, native version/units and authored creep/load selectors. This \
projection does not infer or author model bindings.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        sources::SOURCE_ARG,
        Arg::switch(
            "blocked",
            "List blocked capabilities too, with their reasons.",
        ),
        Arg::switch(
            "material-history",
            "Read retained native creep/load selectors instead of conversion capabilities.",
        ),
        Arg::value(
            "limit",
            "<1..5000>",
            "Cap each material-history list; totals and omitted counts remain explicit.",
        )
        .default("50"),
    ],
    output: "\
One entry per source with its classification, digest, member count and any \
version or units evidence the engine recovered; then the capabilities, \
available ones by default. GIS sources also list their layers, geometry, feature counts and CRS evidence. \
With --material-history: bounded sources, original/round-trip backups and CRI \
history selectors, with exact source/backup/CRI SHA-256, authored filenames, \
explicit missing backups and per-list totals/omitted counts. Retained selectors \
are source evidence, not current model bindings.",
    examples: &[
        Example {
            command: "ds dsgrid-exchange inspect --source ./workspace --output json",
            note: "A PLS-CADD workspace directory, read as one folder source.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange inspect --source ./model.dsgrid --material-history --limit 50 --output json",
            note: "Read retained native history without importing or converting the model.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange inspect --source ./a.dsgrid --source ./b.dsgrid --blocked --output json",
            note: "Several sources at once, with the reasons nothing else is offered.",
            runnable: false,
        },
    ],
    refusals: &REFUSALS,
    reference: Some("docs/reference/dsgrid-exchange.md"),
    search: &["creep", "load history", "retained criteria"],
    requires: Requires::Server,
    availability: available,
};

/// The source and history refusals, then the addon's own.
const OWN: &[Refusal] = &[
    sources::SHARED_REFUSALS[0],
    sources::SHARED_REFUSALS[1],
    sources::SHARED_REFUSALS[2],
    Refusal {
        code: "invalid_limit",
        when: "--limit is outside 1..5000",
        remedy: "pass --limit 1..5000",
    },
    Refusal {
        code: "retained_backup_missing",
        when: "a package has neither retained original nor round-trip backup",
        remedy: "select a package retaining its native backup, or the explicit backup/CRI source",
    },
    Refusal {
        code: "retained_package_invalid",
        when: "a selected retained asset is missing, ambiguous or fails byte verification",
        remedy: "use the exact intact package; resolve missing, ambiguous or mismatched retained assets",
    },
    Refusal {
        code: "native_history_parse_failed",
        when: "a native backup or CRI history cannot be parsed, even beyond the output bound",
        remedy: "select intact native CRI/backup bytes with declared history weather cases; inspect the named parse failure",
    },
];
static REFUSALS: [Refusal; OWN.len() + native::REFUSALS.len()] =
    native::extend_refusals::<{ OWN.len() + native::REFUSALS.len() }>(OWN);

fn available() -> Availability {
    Availability::Available
}

/// Classify the sources, or read their retained material history, in the
/// addon. Capabilities that are neither ready nor unverified are offered
/// only with `--blocked`; the owner counts the rest under `more`.
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let material_history_limit = if inputs.switch("material-history") {
        Some(ds_cli_dsgrid::package::parse_limit(inputs.value("limit"))?)
    } else {
        None
    };
    let loaded = sources::load(inputs.repeated("source"))?;
    let outcome = crate::exchange(
        &GridOperationV1::ExchangeInspect(ExchangeInspectParamsV1 {
            sources: loaded.sources,
            blocked: inputs.switch("blocked"),
            material_history_limit,
        }),
        &loaded.inputs,
    )?;
    let mut answer: Value = crate::result(outcome)?;
    let Some(object) = answer.as_object_mut() else {
        return Err(Failure::failed(
            "network_result_invalid",
            "the exchange inspection is not an object",
        ));
    };
    object.insert("byte_len".into(), json!(loaded.byte_len));
    object.insert("warnings".into(), json!(loaded.warnings));
    Ok(answer)
}

pub fn render(data: &Value) -> String {
    if let Some(history) = data.get("material_history") {
        return format!(
            "Retained native material history\n{}\n",
            serde_json::to_string_pretty(history).unwrap_or_default()
        );
    }
    let mut out = format!(
        "{} source(s) · {} bytes\n\n",
        data["sources"].as_array().map_or(0, Vec::len),
        data["byte_len"],
    );
    for source in data["sources"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} {:<22} {} member(s)\n",
            source["name"].as_str().unwrap_or(""),
            source["kind"].as_str().unwrap_or(""),
            source["members"],
        ));
        out.push_str(&format!(
            "  {:<28} {}\n",
            "",
            source["digest"].as_str().unwrap_or("")
        ));
        for key in ["version_evidence", "units_evidence"] {
            if let Some(evidence) = source[key].as_str() {
                out.push_str(&format!("  {:<28} {key}: {evidence}\n", ""));
            }
        }
        for layer in source["gis_layers"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {:<28} layer {:<24} {:>6} {:<10} {} → {}\n",
                "",
                layer["name"].as_str().unwrap_or(""),
                layer["feature_count"],
                layer["geometry_type"].as_str().unwrap_or("unknown"),
                layer["source_crs"].as_str().unwrap_or("unknown"),
                layer["normalized_crs"].as_str().unwrap_or("unknown"),
            ));
        }
    }

    out.push_str("\nCAPABILITIES\n");
    let capabilities = data["capabilities"].as_array();
    match capabilities.filter(|list| !list.is_empty()) {
        Some(list) => {
            for capability in list {
                out.push_str(&format!(
                    "  {:<34} {}\n",
                    capability["id"].as_str().unwrap_or(""),
                    capability["state"].as_str().unwrap_or(""),
                ));
                if let Some(reason) = capability["reason"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    out.push_str(&format!("  {:<34}   {reason}\n", ""));
                }
            }
        }
        None => out.push_str("  none available from this source set\n"),
    }

    if let Some(blocked) = data["more"]["blocked"].as_u64() {
        out.push_str(&format!(
            "\n{blocked} blocked — see `ds dsgrid-exchange inspect --blocked`\n"
        ));
    }
    crate::render::list(&mut out, "WARNINGS", &data["warnings"]);
    out
}
