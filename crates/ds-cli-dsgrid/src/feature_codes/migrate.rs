//! `ds dsgrid feature-codes migrate` — map every survey token of a model to a
//! standard code (normalise → alias → heuristics → residue, exactly as the
//! standard's classifier; legacy FEA numbers through the standard's legacy
//! map) and re-classify the points as one revision.

use std::collections::BTreeMap;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::feature_code_report;
use ds_grid_engine::{MigrationError, MigrationOptions, plan_migration};
use serde_json::{Value, json};

use crate::mutation::{self, Planned};
use crate::package;

const MAX_PROPOSALS_BYTES: u64 = 16 * 1024 * 1024;

const OWN: &[Refusal] = &[
    super::STANDARD_REFUSALS[0],
    super::STANDARD_REFUSALS[1],
    super::STANDARD_REFUSALS[2],
    super::STANDARD_REFUSALS[3],
    Refusal {
        code: "voltage_class_required",
        when: "the model's table carries no single voltage class and --voltage-class was not given",
        remedy: "pass --voltage-class MV (or the class of the model), or run `feature-codes import` first",
    },
    Refusal {
        code: "proposals_invalid",
        when: "--proposals is not a readable JSON list of {token, code} accepted proposals",
        remedy: "write the accepted residue as `[{\"token\": \"…\", \"code\": 30}, …]`",
    },
    Refusal {
        code: "feature_code_unknown_in_delivery",
        when: "--deliver was given and one or more tokens would stay UNKNOWN (999)",
        remedy: "accept proposals for the listed tokens (--proposals) or fix the survey; a delivered model carries no unknown code",
    },
    Refusal {
        code: "invalid_limit",
        when: "--limit is not a whole number in 1..5000",
        remedy: "pass a limit inside the range, or omit it for the default of 50",
    },
];

