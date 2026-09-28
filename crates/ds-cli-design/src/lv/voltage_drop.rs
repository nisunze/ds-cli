//! `ds design lv voltage-drop` — LV voltage-drop compliance and stage-2
//! sizing from local files, method `ds-lv-vd/1`.
//!
//! It reads the same closed `ds.fast-lv.request/v1` that `design.lv.process`
//! reads, with the same bounds and refusals, and processes each transformer
//! with the voltage drop forced on. Every engineering decision — the load
//! flow, each customer's connection phase, the REG ±10 % verdict and the
//! cheapest compliant upgrade — is ds-network's
//! (`process_lv_transformer_with_voltage_drop`); this command reads the file,
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

/// The schema of the document written to `--out`.
pub const RESULT_SCHEMA: &str = "ds.lv-voltage-drop.result/v1";

pub static COMMAND: Command = Command {
    id: "design.lv.voltage-drop",
    path: &["design", "lv", "voltage-drop"],
    contract: 1,
    summary: "Check LV voltage drop against REG and recommend compliant upgrades.",
    purpose: "Check whether every customer of an LV transformer stays within REG's ±10 % of 230 V at saturation design load (method ds-lv-vd/1). Supply the same ds.fast-lv.request/v1 design.lv.process reads: design.lv.project-export --project-config carries the conductor and transformer seeds the method needs. Each transformer is processed with voltage drop on, every customer gets a balanced connection phase and a supply voltage, and stage 2 recommends the cheapest conductor or transformer upgrades that comply. Only the result file is written; nothing is saved to the project. Reports and printing follow separately.",
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
    ],
    output: "`out`, input/result SHA-256 digests, byte count, engine version, method, job/success/failure/compliant counts, and one input-ordered row per job: report status, customers, failing customers, worst drop %, compliance, transformer loading %, stage-2 status, change count, transformer change and infeasible count. Reports, sizing, processed layers and per-job diagnostics are written only to `out`.",
    examples: &[Example {
        command: "ds design lv voltage-drop --input ./T-1042.fast-lv.json --out ./T-1042.vd.json --output json",
        note: "Check one exported transformer against REG without a Desktop session or project identity.",
        runnable: false,
    }],
    refusals: &[
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
    ensure_absent(&output_path, &VOLTAGE_DROP_RESULT)?;

    let input = bounded_read(&input_path)?;
    let input_sha256 = sha256(&input);
    let request = decode_native_fast_lv_request(&input).map_err(map_owner_error)?;
    drop(input);

    // Input order is the result order; each job is independent, so one that
    // fails or panics is reported in its row and never stops the rest.
    let solved: Vec<Solved> = request.jobs.into_iter().map(solve).collect();
    let rows: Vec<Value> = solved.iter().map(row).collect();
    let jobs = rows.len();
    let succeeded = solved.iter().filter(|job| job.outcome.is_ok()).count();
    let compliant = rows.iter().filter(|row| row["compliant"] == true).count();

    let output = encode(solved)?;
    let result_sha256 = sha256(&output);
    write_new(&output_path, &output, &VOLTAGE_DROP_RESULT)?;

    Ok(json!({
        "out": output_path,
        "input_sha256": input_sha256,
        "result_sha256": result_sha256,
        "byte_count": output.len(),
        "engine_core_version": ds_network::CORE_VERSION,
        "method": METHOD,
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
    let worst = rows
        .iter()
        .filter_map(|row| row["worst_vd_pct"].as_f64())
        .reduce(f64::max);
    if let Some(worst) = worst {
        text.push_str(&format!(
            "\nWorst customer drop: {worst:.2} % (limit 10 %)."
        ));
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

/// The compact receipt row: the verdict and what stage 2 asks for, never the
/// layers, the per-customer results or an error text.
fn row(job: &Solved) -> Value {
    let Ok((_, run)) = &job.outcome else {
        return json!({ "transformer_name": job.transformer_name, "ok": false });
    };
    let summary = &run.report.summary;
    json!({
        "transformer_name": job.transformer_name,
        "ok": true,
        "status": run.report.status,
        "customers": summary.customers,
        "customers_failing": summary.customers_failing,
        "worst_vd_pct": summary.worst_vd_pct,
        "compliant": summary.compliant,
        "transformer_loading_pct": run.report.transformer.as_ref().and_then(|t| t.loading_pct),
        "sizing_status": run.sizing.status,
        "changes": run.sizing.changes.len(),
        "transformer_change": run.sizing.transformer_change.as_ref().map(|change| json!({
            "from_kva": change.from_kva,
            "to_kva": change.to_kva,
        })),
        "infeasible": run.sizing.infeasible.len(),
    })
}

/// The complete result document, or a refusal. It is never truncated.
fn encode(solved: Vec<Solved>) -> Result<Vec<u8>, Failure> {
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
        let customers: Vec<Value> = (0..customers)
            .map(|index| {
                let x = 30.0002 + 0.0016 * index as f64 / customers.max(1) as f64;
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
                    "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], [30.0018, -2.0]] },
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
}
