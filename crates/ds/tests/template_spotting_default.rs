mod common;

use std::fs;

use serde_json::json;

#[test]
fn a_lean_spotting_default_is_attested_and_read_back_without_application() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.dsgrid");
    let setup = dir.path().join("setup.json");
    let template = dir.path().join("standards.dsgrid-template");
    let old_template = dir.path().join("old.dsgrid-template");
    let changed_setup = dir.path().join("changed.json");
    let changed_template = dir.path().join("changed.dsgrid-template");
    let bad_setup = dir.path().join("bad.json");
    let bad_template = dir.path().join("bad.dsgrid-template");
    let base = base.to_str().unwrap();
    let setup = setup.to_str().unwrap();
    let template = template.to_str().unwrap();
    let old_template = old_template.to_str().unwrap();
    let changed_setup = changed_setup.to_str().unwrap();
    let changed_template = changed_template.to_str().unwrap();
    let bad_setup = bad_setup.to_str().unwrap();
    let bad_template = bad_template.to_str().unwrap();

    let (created, code) = common::json(&["dsgrid", "create", "--out", base, "--output", "json"]);
    assert_eq!(code, 0, "{created}");
    fs::write(
        setup,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "design_policy_id": "policy-example",
            "criterion_set_id": "criteria-example",
            "station_step_m": 7.0,
            "minimum_span_m": 40.0,
            "conductor": {"default_set_label": "30kV"}
        }))
        .unwrap(),
    )
    .unwrap();
    let (compiled, code) = common::json(&[
        "dsgrid",
        "template",
        "compile",
        "--source",
        base,
        "--out",
        template,
        "--spotting-default",
        setup,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{compiled}");
    let (inspected, code) = common::json(&[
        "dsgrid",
        "template",
        "inspect",
        "--template",
        template,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{inspected}");
    assert_eq!(
        compiled["data"]["template"]["spotting_default"],
        inspected["data"]["spotting_default"]
    );
    assert_eq!(inspected["data"]["spotting_default"]["present"], true);
    assert_eq!(
        inspected["data"]["spotting_default"]["settings"]["minimum_span_m"],
        40.0
    );
    assert!(
        inspected["data"]["spotting_default"]["sha256"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(
        compiled["data"]["artifact"]["sha256"],
        inspected["data"]["artifact"]["sha256"]
    );

    fs::write(
        changed_setup,
        fs::read_to_string(setup).unwrap().replace("40.0", "41.0"),
    )
    .unwrap();
    let (changed, code) = common::json(&[
        "dsgrid",
        "template",
        "compile",
        "--source",
        base,
        "--out",
        changed_template,
        "--spotting-default",
        changed_setup,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{changed}");
    let (changed_readback, code) = common::json(&[
        "dsgrid",
        "template",
        "inspect",
        "--template",
        changed_template,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{changed_readback}");
    assert_eq!(
        changed_readback["data"]["artifact_id"],
        inspected["data"]["artifact_id"]
    );
    assert_ne!(
        changed_readback["data"]["revision_id"],
        inspected["data"]["revision_id"]
    );
    assert_ne!(
        changed_readback["data"]["content_root_digest"],
        inspected["data"]["content_root_digest"]
    );

    let (legacy, code) = common::json(&[
        "dsgrid",
        "template",
        "compile",
        "--source",
        base,
        "--out",
        old_template,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{legacy}");
    let (old, code) = common::json(&[
        "dsgrid",
        "template",
        "inspect",
        "--template",
        old_template,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{old}");
    assert_eq!(old["data"]["spotting_default"]["present"], false);

    fs::write(bad_setup, br#"{"schema_version":1,"unknown_field":true}"#).unwrap();
    let (refused, code) = common::json(&[
        "dsgrid",
        "template",
        "compile",
        "--source",
        base,
        "--out",
        bad_template,
        "--spotting-default",
        bad_setup,
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(refused["error"]["code"], "spotting_default_invalid");
    assert!(!std::path::Path::new(bad_template).exists());
}
