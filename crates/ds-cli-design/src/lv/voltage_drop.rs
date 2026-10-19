//! `ds design lv voltage-drop` — LV voltage-drop compliance and stage-2
//! sizing from local files, method `ds-lv-vd/1`.
//!
//! It reads the same closed `ds.fast-lv.request/v1` that `design.lv.process`
//! reads, with the same bounds and refusals, and processes each transformer
//! with the voltage drop forced on. Every engineering decision — the load
//! flow, each customer's connection phase, the verdict against the project
//! rule set's voltage limit, the outlook by year and the cheapest compliant
//! upgrade — is ds-network's (`process_lv_transformer_with_voltage_drop`);
//! this command reads the file, writes the run's scenario (`--year`,
//! `--outlook`, `--no-outlook`, `--load`, `--set`) into the in-memory request,
//! captures one admitted Network executable for the batch and writes one result document it never
//! overwrites. Nothing here reaches a project.

use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network_contracts::lv::LvProcessOutput;
use ds_network_contracts::lv_processing::{
    MAX_NATIVE_LV_OUTPUT_BYTES, NativeLvError, NativeLvJobResultV1, NativeLvJobV1,
    decode_native_lv_request,
};
use ds_network_contracts::voltage_drop::{AnalysisStatus, VOLTAGE_DROP_METHOD as METHOD};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::artifact::{VOLTAGE_DROP_RESULT, ensure_absent, sha256, write_new};
use super::process::{bounded_read, map_network_error, map_owner_error};
use super::scenario::Scenario;

/// The schema of the document written to `--out`.
pub const RESULT_SCHEMA: &str = "ds.lv-voltage-drop.result/v1";

// Inherit the process owner's complete refusal vocabulary without a second list.
const SCENARIO_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "vd_scenario_invalid",
        when: "a scenario flag is malformed or outside its bound",
        remedy: "the refusal names the flag and its accepted form",
    },
    Refusal {
        code: "vd_scenario_setting_refused",
        when: "--set names a non-vd_* setting or one a flag owns",
        remedy: "--set only vd_* settings; the refusal names the flag",
    },
    Refusal {
        code: "vd_scenario_conflict",
        when: "an override is repeated, or --outlook meets --no-outlook",
        remedy: "give each override once",
    },
    Refusal {
        code: "vd_scenario_category_unknown",
        when: "--load names a category no job's cust_category defines",
        remedy: "use a clean_name the refusal lists",
    },
];
const REFUSALS: [Refusal; SCENARIO_REFUSALS.len() + super::process::COMMAND.refusals.len()] = {
    let mut result =
        [SCENARIO_REFUSALS[0]; SCENARIO_REFUSALS.len() + super::process::COMMAND.refusals.len()];
    let mut index = 0;
    while index < SCENARIO_REFUSALS.len() {
        result[index] = SCENARIO_REFUSALS[index];
        index += 1;
    }
    let mut process_index = 0;
    while process_index < super::process::COMMAND.refusals.len() {
        result[index] = super::process::COMMAND.refusals[process_index];
        index += 1;
        process_index += 1;
    }
    result
};

