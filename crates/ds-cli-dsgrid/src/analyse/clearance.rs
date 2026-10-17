//! `ds dsgrid analyse clearance` — every terrain/feature point within the
//! span corridor against its code's required clearances at the survey-point
//! clearance cases; deficits, violations, per-alignment and per-code totals.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_model::{AlignmentId, CriterionSetId};
use ds_network_client::{CapturedNetworkClient, NetworkFailure};
use ds_network_contracts::grid::{
    NATIVE_GRID_QUERY_SCHEMA_V1, NativeGridQueryRequestV1, NativeGridQuerySourcePin,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::feature_codes;
use crate::mutation;
use crate::{native, package};

const OWN: &[Refusal] = &[
    Refusal {
        code: "clearance_criteria_not_configured",
        when: "the criterion set has no survey-point clearance cases",
        remedy: "run `ds dsgrid criteria clearance set` first",
    },
    Refusal {
        code: "bound_case_reference_unresolved",
        when: "a requested clearance binding names an absent analysis case or weather state",
        remedy: "restore the missing analysis case or weather state, or update the named criterion-set binding through `ds dsgrid apply`",
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

const BASE_REFUSALS: &[Refusal; OWN.len() + feature_codes::READ_REFUSALS.len()] = &splice();
const REFUSALS: &[Refusal] =
    &native::extend_refusals::<{ BASE_REFUSALS.len() + native::REFUSALS.len() }>(BASE_REFUSALS);
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
Definitions follow PLS-CADD Terrain › Clearances. Verification is a proposal; \
PLS-CADD 16.81 confirms.",
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
        Arg::value(
            "case",
            "<vertical|horizontal|both>",
            "Which clearance to evaluate.",
        )
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
    run_with_capture(inputs, native::capture)
}

fn run_with_capture(
    inputs: &Inputs,
    capture: impl FnOnce() -> Result<CapturedNetworkClient, Failure>,
) -> Result<Value, Failure> {
    let limit = package::parse_limit(inputs.value("limit"))?;
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
                .filter(|value| value.is_finite() && *value > 0.0)
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
    let path = mutation::Target::resolve(inputs, false)?.path();
    let bytes = package::read_bytes(&path)?;
    let request = NativeGridQueryRequestV1 {
        schema: NATIVE_GRID_QUERY_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: format!("{:x}", Sha256::digest(&bytes)),
            expected_revision: inputs
                .value("revision")
                .map(|value| value.trim().to_owned()),
            engineering_input_root: None,
        },
        operation_id: "clearance_report".into(),
        params: json!({
            "alignment_ids": alignment_ids,
            "case": inputs.value("case").unwrap_or("both"),
            "criterion_set_id": criterion_set_id,
            "corridor_half_width_m": corridor_half_width_m,
        }),
    };
    let client = capture()?;
    let scratch = native::scratch()?;
    let query = client
        .grid_query(&request, &bytes, scratch.path(), &|| false)
        .map_err(|error| query_failure(&path, error))?
        .result;
    let target = json!({
        "path": path,
        "model_id": query.source.model_id.as_str(),
        "package_revision": query.model_revision,
        "authored_revision": query.source.expected_revision,
    });
    project(target, query.engine_version, query.result, limit)
}

// Decode only the CLI's projection. Complete nested owner evidence stays opaque.
#[derive(Deserialize)]
struct Report {
    schema_version: u32,
    classification: String,
    verification_level: String,
    criterion_set_id: String,
    clearance_voltage_kv: Option<f64>,
    #[serde(default)]
    vertical_case: Value,
    #[serde(default)]
    horizontal_case: Value,
    evaluated_vertical_cases: Vec<Value>,
    evaluated_horizontal_cases: Vec<Value>,
    corridor_half_width_m: f64,
    definitions: serde_json::Map<String, Value>,
    alignments: Vec<Value>,
    per_code: Vec<Value>,
    points_checked: u64,
    points_unresolved: u64,
    points_outside_spans: u64,
    vertical_violations: u64,
    horizontal_violations: u64,
    questionable: u64,
    finding_count: usize,
    findings: Vec<Value>,
    warnings: Vec<Value>,
    sections_evaluated: usize,
    sections_unavailable: Vec<Value>,
}

fn project(target: Value, engine: String, result: Value, limit: usize) -> Result<Value, Failure> {
    let report: Report = serde_json::from_value(result).map_err(|_| {
        Failure::failed(
            "network_result_invalid",
            "Network returned a malformed clearance report",
        )
    })?;
    if report.finding_count != report.findings.len() {
        return Err(Failure::failed(
            "network_result_invalid",
            "Network clearance finding count differs from its complete list",
        ));
    }
    let finding_count = report.finding_count;
    let (findings, withheld) = package::take(report.findings.clone(), limit);
    let mut answer = json!({
        "target": target,
        "engine": engine,
        "operation": { "id": "clearance_report", "schema_version": report.schema_version },
        "classification": report.classification,
        "verification_level": report.verification_level,
        "criterion_set_id": report.criterion_set_id,
        "clearance_voltage_kv": report.clearance_voltage_kv,
        "vertical_case": report.vertical_case,
        "horizontal_case": report.horizontal_case,
        "evaluated_vertical_cases": report.evaluated_vertical_cases,
        "evaluated_horizontal_cases": report.evaluated_horizontal_cases,
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

fn query_failure(path: &str, error: NetworkFailure) -> Failure {
    match error.owner_code() {
        Some("package_refused") => Failure::failed("package_decode_failed",
            format!("`{path}` did not decode"))
            .remedy("the canonical reader accepts current schema only; retain the original and use `ds dsgrid package migrate --path <source> --expected-source-sha256 <capture> --dry-run`, then open the saved current package")
            .detail(json!({ "detail": error.to_string() })),
        Some("stale_revision") => {
            let mut failure = Failure::conflict("revision_conflict",
                "the head moved since the revision you observed")
                .remedy("re-read the head and decide again against it");
            if let Some(detail) = error.owner_detail { failure = failure.detail(*detail); }
            failure
        }
        Some("operation_failed") => {
            // Match only the owner's serialized refusal variants, never prose
            // or an engineering condition reconstructed in the host.
            let detail = error.owner_detail.as_deref();
            let refusal = detail.and_then(|value| value.get("refusal"));
            refusal.and_then(|value| project_refusal(value, detail?))
                .unwrap_or_else(|| native::map_failure(error))
        }
        _ => native::map_failure(error),
    }
}

fn project_refusal(refusal: &Value, owner: &Value) -> Option<Failure> {
    let detail = &refusal["detail"];
    Some(match refusal["code"].as_str()? {
        "criterion_set" => match detail["code"].as_str()? {
            "criterion_set_missing" => Failure::invalid("criterion_set_missing",
                "the model has no criterion set")
                .remedy("convert the workspace with its CRI member first"),
            "criterion_set_ambiguous" => Failure::invalid("criterion_set_ambiguous",
                "the model has several criterion sets")
                .remedy("pass --criterion-set <id>")
                .detail(json!({"available": detail["detail"]["available"].as_array()?})),
            "criterion_set_not_found" => Failure::invalid("criterion_set_not_found",
                format!("criterion set `{}` is not in the model", detail["detail"].as_str()?))
                .remedy("pass an id from `ds dsgrid criteria show`"),
            _ => return None,
        },
        "clearance_criteria_not_configured" => Failure::invalid(
            "clearance_criteria_not_configured",
            format!("criterion set {} has no survey-point clearance cases", detail["criterion_set"].as_str()?))
            .remedy("run `ds dsgrid criteria clearance set --voltage-class MV --vertical-case <label> --horizontal-case <label> --yes` first")
            .next("ds dsgrid criteria show"),
        "bound_case_reference_unresolved" => Failure::invalid("bound_case_reference_unresolved",
            owner["engine"].as_str()?)
            .remedy("restore the missing analysis case or weather state, or update the named criterion-set binding through `ds dsgrid apply`")
            .next("ds dsgrid describe --kind operations --id update_criterion_case_binding")
            .detail(json!({"refusal": refusal})),
        "alignment_not_found" => Failure::invalid("alignment_not_found",
            format!("alignment `{}` is not in the model", detail.as_str()?))
            .remedy("use an alignment id of the model"),
        "alignment_has_no_route" => Failure::invalid("alignment_not_found",
            format!("alignment `{}` has no route", detail.as_str()?))
            .remedy("use a routed alignment"),
        "no_feature_code_clearances" => Failure::invalid("feature_codes_without_clearances",
            "no feature code carries clearances")
            .remedy("run `ds dsgrid feature-codes import` and `migrate` first"),
        _ => return None,
    })
}

/// The exact cases the report evaluated for one role. The engine's envelope
/// list is the authority; its legacy singular basis exists only when one case
/// applies. A role the report did not evaluate says so — never a null
/// temperature or wind under a dash.
fn evaluated_case_identity(data: &Value, role: &str, quantity: fn(&Value) -> String) -> String {
    let listed = data
        .get(format!("evaluated_{role}_cases"))
        .and_then(Value::as_array)
        .filter(|cases| !cases.is_empty())
        .cloned()
        .or_else(|| {
            data.get(format!("{role}_case"))
                .filter(|case| case.is_object())
                .map(|case| vec![case.clone()])
        });
    match listed {
        Some(cases) => cases
            .iter()
            .map(|case| {
                format!(
                    "{} ({})",
                    case["weather_label"].as_str().unwrap_or("?"),
                    quantity(case)
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
        None => "— (not evaluated)".to_string(),
    }
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "clearance report ({})  set {}  voltage {} kV\nvertical at {}  horizontal at {}\nchecked {} point(s); vertical violations {}, horizontal {}, questionable {}; unresolved {}, outside spans {}\n",
        data["verification_level"].as_str().unwrap_or("proposal"),
        data["criterion_set_id"].as_str().unwrap_or("?"),
        data["clearance_voltage_kv"],
        evaluated_case_identity(data, "vertical", |case| format!(
            "{} °C",
            case["temperature_c"]
        )),
        evaluated_case_identity(data, "horizontal", |case| format!(
            "{} Pa",
            case["wind_pressure_pa"]
        )),
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
            total["worst_vertical_deficit_m"]
                .as_f64()
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            total["worst_horizontal_deficit_m"]
                .as_f64()
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            if total["assumed_clearance"].as_bool().unwrap_or(false) {
                " (assumed)"
            } else {
                ""
            },
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
            out.push_str(&format!(
                "  … {} more findings withheld by --limit\n",
                entry["withheld"]
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(label: &str, temperature_c: f64, wind_pressure_pa: f64) -> Value {
        json!({
            "analysis_case_id": format!("case-{label}"),
            "weather_label": label,
            "temperature_c": temperature_c,
            "wind_pressure_pa": wind_pressure_pa,
            "condition": "after_creep",
        })
    }

    fn header(data: Value) -> String {
        render(&data).lines().nth(1).unwrap().to_string()
    }

    #[test]
    fn header_names_each_evaluated_case_without_a_singular_basis() {
        // Gisagara M3 rev 55 printed `vertical at — (null °C)`: several
        // evaluated cases leave the legacy singular basis absent.
        let line = header(json!({
            "vertical_case": null,
            "horizontal_case": case("High wind", 25.0, 551.7),
            "evaluated_vertical_cases": [
                case("Maximum Conductor Temperature", 75.0, 0.0),
                case("Every day", 20.0, 0.0),
            ],
            "evaluated_horizontal_cases": [case("High wind", 25.0, 551.7)],
        }));
        assert_eq!(
            line,
            "vertical at Maximum Conductor Temperature (75.0 °C), Every day (20.0 °C)  horizontal at High wind (551.7 Pa)"
        );
        assert!(!line.contains("null"));
    }

    #[test]
    fn header_keeps_a_singular_basis_and_names_an_unevaluated_role() {
        let line = header(json!({
            "vertical_case": case("Maximum Conductor Temperature", 75.0, 0.0),
            "horizontal_case": null,
            "evaluated_horizontal_cases": [],
        }));
        assert_eq!(
            line,
            "vertical at Maximum Conductor Temperature (75.0 °C)  horizontal at — (not evaluated)"
        );
    }
}

#[cfg(test)]
#[path = "../../tests/support/clearance_process.rs"]
mod process_tests;
