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
    // Nobody names the combined model: its identity derives from the parts.
    let combined =
        ds_grid_engine::composite::combined_identity(&std::collections::BTreeSet::from([
            "north".to_string(),
            "south".to_string(),
        ]));
    assert_eq!(dry["composite"], combined.as_str());
    // Only submodels are bound: the combined model's binding is derived.
    let publication = root.join("publication.json");
    let binding = json!({"project":"project-1","expected_generation":null,"model_kind":"general","participants":{
      "north":{"model_id":"project-north","expected_head_revision_id":"","display_name":"North"},
      "south":{"model_id":"project-south","expected_head_revision_id":"","display_name":"South"}},"retire":null,"reason":"Initial linked split"});
    write_json(&publication, &binding);
    let mut naming_combined = binding.clone();
    naming_combined["participants"][combined.as_str()] = json!({"model_id":"project-combined","expected_head_revision_id":"","display_name":"Combined"});
    let named = root.join("publication-naming-combined.json");
    write_json(&named, &naming_combined);
    let (refused, code) = common::json(&[
        "dsgrid",
        "model",
        "split",
        "--package",
        path(&source),
        "--request",
        path(&split_request),
        "--publication",
        path(&named),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0, "{refused}");
    assert_eq!(refused["error"]["code"], "composite_publication_invalid");
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
    let combined_model = combined.replacen('.', "-", 1);
    assert_eq!(linked_request["graph_id"], "combined");
    assert_eq!(
        linked_request["composite_model_id"],
        combined_model.as_str()
    );
    let derived = &project_dry["publication"]["bindings"][combined.as_str()];
    assert_eq!(derived["derived"], true);
    assert_eq!(derived["action"], "create");
    assert!(
        linked_request["versions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|version| version["model_id"] == combined_model.as_str()
                && version["display_name"] == "Combined model")
    );
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
    assert_eq!(status["candidate_sha256"], applied["candidate_sha256"]);
    let heads = status["participants"].as_array().unwrap();
    assert_eq!(heads.len(), 3);
    assert!(
        heads
            .iter()
            .all(|head| head["state"] == "in_step" && head["generation"] == 0)
    );
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
    let steady = ok(&[
        "dsgrid",
        "model",
        "reconcile",
        "--bundle",
        path(&bundle),
        "--request",
        path(&burst),
        "--output",
        "json",
    ]);
    assert_eq!(steady["counts"]["dirty"], 0);
    assert_eq!(steady["generation"], 0);
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
    // The explicit diff names the adopted edit, its author and its fields.
    assert_eq!(
        reconciled["changes"]["features"]["terrain_points:[\"point-a\"]"],
        json!({"change": "modified", "by": "north", "fields": ["description"]})
    );
    // A staged edit is ahead of the generation until the burst adopts it.
    let pending = ok(&[
        "dsgrid",
        "model",
        "status",
        "--bundle",
        path(&bundle),
        "--edited",
        &edit_arg,
        "--output",
        "json",
    ]);
    let state = |id: &str| {
        pending["participants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|head| head["id"] == id)
            .unwrap()["state"]
            .clone()
    };
    assert_eq!(state("north"), "ahead");
    assert_eq!(state("south"), "in_step");
    // A terrain edit also refreshes the other split part's read-only ground mirror.
    let written = |id: &str| {
        reconciled["participants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|head| head["id"] == id)
            .unwrap()["state"]
            .clone()
    };
    assert_eq!(written("north"), "new_revision");
    assert_eq!(written(&combined), "new_revision");
    assert_eq!(written("south"), "new_revision");
    // Extract writes one participant's exact attested bytes, never repacked.
    let extracted_path = root.join("extracted-south.dsgrid");
    let extracted = ok(&[
        "dsgrid",
        "model",
        "extract",
        "--bundle",
        path(&next),
        "--model",
        "south",
        "--out",
        path(&extracted_path),
        "--output",
        "json",
    ]);
    assert_eq!(extracted["role"], "part");
    let south =
        linked_models::exact_packages(&std::fs::read(&next).unwrap()).unwrap()["south"].clone();
    assert_eq!(std::fs::read(&extracted_path).unwrap(), south);
    let (again, code) = common::json(&[
        "dsgrid",
        "model",
        "extract",
        "--bundle",
        path(&next),
        "--model",
        "south",
        "--out",
        path(&extracted_path),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(again["error"]["code"], "linked_output_exists");
    let mut composite = checkpoint.packages[&combined].clone();
    composite.snapshot.terrain_points[0].description = Some("other change".into());
    let composite_path = root.join("composite.dsgrid");
    std::fs::write(&composite_path, repack(&composite)).unwrap();
    let composite_arg = format!("{combined}={}", composite_path.display());
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
    assert_eq!(code, 2, "{error}");
    assert_eq!(error["error"]["code"], "composite_model_protected");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("north")
    );
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("south")
    );
    let mut sources = Vec::new();
    for id in ["north", "south"] {
        let p = root.join(format!("{id}.dsgrid"));
        std::fs::write(&p, repack(&checkpoint.packages[id])).unwrap();
        sources.push(p);
    }
    // Deriving the combined model again from the same two submodels is the
    // same deterministic master: same identity, same canonical digest.
    let derive_request = root.join("derive.json");
    write_json(
        &derive_request,
        &json!({"sources":sources,"shared_owners":{"terrain_points:[\"point-a\"]":"north", "terrain_points:[\"point-b\"]":"south"}}),
    );
    let missing_owners_request = root.join("missing-owners.json");
    write_json(
        &missing_owners_request,
        &json!({"sources":sources,"shared_owners":{}}),
    );
    let (missing, code) = common::json(&[
        "dsgrid",
        "model",
        "reconcile",
        "--request",
        path(&missing_owners_request),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(missing["error"]["code"], "composite_owner_required");
    assert_eq!(
        missing["error"]["detail"]["features"],
        json!([
            "terrain_points:[\"point-a\"]",
            "terrain_points:[\"point-b\"]"
        ])
    );
    let derived = ok(&[
        "dsgrid",
        "model",
        "reconcile",
        "--request",
        path(&derive_request),
        "--output",
        "json",
    ]);
    assert_eq!(derived["status"], "dry_run");
    assert_eq!(derived["features"], 2);
    assert_eq!(derived["composite"], combined.as_str());
    assert_eq!(derived["canonical_digest"], applied["canonical_digest"]);
    // Publishing a derived generation keeps every unchanged submodel's head:
    // its package is its head's exact bytes, so no revision is written.
    let bind = |heads: Value| {
        json!({"project":"project-1","expected_generation":null,"model_kind":"general","participants":{
            "north":{"model_id":"project-north","expected_head_revision_id":heads["north"],"display_name":null},
            "south":{"model_id":"project-south","expected_head_revision_id":heads["south"],"display_name":null}},
            "retire":null,"reason":"Derive the combined model"})
    };
    let captured = root.join("derive-publication-1.json");
    write_json(
        &captured,
        &bind(json!({"north":"head-north","south":"head-south"})),
    );
    let planned = ok(&[
        "dsgrid",
        "model",
        "reconcile",
        "--request",
        path(&derive_request),
        "--publication",
        path(&captured),
        "--output",
        "json",
    ]);
    let bindings = &planned["publication"]["bindings"];
    assert_eq!(bindings["north"]["action"], "new_revision");
    let heads = json!({"north": bindings["north"]["revision_id"], "south": bindings["south"]["revision_id"]});
    let current = root.join("derive-publication-2.json");
    write_json(&current, &bind(heads));
    let kept = ok(&[
        "dsgrid",
        "model",
        "reconcile",
        "--request",
        path(&derive_request),
        "--publication",
        path(&current),
        "--output",
        "json",
    ]);
    let bindings = &kept["publication"]["bindings"];
    assert_eq!(bindings["north"]["action"], "keep");
    assert_eq!(bindings["south"]["action"], "keep");
    assert_eq!(bindings[combined.as_str()]["action"], "create");
    // No user-facing create or combine: `model link` is the PLS-CADD
    // provenance link only, and `model combine` does not exist.
    let (refused, code) = common::json(&[
        "dsgrid",
        "model",
        "link",
        "--request",
        path(&derive_request),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0, "{refused}");
    let (refused, code) = common::json(&[
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
    assert_ne!(code, 0, "{refused}");
    // The combined model is never a PLS-CADD export unit.
    let packages = linked_models::exact_packages(&std::fs::read(&bundle).unwrap()).unwrap();
    let combined_package = root.join("combined.dsgrid");
    std::fs::write(&combined_package, &packages[&combined]).unwrap();
    // Exact extracted combined bytes retain protected mode at every local door.
    let protected = unpack(&packages[&combined]).unwrap();
    let envelope = ds_grid_engine::CommandEnvelope::new(
        "protected-terrain-edit",
        linked_models::open_session(&protected)
            .current_revision()
            .revision_id
            .clone(),
        ds_grid_engine::GridCommand::SetTerrainPointElevation {
            id: protected.snapshot.terrain_points[0].id.clone(),
            elevation_m: 99.0,
        },
    );
    let envelope_path = root.join("protected-edit.json");
    write_json(&envelope_path, &envelope);
    let refused_out = root.join("protected-save.dsgrid");
    for mode in [vec!["--dry-run"], vec!["--out", path(&refused_out)]] {
        let mut args = vec![
            "dsgrid",
            "apply",
            "--model",
            path(&combined_package),
            "--envelope",
            path(&envelope_path),
        ];
        args.extend(mode);
        args.extend(["--output", "json"]);
        let (refusal, code) = common::json(&args);
        assert_ne!(code, 0, "{refusal}");
        assert_eq!(refusal["error"]["code"], "composite_model_protected");
        assert!(
            refusal["error"]["message"]
                .as_str()
                .unwrap()
                .contains("north")
        );
        assert!(!refused_out.exists());
    }
    let (refusal, code) = common::json(&[
        "dsgrid",
        "publish-version",
        "--path",
        path(&combined_package),
        "--project",
        "protected-copy",
        "--kind",
        "mv_line",
        "--name",
        "Combined",
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(code, 0, "{refusal}");
    assert_eq!(
        refusal["error"]["code"], "composite_model_protected",
        "{refusal}"
    );
    for owner in ["north", "south"] {
        assert!(
            refusal["error"]["message"]
                .as_str()
                .unwrap()
                .contains(owner)
        );
    }
    let (refusal, code) = common::json(&[
        "dsgrid",
        "model",
        "import-external",
        "--path",
        path(&combined_package),
        "--name",
        "User combined",
        "--account",
        "test-protected",
        "--output",
        "json",
    ]);
    assert_ne!(code, 0, "{refusal}");
    assert_eq!(
        refusal["error"]["code"], "composite_model_protected",
        "{refusal}"
    );
    // A persisted combined read view cannot be deleted or linked by the CLI.
    // Seed it through the native store as a fixture, outside real catalogues.
    {
        use ds_command_kernel::local_models::{Op, Origin, Scope};
        use sha2::{Digest, Sha256};
        let layer_root = root.join("isolated-models");
        let scope = Scope {
            lane: "stable".into(),
            uid: "test-protected".into(),
        };
        let combined_bytes = &packages[&combined];
        ds_layer_store::local_models::execute_at(
            &layer_root,
            &scope,
            Op::Register {
                id: "local-protected".into(),
                display_name: "Combined read view".into(),
                origin: Origin::Imported,
                crs: protected.manifest.model.coordinate_system.to_string(),
                model_revision: 1,
                bytes: combined_bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(combined_bytes)),
                created_at: Some("2026-10-06T00:00:00Z".into()),
                project: None,
                head_revision: None,
                activate: false,
            },
            Some(combined_bytes),
        )
        .unwrap();
        let baseline = ds_layer_store::local_models::read_at(&layer_root, &scope).unwrap();
        for operation in ["forget", "link", "unlink"] {
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ds"));
            command.args([
                "dsgrid",
                "model",
                operation,
                "--model",
                "local-protected",
                "--account",
                "test-protected",
                "--output",
                "json",
            ]);
            if operation == "link" {
                command.args(["--workspace", path(root)]);
            }
            let output = command.env("DS_LAYER_HOME", &layer_root).output().unwrap();
            let refusal: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(!output.status.success(), "{refusal}");
            assert_eq!(
                refusal["error"]["code"], "composite_model_protected",
                "{refusal}"
            );
            for owner in ["north", "south"] {
                assert!(
                    refusal["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains(owner)
                );
            }
            assert_eq!(
                ds_layer_store::local_models::read_at(&layer_root, &scope).unwrap(),
                baseline
            );
        }
    }
    let (refusal, code) = common::json(&[
        "dsgrid",
        "create",
        "--model-id",
        &combined,
        "--crs",
        "EPSG:32735",
        "--out",
        path(&refused_out),
        "--output",
        "json",
    ]);
    assert_ne!(code, 0, "{refusal}");
    assert_eq!(refusal["error"]["code"], "composite_model_protected");
    assert!(!refused_out.exists());
    // MCP relays the same native refusal through a real stdio call.
    let batch_path = root.join("protected-batch.json");
    write_json(
        &batch_path,
        &json!({"expected_revision": envelope.expected_revision,
        "commands":[{"command_id":"protected-batch", "command":envelope.command}]}),
    );
    let (refusal, code) = common::json(&[
        "dsgrid",
        "apply-batch",
        "--model",
        path(&combined_package),
        "--batch",
        path(&batch_path),
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_ne!(code, 0, "{refusal}");
    assert_eq!(refusal["error"]["code"], "composite_model_protected");
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
                "mcp",
                "serve",
                "--exposure",
                "commands",
                "--profile",
                "grid-native",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let stdin = child.stdin.as_mut().unwrap();
            for request in [
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"dsgrid_apply","arguments":{"model":combined_package,"envelope":envelope_path,"dry-run":true}}}),
                json!({"jsonrpc":"2.0","id":999,"method":"shutdown"}),
            ] {
                serde_json::to_writer(&mut *stdin, &request).unwrap();
                stdin.write_all(b"\n").unwrap();
            }
        }
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let response = String::from_utf8(result.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|r| r["id"] == 2)
            .unwrap();
        assert!(
            response.to_string().contains("composite_model_protected"),
            "{response}"
        );
        assert!(response.to_string().contains("north"), "{response}");
    }
    // Reading the same protected bytes remains available.
    let read = ok(&[
        "dsgrid",
        "inspect",
        "--model",
        path(&combined_package),
        "--output",
        "json",
    ]);
    assert!(read.is_object());
    let plan = ok(&[
        "dsgrid-exchange",
        "plan",
        "--source",
        path(&combined_package),
        "--target",
        "pls-bak",
        "--output",
        "json",
    ]);
    assert_eq!(plan["executable"], false, "{plan}");
    assert!(
        plan["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker
                .as_str()
                .is_some_and(|text| text.contains("PLS-CADD export refused"))),
        "{plan}"
    );
    let descriptor = ok(&["dsgrid", "describe", "--linked-models", "--output", "json"]);
    assert_eq!(descriptor, ds_grid_engine::composite::descriptors());
    for verb in ["split", "reconcile", "status", "extract"] {
        let d = ok(&[
            "capabilities",
            &format!("dsgrid.model.{verb}"),
            "--output",
            "json",
        ]);
        assert_eq!(d["command"]["availability"], "available");
        let tool = ds_cli_mcp::tools::tool_from_descriptor(&d["command"]).unwrap();
        assert!(
            tool.description.contains("composite_model_protected"),
            "{tool:?}"
        );
        assert!(tool.description.contains("protected mode"), "{tool:?}");
    }
}

#[test]
fn every_engine_linked_refusal_is_declared_by_the_cli() {
    for id in [
        "dsgrid.apply",
        "dsgrid.apply-batch",
        "dsgrid.model.link",
        "dsgrid.model.unlink",
        "dsgrid.model.forget",
        "dsgrid.publish-version",
        "dsgrid.create",
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let refusals = descriptor["command"]["refusals"].as_array().unwrap();
        let codes = refusals
            .iter()
            .map(|r| r["code"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(codes.len(), refusals.len(), "duplicate refusal on {id}");
        assert!(codes.contains("composite_model_protected"), "{id}");
        let tool = ds_cli_mcp::tools::tool_from_descriptor(&descriptor["command"]).unwrap();
        assert!(
            tool.description.contains("composite_model_protected"),
            "{id}"
        );
    }
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

/// Pinned with the same rows in ds-grid-engine's composite suite: the CLI
/// builds serde_json with `preserve_order`, and the canonical digest of a
/// combined model must not depend on that.
#[test]
fn canonical_digest_is_the_engine_constant_in_this_host() {
    let mut snapshot = GridModelSnapshot::default();
    snapshot.alignments.push(
        serde_json::from_value(
            json!({"id": "al-host", "label": "Host", "global_station_gap_m": 100.0}),
        )
        .unwrap(),
    );
    snapshot.feature_codes.push(
        serde_json::from_value(json!({"id": "fc-host", "code_token": "HOST", "name": "HOST",
            "description": null, "namespace": null, "applies_to_points": true,
            "applies_to_lines": false, "applies_to_polygons": false, "survey_form": null,
            "plan_policy": {"visible": true, "color": null, "symbol": null, "point_radius_px": null, "line_width_px": null},
            "profile_policy": {"visible": true, "color": null, "marker": null, "show_ordinate": true},
            "label_policy": {"show_by_default": true, "source": "code_token", "min_zoom": null},
            "superseded_by": null}))
        .unwrap(),
    );
    assert_eq!(
        ds_grid_engine::composite::canonical_digest(&snapshot).unwrap(),
        "sha256:f7fd7576795eeda545f644901bb65ac44f3f38d8cfbc012192bf5fdc2b790e03"
    );
}
