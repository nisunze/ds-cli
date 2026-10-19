//! `ds dsgrid report structures` — the structure list / staking table
//! (program contract 04 §5): one row per placed structure with its
//! description, station, line angle, pole family, drawing number,
//! foundation and the REG structure-rule findings, to CSV or XLSX.
//!
//! Every value comes from the engine's `report_structures` operation; this
//! module writes the file and bounds what it prints.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid_operation::{GridOperationV1, ReportStructuresParamsV1};
use serde_json::{Value, json};

use crate::{mutation, native};

const OUT_ARG: Arg = Arg::value(
    "out",
    "<file.csv|file.xlsx>",
    "Write the whole list here (never overwrites). Omitted, rows are printed, bounded by --limit.",
);
const LIMIT_ARG: Arg = Arg::value("limit", "<n>", "Rows printed in the receipt.")
    .default(crate::package::DEFAULT_LIMIT);
const ONLY_FINDINGS_ARG: Arg = Arg::switch(
    "only-findings",
    "Keep only structures that carry at least one finding.",
);

const OWN: &[Refusal] = &[
    Refusal {
        code: "output_format_unknown",
        when: "--out is neither .csv nor .xlsx",
        remedy: "name a .csv or .xlsx file",
    },
    Refusal {
        code: "rule_source_missing",
        when: "the vendored REG structure-rules standard is not the owner-issued bytes",
        remedy: "rebuild ds from a clean checkout; report the digests in detail",
    },
    Refusal {
        code: "operation_failed",
        when: "the engine could not read the model's alignment geometry",
        remedy: "run `ds dsgrid validate` on the package and read detail.engine",
    },
    Refusal {
        code: "invalid_limit",
        when: "--limit is not a whole number in 1..5000",
        remedy: "pass a limit inside the range, or omit it for the default of 50",
    },
];

/// Shared read-target and file refusals, minus the writing-only ones.
const SHARED: &[Refusal] = &[
    mutation::refusal("target_required"),
    mutation::refusal("output_exists"),
    mutation::refusal("output_parent_missing"),
    mutation::refusal("local_model_not_found"),
    mutation::refusal("local_model_ambiguous"),
    mutation::refusal("local_model_store_unavailable"),
    mutation::refusal("model_not_found"),
    mutation::refusal("not_a_dsgrid_package"),
    mutation::refusal("package_decode_failed"),
    mutation::refusal("output_unwritable"),
];

const REFUSALS: &[Refusal; OWN.len() + SHARED.len()] = &splice();
const ALL_REFUSALS: &[Refusal] =
    &native::extend_refusals::<{ REFUSALS.len() + native::REFUSALS.len() }>(REFUSALS);
