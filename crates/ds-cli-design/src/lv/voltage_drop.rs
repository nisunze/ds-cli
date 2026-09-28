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
//! runs the jobs in input order and writes one result document it never
//! overwrites. Nothing here reaches a project.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network::network::native_fast_lv::{
    MAX_NATIVE_FAST_LV_OUTPUT_BYTES, NativeFastLvError, NativeFastLvJobV1,
    decode_native_fast_lv_request, native_fast_lv_job_input,
};
use ds_network::network::process_lv_transformer_with_voltage_drop;
use ds_network::network::voltage_drop::VoltageDropRun;
use ds_network::network::voltage_drop::params::METHOD;
use serde_json::{Map, Value, json};

use super::artifact::{VOLTAGE_DROP_RESULT, ensure_absent, sha256, write_new};
use super::process::{bounded_read, map_owner_error};
use super::scenario::Scenario;

/// The schema of the document written to `--out`.
pub const RESULT_SCHEMA: &str = "ds.lv-voltage-drop.result/v1";

pub static COMMAND: Command = Command {
    id: "design.lv.voltage-drop",
    path: &["design", "lv", "voltage-drop"],
    contract: 1,
    summary: "Check LV voltage drop by year and recommend compliant upgrades.",
    purpose: "Check whether every customer of an LV transformer stays within the voltage limit of the project's rule set (IEC 60038 ±10 % when it has none) at the design year's load, and the first outlook year the drawn design fails (method ds-lv-vd/1). Input: the ds.fast-lv.request/v1 of design.lv.project-export --project-config. Stage 2 recommends the cheapest compliant upgrades, each scheduled by the year it is first needed. --year, --outlook, --no-outlook, --load and --set change this run only, never the project; the result echoes them.",
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
    output: "`out`, digests, byte count, engine version, method, counts, `scenario` (every override), and one row per job: status, customers, failing customers, worst drop %, limit %, compliance, transformer loading %, stage-2 status, changes, transformer change, infeasible, added cost and basis, design year, first failing year and the engine's outlook schedule. Layers and diagnostics go only to `out`.",
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
    refusals: &[
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
        Refusal {
            code: "fast_lv_source_not_found",
            when: "--input is absent, not a regular file, or cannot be read",
            remedy: "pass a readable local ds.fast-lv.request/v1 file",
        },
        Refusal {
            code: "fast_lv_input_too_large",
            when: "the request file exceeds 64 MiB",
            remedy: "split the work into smaller closed batches",
        },
        Refusal {
            code: "fast_lv_input_invalid",
            when: "the request is malformed, has unknown fields/settings, or a layer is not a FeatureCollection",
            remedy: "write the exact ds.fast-lv.request/v1 shape shown in the design reference",
        },
        Refusal {
            code: "fast_lv_schema_unsupported",
            when: "the request schema is not ds.fast-lv.request/v1",
            remedy: "migrate the request to the supported v1 schema",
        },
        Refusal {
            code: "fast_lv_bound_refused",
            when: "job, feature, layer, config-sheet, name, or uniqueness bounds are exceeded",
            remedy: "split the batch or shorten the named field reported by the refusal",
        },
        Refusal {
            code: "fast_lv_output_exists",
            when: "--out already exists",
            remedy: "choose a new result path; this command never overwrites",
        },
        Refusal {
            code: "fast_lv_output_write_failed",
            when: "the result cannot be durably written at --out",
            remedy: "choose a writable absent path and retry from the unchanged input",
        },
        Refusal {
            code: "fast_lv_output_too_large",
            when: "the complete result exceeds 256 MiB",
            remedy: "split the request into smaller batches; results are never truncated",
        },
        Refusal {
            code: "fast_lv_result_encoding_failed",
            when: "the voltage-drop result cannot be encoded as its v1 document",
            remedy: "keep the input unchanged and report this internal encoding failure",
        },
    ],
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
    availability: || Availability::Available,
};

/// One transformer's outcome: its processed layers and voltage-drop run, or
/// the reason the engine gave for not producing them.
struct Solved {
    transformer_name: String,
    outcome: Result<(Map<String, Value>, VoltageDropRun), String>,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let input_path = PathBuf::from(inputs.require("input")?);
    let output_path = PathBuf::from(inputs.require("out")?);
    // A malformed scenario is refused before anything is read.
    let scenario = Scenario::parse(inputs)?;
    ensure_absent(&output_path, &VOLTAGE_DROP_RESULT)?;

