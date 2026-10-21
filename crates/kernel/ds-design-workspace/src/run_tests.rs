use super::*;

fn captured() -> (tempfile::TempDir, Workspace, Value) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    Workspace::init(&root, "test-project").unwrap();
    let mut workspace = Workspace::open(&root).unwrap();
    let snapshot = json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
            "layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":"T1","names":"T1"}}]},
                "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]}},
            "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true});
    workspace
        .write(&serde_json::to_vec(&snapshot).unwrap(), None, "create")
        .unwrap();
    workspace.prepare_run("run", &["T1".into()]).unwrap();
    assert_eq!(workspace.process("run", 1).unwrap().completed, 1);
    let (bytes, _, _) = workspace.result("run", "T1").unwrap();
    let result = serde_json::from_slice(&bytes).unwrap();
    (temp, workspace, result)
}

// A synthetic retained transport fixture, including historical/damaged
// variants. No test repair or export reruns the producer.
fn store(workspace: &Workspace, bytes: &[u8]) {
    workspace
        .db
        .execute(
            "UPDATE jobs SET result=?1,result_sha256=?2 WHERE run='run' AND transformer='T1'",
            params![bytes, format!("{:x}", Sha256::digest(bytes))],
        )
        .unwrap();
}

#[test]
#[ignore = "requires an explicitly qualified native Network design-snapshot executable"]
fn report_capture_keeps_exact_saved_json_member_and_final_layer_pin() {
    let (temp, workspace, result) = captured();
    let bytes = serde_json::to_vec_pretty(&result).unwrap();
    store(&workspace, &bytes);
    let raw: CapturedResult<'_> = serde_json::from_slice(&bytes).unwrap();
    let source = raw.output.voltage_drop.unwrap().get().as_bytes();
    assert!(source.contains(&b'\n'));
    let before = workspace.outbox(0, 100).unwrap();
    let out = temp.path().join("report");
    let receipt = workspace.report_inputs("run", "T1", &out).unwrap();
    assert_eq!(fs::read(out.join("voltage-drop.json")).unwrap(), source);
    assert_eq!(
        receipt["voltage_drop_result"]["sha256"],
        format!("{:x}", Sha256::digest(source))
    );
    assert_eq!(
        receipt["files"]["voltage-drop.json"],
        receipt["voltage_drop_result"]["sha256"]
    );
    assert_eq!(
        receipt["layers_sha256"],
        design::digest(&result["output"]["gdfs"]).unwrap()
    );
    assert_eq!(workspace.result("run", "T1").unwrap().0, bytes);
    assert_eq!(workspace.outbox(0, 100).unwrap(), before);
}

#[test]
#[ignore = "requires an explicitly qualified native Network design-snapshot executable"]
fn missing_invalid_and_stale_saved_analysis_refuse_before_staging() {
    let (temp, mut workspace, original) = captured();
    let changed = workspace.edit(&serde_json::to_vec(&json!({
            "schema":"ds.design.edit/v1","transformer":"T1","expected_revision":original["input_revision"],
            "operation_id":"later-edit","mutations":[{"kind":"set_properties","layer":"lv_lines","ids":["line"],"values":{"note":"later held revision"}}]
        })).unwrap()).unwrap();
    assert!(workspace.read("T1", changed["revision"].as_str()).is_ok());
    let before = workspace.outbox(0, 100).unwrap();
    for (case, code) in [
        ("missing", "voltage_drop_result_missing"),
        ("null", "voltage_drop_result_missing"),
        ("schema", "voltage_drop_result_invalid"),
        ("state", "voltage_drop_result_invalid"),
        ("identity", "voltage_drop_result_invalid"),
        ("digest", "voltage_drop_result_stale"),
        ("layers", "voltage_drop_result_stale"),
        ("revision", "voltage_drop_result_stale"),
        ("settings", "voltage_drop_result_stale"),
        ("summary", "voltage_drop_result_stale"),
    ] {
        let mut result = original.clone();
        match case {
            "missing" => {
                result["output"]
                    .as_object_mut()
                    .unwrap()
                    .remove("voltage_drop");
            }
            "null" => result["output"]["voltage_drop"] = Value::Null,
            "schema" => result["output"]["voltage_drop"]["schema"] = json!("unknown"),
            "state" => result["output"]["voltage_drop"]["status"] = json!("held"),
            "identity" => result["transformer"] = json!("T2"),
            "digest" => {
                result["output"]["voltage_drop"]["processed_digest"] = json!("0".repeat(64))
            }
            "layers" => {
                result["output"]["gdfs"]["tr"]["features"][0]["geometry"]["coordinates"] =
                    json!([30.1, -2.0])
            }
            "revision" => result["input_revision"] = changed["revision"].clone(),
            "settings" => {
                let previous = result["output"]["settings_used"]["keep_lv_lines_topology"]
                    .as_bool()
                    .unwrap();
                result["output"]["settings_used"]["keep_lv_lines_topology"] = json!(!previous);
            }
            "summary" => {
                result["output"]["voltage_drop"]["summary"]["calculation"] =
                    json!("changed after capture")
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&result).unwrap();
        store(&workspace, &bytes);
        let out = temp.path().join(case);
        let error = workspace.report_inputs("run", "T1", &out).unwrap_err();
        assert!(error.to_string().starts_with(code), "{case}: {error}");
        assert!(!out.exists(), "{case}");
        if case == "revision" {
            let report = json!({"transformer":"T1","status":"completed"});
            let error = workspace
                .retain_report("run", "T1", &out, &report)
                .unwrap_err();
            assert!(error.to_string().starts_with(code), "retention: {error}");
        }
        assert_eq!(workspace.result("run", "T1").unwrap().0, bytes);
        assert_eq!(workspace.outbox(0, 100).unwrap(), before);
    }
}

fn artifact(root: &Path, format: &str, bytes: &[u8]) -> Value {
    let filename = ds_command_kernel::report_formats::report_filename("T1", format).unwrap();
    let path = root.join("artifacts").join(&filename);
    fs::write(&path, bytes).unwrap();
    json!({"format":format,"filename":filename,"path":path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))})
}

