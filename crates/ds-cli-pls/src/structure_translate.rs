//! `ds pls structure-translate` — a local design's structure models, named
//! into the canonical library and accepted by their bill of quantities.
//!
//! The conversion, the staking reader, the map check, the re-binding, the
//! engine application, the blind staking table and the comparison are all
//! owned by `ds-grid-tasks` and the exchange crate behind it. This adapter
//! turns the live CLI contract into the task's typed request and presents
//! its receipt.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_tasks::{
    TranslatePlsStructureModelsRequest, TranslationExpectedLocation, TranslationMappingEntry,
    translate_pls_structure_models, translate_pls_structure_models_request_schema,
};
use serde_json::{Value, json};

use crate::{
    bounded_limit, encode, file_digest, output_path, source_directory, source_path, task_failure,
};

pub static COMMAND: Command = Command {
    id: "pls.structure-translate",
    path: &["pls", "structure-translate"],
    contract: 1,
    summary: "Name a local design's structure models into the canonical library.",
    purpose: "Translates the structure models of a local designer's PLS-CADD backup (EDCL-style names such as S190_1p_strain_12) into canonical library members, one local model to one canonical member, never by renaming or swapping bytes. The design's staking table is the key: a dry run shows per local model the bill-of-quantities signatures its rows carry, so a reviewer decides the map (--mapping, --map). The write brings each member in with its exact library bytes, re-binds every wire onto the member's sets, and accepts the result by comparing DS's blind staking table with the design's. Without a staking table the baseline is the design as DS reads it. Missing members are reported, never borrowed.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "backup",
            "<path>",
            "The local design's PLS-CADD backup (raw or ZIP-wrapped).",
        )
        .required(),
        Arg::value(
            "crs",
            "<code>",
            "The design's projected CRS (native members carry none); `rwanda-tm` is the Rwanda TM (EDCL) grid.",
        )
        .required(),
        Arg::value(
            "expect-lon",
            "<degrees>",
            "Assert where the design lands. Requires --expect-lat and --expect-radius-km.",
        ),
        Arg::value("expect-lat", "<degrees>", "See --expect-lon."),
        Arg::value(
            "expect-radius-km",
            "<km>",
            "How far from --expect-lon/--expect-lat is still acceptable.",
        ),
        Arg::value(
            "staking",
            "<xlsx>",
            "The design's staking table: the evidence and the acceptance baseline. Omitted, DS reads the table from the design as it stands.",
        ),
        Arg::value(
            "sheet",
            "<name>",
            "The staking sheet; omitted, the first sheet with a staking header.",
        ),
        Arg::repeated(
            "library",
            "<dir>",
            "A canonical library directory of structure files; repeat for several, the first holding a name wins.",
        ),
        Arg::value(
            "mapping",
            "<json>",
            "A reviewed map (`ds.structure-translation-map/v1`); a dry run's mapping_template is its starting point.",
        ),
        Arg::repeated(
            "map",
            "<local>=<canonical>",
            "One decision, e.g. `S190_2p_12.str=j-w-S190.012`; replaces --mapping's for that local model.",
        ),
        Arg::switch(
            "allow-partial",
            "Write although some local models stay undecided, missing or held back.",
        ),
        Arg::value(
            "join-tolerance-m",
            "<metres>",
            "How far a staking row may stand from its placed structure.",
        )
        .default("0.05"),
        Arg::value(
            "source-sha256",
            "<sha256:…>",
            "The backup's digest; required to write.",
        ),
        Arg::value(
            "out",
            "<new-dir>",
            "Absent output root: translated .dsgrid, blind staking table, map and receipt.",
        ),
        Arg::value("limit", "<n>", "Cap each listed collection in the receipt.").default("50"),
        Arg::switch(
            "dry-run",
            "Translate and compare in memory; write nothing.",
        ),
    ],
    output: "source and baseline identity; per local model its decision, status, library member and staking evidence; counts; members imported; the set re-binding; placements held back; the acceptance (column totals, difference patterns, same_structure_bom, same_site_quantities); complete; refusal_on_write; a mapping_template to review; and, when written, the artifacts.",
    examples: &[
        Example {
            command: "ds pls structure-translate --backup './HUYE ASCENT MV R1.bak' --crs rwanda-tm --staking './MV STAKING TABLE.xlsx' --library './canonical/structures' --dry-run --output json",
            note: "Read the evidence and the library; the receipt's mapping_template is what the reviewer fills.",
            runnable: false,
        },
        Example {
            command: "ds pls structure-translate --backup './HUYE ASCENT MV R1.bak' --crs rwanda-tm --staking './MV STAKING TABLE.xlsx' --library './canonical/structures' --mapping ./huye-map.json --map 'EXISTING-TAP=ex-s-S800.012' --source-sha256 'sha256:…' --out ./huye-canonical --yes --output json",
            note: "Write the translated model, DS's blind staking table and the acceptance receipt.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "source_not_found",
            when: "--backup is not a file, or a --library, --staking or --mapping path does not exist",
            remedy: "check each path",
        },
        Refusal {
            code: "library_required",
            when: "no --library is given",
            remedy: "name at least one canonical library directory",
        },
        Refusal {
            code: "invalid_decision",
            when: "a --map value is not `<local>=<canonical>`",
            remedy: "write it as `--map 'S190_2p_12.str=j-w-S190.012'`",
        },
        Refusal {
            code: "incomplete_expected_location",
            when: "only some of --expect-lon, --expect-lat and --expect-radius-km were given",
            remedy: "pass all three, or none",
        },
        Refusal {
            code: "invalid_number",
            when: "an --expect-* value, --join-tolerance-m or --limit is not a number in bounds",
            remedy: "pass decimal degrees, kilometres, metres, and a whole-number limit",
        },
        Refusal {
            code: "confirmation_required",
            when: "neither --dry-run nor --yes was given",
            remedy: "run with --dry-run first; then repeat with --yes",
        },
        Refusal {
            code: "mode_conflict",
            when: "--dry-run and --yes were both given",
            remedy: "choose exactly one mode",
        },
        Refusal {
            code: "output_required",
            when: "--yes without --out",
            remedy: "name a new output directory with --out",
        },
        Refusal {
            code: "output_exists",
            when: "--out already exists",
            remedy: "choose a new immutable output root",
        },
        Refusal {
            code: "missing_digest_pin",
            when: "--yes without --source-sha256",
            remedy: "use the observed digest returned in detail and retry",
        },
        Refusal {
            code: "task_refused",
            when: "the owner refused: conversion, staking table, library, map, engine validation, an incomplete translation without --allow-partial, or nothing to translate",
            remedy: "read detail.code and detail.detail; a dry run shows refusal_on_write before a write",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    search: &[
        "translate",
        "rename structures",
        "structure names",
        "canonical library",
        "staking table",
        "bill of quantities",
        "boq",
        "edcl",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

fn number(inputs: &Inputs, name: &str) -> Result<Option<f64>, Failure> {
    let Some(raw) = inputs.value(name) else {
        return Ok(None);
    };
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(Some)
        .ok_or_else(|| {
            Failure::invalid("invalid_number", format!("`--{name}` is not a number"))
                .remedy("pass decimal degrees, kilometres or metres")
        })
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = match (inputs.switch("dry-run"), context.confirmed) {
        (true, false) => false,
        (false, true) => true,
        (true, true) => {
            return Err(Failure::invalid(
                "mode_conflict",
                "--dry-run and --yes cannot be combined",
            )
            .remedy("choose exactly one mode"));
        }
        (false, false) => {
            return Err(Failure::invalid(
                "confirmation_required",
                "choose a dry run or confirm the write",
            )
            .remedy("run with --dry-run first; then repeat with --yes"));
        }
    };
    let source = source_path(inputs.require("backup")?, "backup")?;
    let libraries = inputs.repeated("library");
    if libraries.is_empty() {
        return Err(Failure::invalid(
            "library_required",
            "a translation needs a canonical library",
        )
        .remedy("name at least one canonical library directory with --library"));
    }
    let library_directories = libraries
        .iter()
        .map(|raw| source_directory(raw, "library"))
        .collect::<Result<Vec<_>, _>>()?;
    let staking_path = inputs
        .value("staking")
        .map(|raw| source_path(raw, "staking"))
        .transpose()?;
    let mapping_path = inputs
        .value("mapping")
        .map(|raw| source_path(raw, "mapping"))
        .transpose()?;
    let mapping = inputs
        .repeated("map")
        .iter()
        .map(|raw| {
            let (local, canonical) = raw.split_once('=').ok_or_else(|| {
                Failure::invalid(
                    "invalid_decision",
                    format!("`{raw}` is not local=canonical"),
                )
                .remedy("write it as `--map 'S190_2p_12.str=j-w-S190.012'`")
            })?;
            if local.trim().is_empty() || canonical.trim().is_empty() {
                return Err(Failure::invalid(
                    "invalid_decision",
                    format!("`{raw}` names an empty side"),
                )
                .remedy("write it as `--map 'S190_2p_12.str=j-w-S190.012'`"));
            }
            Ok(TranslationMappingEntry {
                local: local.trim().to_string(),
                canonical: canonical.trim().to_string(),
                note: None,
                decided_by: Some("--map".to_string()),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected_location = match (
        number(inputs, "expect-lon")?,
        number(inputs, "expect-lat")?,
        number(inputs, "expect-radius-km")?,
    ) {
        (None, None, None) => None,
        (Some(longitude_deg), Some(latitude_deg), Some(radius_km)) => {
            Some(TranslationExpectedLocation {
                longitude_deg,
                latitude_deg,
                radius_km,
            })
        }
        _ => {
            return Err(Failure::invalid(
                "incomplete_expected_location",
                "the expected location needs --expect-lon, --expect-lat and --expect-radius-km",
            )
            .remedy("pass all three, or none"));
        }
    };
    let join_tolerance_m = number(inputs, "join-tolerance-m")?.unwrap_or(0.05);
    let limit = bounded_limit(
        inputs.value("limit"),
        &translate_pls_structure_models_request_schema(),
        "limit",
    )?;
    let (output_root, expected_source_sha256) = if writing {
        let out = inputs.value("out").ok_or_else(|| {
            Failure::invalid("output_required", "a write needs --out")
                .remedy("name a new output directory with --out")
        })?;
        let output_root = output_path(out)?;
        let Some(pin) = inputs.value("source-sha256") else {
            return Err(Failure::invalid(
                "missing_digest_pin",
                "a translation write is digest-pinned",
            )
            .remedy("pin the observed digest below with --source-sha256")
            .detail(json!({ "observed": file_digest(&source) })));
        };
        (Some(output_root), Some(pin.to_string()))
    } else {
        (None, inputs.value("source-sha256").map(str::to_string))
    };

    let request = TranslatePlsStructureModelsRequest {
        source_backup_path: source,
        expected_source_sha256,
        declared_crs: inputs.require("crs")?.to_string(),
        expected_location,
        staking_path,
        staking_sheet: inputs.value("sheet").map(str::to_string),
        library_directories,
        mapping_path,
        mapping,
        allow_partial: inputs.switch("allow-partial"),
        output_root,
        join_tolerance_m,
        limit,
    };
    let result = translate_pls_structure_models(&request)
        .map_err(|error| task_failure(&error.code, &error.detail))?;
    encode(&result)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} {}\n  placements  {} of {} carried onto canonical members ({} held back)\n",
        if data["status"] == "written" {
            "Translated"
        } else {
            "Dry run"
        },
        data["source"]["project_name"].as_str().unwrap_or(""),
        data["counts"]["retyped"],
        data["counts"]["placements"],
        data["counts"]["held_back"],
    );
    for local in data["local_models"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<32} {:>5}  -> {:<26} {}\n",
            local["local_type"].as_str().unwrap_or(""),
            local["placements"].as_u64().unwrap_or(0),
            local["canonical"].as_str().unwrap_or("?"),
            local["status"].as_str().unwrap_or(""),
        ));
    }
    let acceptance = &data["acceptance"];
    out.push_str(&format!(
        "  acceptance  structure BOM {} · site quantities {} · {} difference pattern(s)\n",
        if acceptance["same_structure_bom"] == true {
            "same"
        } else {
            "differs"
        },
        if acceptance["same_site_quantities"] == true {
            "same"
        } else {
            "differ"
        },
        acceptance["pattern_count"],
    ));
    if let Some(code) = data["refusal_on_write"].as_str() {
        out.push_str(&format!("  a write would refuse: {code}\n"));
    }
    for artifact in data["artifacts"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  wrote {}  {}\n",
            artifact["role"].as_str().unwrap_or(""),
            artifact["path"].as_str().unwrap_or("")
        ));
    }
    out
}
