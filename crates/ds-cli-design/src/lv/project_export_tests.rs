use std::collections::BTreeMap;

use serde_json::json;

use super::*;

fn layers() -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            "tr".to_owned(),
            json!({ "type": "FeatureCollection", "features": [] }),
        ),
        (
            "customers".to_owned(),
            json!({ "type": "FeatureCollection", "features": [] }),
        ),
    ])
}

#[test]
fn nature_is_carried_only_from_the_matching_governed_point() {
    let mut saved = layers();
    saved.get_mut("tr").unwrap()["features"] = json!([{
        "type":"Feature", "id":"tr-1", "properties":{}
    }]);
    let mut projected = saved.clone();
    projected.get_mut("tr").unwrap()["features"][0]["properties"]["transformer_nature"] =
        json!("fill_in");
    carry_projected_nature(&mut saved, &projected).unwrap();
    assert_eq!(
        saved["tr"]["features"][0]["properties"]["transformer_nature"],
        "fill_in"
    );
    projected.get_mut("tr").unwrap()["features"][0]["properties"]["transformer_nature"] =
        json!(["fill_in", "productive_use"]);
    carry_projected_nature(&mut saved, &projected).unwrap();
    assert_eq!(
        saved["tr"]["features"][0]["properties"]["transformer_nature"],
        json!(["fill_in", "productive_use"])
    );
    projected.get_mut("tr").unwrap()["features"][0]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("transformer_nature");
    carry_projected_nature(&mut saved, &projected).unwrap();
    assert!(
        saved["tr"]["features"][0]["properties"]
            .get("transformer_nature")
            .is_none()
    );
    projected.get_mut("tr").unwrap()["features"][0]["geometry"] =
        json!({"type":"Point", "coordinates":[1,2]});
    assert!(carry_projected_nature(&mut saved, &projected).is_err());
    projected.get_mut("tr").unwrap()["features"][0]
        .as_object_mut()
        .unwrap()
        .remove("geometry");
    projected.get_mut("tr").unwrap()["features"][0]["id"] = json!("another-tr");
    assert!(carry_projected_nature(&mut saved, &projected).is_err());
}

#[test]
fn contract_is_one_fenced_project_read_and_local_file_write() {
    assert_eq!(COMMAND.authority, Authority::HeadlessProject);
    assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
    assert_eq!(COMMAND.path, ["design", "lv", "project-export"]);
    assert!(COMMAND.args.iter().any(|arg| arg.name == "project"));
    assert!(
        COMMAND
            .args
            .iter()
            .all(|arg| arg.name != "desktop-descriptor")
    );
}

#[test]
fn owner_builds_the_request_and_the_cli_does_not_shape_engine_fields() {
    let bytes = encode_native_lv_request_from_layers("T-1", &layers()).unwrap();
    let request: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(request["schema"], "ds.lv.process.request/v1");
    assert_eq!(request["jobs"][0]["config_dfs"], json!({}));
    assert_eq!(request["jobs"][0]["settings"]["delete_strange_cols"], true);
    assert!(request["jobs"][0].get("ds_project").is_none());
}

#[test]
fn request_artifacts_are_create_new_and_renderer_is_bounded() {
    let root = std::env::temp_dir().join(format!(
        "ds-project-export-{}-{}",
        std::process::id(),
        sha256(b"project-export-test")
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("request.json");
    write_new(&path, b"{}", &PROJECT_REQUEST).unwrap();
    assert_eq!(
        ensure_absent(&path, &PROJECT_REQUEST).unwrap_err().code(),
        "fast_lv_request_output_exists"
    );

    let text = render(&json!({
        "transformer": "T-1",
        "layers": 2,
        "out": path,
        "request_sha256": "abc",
        "secret_layers": { "customers": [1, 2, 3] },
    }));
    assert!(text.contains("owner defaults"));
    assert!(!text.contains("customers"));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn a_dangling_output_symlink_is_existing_operator_state() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "ds-project-export-symlink-{}-{}",
        std::process::id(),
        sha256(b"project-export-symlink-test")
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("request.json");
    symlink(root.join("absent-target"), &path).unwrap();

    assert_eq!(
        ensure_absent(&path, &PROJECT_REQUEST).unwrap_err().code(),
        "fast_lv_request_output_exists"
    );
    assert!(std::fs::symlink_metadata(&path).is_ok());
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn canonical_receipt_keeps_project_save_raw_configuration_equivalent() {
    let raw = json!({"printing_context":{"project_id":"project","project":"Raw sheet caption"}});
    let sealed =
        json!({"printing_context":{"project_id":"project","project":"Gisagara, CJIC"}}).to_string();
    let configuration = json!({"sheets":raw,"network_reporter_input_receipt":{"schema":1,"country":"Rwanda","sheets_json":sealed,"sheets_sha256":sha256(sealed.as_bytes()),"reference_semantic_sha256":"a".repeat(64)}});
    let receipt = captured_printing_receipt(&configuration, "project").unwrap();
    let sheets: BTreeMap<String, Value> =
        serde_json::from_value(configuration["sheets"].clone()).unwrap();
    let encoded = encode_native_lv_request_with_config("T-1", &layers(), &sheets).unwrap();
    let request: Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(request["jobs"][0]["config_dfs"], configuration["sheets"]);
    for (key, expected) in sheets {
        assert_eq!(
            configuration["sheets"][key], expected,
            "fresh native project-save equality"
        );
    }
    assert_eq!(
        receipt.printing_context_for_project("project").unwrap()["project"],
        "Gisagara, CJIC"
    );
    assert_eq!(configuration["sheets"], raw);
    for config in [json!({}), json!({"network_reporter_input_receipt":{}}), {
        let mut c = configuration.clone();
        c["network_reporter_input_receipt"]["sheets_json"] = json!("{}");
        c
    }] {
        assert!(captured_printing_receipt(&config, "project").is_none());
    }
    assert!(captured_printing_receipt(&configuration, "other").is_none());
}
