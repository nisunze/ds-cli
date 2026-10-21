use super::*;
use ds_cli_contract::{Format, Output};

// All successful actual-process cases compare every full result field to
// the former linked owner, including scenario and preserved topology.
fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let receipt = super::run(inputs, context)?;
    let bytes = std::fs::read(inputs.require("input")?).unwrap();
    let mut request = decode_native_lv_request(&bytes).unwrap();
    Scenario::parse(inputs)
        .unwrap()
        .apply(&mut request.jobs)
        .unwrap();
    let actual = document(Path::new(inputs.require("out")?));
    for (source, actual) in request
        .jobs
        .into_iter()
        .zip(actual["jobs"].as_array().unwrap())
    {
        let name = source.transformer_name.clone();
        let expected = ds_network::network::process_lv_transformer_with_voltage_drop(
            ds_network::network::lv_processing::native_lv_job_input(source),
        );
        assert_eq!(actual["transformer_name"], name);
        match expected {
            Ok((output, run)) => {
                assert_eq!(actual["ok"], true);
                assert_eq!(actual["layers"], serde_json::to_value(output.gdfs).unwrap());
                assert_eq!(
                    actual["analysis"],
                    serde_json::to_value(output.voltage_drop).unwrap()
                );
                if let Some(run) = run {
                    assert_eq!(actual["report"], serde_json::to_value(run.report).unwrap());
                    assert_eq!(actual["sizing"], serde_json::to_value(run.sizing).unwrap());
                }
            }
            Err(error) => {
                assert_eq!(actual["ok"], false);
                assert_eq!(actual["error"], error.to_string());
            }
        }
    }
    assert_eq!(receipt["engine_identity"]["jsonObjectOrder"], "insertion");
    assert_eq!(
        receipt["engine_core_version"],
        receipt["engine_identity"]["coreVersion"]
    );
    let executable =
        std::fs::read(std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit actual engine"))
            .unwrap();
    assert_eq!(receipt["executable_sha256"], sha256(&executable));
    Ok(receipt)
}

#[test]
fn receipt_projection_refuses_missing_and_mistyped_owner_fields() {
    let native = || {
        ds_network::network::lv_processing::process_native_lv(
            decode_native_lv_request(&request(vec![job("projection", 1)])).unwrap(),
        )
        .unwrap()
        .jobs
        .remove(0)
    };
    let good = native();
    let mut document = serde_json::to_value(&good).unwrap();
    let expected_run = document["output"]["voltage_drop"]["run"].clone();
    let mut solved = admit_job(good).unwrap();
    let extra = json!({"future_customer_evidence":[1,2,3]});
    solved
        .outcome
        .as_mut()
        .unwrap()
        .voltage_drop
        .as_mut()
        .unwrap()
        .run
        .as_mut()
        .unwrap()["report"]["additional_evidence"] = extra.clone();
    assert!(row(&solved).is_ok());
    let encoded: Value =
        serde_json::from_slice(&encode(vec![solved], &json!({}), "captured-version").unwrap())
            .unwrap();
    assert_eq!(encoded["engine_core_version"], "captured-version");
    assert_eq!(encoded["jobs"][0]["report"]["additional_evidence"], extra);
    assert_eq!(encoded["jobs"][0]["sizing"], expected_run["sizing"]);
    document["output"]["voltage_drop"]["run"]["report"]
        .as_object_mut()
        .unwrap()
        .remove("parameters");
    let broken = admit_job(serde_json::from_value(document).unwrap()).unwrap();
    assert_eq!(row(&broken).unwrap_err().code(), "network_result_invalid");
    let mut malformed = native();
    malformed
        .output
        .as_mut()
        .unwrap()
        .voltage_drop
        .as_mut()
        .unwrap()
        .run
        .as_mut()
        .unwrap()["sizing"]["changes"] = json!("not an array");
    let malformed = admit_job(malformed).unwrap();
    assert_eq!(
        row(&malformed).unwrap_err().code(),
        "network_result_invalid"
    );
    let mut malformed_schedule = native();
    malformed_schedule
        .output
        .as_mut()
        .unwrap()
        .voltage_drop
        .as_mut()
        .unwrap()
        .run
        .as_mut()
        .unwrap()["sizing"]["schedule"] = json!([{
        "year": 5,
        "as_drawn": {"worst_vd_pct": null, "customers_failing": 0},
        "changes": [],
        "transformer_to_kva": null
    }]);
    let malformed_schedule = admit_job(malformed_schedule).unwrap();
    assert_eq!(
        row(&malformed_schedule).unwrap_err().code(),
        "network_result_invalid"
    );
    let mut missing = native();
    missing.output.as_mut().unwrap().voltage_drop = None;
    assert_eq!(
        admit_job(missing).err().unwrap().code(),
        "network_result_invalid"
    );
}