const fn splice() -> [Refusal; OWN.len() + SHARED.len()] {
    let mut all = [OWN[0]; OWN.len() + SHARED.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < SHARED.len() {
        all[OWN.len() + shared] = SHARED[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.report.structures",
    path: &["dsgrid", "report", "structures"],
    contract: 1,
    summary: "Structure list: description, angle, pole, foundation, rule findings.",
    purpose: "\
Lists every placed structure of a working copy or package — id and number, \
alignment and station, the line angle from the model's own alignment \
geometry (and the native source's recorded angle beside it), structure \
type, the structure's own description, pole family / material / height / \
class / stays read from the type name, the assembly drawing number when a \
description carries one, REG v7 Table 14 foundation depth and width by \
pole height, and the rule findings the engine evaluates today \
(`structure_type_not_allowed`: an unsupported single pole carrying 10° ≤ |angle| < 60°) \
with their rule id, source clause and an empty reason slot. Written whole \
to CSV or XLSX with --out; the receipt carries the counts, the standard's \
digest, the declared assumptions and a bounded page of rows. A proposal: \
PLS-CADD confirms.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        OUT_ARG,
        ONLY_FINDINGS_ARG,
        LIMIT_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "\
`target`, `revision`, `standard` {schema, version, digest}, `assumptions[]` \
(each `assumed: true`), `verification_level: proposal`, `line_angle_definition`, \
`counts` {structures, with_description, on_alignment, findings by id}, \
`rows[]` bounded by --limit with `more.withheld`, and `artifact` {path, \
format, rows, byte_len, sha256} when --out was given.",
    examples: &[
        Example {
            command: "ds dsgrid report structures --model local-… --out structures.csv",
            note: "The whole list to CSV; the receipt says how many carry each finding.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid report structures --model local-… --only-findings --output json",
            note: "Only the structures with a finding, printed.",
            runnable: false,
        },
    ],
    refusals: ALL_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "staking table",
        "pole schedule",
        "line angle",
        "csv",
        "xlsx",
        "excel",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::package::parse_limit(inputs.value("limit"))?;
    let out = inputs.value("out").map(str::trim).filter(|v| !v.is_empty());
    let format = match out {
        None => None,
        Some(path) => Some(output_format(path)?),
    };
    if let Some(path) = out {
        crate::apply::validate_output_path(path)?;
    }

    let (target, path) = mutation::read_target_path(inputs)?;
    let bytes = crate::package::read_bytes(&path)?;
    let only_findings = inputs.switch("only-findings");
    let mut outcome = native::operation(
        &GridOperationV1::ReportStructures(ReportStructuresParamsV1 {
            only_findings,
            format: format.map(str::to_owned),
        }),
        Some(&bytes),
        &[],
    )
    .map_err(|failure| native::at_path(failure, &path))?;
    let report = std::mem::take(&mut outcome.result.result);
    let rows = report["rows"].as_array().cloned().unwrap_or_default();

    let mut artifact = Value::Null;
    if let (Some(path), Some(format)) = (out, format) {
        let bytes = outcome
            .take_artifact(&format!("structures.{format}"))
            .ok_or_else(|| {
                Failure::failed(
                    "network_result_invalid",
                    "the structure list returned no file for the requested format",
                )
            })?;
        crate::apply::write_new(path, &bytes)?;
        artifact = json!({
            "path": path,
            "format": format,
            "rows": rows.len(),
            "columns": report["columns"],
            "byte_len": bytes.len(),
            "sha256": format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes)),
        });
    }

    let total_rows = rows.len();
    let shown: Vec<Value> = rows.into_iter().take(limit).collect();
    let withheld = total_rows.saturating_sub(shown.len());
    Ok(json!({
        "target": target,
        "revision": report["revision"],
        "operation": report["operation"],
        "engine": report["engine"],
        "verification_level": report["verification_level"],
        "standard": report["standard"],
        "assumptions": report["assumptions"],
        "line_angle_definition": report["line_angle_definition"],
        "counts": report["counts"],
        "only_findings": only_findings,
        "rows": shown,
        "more": { "withheld": withheld, "total": total_rows },
        "artifact": artifact,
        "pls_source": target["pls_source"].clone(),
    }))
}

fn output_format(path: &str) -> Result<&'static str, Failure> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".csv") {
        Ok("csv")
    } else if lower.ends_with(".xlsx") {
        Ok("xlsx")
    } else {
        Err(Failure::invalid(
            "output_format_unknown",
            format!("`{path}` is neither .csv nor .xlsx"),
        )
        .remedy("name a .csv or .xlsx file"))
    }
}

pub fn render(data: &Value) -> String {
    let counts = &data["counts"];
    let mut out = format!(
        "structures {} · described {} · on alignment {} · revision {}\n",
        counts["structures"],
        counts["with_description"],
        counts["on_alignment"],
        mutation::short(data["revision"].as_str().unwrap_or("?")),
    );
    if let Some(findings) = counts["findings"].as_object() {
        for (finding, count) in findings {
            out.push_str(&format!("finding    {finding}: {count}\n"));
        }
    }
    for assumption in data["assumptions"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "assumed    {} — {}\n",
            assumption["rule_id"].as_str().unwrap_or("?"),
            assumption["value"].as_str().unwrap_or("?"),
        ));
    }
    for row in data["rows"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  #{:<5} {:<24} {:>9} {:<24} {}  {}\n",
            row["number"].as_str().unwrap_or("—"),
            crate::model::truncate(row["structure_type"].as_str().unwrap_or("?"), 24),
            row["line_angle_deg"]
                .as_f64()
                .map(|a| format!("{a:.4}°"))
                .unwrap_or_else(|| "—".into()),
            crate::model::truncate(row["description"].as_str().unwrap_or("—"), 24),
            row["findings"].as_str().unwrap_or(""),
            row["station_m"]
                .as_f64()
                .map(|s| format!("@ {s:.2} m"))
                .unwrap_or_default(),
        ));
    }
    let withheld = data["more"]["withheld"].as_u64().unwrap_or(0);
    if withheld > 0 {
        out.push_str(&format!(
            "  … {withheld} more rows; raise --limit or read the file\n"
        ));
    }
    if let Some(path) = data["artifact"]["path"].as_str() {
        out.push_str(&format!(
            "written    {} ({} rows, {})\n",
            path,
            data["artifact"]["rows"],
            data["artifact"]["sha256"].as_str().unwrap_or("?")
        ));
    }
    out
}