pub static COMMAND: Command = Command {
    id: "design.lv.voltage-drop",
    path: &["design", "lv", "voltage-drop"],
    contract: 1,
    summary: "Check LV voltage drop by year and recommend compliant upgrades.",
    purpose: "Check whether every customer of an LV transformer stays within the voltage limit of the project's rule set (IEC 60038 ±10 % when it has none) at the design year's load, and the first outlook year the drawn design fails (method ds-lv-vd/1). Input: the ds.fast-lv.request/v1 of design.lv.project-export --project-config. Every transformer is calculated, fill-ins and upgrades included. Stage 2 recommends the cheapest compliant upgrades for calculated transformers, each scheduled by the year it is first needed. --year, --outlook, --no-outlook, --load and --set change this run only, never the project; the result echoes them.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "input",
            "<path>",
            "Closed ds.fast-lv.request/v1 batch document (maximum 64 MiB).",
        )
        .required(),
        Arg::value(
            "out",
            "<path>",
            "Absent path for the complete ds.lv-voltage-drop.result/v1 document.",
        )
        .required(),
        Arg::value(
            "year",
            "<n>",
            "Design year to check, 0..50 (vd_design_year). Default: vd_growth_years, else 5.",
        ),
        Arg::value(
            "outlook",
            "<years>",
            "Comma-separated years, each 0..50 (vd_outlook_years). Default: 0 to the design year.",
        ),
        Arg::switch(
            "no-outlook",
            "Design year only: no outlook or schedule (vd_staged_plan=false).",
        ),
        Arg::repeated(
            "load",
            "<Category>=<W>[:<W>]",
            "Category saturation load, 0 < W ≤ 100000, then initial, 0..saturation (flat if omitted); up to 32. Sets cust_category and customer loads. Default: the project's.",
        ),
        Arg::repeated(
            "set",
            "<vd_name>=<value>",
            "Another vd_* project setting, e.g. vd_limit_pct=8; up to 32. Default: the project's. Other names are refused.",
        ),
    ],
    output: "`out`, digests, byte count, captured engine version/identity/executable digest and development-override status, method, counts, `scenario` (every override), and one row per job: calculated status, customers, failing customers, worst drop %, limit %, compliance, transformer loading %, stage-2 status, changes, transformer change, infeasible, added cost and basis, design year, first failing year and the engine's outlook schedule; a reservation from an older Network engine gives its status, nature and calculation reason instead. Layers and diagnostics go only to `out`.",
    examples: &[
        Example {
            command: "ds design lv voltage-drop --input ./T-1042.fast-lv.json --out ./T-1042.vd.json --output json",
            note: "Check one exported transformer offline, with no project identity.",
            runnable: false,
        },
        Example {
            command: "ds design lv voltage-drop --input ./T-1042.fast-lv.json --out ./T-1042.res100.vd.json --load Residential=100:60 --outlook 0,5,10",
            note: "Residential at 100 W (60 W at commissioning), years 0, 5 and 10.",
            runnable: true,
        },
    ],
    refusals: &REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[
        "undervoltage",
        "load flow",
        "connection phase",
        "cable sizing",
        "conductor sizing",
        "scenario",
        "load growth",
        "reinforcement",
    ],
    requires: Requires::Server,
    availability: || ds_network_client::available(),
};

/// One transformer's outcome: processed layers, an optional calculation and
/// the producer's analysis envelope. A reservation from an older engine has
/// layers with `tr.vd_summary` and no calculation.
struct Solved {
    transformer_name: String,
    outcome: Result<LvProcessOutput, String>,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let input_path = PathBuf::from(inputs.require("input")?);
    let output_path = PathBuf::from(inputs.require("out")?);
    // A malformed scenario is refused before anything is read.
    let scenario = Scenario::parse(inputs)?;
    ensure_absent(&output_path, &VOLTAGE_DROP_RESULT)?;

    let input = bounded_read(&input_path)?;
    let input_sha256 = sha256(&input);
    let mut request = decode_native_lv_request(&input).map_err(map_owner_error)?;
    drop(input);
    // The scenario changes this run's copy of the request only: the file and
    // the project keep their own settings and loads.
    let rewritten = scenario.apply(&mut request.jobs)?;

    // The existing Network process owns ordered partial failure and complete
    // engineering results. Private staging stays on the output volume.
    #[derive(Serialize)]
    struct RequestWire<'a> {
        schema: &'a str,
        jobs: &'a [NativeLvJobV1],
    }
    let request_bytes = serde_json::to_vec(&RequestWire {
        schema: &request.schema,
        jobs: &request.jobs,
    })
    .map_err(|error| map_owner_error(NativeLvError::InvalidDocument(error.to_string())))?;
    let scratch = std::fs::canonicalize(
        output_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .map_err(|error| {
        Failure::failed(
            "fast_lv_output_write_failed",
            format!("Could not resolve the LV result directory: {error}"),
        )
    })?;
    let outcome = ds_network_client::process_lv_with_order(
        &request_bytes,
        std::thread::available_parallelism().map_or(1, usize::from),
        &scratch,
        &|| false,
        ds_network_contracts::engine::JsonObjectOrder::Insertion,
    )
    .map_err(map_network_error)?;
    if outcome.cancelled {
        return Err(Failure::failed(
            "network_task_cancelled",
            "The Network engine did not complete the voltage-drop batch.",
        )
        .remedy("Retry the unchanged input at a new output path."));
    }
    let engine_core_version = outcome.result.engine_core_version;
    let solved = outcome
        .result
        .jobs
        .into_iter()
        .map(admit_job)
        .collect::<Result<Vec<_>, _>>()?;
    let scenario = scenario.echo(&rewritten);
    let rows = solved.iter().map(row).collect::<Result<Vec<_>, _>>()?;
    let jobs = rows.len();
    let succeeded = solved.iter().filter(|job| job.outcome.is_ok()).count();
    let compliant = rows.iter().filter(|row| row["compliant"] == true).count();

    let output = encode(solved, &scenario, &engine_core_version)?;
    let result_sha256 = sha256(&output);
    write_new(&output_path, &output, &VOLTAGE_DROP_RESULT)?;

    Ok(json!({
        "out": output_path,
        "input_sha256": input_sha256,
        "result_sha256": result_sha256,
        "byte_count": output.len(),
        "engine_core_version": engine_core_version,
        "engine_identity": outcome.identity,
        "executable_sha256": outcome.executable_sha256,
        "development_override": outcome.development_override,
        "method": METHOD,
        "scenario": scenario,
        "jobs": jobs,
        "succeeded": succeeded,
        "failed": jobs - succeeded,
        "compliant": compliant,
        "results": rows,
    }))
}

