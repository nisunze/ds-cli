mod common;
use serde_json::Value;
use std::io::{Cursor, Read};
fn call(args: &[&str]) -> Value {
    let (v, code) = common::json(args);
    assert_eq!(code, 0, "{v}");
    v["data"].clone()
}
#[test]
fn installed_contract_plans_and_exports_one_combined_gis_file() {
    let descriptor = call(&[
        "capabilities",
        "dsgrid-exchange.convert",
        "--output",
        "json",
    ]);
    assert!(
        descriptor["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "mode"
                && a["choices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c == "merge-gis"))
    );
    let fixture = common::fixture();
    let source = fixture.as_str();
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/merge-gis-tests");
    std::fs::create_dir_all(&root).unwrap();
    let temp = tempfile::TempDir::new_in(root).unwrap();
    let out = temp.path().join("gis");
    let args = [
        "dsgrid-exchange",
        "plan",
        "--source",
        source,
        "--source",
        source,
        "--target",
        "shp",
        "--mode",
        "merge-gis",
        "--output",
        "json",
    ];
    let plan = call(&args);
    assert_eq!(plan["executable"], true);
    assert_eq!(plan["per_source"].as_array().unwrap().len(), 2);
    let exported = call(&[
        "dsgrid-exchange",
        "convert",
        "--source",
        source,
        "--source",
        source,
        "--target",
        "shp",
        "--mode",
        "merge-gis",
        "--out",
        out.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(exported["plan_id"], plan["plan_id"]);
    let mut zip = zip::ZipArchive::new(Cursor::new(
        std::fs::read(out.join("model.shp.zip")).unwrap(),
    ))
    .unwrap();
    let name = zip
        .file_names()
        .find(|n| n.ends_with("structures.dbf"))
        .unwrap()
        .to_owned();
    let mut header = [0; 8];
    zip.by_name(&name).unwrap().read_exact(&mut header).unwrap();
    let expected = ds_grid_exchange::package::unpack(&std::fs::read(&fixture).unwrap())
        .unwrap()
        .snapshot
        .structures
        .len()
        * 2;
    assert_eq!(
        u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize,
        expected
    );
    let (v, code) = common::json(&[
        "dsgrid-exchange",
        "convert",
        "--source",
        source,
        "--source",
        source,
        "--target",
        "pls-bak",
        "--mode",
        "merge-gis",
        "--out",
        temp.path().join("refused").to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(v["error"]["code"], "plan_blocked");
    assert!(!temp.path().join("refused").exists());
}