#[test]
#[ignore = "requires an explicitly qualified native Network design-snapshot executable"]
fn delivery_requires_complete_selection_and_exact_saved_json_before_outbox() {
    let (temp, mut workspace, _) = captured();
    let root = temp.path().join("report");
    workspace.report_inputs("run", "T1", &root).unwrap();
    fs::create_dir(root.join("artifacts")).unwrap();
    let source = fs::read(root.join("voltage-drop.json")).unwrap();
    let layers = fs::read(root.join("transformer.json")).unwrap();
    let report = json!({"transformer":"T1","status":"completed","country":"Test",
            "formats_requested":["xlsx","voltage_drop"],
            "artifacts":[artifact(&root,"xlsx",b"test workbook bytes"),artifact(&root,"voltage_drop",&source)]});
    let before = workspace.outbox(0, 100).unwrap();
    for case in [
        "unrequested_analysis",
        "missing_artifact",
        "duplicate",
        "extra",
        "identity",
        "source_artifact",
        "source_input",
        "layers",
        "digest",
    ] {
        let mut bad = report.clone();
        match case {
            "unrequested_analysis" => bad["formats_requested"] = json!(["xlsx"]),
            "missing_artifact" => {
                bad["artifacts"].as_array_mut().unwrap().pop();
            }
            "duplicate" => {
                let duplicate = bad["artifacts"][0].clone();
                bad["artifacts"].as_array_mut().unwrap().push(duplicate);
            }
            "extra" => bad["artifacts"]
                .as_array_mut()
                .unwrap()
                .push(artifact(&root, "geojson", b"extra")),
            "identity" => bad["transformer"] = json!("T2"),
            "source_artifact" => {
                let mut altered = source.clone();
                altered.push(b'\n');
                bad["artifacts"][1] = artifact(&root, "voltage_drop", &altered);
            }
            "source_input" => fs::write(root.join("voltage-drop.json"), b"{}").unwrap(),
            "layers" => fs::write(root.join("transformer.json"), b"{}").unwrap(),
            "digest" => bad["artifacts"][0]["sha256"] = json!("0".repeat(64)),
            _ => unreachable!(),
        }
        assert!(
            workspace.retain_report("run", "T1", &root, &bad).is_err(),
            "{case}"
        );
        assert_eq!(workspace.outbox(0, 100).unwrap(), before, "{case}");
        fs::write(root.join("voltage-drop.json"), &source).unwrap();
        fs::write(root.join("transformer.json"), &layers).unwrap();
        artifact(&root, "voltage_drop", &source);
    }
    assert_eq!(
        workspace
            .retain_report("run", "T1", &root, &report)
            .unwrap()["artifact_count"],
        2
    );
    let after = workspace.outbox(0, 100).unwrap();
    assert_eq!(
        after["rows"].as_array().unwrap().len(),
        before["rows"].as_array().unwrap().len() + 1
    );
    workspace
        .retain_report("run", "T1", &root, &report)
        .unwrap();
    assert_eq!(workspace.outbox(0, 100).unwrap(), after);
}