const REFUSALS: &[Refusal; OWN.len() + mutation::REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + mutation::REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + mutation::REFUSALS.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < mutation::REFUSALS.len() {
        all[OWN.len() + shared] = mutation::REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.feature-codes.migrate",
    path: &["dsgrid", "feature-codes", "migrate"],
    contract: 1,
    summary: "Map every survey token to a standard feature code, as one revision.",
    purpose: "\
Classifies every terrain point's survey token exactly as the standard's \
classifier does — normalise, exact alias, heuristics, residue — and joins \
legacy FEA numbers through the standard's legacy code map (Nyamagabe: 4 → \
SWAMP 20, 5 → FOREST_EDGE 51, 6 → ROAD_PUBLIC 30, 7 → RIVER_EDGE 22, 8 → \
GROUND 200, 9 → T_OFF 4). Trailing numbers and `(NNkva)` become description \
fields and the description is rendered from the code's template; a token \
nothing matches becomes UNKNOWN (999) with the raw token kept as its \
description. The receipt is the mapping table, the counts per code, the \
residue and the list of 999s. --deliver refuses while any 999 remains. \
Points already on a standard code are left alone; --proposals feeds accepted \
residue back as exact aliases for a second pass.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
        super::STANDARD_ARG,
        super::VOLTAGE_CLASS_ARG,
        Arg::value(
            "proposals",
            "<json-path>",
            "Accepted residue proposals: a JSON list of {token, code} applied as exact aliases.",
        ),
        Arg::switch(
            "deliver",
            "Refuse unless every token maps to a standard code (no UNKNOWN 999).",
        ),
        Arg::value("limit", "<n>", "Cap the mapping rows listed.").default(package::DEFAULT_LIMIT),
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
    ],
    output: "\
The family receipt plus the mapping table (token, count, code, name, \
confidence, reason, fields, residue), the legacy map applied, counts per \
code, the residue, the UNKNOWN tokens, disagreements between a legacy number \
and its text, and the before/after report totals.",
    examples: &[
        Example {
            command: "ds dsgrid feature-codes migrate --model local-e9b0ccbf92d7447b --standard pls-feature-codes.v1.json --dry-run --output json",
            note: "The mapping table and counts per code, nothing written.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid feature-codes migrate --model local-e9b0ccbf92d7447b --standard pls-feature-codes.v1.json --deliver --yes",
            note: "Migrate for delivery: refused while any token would stay UNKNOWN.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "feature code",
        "classify",
        "survey token",
        "alias",
        "unknown code",
        "999",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

fn read_proposals(raw_path: Option<&str>) -> Result<BTreeMap<String, u32>, Failure> {
    let Some(raw_path) = raw_path else {
        return Ok(BTreeMap::new());
    };
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("proposals_invalid", format!("cannot read `{raw_path}`"))
            .remedy("--proposals takes one JSON file")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_PROPOSALS_BYTES {
        return Err(Failure::invalid(
            "proposals_invalid",
            format!("`{raw_path}` is not a bounded JSON file"),
        )
        .remedy("--proposals takes one JSON list under 16 MiB"));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::failed("proposals_invalid", format!("cannot read `{raw_path}`"))
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid("proposals_invalid", "the proposals file is not valid JSON")
            .remedy("write a JSON list of {token, code}")
            .detail(json!({ "detail": error.to_string() }))
    })?;
    let mut accepted = BTreeMap::new();
    for item in value.as_array().into_iter().flatten() {
        let token = item["token"].as_str();
        let code = item["code"]
            .as_u64()
            .or_else(|| item["accepted_code"].as_u64())
            .or_else(|| item["proposed_code"].as_u64());
        match (token, code) {
            (Some(token), Some(code)) => {
                accepted.insert(token.to_string(), code as u32);
            }
            _ => {
                return Err(Failure::invalid(
                    "proposals_invalid",
                    "every proposal needs a `token` and an accepted `code`",
                )
                .remedy("write `[{\"token\": \"…\", \"code\": 30}, …]`")
                .detail(json!({ "item": item })));
            }
        }
    }
    if accepted.is_empty() && !value.is_array() {
        return Err(Failure::invalid(
            "proposals_invalid",
            "the proposals document must be a JSON list",
        )
        .remedy("write `[{\"token\": \"…\", \"code\": 30}, …]`"));
    }
    Ok(accepted)
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = mutation::write_mode(inputs, context)?;
    let limit = package::parse_limit(inputs.value("limit"))?;
    let standard = super::load_standard(inputs)?;
    let accepted = read_proposals(inputs.value("proposals"))?;
    let deliver = inputs.switch("deliver");
    let target = mutation::Target::resolve(inputs, writing)?;
    let opened = mutation::open(target, inputs)?;
    let before = feature_code_report(opened.session.snapshot());
    let voltage_class = match inputs.value("voltage-class").map(str::trim) {
        Some(class) if !class.is_empty() => {
            standard
                .voltage_class(class)
                .map_err(super::map_standard_error)?;
            class.to_string()
        }
        _ => match before.voltage_classes.as_slice() {
            [only] => only.clone(),
            [] => "MV".to_string(),
            many => {
                return Err(Failure::invalid(
                    "voltage_class_required",
                    format!("the model's table carries {} voltage classes", many.len()),
                )
                .remedy("pass --voltage-class to say which one the migrated codes take")
                .detail(json!({ "classes": many })));
            }
        },
    };
    let plan = plan_migration(
        opened.session.snapshot(),
        &standard,
        &voltage_class,
        &MigrationOptions {
            accepted,
            legacy_map: None,
        },
    )
    .map_err(|error| match error {
        MigrationError::Standard(error) => super::map_standard_error(error),
        MigrationError::StandardCodeMissing(code) => Failure::failed(
            "standard_invalid",
            format!("the standard names no code {code}"),
        )
        .remedy("use the issued standard"),
    })?;
    if deliver && plan.unknown_point_count > 0 {
        return Err(Failure::invalid(
            "feature_code_unknown_in_delivery",
            format!(
                "{} point(s) on {} token(s) would stay UNKNOWN (999)",
                plan.unknown_point_count,
                plan.unknown_tokens.len()
            ),
        )
        .remedy("accept proposals for these tokens with --proposals, or fix the survey; nothing was written")
        .detail(json!({
            "unknown_tokens": plan.unknown_tokens,
            "residue": plan.residue,
        })));
    }
    let head = opened.head.clone();
    let planned: Vec<Planned> = plan
        .commands
        .iter()
        .enumerate()
        .map(|(index, command)| Planned {
            command_id: mutation::command_id(
                "feature-codes-migrate",
                &head,
                &format!("{voltage_class}:{}:{index}", standard.version),
            ),
            command: command.clone(),
        })
        .collect();
    let mut warnings = Vec::new();
    if !standard.pinned {
        warnings.push(json!({
            "code": "standard_unpinned",
            "message": format!("standard version {} is not one this build pins", standard.version),
        }));
    }
    if plan.unknown_point_count > 0 {
        warnings.push(json!({
            "code": "feature_code_unknown",
            "message": format!("{} point(s) stay UNKNOWN (999): {}", plan.unknown_point_count,
                plan.unknown_tokens.iter().map(|row| format!("{:?} x{}", row.token, row.count)).collect::<Vec<_>>().join(", ")),
        }));
    }
    if !plan.residue.is_empty() {
        warnings.push(json!({
            "code": "residue",
            "message": format!("{} token(s) classified below the confidence floor or unmatched; review them", plan.residue.len()),
        }));
    }
    if !plan.disagreements.is_empty() {
        warnings.push(json!({
            "code": "legacy_code_text_disagreement",
            "message": format!("{} token(s) whose survey text classifies differently from their legacy code; the legacy code was kept", plan.disagreements.len()),
        }));
    }
    if !plan.untouched_line_tokens.is_empty() {
        warnings.push(json!({
            "code": "breaklines_crossings_untouched",
            "message": format!("{} breakline/crossing token(s) are not migrated by this verb", plan.untouched_line_tokens.len()),
        }));
    }
    let (mapping, mapping_withheld) = package::take(plan.mapping.clone(), limit);
    let mut extra = json!({
        "standard": super::standard_receipt(&standard),
        "voltage_class": voltage_class,
        "deliver": deliver,
        "legacy_map": plan.legacy_map_name,
        "legacy_mapping": plan.legacy_mapping,
        "mapping": mapping,
        "token_count": plan.token_count,
        "counts_per_code": plan.counts_per_code,
        "residue": plan.residue,
        "unknown_tokens": plan.unknown_tokens,
        "unknown_point_count": plan.unknown_point_count,
        "disagreements": plan.disagreements,
        "point_count": plan.point_count,
        "migrated_point_count": plan.migrated_point_count,
        "already_standard_point_count": plan.already_standard_point_count,
        "untouched_line_tokens": plan.untouched_line_tokens,
        "before": {
            "resolved_token_count": before.resolved_token_count,
            "unresolved_token_count": before.unresolved_token_count,
            "unresolved_row_count": before.unresolved_row_count,
            "unknown_code_rows": before.unknown_code_rows,
        },
    });
    if mapping_withheld > 0 {
        extra["more"] = json!({ "truncated": [{ "field": "mapping", "total": plan.token_count, "shown": limit, "withheld": mapping_withheld, "limit": limit }] });
    }
    if planned.is_empty() {
        let mut receipt = json!({
            "target": match &opened.target {
                mutation::Target::WorkingCopy { row, .. } => json!({ "kind": "working_copy", "model": row.id }),
                mutation::Target::Package { path, .. } => json!({ "kind": "package", "path": path }),
            },
            "dry_run": !writing,
            "persisted": false,
            "commands": 0,
            "source_revision": head.as_str(),
            "resulting_revision": head.as_str(),
            "changed": false,
            "warnings": warnings,
        });
        if let Value::Object(map) = extra {
            for (key, value) in map {
                receipt[key] = value;
            }
        }
        return Ok(receipt);
    }
    mutation::run(opened, planned, writing, extra, warnings, &["FEA", "XYZ"])
}

pub fn render(data: &Value) -> String {
    let mut out = mutation::render_receipt(data);
    out.push_str(&format!(
        "tokens {}  points {} (migrated {}, already standard {})  UNKNOWN {}  residue {}\n",
        data["token_count"],
        data["point_count"],
        data["migrated_point_count"],
        data["already_standard_point_count"],
        data["unknown_point_count"],
        data["residue"].as_array().map_or(0, Vec::len),
    ));
    for row in data["counts_per_code"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:>6}  {:>4} {}\n",
            row["count"],
            row["code"],
            row["name"].as_str().unwrap_or("?")
        ));
    }
    for row in data["unknown_tokens"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  unknown {:?} x{}\n",
            row["token"].as_str().unwrap_or("?"),
            row["count"]
        ));
    }
    out
}
