//! `ds dsgrid analyse clearance` — every terrain/feature point within the
//! span corridor against its code's required clearances at the survey-point
//! clearance cases; deficits, violations, per-alignment and per-code totals.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{ClearanceCase, ClearanceReportError, ClearanceReportOptions};
use ds_grid_model::{AlignmentId, CriterionSetId};
use serde_json::{Value, json};

use crate::criteria::clearance_set::map_error as map_set_error;
use crate::feature_codes;
use crate::mutation;
use crate::package;

const OWN: &[Refusal] = &[
    Refusal {
        code: "clearance_criteria_not_configured",
        when: "the criterion set has no survey-point clearance cases",
        remedy: "run `ds dsgrid criteria clearance set` first",
    },
    Refusal {
        code: "feature_codes_without_clearances",
        when: "no feature code in the model carries clearances",
        remedy: "run `ds dsgrid feature-codes import` and `migrate` first",
    },
    Refusal {
        code: "alignment_not_found",
        when: "--alignment is not an alignment of the model, or has no route",
        remedy: "use an alignment id from `ds dsgrid run --operation project_plan`",
    },
    Refusal {
        code: "criterion_set_missing",
        when: "the model has no criterion set",
        remedy: "convert the workspace with its CRI member first",
    },
    Refusal {
        code: "criterion_set_ambiguous",
        when: "the model has several criterion sets and --criterion-set was not given",
        remedy: "pass --criterion-set <id> from `ds dsgrid criteria show`",
    },
    Refusal {
        code: "criterion_set_not_found",
        when: "--criterion-set is not a criterion set of the model",
        remedy: "pass an id from `ds dsgrid criteria show`",
    },
    Refusal {
        code: "invalid_corridor",
        when: "--corridor-half-width-m is not a positive number",
        remedy: "pass the plan half-width in metres (PLS-CADD's max offset from wire, 12 m in the Nyamagabe criteria)",
    },
];