#[test]
#[ignore = "requires DS_NETWORK_ENGINE_BIN to name an absent explicit executable"]
fn actual_missing_network_refuses_without_output_or_fallback() {
    let selected = std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit absent engine");
    assert!(!Path::new(&selected).exists());
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("request.json");
    let output = root.path().join("result.json");
    std::fs::write(&input, request(vec![job("missing-engine", 1)])).unwrap();
    let error = super::run(&inputs(&input, &output), &context()).unwrap_err();
    assert_eq!(error.code(), "network_engine_missing");
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

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
#[ignore = "requires an explicitly selected compatible Network executable"]
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
    // The producer's exact analysis envelope rides beside the layers it
    // binds: one input digest for this result and a saved analysis.
    let analysis = &first["analysis"];
    assert_eq!(analysis["schema"], "ds.lv-voltage-drop.analysis/v1");
    assert_eq!(analysis["status"], "calculated");
    assert_eq!(analysis["run"]["report"], first["report"]);
    assert_eq!(
        analysis["summary"],
        first["layers"]["tr"]["features"][0]["properties"]["vd_summary"]
    );
    for key in [
        "inputs_digest",
        "analysis_inputs_digest",
        "processed_digest",
    ] {
        assert!(
            analysis[key]
                .as_str()
                .is_some_and(|digest| digest.len() == 64),
            "{key}"
        );
    }
    assert_eq!(document["jobs"][1]["transformer_name"], "T1");

    let text = render(&receipt);
    assert!(text.contains("2 compliant as drawn"), "{text}");
    assert!(!text.contains("T1") && !text.contains("layers"), "{text}");
}

