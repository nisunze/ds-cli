//! The CLI only transports the native omission choice and its exact refusal.
mod common;

use ds_grid_exchange::{
    blank_model::{BlankModelRequest, create_blank_model},
    conversion::{
        BatchMode, ConversionRequest, PlsContainer, PlsVersionIntent, SourceCandidate, SourceSet,
        TargetFormat, plan_conversion,
    },
    package::unpack,
    structure_import::import_structure_package,
};

#[test]
fn omission_contract_and_read_only_plan_match_the_native_owner() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out");
    std::fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root).unwrap();
    let blank = create_blank_model(&BlankModelRequest::default()).unwrap();
    let bytes = import_structure_package(&blank.bytes, "hp-m1-strain.012",
        include_bytes!("../../../../ds-network/fixtures/pls-public/humble-pole/workspace/structures/hp-m1-strain.012"), None, None).unwrap().bytes;
    let id = unpack(&bytes).unwrap().snapshot.structure_types[0]
        .id
        .clone();
    let source = temp.path().join("source.dsgrid");
    std::fs::write(&source, &bytes).unwrap();
    for command in ["dsgrid-exchange.plan", "dsgrid-exchange.convert"] {
        let (descriptor, code) = common::json(&["capabilities", command, "--output", "json"]);
        assert_eq!(code, 0, "{descriptor}");
        assert!(
            descriptor["data"]["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg["name"] == "omit-unused-structure-type")
        );
    }
    let native = plan_conversion(&ConversionRequest {
        sources: SourceSet::new(vec![SourceCandidate::file("source.dsgrid", bytes.clone())]),
        batch_mode: BatchMode::Separate,
        target: TargetFormat::PlsBackup,
        pls_container: PlsContainer::Folder,
        pls_version_intent: PlsVersionIntent::ConvertTo16_81,
        combine: None,
        declared_crs: None,
        expected_location: None,
        swap_xy: false,
        selection: vec![],
        gis_include_layers: vec![],
        pls_project: None,
        omitted_structure_type_ids: vec![id.clone()],
    });
    let (response, code) = common::json(&[
        "dsgrid-exchange",
        "plan",
        "--source",
        source.to_str().unwrap(),
        "--target",
        "pls-bak",
        "--omit-unused-structure-type",
        id.as_str(),
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{response}");
    let data = &response["data"];
    assert_eq!(data["plan_id"], native.plan_id);
    assert_eq!(data["omitted_structure_type_ids"], serde_json::json!([id]));
    assert_eq!(data["blockers"], serde_json::json!(native.blockers));
    assert_eq!(data["losses"], serde_json::json!(native.losses));
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    let out = temp.path().join("must-not-exist");
    let (refusal, code) = common::json(&[
        "dsgrid-exchange",
        "convert",
        "--source",
        source.to_str().unwrap(),
        "--target",
        "pls-bak",
        "--omit-unused-structure-type",
        "missing-type",
        "--out",
        out.to_str().unwrap(),
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(code, 0, "{refusal}");
    assert!(
        refusal.to_string().contains("structure_omission_unknown"),
        "{refusal}"
    );
    assert!(!out.exists());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}