pub fn render(value: &Value) -> String {
    let jobs = value["jobs"].as_u64().unwrap_or(0);
    let failed = value["failed"].as_u64().unwrap_or(0);
    let rows = value["results"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let count = |status: &str| {
        rows.iter()
            .filter(|row| row["sizing_status"] == status)
            .count()
    };
    let mut text = format!(
        "LV voltage drop ({}) for {jobs} transformer(s): {} compliant as drawn, {} compliant after the recommended changes, {} need a design decision, {} not calculated, {failed} failed.",
        value["method"].as_str().unwrap_or(METHOD),
        count("compliant_as_drawn"),
        count("compliant_after_changes"),
        count("partially_resolved"),
        count("not_calculated"),
    );
    if let Some(line) = scenario_line(&value["scenario"]) {
        text.push('\n');
        text.push_str(&line);
    }
    let worst = rows
        .iter()
        .filter_map(|row| row["worst_vd_pct"].as_f64())
        .reduce(f64::max);
    if let Some(worst) = worst {
        let at = uniform(rows, "design_year")
            .and_then(|year| year.as_u64())
            .map(|year| format!(" at design year {year}"))
            .unwrap_or_default();
        let limit = uniform(rows, "limit_pct")
            .and_then(|limit| limit.as_f64())
            .map(|limit| format!(" (limit {limit} %)"))
            .unwrap_or_default();
        text.push_str(&format!("\nWorst customer drop{at}: {worst:.2} %{limit}."));
    }
    for line in outlook_lines(rows) {
        text.push('\n');
        text.push_str(&line);
    }
    text.push_str(&format!(
        "\nResult: {}\nSHA-256: {}",
        value["out"].as_str().unwrap_or(""),
        value["result_sha256"].as_str().unwrap_or(""),
    ));
    if failed > 0 {
        text.push_str("\nInspect the result document for per-transformer diagnostics.");
    }
    text
}

/// The one value every calculated row carries for `key`, when they agree.
fn uniform<'a>(rows: &'a [Value], key: &str) -> Option<&'a Value> {
    let mut values = rows
        .iter()
        .filter(|row| row["ok"] == true)
        .map(|row| &row[key])
        .filter(|value| !value.is_null());
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