    let input = bounded_read(&input_path)?;
    let input_sha256 = sha256(&input);
    let mut request = decode_native_fast_lv_request(&input).map_err(map_owner_error)?;
    drop(input);
    // The scenario changes this run's copy of the request only: the file and
    // the project keep their own settings and loads.
    let rewritten = scenario.apply(&mut request.jobs)?;

    // Input order is the result order; each job is independent, so one that
    // fails or panics is reported in its row and never stops the rest.
    let solved: Vec<Solved> = request.jobs.into_iter().map(solve).collect();
    let scenario = scenario.echo(&rewritten);
    let rows: Vec<Value> = solved.iter().map(row).collect();
    let jobs = rows.len();
    let succeeded = solved.iter().filter(|job| job.outcome.is_ok()).count();
    let compliant = rows.iter().filter(|row| row["compliant"] == true).count();

    let output = encode(solved, &scenario)?;
    let result_sha256 = sha256(&output);
    write_new(&output_path, &output, &VOLTAGE_DROP_RESULT)?;

    Ok(json!({
        "out": output_path,
        "input_sha256": input_sha256,
        "result_sha256": result_sha256,
        "byte_count": output.len(),
        "engine_core_version": ds_network::CORE_VERSION,
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

fn solve(job: NativeFastLvJobV1) -> Solved {
    let transformer_name = job.transformer_name.clone();
    let input = native_fast_lv_job_input(job);
    // The same containment the batch kernel gives `design.lv.process`: a
    // panicking job is that job's failure, not the command's.
    let outcome = match catch_unwind(AssertUnwindSafe(|| {
        process_lv_transformer_with_voltage_drop(input)
    })) {
        Ok(Ok((output, run))) => Ok((output.gdfs, run)),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("local processing panicked".to_string()),
    };
    Solved {
        transformer_name,
        outcome,
    }
}

/// The receipt row: the verdict, what stage 2 asks for and the engine's
/// outlook schedule, never the layers, the per-customer results or an error
/// text.
fn row(job: &Solved) -> Value {
    let Ok((_, run)) = &job.outcome else {
        return json!({ "transformer_name": job.transformer_name, "ok": false });
    };
    let summary = &run.report.summary;
    let parameters = run.report.parameters.as_ref();
    let sizing = &run.sizing;
    json!({
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
    })
}

/// The complete result document, or a refusal. It is never truncated.
fn encode(solved: Vec<Solved>, scenario: &Value) -> Result<Vec<u8>, Failure> {
    let encoding = |error: serde_json::Error| {
        map_owner_error(NativeFastLvError::ResultEncoding(error.to_string()))
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
            Ok((layers, run)) => {
                let report = serde_json::to_value(&run.report).map_err(encoding)?;
                let sizing = serde_json::to_value(&run.sizing).map_err(encoding)?;
                entry.insert("report".into(), report);
                entry.insert("sizing".into(), sizing);
                entry.insert("layers".into(), Value::Object(layers));
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
    document.insert(
        "engine_core_version".into(),
        ds_network::CORE_VERSION.into(),
    );
    document.insert("scenario".into(), scenario.clone());
    document.insert("jobs".into(), Value::Array(jobs));
    let bytes = serde_json::to_vec(&document).map_err(encoding)?;
    if bytes.len() > MAX_NATIVE_FAST_LV_OUTPUT_BYTES {
        return Err(map_owner_error(NativeFastLvError::OutputTooLarge {
            actual: bytes.len(),
            maximum: MAX_NATIVE_FAST_LV_OUTPUT_BYTES,
        }));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::{Format, Output};

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    fn inputs(input: &std::path::Path, out: &std::path::Path) -> Inputs {
        let tokens = [
            "--input".to_string(),
            input.display().to_string(),
            "--out".to_string(),
            out.display().to_string(),
        ];
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    fn abc(name: &str, r: f64, x: f64, amps: f64) -> Value {
        json!({
            "clean_name": name,
            "electrical_params": json!({
                "r_ohm_per_km": r, "x_ohm_per_km": x, "max_i_ka": amps / 1000.0
            }).to_string(),
        })
    }

    /// The seeds the process and the method read: the project settings the
    /// builder needs, cos φ, the category that gives a load its watts, the
    /// ABC ladder, one service cable and two transformer sizes, each with its
    /// `electrical_params`.
    fn config() -> Value {
        json!({
            "project_settings": [
                { "parameter": "cos_phi", "value": 0.85 },
                { "parameter": "sc_real_length", "value": "x + (x*0.01) + 6" },
                { "parameter": "max_service_length", "value": 37 },
                { "parameter": "pole_spacing", "value": 50 },
            ],
            "cust_category": [{
                "clean_name": "Residential",
                "peak_power_w": 250,
                "misspelled_names": ["Residential"],
            }],
            "lv_lines": [
                abc("3 x 35 + 54.6mm² ABC", 0.868, 0.0982, 132.0),
                abc("3 x 70 + 54.6mm² ABC", 0.443, 0.0893, 205.0),
                abc("3 x 120 + 54.6mm² ABC", 0.253, 0.0855, 300.0),
            ],
            "service_cable_sizes": [{
                "service_cable_size": "2 x 6 mm²",
                "meter_type": "Single Phase",
                "electrical_params": json!({
                    "r_ohm_per_km": 3.08, "x_ohm_per_km": 0.09, "max_i_ka": 0.039
                }).to_string(),
            }],
            "transfo_sizes": [
                { "transfo_sizes": 100, "electrical_params": json!({"vk_percent": 4.0, "vkr_percent": 1.45}).to_string() },
                { "transfo_sizes": 160, "electrical_params": json!({"vk_percent": 4.0, "vkr_percent": 1.375}).to_string() },
            ],
        })
    }

    /// A 100 kVA transformer, one ~200 m LV line and `customers` 250 W
    /// single-phase customers beside it.
    fn job(name: &str, customers: usize) -> Value {
        feeder(name, customers, 0.0018)
    }

    /// A 100 kVA transformer, one 3 x 35 mm² line `length_deg` long (0.0018°
    /// ≈ 200 m) and `customers` 250 W single-phase customers spread along it.
    fn feeder(name: &str, customers: usize, length_deg: f64) -> Value {
        let customers: Vec<Value> = (0..customers)
            .map(|index| {
                let x = 30.0002 + (length_deg - 0.0002) * index as f64 / customers.max(1) as f64;
                json!({
                    "type": "Feature",
                    "id": format!("{name}-c{index}"),
                    "geometry": { "type": "Point", "coordinates": [x, -2.00008] },
                    "properties": {
                        "load": "Residential, 250",
                        "meter_type": "Single Phase",
                        "service_length": 10.0,
                        "service_cable_size": "2 x 6 mm²",
                    }
                })
            })
            .collect();
        json!({
            "transformer_name": name,
            "gdfs": {
                "tr": { "type": "FeatureCollection", "features": [{
                    "type": "Feature",
                    "id": format!("{name}-tr"),
                    "geometry": { "type": "Point", "coordinates": [30.0, -2.0] },
                    "properties": { "name": name, "names": name, "transfo_size": 100 }
                }]},
                "lv_lines": { "type": "FeatureCollection", "features": [{
                    "type": "Feature",
                    "id": format!("{name}-line"),
                    "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], [30.0 + length_deg, -2.0]] },
                    "properties": { "cable_size": "3 x 35 + 54.6mm² ABC" }
                }]},
                "customers": { "type": "FeatureCollection", "features": customers },
            },
            "settings": {},
            "config_dfs": config(),
        })
    }

    fn request(jobs: Vec<Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({ "schema": "ds.fast-lv.request/v1", "jobs": jobs })).unwrap()
    }

    #[test]
    fn contract_is_offline_local_file_compute() {
        assert_eq!(COMMAND.authority, Authority::None);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(COMMAND.requires, Requires::Server);
        assert_eq!(COMMAND.path, ["design", "lv", "voltage-drop"]);
        assert_eq!(METHOD, "ds-lv-vd/1");
    }

    #[test]
    fn a_request_is_solved_in_input_order_into_one_result_document() {
        let root = tempfile::tempdir().expect("temp dir");
        let input = root.path().join("request.json");
        let out = root.path().join("result.vd.json");
        std::fs::write(&input, request(vec![job("T2", 6), job("T1", 3)])).unwrap();

        let receipt = run(&inputs(&input, &out), &context()).expect("solved");
        assert_eq!(receipt["method"], "ds-lv-vd/1");
        assert_eq!(receipt["jobs"], 2);
        // A failed job names its reason only in the document.
        let document_head = || {
            std::fs::read_to_string(&out)
                .unwrap_or_default()
                .chars()
                .take(2000)
                .collect::<String>()
        };
        assert_eq!(receipt["succeeded"], 2, "{}", document_head());
        assert_eq!(receipt["failed"], 0);
        let rows = receipt["results"].as_array().expect("rows");
        assert_eq!(rows[0]["transformer_name"], "T2");
        assert_eq!(rows[1]["transformer_name"], "T1");
        for (row, customers) in rows.iter().zip([6, 3]) {
            assert_eq!(row["ok"], true, "{row}");
            assert_eq!(row["status"], "calculated", "{row}");
            assert_eq!(row["customers"], customers, "{row}");
            assert_eq!(row["customers_failing"], 0, "{row}");
            assert_eq!(row["compliant"], true, "{row}");
            assert_eq!(row["sizing_status"], "compliant_as_drawn", "{row}");
            assert_eq!(row["changes"], 0, "{row}");
            let worst = row["worst_vd_pct"].as_f64().expect("a worst drop");
            assert!(worst > 0.0 && worst < 10.0, "{row}");
            assert!(
                row["transformer_loading_pct"]
                    .as_f64()
                    .is_some_and(|pct| pct > 0.0)
            );
            assert!(row.get("layers").is_none() && row.get("report").is_none());
        }
        assert_eq!(receipt["compliant"], 2);

        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(receipt["byte_count"], bytes.len());
        assert_eq!(receipt["result_sha256"], sha256(&bytes));
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["schema"], RESULT_SCHEMA);
        assert_eq!(document["method"], "ds-lv-vd/1");
        assert_eq!(document["engine_core_version"], ds_network::CORE_VERSION);
        let first = &document["jobs"][0];
        assert_eq!(first["transformer_name"], "T2");
        assert_eq!(first["report"]["method"], "ds-lv-vd/1");
        assert_eq!(first["report"]["summary"]["customers"], 6);
        assert!(first["sizing"]["status"].is_string());
        // Every customer carries its connection phase and its `vd_*` result.
        let customers = first["layers"]["customers"]["features"].as_array().unwrap();
        assert_eq!(customers.len(), 6);
        for customer in customers {
            let properties = &customer["properties"];
            assert!(properties["connection_phase"].is_string(), "{properties}");
            assert!(properties["vd_pct"].is_number(), "{properties}");
        }
        assert!(first.get("error").is_none());
        assert_eq!(document["jobs"][1]["transformer_name"], "T1");

        let text = render(&receipt);
        assert!(text.contains("2 compliant as drawn"), "{text}");
        assert!(!text.contains("T1") && !text.contains("layers"), "{text}");
    }

    #[test]
    fn an_existing_output_is_refused_before_the_input_is_read() {
        let root = tempfile::tempdir().expect("temp dir");
        let out = root.path().join("result.vd.json");
        std::fs::write(&out, b"operator-owned").unwrap();
        let error = run(&inputs(&root.path().join("absent.json"), &out), &context())
            .expect_err("never overwrites");
        assert_eq!(error.code(), "fast_lv_output_exists");
        assert_eq!(std::fs::read(&out).unwrap(), b"operator-owned");
    }

    #[test]
    fn request_refusals_are_the_process_commands_own() {
        let root = tempfile::tempdir().expect("temp dir");
        let out = root.path().join("result.vd.json");

        let missing = run(&inputs(&root.path().join("absent.json"), &out), &context());
        assert_eq!(
            missing.expect_err("absent").code(),
            "fast_lv_source_not_found"
        );

        let malformed = root.path().join("malformed.json");
        std::fs::write(&malformed, b"{\"schema\":").unwrap();
        let invalid = run(&inputs(&malformed, &out), &context());
        assert_eq!(
            invalid.expect_err("malformed").code(),
            "fast_lv_input_invalid"
        );

        let other = root.path().join("other.json");
        std::fs::write(
            &other,
            serde_json::to_vec(
                &json!({ "schema": "ds.fast-lv.request/v2", "jobs": [job("T1", 1)] }),
            )
            .unwrap(),
        )
        .unwrap();
        let unsupported = run(&inputs(&other, &out), &context());
        assert_eq!(
            unsupported.expect_err("v2").code(),
            "fast_lv_schema_unsupported"
        );

        let empty = root.path().join("empty.json");
        std::fs::write(&empty, request(Vec::new())).unwrap();
        let bound = run(&inputs(&empty, &out), &context());
        assert_eq!(bound.expect_err("no jobs").code(), "fast_lv_bound_refused");

        assert!(!out.exists(), "a refused request writes nothing");
    }

    #[test]
    fn a_failed_job_keeps_its_row_and_its_reason_stays_in_the_document() {
        let root = tempfile::tempdir().expect("temp dir");
        let input = root.path().join("request.json");
        let out = root.path().join("result.vd.json");
        let mut broken = job("Broken", 1);
        // No transformer point: the engine has no source to solve from.
        broken["gdfs"]["tr"]["features"] = json!([]);
        std::fs::write(&input, request(vec![broken, job("T1", 2)])).unwrap();

        let receipt = run(&inputs(&input, &out), &context()).expect("the batch still completes");
        assert_eq!(receipt["failed"], 1);
        assert_eq!(
            receipt["results"][0],
            json!({ "transformer_name": "Broken", "ok": false })
        );
        assert_eq!(receipt["results"][1]["ok"], true);

        let document: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(document["jobs"][0]["ok"], false);
        assert!(document["jobs"][0]["error"].is_string());
        assert!(document["jobs"][0].get("layers").is_none());
        assert!(render(&receipt).contains("Inspect the result document"));
    }

    fn scenario_inputs(input: &std::path::Path, out: &std::path::Path, flags: &[&str]) -> Inputs {
        let mut tokens = vec![
            "--input".to_string(),
            input.display().to_string(),
            "--out".to_string(),
            out.display().to_string(),
        ];
        tokens.extend(flags.iter().map(|flag| flag.to_string()));
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    fn document(path: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).expect("a result")).expect("json")
    }

    /// The owner's question to the utility, answered by the method: the same
    /// ~1.2 km feeder of 150 customers fails at 250 W per Residential
    /// customer and complies at 100 W; at commissioning (year 0, 60 W) it
    /// complies too. Each scenario is echoed and the request is untouched.
    #[test]
    fn a_scenario_changes_what_the_method_checks_and_is_echoed() {
        let root = tempfile::tempdir().expect("temp dir");
        let input = root.path().join("request.json");
        let mut long = feeder("LONG", 150, 0.0108);
        long["config_dfs"]["cust_category"][0]["initial_power_w"] = json!(60);
        let bytes = request(vec![long]);
        std::fs::write(&input, &bytes).unwrap();

        // As configured: 250 W at saturation, checked at year 5.
        let base_out = root.path().join("base.vd.json");
        let base = run(&scenario_inputs(&input, &base_out, &[]), &context()).expect("solved");
        let row = &base["results"][0];
        assert_eq!(row["status"], "calculated", "{row}");
        assert_eq!(row["compliant"], false, "{row}");
        assert!(
            row["customers_failing"].as_u64().is_some_and(|n| n > 0),
            "{row}"
        );
        assert_eq!(row["design_year"], 5, "{row}");
        assert_eq!(row["limit_pct"], 10.0, "{row}");
        let failing = row["first_failing_year"]
            .as_u64()
            .expect("the drawn design fails in an outlook year");
        assert!(failing > 0 && failing <= 5, "{row}");
        // The receipt carries the engine's outlook exactly as the document has it.
        let written = document(&base_out);
        assert_eq!(row["schedule"], written["jobs"][0]["sizing"]["schedule"]);
        assert_eq!(
            row["first_failing_year"],
            written["jobs"][0]["sizing"]["first_failing_year"]
        );
        let years: Vec<u64> = row["schedule"]
            .as_array()
            .expect("a schedule")
            .iter()
            .filter_map(|year| year["year"].as_u64())
            .collect();
        assert_eq!(years, [0, 1, 2, 3, 4, 5]);
        assert_eq!(row["schedule"][0]["as_drawn"]["compliant"], true, "{row}");
        let empty = json!({ "project_settings": {}, "loads": [] });
        assert_eq!(base["scenario"], empty);
        assert_eq!(written["scenario"], empty);
        let text = render(&base);
        assert!(text.contains("Outlook of the drawn design:"), "{text}");
        assert!(text.contains("  year 0: worst drop "), "{text}");
        assert!(text.contains("  year 5: worst drop "), "{text}");
        assert!(text.contains(" over the limit"), "{text}");
        assert!(
            text.contains(&format!("First failing year: {failing}.")),
            "{text}"
        );
        assert!(text.contains("(limit 10 %)"), "{text}");
        assert!(!text.contains("Scenario"), "{text}");

        // Residential at 100 W: the same feeder complies in every year.
        let light_out = root.path().join("light.vd.json");
        let light = run(
            &scenario_inputs(
                &input,
                &light_out,
                &[
                    "--load",
                    "Residential=100",
                    "--set",
                    "vd_max_recommended_abc_mm2=70",
                ],
            ),
            &context(),
        )
        .expect("solved");
        let row = &light["results"][0];
        assert_eq!(row["compliant"], true, "{row}");
        assert_eq!(row["customers_failing"], 0, "{row}");
        assert_eq!(row["first_failing_year"], Value::Null, "{row}");
        assert_eq!(row["sizing_status"], "compliant_as_drawn", "{row}");
        assert_eq!(
            light["scenario"],
            json!({
                "project_settings": { "vd_max_recommended_abc_mm2": 70 },
                "loads": [{
                    "category": "Residential", "saturation_w": 100.0, "initial_w": 100.0,
                    "customer_loads_rewritten": 150,
                }],
            })
        );
        let written = document(&light_out);
        assert_eq!(written["scenario"], light["scenario"]);
        // The method took the setting from the scenario, and says so.
        assert_eq!(
            written["jobs"][0]["report"]["parameters"]["sources"]["vd_max_recommended_abc_mm2"],
            "project_settings.vd_max_recommended_abc_mm2"
        );
        let text = render(&light);
        assert!(
            text.contains(
                "Scenario (this run only): vd_max_recommended_abc_mm2=70; \
                 Residential 100 W (initial 100 W)."
            ),
            "{text}"
        );
        assert!(
            text.contains("The drawn design complies in every outlook year."),
            "{text}"
        );

        // Year 0, the 60 W commissioning load, without the outlook, at 8 %.
        let early_out = root.path().join("year0.vd.json");
        let early = run(
            &scenario_inputs(
                &input,
                &early_out,
                &["--year", "0", "--no-outlook", "--set", "vd_limit_pct=8"],
            ),
            &context(),
        )
        .expect("solved");
        let row = &early["results"][0];
        assert_eq!(row["design_year"], 0, "{row}");
        assert_eq!(row["limit_pct"], 8.0, "{row}");
        assert_eq!(row["compliant"], true, "{row}");
        assert_eq!(row["schedule"], json!([]), "{row}");
        assert_eq!(
            early["scenario"]["project_settings"],
            json!({ "vd_design_year": 0, "vd_limit_pct": 8, "vd_staged_plan": false })
        );
        let text = render(&early);
        assert!(!text.contains("Outlook"), "{text}");
        assert!(text.contains("at design year 0: "), "{text}");
        assert!(text.contains("(limit 8 %)"), "{text}");

        // Run-scoped: the request on disk is byte-identical.
        assert_eq!(std::fs::read(&input).unwrap(), bytes);
    }

    #[test]
    fn scenario_refusals_come_before_anything_is_written() {
        let root = tempfile::tempdir().expect("temp dir");
        let input = root.path().join("request.json");
        let out = root.path().join("result.vd.json");
        std::fs::write(&input, request(vec![job("T1", 3)])).unwrap();
        let refused = |flags: &[&str]| {
            run(&scenario_inputs(&input, &out, flags), &context())
                .expect_err("refused")
                .code()
                .to_string()
        };
        assert_eq!(
            refused(&["--set", "cos_phi=0.9"]),
            "vd_scenario_setting_refused"
        );
        assert_eq!(
            refused(&["--set", "max_service_length=40"]),
            "vd_scenario_setting_refused"
        );
        assert_eq!(refused(&["--year", "99"]), "vd_scenario_invalid");
        assert_eq!(
            refused(&["--load", "Industrial=5000"]),
            "vd_scenario_category_unknown"
        );
        assert!(!out.exists(), "a refused scenario writes nothing");
    }
}