const REFUSALS: &[Refusal; OWN.len() + feature_codes::READ_REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + feature_codes::READ_REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + feature_codes::READ_REFUSALS.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < feature_codes::READ_REFUSALS.len() {
        all[OWN.len() + shared] = feature_codes::READ_REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.analyse.clearance",
    path: &["dsgrid", "analyse", "clearance"],
    contract: 1,
    summary: "Report survey-point clearances by feature code: deficits, violations.",
    purpose: "\
For every span and every terrain/feature point within the span corridor: the \
point's feature code, the required vertical and horizontal clearance (the \
code's class table, the obstacle height applied where the rule says h + k), \
the clearance available at the governing case — vertical at the survey-point \
vertical case (maximum conductor temperature, final), horizontal blow-out at \
the survey-point horizontal case (high wind) — the deficit, the violation \
flag, the station and offset, and the span's structures. Per-alignment and \
per-code totals; obstacles checked on a default height and assumed \
clearances are flagged. Only violations are listed; every point is counted. \
Definitions follow PLS-CADD's Terrain › Clearances so the two compare; \
verification level `proposal` — PLS-CADD 16.81 confirms.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
        Arg::value("alignment", "<id>", "Restrict to one alignment."),
        Arg::value("case", "<vertical|horizontal|both>", "Which clearance to evaluate.")
            .default("both")
            .choices(&["vertical", "horizontal", "both"]),
        Arg::value(
            "criterion-set",
            "<id>",
            "The criterion set whose clearance cases apply; the model's only set when omitted.",
        ),
        Arg::value(
            "corridor-half-width-m",
            "<m>",
            "Plan half-width a point must lie within to be checked; the alignment's terrain corridor, else 12 m.",
        ),
        Arg::value("limit", "<n>", "Cap the findings listed.").default(package::DEFAULT_LIMIT),
    ],
    output: "\
The target, the basis (criterion set, voltage, vertical and horizontal case), \
the definitions, the findings (violations only: point, station, offset, code, \
kind, height, required/available/deficit per case, flags, span structures), \
per-alignment and per-code totals, the counts checked/unresolved/outside, the \
warnings and the sections that could not be solved. `verification_level` is \
`proposal`. `more.truncated` names the findings withheld by --limit.",
    examples: &[
        Example {
            command: "ds dsgrid analyse clearance --model local-e9b0ccbf92d7447b --output json --limit 500",
            note: "Every violation by feature code, with the structures each span runs between.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid analyse clearance --model local-e9b0ccbf92d7447b --case vertical",
            note: "Vertical clearances only.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "clearance",
        "clearance report",
        "violation",
        "vertical clearance",
        "horizontal clearance",
        "blow-out",
        "swamp",
        "road",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = package::parse_limit(inputs.value("limit"))?;
    let case = match inputs.value("case").unwrap_or("both") {
        "vertical" => ClearanceCase::Vertical,
        "horizontal" => ClearanceCase::Horizontal,
        _ => ClearanceCase::Both,
    };
    let alignment_ids = match inputs.value("alignment") {
        Some(raw) => vec![AlignmentId::new(raw.trim()).map_err(|error| {
            Failure::invalid("alignment_not_found", error.to_string())
                .remedy("use an alignment id of the model")
        })?],
        None => Vec::new(),
    };
    let criterion_set_id = match inputs.value("criterion-set") {
        Some(raw) => Some(CriterionSetId::new(raw.trim()).map_err(|error| {
            Failure::invalid("criterion_set_not_found", error.to_string())
                .remedy("pass an id from `ds dsgrid criteria show`")
        })?),
        None => None,
    };
    let corridor_half_width_m = match inputs.value("corridor-half-width-m") {
        Some(raw) => Some(
            raw.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| {
                    Failure::invalid(
                        "invalid_corridor",
                        "--corridor-half-width-m must be a positive number",
                    )
                    .remedy("pass the plan half-width in metres")
                })?,
        ),
        None => None,
    };
    let (target, opened) = feature_codes::open_read_only(inputs)?;
    let report = opened
        .session
        .clearance_report(&ClearanceReportOptions {
            alignment_ids,
            case,
            criterion_set_id,
            corridor_half_width_m,
        })
        .map_err(|error| match error {
            ClearanceReportError::CriterionSet(error) => map_set_error(error),
            ClearanceReportError::ClearanceCriteriaNotConfigured { criterion_set } => {
                Failure::invalid(
                    "clearance_criteria_not_configured",
                    format!("criterion set {criterion_set} has no survey-point clearance cases"),
                )
                .remedy("run `ds dsgrid criteria clearance set --voltage-class MV --vertical-case <label> --horizontal-case <label> --yes` first")
                .next("ds dsgrid criteria show")
            }
            ClearanceReportError::AlignmentNotFound(id) => Failure::invalid(
                "alignment_not_found",
                format!("alignment `{id}` is not in the model"),
            )
            .remedy("use an alignment id of the model"),
            ClearanceReportError::AlignmentHasNoRoute(id) => Failure::invalid(
                "alignment_not_found",
                format!("alignment `{id}` has no route"),
            )
            .remedy("use a routed alignment"),
            ClearanceReportError::NoFeatureCodeClearances => Failure::invalid(
                "feature_codes_without_clearances",
                "no feature code carries clearances",
            )
            .remedy("run `ds dsgrid feature-codes import` and `migrate` first"),
        })?;
    let finding_count = report.finding_count;
    let (findings, withheld) = package::take(report.findings.clone(), limit);
    let mut answer = json!({
        "target": target,
        "engine": ds_grid_engine::ENGINE_VERSION,
        "operation": { "id": ds_grid_engine::clearance_report::CLEARANCE_REPORT_OPERATION_ID, "schema_version": report.schema_version },
        "classification": report.classification,
        "verification_level": report.verification_level,
        "criterion_set_id": report.criterion_set_id,
        "clearance_voltage_kv": report.clearance_voltage_kv,
        "vertical_case": report.vertical_case,
        "horizontal_case": report.horizontal_case,
        "corridor_half_width_m": report.corridor_half_width_m,
        "definitions": report.definitions,
        "alignments": report.alignments,
        "per_code": report.per_code,
        "points_checked": report.points_checked,
        "points_unresolved": report.points_unresolved,
        "points_outside_spans": report.points_outside_spans,
        "vertical_violations": report.vertical_violations,
        "horizontal_violations": report.horizontal_violations,
        "questionable": report.questionable,
        "finding_count": finding_count,
        "findings": findings,
        "warnings": report.warnings,
        "sections_evaluated": report.sections_evaluated,
        "sections_unavailable": report.sections_unavailable,
        "staged": false,
        "persisted": false,
    });
    if withheld > 0 {
        answer["more"] = json!({ "truncated": [{ "field": "findings", "total": finding_count, "shown": limit, "withheld": withheld, "limit": limit }] });
    }
    Ok(answer)
}

fn structure_label(structure: &Value) -> String {
    structure["engineering_number"]
        .as_str()
        .or_else(|| structure["structure_id"].as_str())
        .unwrap_or("?")
        .to_string()
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "clearance report ({})  set {}  voltage {} kV\nvertical at {} ({} °C)  horizontal at {} ({} Pa)\nchecked {} point(s); vertical violations {}, horizontal {}, questionable {}; unresolved {}, outside spans {}\n",
        data["verification_level"].as_str().unwrap_or("proposal"),
        data["criterion_set_id"].as_str().unwrap_or("?"),
        data["clearance_voltage_kv"],
        data["vertical_case"]["weather_label"].as_str().unwrap_or("—"),
        data["vertical_case"]["temperature_c"],
        data["horizontal_case"]["weather_label"].as_str().unwrap_or("—"),
        data["horizontal_case"]["wind_pressure_pa"],
        data["points_checked"],
        data["vertical_violations"],
        data["horizontal_violations"],
        data["questionable"],
        data["points_unresolved"],
        data["points_outside_spans"],
    );
    for total in data["per_code"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:>4} {:<24} checked {:>5}  V {:>4}  H {:>4}  worst V {} H {}{}\n",
            total["feature_code"],
            total["feature_name"].as_str().unwrap_or("?"),
            total["points_checked"],
            total["vertical_violations"],
            total["horizontal_violations"],
            total["worst_vertical_deficit_m"].as_f64().map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".to_string()),
            total["worst_horizontal_deficit_m"].as_f64().map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".to_string()),
            if total["assumed_clearance"].as_bool().unwrap_or(false) { " (assumed)" } else { "" },
        ));
    }
    for finding in data["findings"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:>9.1} m {:>+6.1} m  {:<20} {}→{}  V req {} avail {} {}  H req {} avail {} {}{}{}\n",
            finding["station_m"].as_f64().unwrap_or(0.0),
            finding["offset_m"].as_f64().unwrap_or(0.0),
            finding["feature_name"].as_str().unwrap_or("?"),
            structure_label(&finding["from_structure"]),
            structure_label(&finding["to_structure"]),
            finding["required_vertical_m"],
            finding["available_vertical_m"].as_f64().map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".to_string()),
            if finding["vertical_violation"].as_bool().unwrap_or(false) { "BUST" } else { "ok" },
            finding["required_horizontal_m"],
            finding["available_horizontal_m"].as_f64().map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".to_string()),
            if finding["horizontal_violation"].as_bool().unwrap_or(false) { "BUST" } else { "ok" },
            if finding["questionable"].as_bool().unwrap_or(false) { " questionable" } else { "" },
            if finding["obstacle_height_default_used"].as_bool().unwrap_or(false) { " h=default" } else { "" },
        ));
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "warning  {} x{}: {}\n",
            warning["code"].as_str().unwrap_or("?"),
            warning["count"],
            warning["detail"].as_str().unwrap_or("")
        ));
    }
    if let Some(truncated) = data["more"]["truncated"].as_array() {
        for entry in truncated {
            out.push_str(&format!("  … {} more findings withheld by --limit\n", entry["withheld"]));
        }
    }
    out
}
