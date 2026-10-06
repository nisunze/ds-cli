mod common;
use ds_grid_exchange::package::{PackOptions, pack, unpack};
use ds_grid_model::{EntityId, GridModelSnapshot, ModelCrs, TerrainPointId};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn call(args: &[&str]) -> Value {
    let (value, code) = common::json(args);
    assert_eq!(code, 0, "{value}");
    value["data"].clone()
}
fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}
fn model(id: &str, x: f64, crs: &str) -> Vec<u8> {
    let mut snapshot = GridModelSnapshot::default();
    let mut point = unpack(&std::fs::read(common::fixture()).unwrap())
        .unwrap()
        .snapshot
        .terrain_points[0]
        .clone();
    point.id = TerrainPointId::new(id).unwrap();
    point.source_id = None;
    point.x_m = x;
    snapshot.terrain_points.push(point);
    pack(
        &snapshot,
        &PackOptions {
            model_id: EntityId::new(id).unwrap(),
            model_revision: 1,
            presentation: Default::default(),
            coordinate_system: ModelCrs::new(crs).unwrap(),
            library_pins: vec![],
            library_needs: vec![],
            assets: vec![],
            exchange_bindings: BTreeMap::new(),
        },
    )
    .unwrap()
}

#[test]
fn review_then_save_is_editable_and_preserves_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/combine-tests");
    std::fs::create_dir_all(&root).unwrap();
    let temp = tempfile::TempDir::new_in(root).unwrap();
    let a = temp.path().join("a.dsgrid");
    let b = temp.path().join("b.dsgrid");
    let out = temp.path().join("combined.dsgrid");
    let originals = [
        model("a", 500100., "EPSG:32735"),
        model("b", 500200., "EPSG:32735"),
    ];
    std::fs::write(&a, &originals[0]).unwrap();
    std::fs::write(&b, &originals[1]).unwrap();
    let preview = call(&[
        "dsgrid",
        "model",
        "combine",
        "--source",
        path(&a),
        "--source",
        path(&b),
        "--output",
        "json",
    ]);
    assert_eq!(preview["executable"], true);
    assert_eq!(preview["persisted"], false);
    assert!(!out.exists());
    let plan = preview["plan_id"].as_str().unwrap();
    let (value, code) = common::json(&[
        "dsgrid",
        "model",
        "combine",
        "--source",
        path(&a),
        "--source",
        path(&b),
        "--apply",
        "--out",
        path(&out),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(value["error"]["code"], "composition_review_required");
    assert!(!out.exists());
    let (value, code) = common::json(&[
        "dsgrid",
        "model",
        "combine",
        "--source",
        path(&b),
        "--source",
        path(&a),
        "--apply",
        "--plan-id",
        plan,
        "--out",
        path(&out),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(value["error"]["code"], "composition_plan_mismatch");
    assert!(!out.exists());
    let receipt = call(&[
        "dsgrid",
        "model",
        "combine",
        "--source",
        path(&a),
        "--source",
        path(&b),
        "--apply",
        "--plan-id",
        plan,
        "--out",
        path(&out),
        "--output",
        "json",
    ]);
    assert_eq!(receipt["persisted"], true);
    let combined = unpack(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(combined.snapshot.terrain_points.len(), 2);
    assert!(
        combined
            .manifest
            .model
            .model_id
            .as_str()
            .starts_with("composed-")
    );
    let mut session = ds_grid_engine::GridSession::open(combined.snapshot);
    assert!(session.save_checkpoint().is_ok());
    assert_eq!(std::fs::read(a).unwrap(), originals[0]);
    assert_eq!(std::fs::read(b).unwrap(), originals[1]);
    let before = std::fs::read(&out).unwrap();
    let (_, code) = common::json(&[
        "dsgrid",
        "model",
        "combine",
        "--source",
        path(&out),
        "--source",
        path(&out),
        "--apply",
        "--plan-id",
        plan,
        "--out",
        path(&out),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(std::fs::read(out).unwrap(), before);
}

#[test]
fn incompatible_crs_is_a_blocked_preview_without_mutation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/combine-tests");
    std::fs::create_dir_all(&root).unwrap();
    let temp = tempfile::TempDir::new_in(root).unwrap();
    let a = temp.path().join("a.dsgrid");
    let b = temp.path().join("b.dsgrid");
    std::fs::write(&a, model("a", 500100., "EPSG:32735")).unwrap();
    std::fs::write(&b, model("b", 500200., "EPSG:32635")).unwrap();
    let preview = call(&[
        "dsgrid",
        "model",
        "combine",
        "--source",
        path(&a),
        "--source",
        path(&b),
        "--output",
        "json",
    ]);
    assert_eq!(preview["executable"], false);
    assert!(!preview["blockers"].as_array().unwrap().is_empty());
}

#[test]
fn automatic_commands_are_retired_and_manual_combination_is_described() {
    let descriptor = call(&["capabilities", "dsgrid.model.combine", "--output", "json"]);
    assert!(descriptor.to_string().contains("plan-id"));
    for id in [
        "dsgrid.model.split",
        "dsgrid.model.reconcile",
        "dsgrid.model.status",
        "dsgrid.model.extract",
    ] {
        let (_, code) = common::json(&["capabilities", id, "--output", "json"]);
        assert_ne!(code, 0, "{id} must be absent");
    }
}
