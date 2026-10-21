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
//! The classification and the capability matrix are both the engine's. `ds`
//! reads bytes, hands them over, and shapes the answer.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::conversion::{CapabilityState, conversion_capabilities, inspect_sources};
use ds_grid_exchange::material_history::inspect_material_history;
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
    refusals: &[
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
    ],
    reference: Some("docs/reference/dsgrid-exchange.md"),
    search: &["creep", "load history", "retained criteria"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

/// Whether a capability is worth offering by default.
///
/// Matched on the enum, not on its `Debug` spelling. An earlier version
/// compared `format!("{:?}", state)` against `"Available"` — a variant that
/// does not exist — so every capability was filtered out and a source set
/// with six ready conversions reported "none available". A stringly-typed
/// comparison against a name nobody checked is how that happens.
///
/// `Unverified` is included deliberately: a path that exists but has not been
/// verified for these inputs is something a caller may want to attempt, and
/// its reason says so. `Unsupported` and `NotImplemented` are not offers.
pub const fn offerable(state: CapabilityState) -> bool {
    matches!(state, CapabilityState::Ready | CapabilityState::Unverified)
}

/// The state's stable lowercase token. `ds` reports states in snake_case
/// everywhere; the engine's `Debug` spelling is an implementation detail.
pub const fn state_token(state: CapabilityState) -> &'static str {
    match state {
        CapabilityState::Ready => "ready",
        CapabilityState::Unsupported => "unsupported",
        CapabilityState::NotImplemented => "not_implemented",
        CapabilityState::Unverified => "unverified",
    }
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let history_limit = if inputs.switch("material-history") {
        Some(ds_cli_dsgrid::package::parse_limit(inputs.value("limit"))?)
    } else {
        None
    };
    let loaded = sources::load(inputs.repeated("source"))?;
    if let Some(limit) = history_limit {
        let history = inspect_material_history(&loaded.sources, limit).map_err(|error| {
            Failure::invalid(error.code(), error.to_string()).remedy(error.remedy())
        })?;
        return Ok(json!({
            "material_history": history,
            "byte_len": loaded.byte_len,
            "warnings": loaded.warnings,
        }));
    }
    let inspection = inspect_sources(&loaded.sources);
    let capabilities = conversion_capabilities(&inspection);

    let show_blocked = inputs.switch("blocked");
    let capability_values: Vec<Value> = capabilities
        .iter()
        .filter(|capability| show_blocked || offerable(capability.state))
        .map(|capability| {
            json!({
                "id": capability.id,
                "state": state_token(capability.state),
                "reason": capability.reason,
            })
        })
        .collect();

    let candidates: Vec<Value> = inspection
        .candidates
        .iter()
        .map(|candidate| {
            json!({
                "name": candidate.display_name,
                "kind": format!("{:?}", candidate.kind),
                "digest": candidate.digest,
                "members": candidate.member_count,
                "version_evidence": candidate.version_evidence,
                "units_evidence": candidate.units_evidence,
                "counts": candidate.counts,
                "gis_layers": candidate.gis_layers,
            })
        })
        .collect();

    let mut answer = json!({
        "sources": candidates,
        "byte_len": loaded.byte_len,
        "capabilities": capability_values,
        "warnings": loaded.warnings,
    });

    if !show_blocked {
        let blocked = capabilities
            .iter()
            .filter(|capability| !offerable(capability.state))
            .count();
        if blocked > 0 {
            answer["more"] = json!({
                "blocked": blocked,
                "next": "ds dsgrid-exchange inspect --blocked",
            });
        }
    }

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