#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn fill_ins_and_upgrades_are_calculated_like_every_transformer() {
    let root = tempfile::tempdir().expect("temp dir");
    let input = root.path().join("request.json");
    let out = root.path().join("result.vd.json");
    // A false vd_calculate_<nature> switches nothing off (owner, 2026-10-19).
    let jobs = ["fill_in", "upgrade"]
        .into_iter()
        .map(|nature| {
            let mut job = job(nature, 2);
            job["gdfs"]["tr"]["features"][0]["properties"]["transformer_nature"] = json!(nature);
            job["config_dfs"]["project_settings"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "parameter": format!("vd_calculate_{nature}"),
                    "value": false,
                }));
            job
        })
        .collect::<Vec<_>>();
    std::fs::write(&input, request(jobs)).unwrap();

    let receipt = run(&inputs(&input, &out), &context()).expect("every nature is calculated");
    assert_eq!(receipt["succeeded"], 2);
    assert_eq!(receipt["failed"], 0);
    for row in receipt["results"].as_array().unwrap() {
        assert_eq!(row["ok"], true);
        assert_eq!(row["status"], "calculated", "{row}");
        assert!(row["worst_vd_pct"].is_number(), "{row}");
    }
    let document = document(&out);
    for (entry, nature) in document["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .zip(["fill_in", "upgrade"])
    {
        assert_eq!(entry["ok"], true);
        let analysis = &entry["analysis"];
        assert_eq!(analysis["status"], "calculated");
        assert_eq!(analysis["summary"]["transformer_nature"], nature);
        assert_eq!(
            analysis["summary"]["calculation"],
            format!("method default: a {nature} transformer is calculated")
        );
        assert_eq!(
            entry["layers"]["tr"]["features"][0]["properties"]["vd_summary"],
            analysis["summary"]
        );
        assert!(entry["report"].is_object() && entry["sizing"].is_object());
        assert!(entry.get("vd_summary").is_none());
    }
    let text = render(&receipt);
    assert!(text.contains(", 0 not calculated,"), "{text}");
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
        serde_json::to_vec(&json!({ "schema": "ds.fast-lv.request/v2", "jobs": [job("T1", 1)] }))
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
#[ignore = "requires an explicitly selected compatible Network executable"]
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
#[ignore = "requires an explicitly selected compatible Network executable"]
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
// These use the real offline command's decoder, engine and durable file
// writer. No project authority or save is acquired by a topology check.
fn saved_topology_job() -> Value {
    fn point(id: &str, xy: [f64; 2], properties: Value) -> Value {
        json!({"type":"Feature","id":id,"geometry":{"type":"Point","coordinates":xy},"properties":properties})
    }
    fn line(id: &str, a: [f64; 2], b: [f64; 2], properties: Value) -> Value {
        json!({"type":"Feature","id":id,"geometry":{"type":"LineString","coordinates":[a,b]},"properties":properties})
    }
    let tr = [30.0, -2.0];
    let p1 = [30.001, -2.0];
    let p2 = [30.002, -2.0];
    let c1 = [30.001, -2.0001];
    let c2 = [30.002, -2.0001];
    let mut job = json!({"transformer_name":"saved-proof","settings":{"keep_network_as_drawn":true},"config_dfs":config(),"gdfs":{}});
    let layers = [
        (
            "tr",
            vec![point(
                "tr",
                tr,
                json!({"node_id":"tr","transfo_size":100,"name":"saved-proof"}),
            )],
        ),
        (
            "lv_poles",
            vec![
                point(
                    "p1",
                    p1,
                    json!({"node_id":"p1","pole_number":"P001","x":488149.48,"y":4711326.98}),
                ),
                point("p2", p2, json!({"node_id":"p2","pole_number":"P002"})),
            ],
        ),
        (
            "customers",
            vec![
                point(
                    "c1",
                    c1,
                    json!({"node_id":"c1","connection_pole_id":"p1","connection_phase":"1","load":"Residential, 250","meter_type":"Single Phase","service_length":15.0,"service_cable_size":"2 x 6 mm\u{b2}"}),
                ),
                point(
                    "c2",
                    c2,
                    json!({"node_id":"c2","connection_pole_id":"p2","connection_phase":"2","load":"Residential, 250","meter_type":"Single Phase","service_length":15.0,"service_cable_size":"2 x 6 mm\u{b2}"}),
                ),
            ],
        ),
        (
            "lv_lines",
            vec![line(
                "l1",
                tr,
                p2,
                json!({"cable_size":"3 x 35 + 54.6mm\u{b2} ABC","length":200.0}),
            )],
        ),
        (
            "spans",
            vec![
                line(
                    "s1",
                    tr,
                    p1,
                    json!({"from_node_id":"tr","to_node_id":"p1","cable_size":"3 x 35 + 54.6mm\u{b2} ABC","length":100.0}),
                ),
                line(
                    "s2",
                    p1,
                    p2,
                    json!({"from_node_id":"p1","to_node_id":"p2","cable_size":"3 x 35 + 54.6mm\u{b2} ABC","length":100.0}),
                ),
            ],
        ),
        (
            "service_cables",
            vec![
                line(
                    "sc1",
                    p1,
                    c1,
                    json!({"from_node_id":"p1","to_node_id":"c1","service_cable_size":"2 x 6 mm\u{b2}","length":15.0}),
                ),
                line(
                    "sc2",
                    p2,
                    c2,
                    json!({"from_node_id":"p2","to_node_id":"c2","service_cable_size":"2 x 6 mm\u{b2}","length":15.0}),
                ),
            ],
        ),
    ];
    for (layer, features) in layers {
        job["gdfs"][layer] = json!({"type":"FeatureCollection","features":features});
    }
    job
}
fn saved_command(job: &Value) -> Value {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("saved.json");
    let out = root.path().join("result.json");
    let bytes = request(vec![job.clone()]);
    std::fs::write(&input, &bytes).unwrap();
    let receipt = run(&inputs(&input, &out), &context()).expect("valid offline command");
    assert_eq!(receipt["failed"], 0, "{receipt}");
    assert_eq!(
        std::fs::read(&input).unwrap(),
        bytes,
        "no source input overwrite"
    );
    let result: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    let output = result["jobs"][0].clone();
    for (layer, fc) in job["gdfs"].as_object().unwrap() {
        let original = fc["features"].as_array().unwrap();
        let actual = output["layers"][layer]["features"].as_array().unwrap();
        assert_eq!(original.len(), actual.len(), "{layer} row count");
        for (source, row) in original.iter().zip(actual) {
            assert_eq!(source["id"], row["id"], "{layer} saved identity");
            assert_eq!(
                source["geometry"], row["geometry"],
                "{layer} authored geometry"
            );
            for (key, value) in source["properties"].as_object().unwrap() {
                assert_eq!(
                    row["properties"].get(key),
                    Some(value),
                    "{layer} authored {key}"
                );
            }
        }
    }
    output
}
#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn native_saved_topology_complete_and_missing_ids_keep_source_design_cells() {
    let job = saved_topology_job();
    let output = saved_command(&job);
    assert_eq!(output["analysis"]["status"], "calculated");
    assert_eq!(output["report"]["summary"]["customers"], 2);
    assert_eq!(output["report"]["summary"]["customers_unreached"], 0);
    assert_eq!(output["report"]["sections"].as_array().unwrap().len(), 2);
    let mut legacy = job.clone();
    for fc in legacy["gdfs"].as_object_mut().unwrap().values_mut() {
        for f in fc["features"].as_array_mut().unwrap() {
            let p = f["properties"].as_object_mut().unwrap();
            for key in [
                "node_id",
                "from_node_id",
                "to_node_id",
                "connection_pole_id",
            ] {
                p.remove(key);
            }
        }
    }
    let output = saved_command(&legacy);
    assert_eq!(
        output["analysis"]["status"], "calculated",
        "{}",
        output["report"]["issues"]
    );
    assert_eq!(output["report"]["summary"]["customers"], 2);
    assert_eq!(output["report"]["summary"]["customers_unreached"], 0);
    assert_eq!(output["report"]["sections"].as_array().unwrap().len(), 2);
    for fc in output["layers"].as_object().unwrap().values() {
        for f in fc["features"].as_array().unwrap() {
            assert!(
                f["properties"].get("node_id").is_none(),
                "analysis IDs stay transient"
            );
        }
    }
    // The legacy x/y property is unchanged; exact WGS84 geometry supplies
    // connectivity, and authored metric lengths supply branch impedance.
    assert_eq!(output["report"]["sections"][0]["length_m"], 100.0);
}
#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn native_saved_topology_partial_unreached_and_ambiguous_never_claim_compliance() {
    for case in 0..3 {
        let mut job = saved_topology_job();
        match case {
            0 => {
                job["gdfs"]["spans"]["features"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            1 => {
                job["gdfs"]["customers"]["features"][1]["properties"]["connection_pole_id"] =
                    json!("unreached");
            }
            _ => {
                let mut duplicate = job["gdfs"]["lv_poles"]["features"][0].clone();
                duplicate["id"] = json!("duplicate");
                duplicate["properties"]["node_id"] = json!("duplicate");
                job["gdfs"]["lv_poles"]["features"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
                job["gdfs"]["spans"]["features"][0]["properties"]
                    .as_object_mut()
                    .unwrap()
                    .remove("from_node_id");
            }
        }
        let output = saved_command(&job);
        assert_eq!(
            output["analysis"]["status"], "refused",
            "case {case}: {}",
            output["report"]["issues"]
        );
        assert_eq!(output["report"]["summary"]["compliant"], false);
        assert_eq!(output["report"]["status"], "not_calculated");
        assert!(output["report"]["customers"].as_array().unwrap().is_empty());
        let issues = output["report"]["issues"].to_string();
        assert!(
            if case == 2 {
                issues.contains("ambiguous")
            } else {
                issues.contains("unreachable") || issues.contains("not connected")
            },
            "{issues}"
        );
    }
}

#[test]
fn a_processed_result_is_refused_as_a_result_not_as_an_unknown_field() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("result.json");
    let out = root.path().join("vd.json");
    std::fs::write(
        &source,
        br#"{"schema":"ds.lv.process.result/v1","engine_core_version":"0.1.0","jobs":[]}"#,
    )
    .unwrap();
    let refused = super::run(&inputs(&source, &out), &context()).unwrap_err();
    assert_eq!(refused.code(), "fast_lv_input_invalid");
    assert!(refused.message().contains("not a result"), "{refused}");
    assert!(
        refused
            .remedy_text()
            .is_some_and(|remedy| remedy.contains("project-export --project-config")),
        "{refused}"
    );
    assert!(!out.exists());

    // A malformed request that is not a result keeps the owner's own sentence.
    std::fs::write(
        &source,
        br#"{"schema":"ds.lv.process.request/v1","oops":1}"#,
    )
    .unwrap();
    let malformed = super::run(&inputs(&source, &out), &context()).unwrap_err();
    assert_eq!(malformed.code(), "fast_lv_input_invalid");
    assert!(!malformed.message().contains("not a result"), "{malformed}");
}