/// `Scenario (this run only): vd_design_year=3; Residential 100 W (initial 60 W).`
fn scenario_line(scenario: &Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(settings) = scenario["project_settings"].as_object() {
        let settings: Vec<String> = settings
            .iter()
            .map(|(name, value)| match value {
                Value::String(text) => format!("{name}={text}"),
                other => format!("{name}={other}"),
            })
            .collect();
        if !settings.is_empty() {
            parts.push(settings.join(", "));
        }
    }
    if let Some(loads) = scenario["loads"].as_array() {
        let loads: Vec<String> = loads
            .iter()
            .map(|load| {
                format!(
                    "{} {} W (initial {} W)",
                    load["category"].as_str().unwrap_or(""),
                    load["saturation_w"].as_f64().unwrap_or(0.0),
                    load["initial_w"].as_f64().unwrap_or(0.0),
                )
            })
            .collect();
        if !loads.is_empty() {
            parts.push(loads.join(", "));
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("Scenario (this run only): {}.", parts.join("; ")))
}

/// One year of the outlook over every calculated transformer.
#[derive(Default)]
struct OutlookYear {
    worst_vd_pct: Option<f64>,
    customers_failing: u64,
    /// Change kind → count, for the changes first needed this year.
    changes: std::collections::BTreeMap<String, u64>,
    transformers: Vec<f64>,
    added_cost: f64,
}

/// One line per outlook year — worst drop as drawn, customers over the
/// limit, the reinforcement first needed and its cost — then the first
/// failing year. Several transformers are summed per year.
fn outlook_lines(rows: &[Value]) -> Vec<String> {
    let scheduled: Vec<&Value> = rows
        .iter()
        .filter(|row| {
            row["schedule"]
                .as_array()
                .is_some_and(|years| !years.is_empty())
        })
        .collect();
    if scheduled.is_empty() {
        return Vec::new();
    }
    let mut years: std::collections::BTreeMap<u64, OutlookYear> = Default::default();
    for row in &scheduled {
        for entry in row["schedule"].as_array().into_iter().flatten() {
            let Some(year) = entry["year"].as_u64() else {
                continue;
            };
            let outlook = years.entry(year).or_default();
            if let Some(worst) = entry["as_drawn"]["worst_vd_pct"].as_f64() {
                outlook.worst_vd_pct =
                    Some(outlook.worst_vd_pct.map_or(worst, |seen| seen.max(worst)));
            }
            outlook.customers_failing +=
                entry["as_drawn"]["customers_failing"].as_u64().unwrap_or(0);
            for change in entry["changes"].as_array().into_iter().flatten() {
                let kind = change["kind"].as_str().unwrap_or("change").to_string();
                *outlook.changes.entry(kind).or_default() += 1;
            }
            if let Some(kva) = entry["transformer_to_kva"].as_f64() {
                outlook.transformers.push(kva);
            }
            outlook.added_cost += entry["added_cost"].as_f64().unwrap_or(0.0);
        }
    }
    let basis = uniform(rows, "cost_basis").and_then(Value::as_str);
    let mut lines = vec![if scheduled.len() == 1 {
        "Outlook of the drawn design:".to_string()
    } else {
        format!(
            "Outlook of the drawn design across {} transformers (worst drop; customers, changes and cost summed):",
            scheduled.len()
        )
    }];
    for (year, outlook) in &years {
        let worst = outlook
            .worst_vd_pct
            .map_or("no drop calculated".to_string(), |worst| {
                format!("worst drop {worst:.2} %")
            });
        let mut needed: Vec<String> = outlook
            .changes
            .iter()
            .map(|(kind, count)| {
                let noun = match kind.as_str() {
                    "lv_line" => "conductor upgrade",
                    "new_circuit" => "new circuit",
                    "service_cable" => "service cable upgrade",
                    _ => "change",
                };
                plural(*count, noun)
            })
            .collect();
        match outlook.transformers.as_slice() {
            [] => {}
            [kva] => needed.push(format!("transformer to {kva} kVA")),
            many => needed.push(plural(many.len() as u64, "transformer upgrade")),
        }
        let reinforcement = if needed.is_empty() {
            "no reinforcement first needed".to_string()
        } else {
            format!("reinforcement first needed: {}", needed.join(", "))
        };
        let cost = match basis {
            Some(_) => format!("; cost {}", amount(outlook.added_cost)),
            None => String::new(),
        };
        lines.push(format!(
            "  year {year}: {worst}, {} over the limit, {reinforcement}{cost}",
            plural(outlook.customers_failing, "customer"),
        ));
    }
    let first_failing = scheduled
        .iter()
        .filter_map(|row| row["first_failing_year"].as_u64())
        .min();
    lines.push(match first_failing {
        Some(year) => format!("First failing year: {year}."),
        None => "The drawn design complies in every outlook year.".to_string(),
    });
    lines.push(match basis {
        Some(basis) => format!("Cost basis: {basis}."),
        None => "Cost bases differ between transformers; each row carries its own.".to_string(),
    });
    lines
}

fn plural(count: u64, noun: &str) -> String {
    ds_cli_contract::args::plural(count, noun)
}

/// A cost with at most two decimals and no trailing zeros.
fn amount(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn invalid_result() -> Failure {
    Failure::failed(
        "network_result_invalid",
        "The Network voltage-drop result is incomplete or has incompatible fields.",
    )
    .remedy("Retain the unchanged input and use an intact compatible Network component.")
}

fn admit_job(job: NativeLvJobResultV1) -> Result<Solved, Failure> {
    let outcome = if job.ok {
        let output = job.output.ok_or_else(invalid_result)?;
        let analysis = output.voltage_drop.as_ref().ok_or_else(invalid_result)?;
        if analysis.schema != ds_network_contracts::voltage_drop::VOLTAGE_DROP_ANALYSIS_SCHEMA_V1
            || analysis.method != METHOD
            || (analysis.status == AnalysisStatus::Reserved) != analysis.run.is_none()
        {
            return Err(invalid_result());
        }
        Ok(output)
    } else {
        Err(job.error.ok_or_else(invalid_result)?)
    };
    Ok(Solved {
        transformer_name: job.transformer_name,
        outcome,
    })
}

// These private readers project only the existing bounded receipt. The full
// owner report/sizing documents below are retained without reserialization
// through these subsets, so customers, sections and future additive facts stay.
#[derive(Deserialize)]
struct RunReceipt {
    report: ReportReceipt,
    sizing: SizingReceipt,
}
#[derive(Deserialize)]
struct ReportReceipt {
    status: String,
    summary: SummaryReceipt,
    #[serde(deserialize_with = "required_nullable")]
    parameters: Option<ParametersReceipt>,
    #[serde(deserialize_with = "required_nullable")]
    transformer: Option<TransformerReceipt>,
}
#[derive(Deserialize)]
struct SummaryReceipt {
    customers: usize,
    customers_failing: usize,
    #[serde(deserialize_with = "required_nullable")]
    worst_vd_pct: Option<f64>,
    compliant: bool,
}
#[derive(Deserialize)]
struct ParametersReceipt {
    voltage_limit_pct: f64,
    growth_years: u32,
}
#[derive(Deserialize)]
struct TransformerReceipt {
    #[serde(deserialize_with = "required_nullable")]
    loading_pct: Option<f64>,
}
#[derive(Deserialize)]
struct TransformerChangeReceipt {
    #[serde(deserialize_with = "required_nullable")]
    from_kva: Option<f64>,
    to_kva: f64,
}
#[derive(Deserialize)]
struct SizingReceipt {
    status: String,
    changes: Vec<Map<String, Value>>,
    #[serde(deserialize_with = "required_nullable")]
    transformer_change: Option<TransformerChangeReceipt>,
    infeasible: Vec<Map<String, Value>>,
    total_added_cost: f64,
    cost_basis: String,
    #[serde(deserialize_with = "required_nullable")]
    first_failing_year: Option<u32>,
    #[serde(deserialize_with = "read_schedule")]
    schedule: Vec<Value>,
}
#[derive(Deserialize)]
struct ReservedReceipt {
    status: String,
    transformer_nature: String,
    calculation: String,
}
// Preserve every schedule field while checking the fields the text receipt
// consumes; a missing year/cost/verdict must never turn into a zero or success.
fn read_schedule<'de, D>(deserializer: D) -> Result<Vec<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct YearState {
        #[serde(rename = "worst_vd_pct", deserialize_with = "required_nullable")]
        _worst_vd_pct: Option<f64>,
        #[serde(rename = "customers_failing")]
        _customers_failing: u64,
    }
    #[derive(Deserialize)]
    struct Change {
        #[serde(rename = "kind")]
        _kind: String,
    }
    #[derive(Deserialize)]
    struct Year {
        #[serde(rename = "year")]
        _year: u32,
        #[serde(rename = "as_drawn")]
        _as_drawn: YearState,
        #[serde(rename = "changes")]
        _changes: Vec<Change>,
        #[serde(rename = "transformer_to_kva", deserialize_with = "required_nullable")]
        _transformer_to_kva: Option<f64>,
        #[serde(rename = "added_cost")]
        _added_cost: f64,
    }
    let schedule = Vec::<Value>::deserialize(deserializer)?;
    for value in &schedule {
        Year::deserialize(value).map_err(serde::de::Error::custom)?;
    }
    Ok(schedule)
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// The receipt row: the verdict, what stage 2 asks for and the engine's
/// outlook schedule, never the layers, the per-customer results or an error
/// text.
fn row(job: &Solved) -> Result<Value, Failure> {
    let Ok(output) = &job.outcome else {
        return Ok(json!({ "transformer_name": job.transformer_name, "ok": false }));
    };
    let analysis = output.voltage_drop.as_ref().ok_or_else(invalid_result)?;
    let Some(run) = &analysis.run else {
        let summary: ReservedReceipt =
            Deserialize::deserialize(&analysis.summary).map_err(|_| invalid_result())?;
        return Ok(json!({
            "transformer_name": job.transformer_name,
            "ok": true,
            "status": summary.status,
            "sizing_status": "not_calculated",
            "transformer_nature": summary.transformer_nature,
            "calculation": summary.calculation,
            "compliant": null,
        }));
    };
    let run: RunReceipt = Deserialize::deserialize(run).map_err(|_| invalid_result())?;
    let summary = &run.report.summary;
    let parameters = run.report.parameters.as_ref();
    let sizing = &run.sizing;
    Ok(json!({
        "transformer_name": job.transformer_name,
        "ok": true,
        "status": run.report.status,
        "customers": summary.customers,
        "customers_failing": summary.customers_failing,
        "worst_vd_pct": summary.worst_vd_pct,
        "limit_pct": parameters.map(|parameters| parameters.voltage_limit_pct),
        "compliant": summary.compliant,
        "transformer_loading_pct": run.report.transformer.as_ref().and_then(|t| t.loading_pct),
        "sizing_status": sizing.status,
        "changes": sizing.changes.len(),
        "transformer_change": sizing.transformer_change.as_ref().map(|change| json!({
            "from_kva": change.from_kva,
            "to_kva": change.to_kva,
        })),
        "infeasible": sizing.infeasible.len(),
        "total_added_cost": sizing.total_added_cost,
        "cost_basis": (!sizing.cost_basis.is_empty()).then_some(&sizing.cost_basis),
        "design_year": parameters.map(|parameters| parameters.growth_years),
        "first_failing_year": sizing.first_failing_year,
        "schedule": sizing.schedule,
    }))
}

/// The complete result document, or a refusal. It is never truncated.
fn encode(
    solved: Vec<Solved>,
    scenario: &Value,
    engine_core_version: &str,
) -> Result<Vec<u8>, Failure> {
    let encoding = |error: serde_json::Error| {
        map_owner_error(NativeLvError::ResultEncoding(error.to_string()))
    };
    // Built by moving values in: `json!` would serialize each layer map into a
    // second copy of itself.
    let mut jobs = Vec::with_capacity(solved.len());
    for job in solved {
        let mut entry = Map::new();
        entry.insert(
            "transformer_name".into(),
            Value::String(job.transformer_name),
        );
        entry.insert("ok".into(), Value::Bool(job.outcome.is_ok()));
        match job.outcome {
            Ok(output) => {
                let analysis = output.voltage_drop.ok_or_else(invalid_result)?;
                entry.insert(
                    "analysis".into(),
                    serde_json::to_value(&analysis).map_err(encoding)?,
                );
                match analysis.run {
                    Some(Value::Object(mut run)) => {
                        let report = run.remove("report").ok_or_else(invalid_result)?;
                        let sizing = run.remove("sizing").ok_or_else(invalid_result)?;
                        entry.insert("report".into(), report);
                        entry.insert("sizing".into(), sizing);
                    }
                    None => {
                        entry.insert("vd_summary".into(), analysis.summary);
                    }
                    Some(_) => return Err(invalid_result()),
                }
                entry.insert("layers".into(), Value::Object(output.gdfs));
            }
            Err(error) => {
                entry.insert("error".into(), Value::String(error));
            }
        }
        jobs.push(Value::Object(entry));
    }
    let mut document = Map::new();
    document.insert("schema".into(), RESULT_SCHEMA.into());
    document.insert("method".into(), METHOD.into());
    document.insert("engine_core_version".into(), engine_core_version.into());
    document.insert("scenario".into(), scenario.clone());
    document.insert("jobs".into(), Value::Array(jobs));
    let bytes = serde_json::to_vec(&document).map_err(encoding)?;
    if bytes.len() > MAX_NATIVE_LV_OUTPUT_BYTES {
        return Err(map_owner_error(NativeLvError::OutputTooLarge {
            actual: bytes.len(),
            maximum: MAX_NATIVE_LV_OUTPUT_BYTES,
        }));
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "voltage_drop_tests.rs"]
mod tests;
