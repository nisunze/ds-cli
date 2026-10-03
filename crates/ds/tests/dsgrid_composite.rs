mod common;
use ds_grid_engine::composite::{
    OriginalPolicy, PartitionSelector, ReconcileRequest, SplitRequest,
};
use ds_grid_exchange::{
    linked_models,
    package::{GridPackage, PackOptions, pack, unpack},
};
use ds_grid_model::{EntityId, GridModelSnapshot, ModelCrs, TableKind};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

fn ok(args: &[&str]) -> Value {
    let (value, code) = common::json(args);
    assert_eq!(code, 0, "{value}");
    value["data"].clone()
}
fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}
fn write_json(path: &Path, value: &impl serde::Serialize) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn repack(package: &GridPackage) -> Vec<u8> {
    let model = &package.manifest.model;
    pack(
        &package.snapshot,
        &PackOptions {
            model_id: model.model_id.clone(),
            model_revision: model.model_revision + 1,
            presentation: model.presentation.clone(),
            coordinate_system: model.coordinate_system.clone(),
            library_pins: model.library_pins.clone(),
            library_needs: model.library_needs.clone(),
            assets: package.assets.clone(),
            exchange_bindings: package.exchange_bindings.clone(),
        },
    )
    .unwrap()
}

#[test]
fn all_linked_commands_plan_by_default_apply_atomically_and_name_conflicts() {
    let scratch =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-network/scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let temp = tempfile::TempDir::new_in(&scratch).unwrap();
    let root = temp.path();
    let mut snapshot = GridModelSnapshot::default();
    let mut point = unpack(&std::fs::read(common::fixture()).unwrap())
        .unwrap()
        .snapshot
        .terrain_points[0]
        .clone();
    point.id = ds_grid_model::TerrainPointId::new("point-a").unwrap();
    point.source_id = None;
    point.x_m = 0.0;
    snapshot.terrain_points.push(point.clone());
    point.id = ds_grid_model::TerrainPointId::new("point-b").unwrap();
    point.x_m = 20.0;
    snapshot.terrain_points.push(point);
    let source_bytes = pack(
        &snapshot,
        &PackOptions {
            model_id: EntityId::new("cli-original").unwrap(),
            model_revision: 1,
            presentation: Default::default(),
            coordinate_system: ModelCrs::new("EPSG:32735").unwrap(),
            library_pins: Vec::new(),
            library_needs: Vec::new(),
            assets: Vec::new(),
            exchange_bindings: BTreeMap::new(),
        },
    )
    .unwrap();
    let source = root.join("source.dsgrid");
    std::fs::write(&source, &source_bytes).unwrap();
    let split_request = root.join("split.json");
    write_json(
        &split_request,
        &SplitRequest {
            original: "cli-original".into(),
            composite: "combined".into(),
            selected_part: "north".into(),
            remainder_part: "south".into(),
            selector: PartitionSelector::Attribute {
                table: TableKind::TerrainPoints,
                field: "x_m".into(),
                value: json!(0.0),
            },
            original_policy: OriginalPolicy::Delete,
        },
    );
    let bundle = root.join("generation-0.dsgrid-links");
    let dry = ok(&[
        "dsgrid",
        "model",
        "split",
        "--package",
        path(&source),
        "--request",
        path(&split_request),
        "--out",
        path(&bundle),
        "--output",
        "json",
    ]);
    assert_eq!(dry["status"], "dry_run");
    assert!(!bundle.exists());
    let publication = root.join("publication.json");
    write_json(
        &publication,
        &json!({"project":"project-1","graph_id":"graph-1","expected_generation":null,"model_kind":"general","participants":{
      "combined":{"model_id":"project-combined","expected_head_revision_id":"","display_name":"Combined"},
      "north":{"model_id":"project-north","expected_head_revision_id":"","display_name":"North"},
      "south":{"model_id":"project-south","expected_head_revision_id":"","display_name":"South"}},"retire":null,"reason":"Initial linked split"}),
    );
    let project_dry = ok(&[
        "dsgrid",
        "model",
        "split",
        "--package",
        path(&source),
        "--request",
        path(&split_request),
        "--publication",
        path(&publication),
        "--out",
        path(&bundle),
        "--output",
        "json",
    ]);
    assert_eq!(project_dry["status"], "dry_run");
    assert_eq!(project_dry["project_published"], false);
    assert_eq!(
        project_dry["publication"]["request"]["action"],
        "publish_linked"
    );
    let linked_request = &project_dry["publication"]["request"]["linked"];
    assert_eq!(linked_request["composite_model_id"], "project-combined");
    assert_eq!(
        linked_request["part_model_ids"],
        json!(["project-north", "project-south"])
    );
    for version in linked_request["versions"].as_array().unwrap() {
        assert_eq!(version["project_id"], "project-1");
        assert_eq!(version["expected_head_revision_id"], "");
        assert!(version["manifest_model_revision"].as_u64().is_some());
    }
    assert!(!bundle.exists());
    let unconfirmed_out = root.join("unconfirmed-project.dsgrid-links");
    let (unconfirmed, code) = common::json(&[
        "dsgrid",
        "model",
        "split",
        "--package",
        path(&source),
        "--request",
        path(&split_request),
        "--publication",
        path(&publication),
        "--apply",
        "--out",
        path(&unconfirmed_out),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(unconfirmed["error"]["code"], "confirmation_required");
    assert!(
        !unconfirmed_out.exists(),
        "the project confirmation gate runs before local or remote writes"
    );
    let publication_schema = ok(&[
        "dsgrid",
        "describe",
        "--linked-publication",
        "--output",
        "json",
    ]);
    assert!(publication_schema["properties"]["participants"].is_object());
    let applied = ok(&[
        "dsgrid",
        "model",
        "split",
        "--package",
        path(&source),
        "--request",
        path(&split_request),
        "--apply",
        "--out",
        path(&bundle),
        "--output",
        "json",
    ]);
    assert_eq!(applied["status"], "applied");
    assert_eq!(applied["candidate_sha256"], dry["candidate_sha256"]);
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    let status = ok(&[
        "dsgrid",
        "model",
        "status",
        "--bundle",
        path(&bundle),
        "--output",
        "json",
    ]);
    assert_eq!(status["status"], "verified");
    assert_eq!(status["generation"], 0);
    assert_eq!(status["features"], 2);
    assert_eq!(status["project_published"], false);
    let burst = root.join("burst.json");
    write_json(
        &burst,
        &ReconcileRequest {
            expected_generation: 0,
            max_affected_features: 100,
            events: std::collections::BTreeMap::new(),
            clearance_half_width_m: 12.0,
            new_owners: BTreeMap::new(),
        },
    );
    let combined = ok(&[
        "dsgrid",
        "model",
        "combine",
        "--bundle",
        path(&bundle),
        "--request",
        path(&burst),
        "--output",
        "json",
    ]);
    assert_eq!(combined["counts"]["dirty"], 0);
    let checkpoint = linked_models::decode(&std::fs::read(&bundle).unwrap()).unwrap();
    let mut owner = checkpoint.packages["north"].clone();
    owner.snapshot.terrain_points[0].description = Some("owner change".into());
    let edited = root.join("edited.dsgrid");
    std::fs::write(&edited, repack(&owner)).unwrap();
    let edit_arg = format!("north={}", edited.display());
    let next = root.join("generation-1.dsgrid-links");
    let reconciled = ok(&[
        "dsgrid",
        "model",
        "reconcile",
        "--bundle",
        path(&bundle),
        "--request",
        path(&burst),
        "--edited",
        &edit_arg,
        "--apply",
        "--out",
        path(&next),
        "--output",
        "json",
    ]);
    assert_eq!(reconciled["generation"], 1);
    assert_eq!(reconciled["counts"]["dirty"], 1);
    let mut composite = checkpoint.packages["combined"].clone();
    composite.snapshot.terrain_points[0].description = Some("other change".into());
    let composite_path = root.join("composite.dsgrid");
    std::fs::write(&composite_path, repack(&composite)).unwrap();
    let composite_arg = format!("combined={}", composite_path.display());
    let (error, code) = common::json(&[
        "dsgrid",
        "model",
        "reconcile",
        "--bundle",
        path(&bundle),
        "--request",
        path(&burst),
        "--edited",
        &edit_arg,
        "--edited",
        &composite_arg,
        "--output",
        "json",
    ]);
    assert_eq!(code, 5, "{error}");
    assert_eq!(error["error"]["code"], "composite_conflict");
    assert_eq!(
        error["error"]["detail"]["features"][0],
        "terrain_points:[\"point-a\"]"
    );
    let mut sources = Vec::new();
    for id in ["north", "south"] {
        let p = root.join(format!("{id}.dsgrid"));
        std::fs::write(&p, repack(&checkpoint.packages[id])).unwrap();
        sources.push(p);
    }
    let link_request = root.join("link.json");
    write_json(
        &link_request,
        &json!({"composite":"linked-again","sources":sources,"shared_owners":{}}),
    );
    let linked = ok(&[
        "dsgrid",
        "model",
        "link",
        "--request",
        path(&link_request),
        "--output",
        "json",
    ]);
    assert_eq!(linked["status"], "dry_run");
    assert_eq!(linked["features"], 2);
    let descriptor = ok(&["dsgrid", "describe", "--linked-models", "--output", "json"]);
    assert_eq!(descriptor, ds_grid_engine::composite::descriptors());
    for verb in ["split", "combine", "link", "reconcile", "status"] {
        let d = ok(&[
            "capabilities",
            &format!("dsgrid.model.{verb}"),
            "--output",
            "json",
        ]);
        assert_eq!(d["command"]["availability"], "available");
    }
}

#[test]
fn every_engine_linked_refusal_is_declared_by_the_cli() {
    let codes = ds_grid_engine::composite::descriptors()["refusals"]
        .as_array()
        .unwrap()
        .clone();
    for code in codes {
        assert!(
            ds_cli_dsgrid::model::composite::REFUSALS
                .iter()
                .any(|refusal| code == refusal.code)
        );
    }
}
