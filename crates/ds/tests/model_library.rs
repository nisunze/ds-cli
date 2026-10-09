use ds_grid_exchange::blank_model::{BlankModelRequest, create_blank_model};
use ds_grid_exchange::library::bundle_digest;
use ds_grid_exchange::structure_import::import_structure_package;
use serde_json::Value;
use std::process::Command;

mod common;

fn invoke(args: &[&str], ok: bool) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .args(["--output", "json"])
        .output()
        .unwrap();
    let response: Value = serde_json::from_slice(&result.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&result.stdout)));
    assert_eq!(result.status.success(), ok, "{response}");
    response
}

#[test]
fn lifecycle_is_discoverable_and_preserves_native_evidence() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../out")
        .join(format!("model-library-smoke-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let blank = create_blank_model(&BlankModelRequest::default()).unwrap();
    let native = include_bytes!(
        "../../../../ds-network/fixtures/pls-public/humble-pole/workspace/structures/hp-m1-strain.012"
    );
    let source = import_structure_package(&blank.bytes, "hp-m1-strain.012", native, None, None)
        .unwrap()
        .bytes;
    let model = dir.join("source.dsgrid");
    std::fs::write(&model, &source).unwrap();
    let library = dir.join("release.dsgrid-library");
    let adopted = dir.join("adopted.dsgrid");
    let detached = dir.join("detached.dsgrid");
    let cloned = dir.join("cloned.dsgrid-library");
    let path = |p: &std::path::Path| p.to_str().unwrap().to_owned();
    let model_path = path(&model);
    let library_path = path(&library);
    let adopted_path = path(&adopted);
    let detached_path = path(&detached);
    let clone_path = path(&cloned);
    let digest = bundle_digest(&source);
    for name in ["create", "attach", "detach", "clone", "show"] {
        let command = format!("library.model.{name}");
        let descriptor = invoke(&["capabilities", &command], true);
        assert_eq!(descriptor["data"]["command"]["chapter"], "grid-model");
        assert_eq!(descriptor["data"]["command"]["authority"], "none");
    }
    let created = invoke(
        &[
            "library",
            "model",
            "create",
            "--model",
            &model_path,
            "--expected-sha256",
            &digest,
            "--library-id",
            "provider-neutral",
            "--library-version",
            "r1",
            "--out",
            &library_path,
        ],
        true,
    );
    let release_digest = created["data"]["bundle_digest"].as_str().unwrap();
    let attached = invoke(
        &[
            "library",
            "model",
            "attach",
            "--model",
            &model_path,
            "--expected-sha256",
            &digest,
            "--release",
            &library_path,
            "--expected-library-sha256",
            release_digest,
            "--out",
            &adopted_path,
        ],
        true,
    );
    let adopted_digest = attached["data"]["resulting_digest"].as_str().unwrap();
    let shown = invoke(
        &[
            "library",
            "model",
            "show",
            "--model",
            &adopted_path,
            "--expected-sha256",
            adopted_digest,
        ],
        true,
    );
    assert_eq!(shown["data"]["managed_export_allowed"], true);
    assert_eq!(shown["data"]["solver_approval"], false);
    assert_eq!(shown["data"]["members"][0]["status"], "exact_native_bytes");
    let adoption = &shown["data"]["releases"][0];
    assert_eq!(adoption["pinned"], true);
    assert_eq!(adoption["bundle_digest"], release_digest);
    assert_eq!(adoption["pin"], attached["data"]["library_pin"]);
    assert_eq!(adoption["captured_from"]["source_model_digest"], digest);
    assert_eq!(adoption["captured_from"]["solver_approval"], false);
    let before = ds_grid_exchange::unpack(&source).unwrap();
    let after = ds_grid_exchange::unpack(&std::fs::read(&adopted).unwrap()).unwrap();
    assert_eq!(after.snapshot, before.snapshot);
    for asset in &before.assets {
        assert!(after.assets.contains(asset));
    }
    let rejected = dir.join("rejected.dsgrid");
    let rejected_path = path(&rejected);
    let wrong = format!("sha256:{}", "0".repeat(64));
    let refused = invoke(
        &[
            "library",
            "model",
            "attach",
            "--model",
            &model_path,
            "--expected-sha256",
            &wrong,
            "--release",
            &library_path,
            "--expected-library-sha256",
            release_digest,
            "--out",
            &rejected_path,
        ],
        false,
    );
    assert_eq!(refused["error"]["code"], "digest_conflict");
    assert!(!rejected.exists());
    invoke(
        &[
            "library",
            "model",
            "clone",
            "--release",
            &library_path,
            "--expected-sha256",
            release_digest,
            "--library-id",
            "another-library",
            "--library-version",
            "r1",
            "--out",
            &clone_path,
        ],
        true,
    );
    let pin = &attached["data"]["library_pin"];
    invoke(
        &[
            "library",
            "model",
            "detach",
            "--model",
            &adopted_path,
            "--expected-sha256",
            adopted_digest,
            "--library-id",
            pin["artifact_id"].as_str().unwrap(),
            "--library-version",
            pin["revision_id"].as_str().unwrap(),
            "--content-root",
            pin["content_root_digest"].as_str().unwrap(),
            "--out",
            &detached_path,
        ],
        true,
    );
    let detached_bytes = std::fs::read(&detached).unwrap();
    let history = invoke(
        &[
            "library",
            "model",
            "show",
            "--model",
            &detached_path,
            "--expected-sha256",
            &bundle_digest(&detached_bytes),
        ],
        true,
    );
    assert_eq!(history["data"]["pins"], serde_json::json!([]));
    assert_eq!(history["data"]["releases"][0]["pinned"], false);
    assert_eq!(history["data"]["releases"][0]["pin"], *pin);
    let detached = ds_grid_exchange::unpack(&detached_bytes).unwrap();
    assert!(detached.manifest.model.library_pins.is_empty());
    assert_eq!(detached.assets, after.assets);
    assert_eq!(std::fs::read(&model).unwrap(), source);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ordinary_cli_edits_preserve_exact_library_pins_and_offline_admission() {
    use ds_grid_engine::{CommandEnvelope, GridCommand};
    use ds_grid_exchange::model_library::{attach_model_library, create_model_library};
    use ds_grid_model::EntityId;

    let scratch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out");
    let dir = tempfile::tempdir_in(scratch).unwrap();
    let source = std::fs::read(common::fixture()).unwrap();
    let library = create_model_library(
        &source,
        &bundle_digest(&source),
        EntityId::new("edit-proof-library").unwrap(),
        EntityId::new("r1").unwrap(),
    )
    .unwrap();
    let attached = attach_model_library(
        &source,
        &bundle_digest(&source),
        &library,
        &bundle_digest(&library),
        &[],
    )
    .unwrap();
    let before = ds_grid_exchange::unpack(&attached.bytes).unwrap();
    assert!(!before.manifest.model.library_pins.is_empty());
    assert!(!before.manifest.model.library_needs.is_empty());
    let model = dir.path().join("pinned.dsgrid");
    std::fs::write(&model, &attached.bytes).unwrap();
    let structure = &before.snapshot.structures[0];
    let envelope = CommandEnvelope::new(
        "describe-pinned-instance",
        ds_grid_exchange::linked_models::open_session(&before)
            .current_revision()
            .revision_id
            .clone(),
        GridCommand::DescribeStructure {
            id: structure.id.clone(),
            description: Some("Reviewed placed instance".into()),
        },
    );
    let request = dir.path().join("command.json");
    std::fs::write(&request, serde_json::to_vec(&envelope).unwrap()).unwrap();
    let typed = vec![
        "dsgrid",
        "structure",
        "describe",
        "--package",
        model.to_str().unwrap(),
        "--structure",
        structure.id.as_str(),
        "--text",
        "Reviewed placed instance",
    ];
    let generic = vec![
        "dsgrid",
        "apply",
        "--model",
        model.to_str().unwrap(),
        "--envelope",
        request.to_str().unwrap(),
    ];
    for (index, args) in [typed, generic].into_iter().enumerate() {
        let output = dir.path().join(format!("edited-{index}.dsgrid"));
        let mut dry = args.clone();
        dry.extend(["--out", output.to_str().unwrap(), "--dry-run"]);
        assert_eq!(invoke(&dry, true)["data"]["persisted"], false);
        assert!(!output.exists());
        assert_eq!(std::fs::read(&model).unwrap(), attached.bytes);
        let mut write = args.clone();
        write.extend(["--out", output.to_str().unwrap()]);
        if index == 0 {
            write.push("--yes");
        }
        assert_eq!(invoke(&write, true)["data"]["persisted"], true);
        let bytes = std::fs::read(&output).unwrap();
        let after = ds_grid_exchange::unpack(&bytes).unwrap();
        assert_eq!(
            after.manifest.model.library_pins,
            before.manifest.model.library_pins
        );
        assert_eq!(
            after.manifest.model.library_needs,
            before.manifest.model.library_needs
        );
        assert_eq!(after.assets, before.assets);
        assert_eq!(after.exchange_bindings, before.exchange_bindings);
        assert_eq!(
            after.snapshot.structure_types,
            before.snapshot.structure_types
        );
        assert_eq!(
            after.manifest.model.model_revision,
            before.manifest.model.model_revision + 1
        );
        let shown = invoke(
            &[
                "library",
                "model",
                "show",
                "--model",
                output.to_str().unwrap(),
                "--expected-sha256",
                &bundle_digest(&bytes),
            ],
            true,
        );
        assert_eq!(shown["data"]["managed_export_allowed"], true);
        assert_eq!(shown["data"]["solver_approval"], false);
        if index == 0 {
            let unchanged = dir.path().join("unchanged.dsgrid");
            let mut no_change = args;
            let package_arg = no_change
                .iter()
                .position(|arg| *arg == "--package")
                .unwrap()
                + 1;
            no_change[package_arg] = output.to_str().unwrap();
            no_change.extend(["--out", unchanged.to_str().unwrap(), "--yes"]);
            assert_eq!(invoke(&no_change, true)["data"]["persisted"], false);
            assert!(!unchanged.exists());
            assert_eq!(std::fs::read(&output).unwrap(), bytes);
        }
    }
    assert_eq!(std::fs::read(&model).unwrap(), attached.bytes);
}

/// A new release revises no model by itself: the plan names the follower and
/// its changed definition, a wrong plan id writes nothing, the reviewed plan
/// writes the follower's new revision, and a follower that moved since the
/// plan is refused by name.
#[test]
fn library_update_is_planned_then_applied_through_ds() {
    use ds_grid_exchange::model_library::{attach_model_library, create_model_library};
    use ds_grid_exchange::package::{PackOptions, pack};
    use ds_grid_model::EntityId;

    let scratch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out");
    let dir = tempfile::tempdir_in(scratch).unwrap();
    let blank = create_blank_model(&BlankModelRequest::default()).unwrap();
    let native = include_bytes!(
        "../../../../ds-network/fixtures/pls-public/humble-pole/workspace/structures/hp-m1-strain.012"
    );
    let source = import_structure_package(&blank.bytes, "hp-m1-strain.012", native, None, None)
        .unwrap()
        .bytes;
    let release = |model: &[u8], version: &str| {
        create_model_library(
            model,
            &bundle_digest(model),
            EntityId::new("shared-poles").unwrap(),
            EntityId::new(version).unwrap(),
        )
        .unwrap()
    };
    let r1 = release(&source, "r1");
    let follower = attach_model_library(
        &source,
        &bundle_digest(&source),
        &r1,
        &bundle_digest(&r1),
        &[],
    )
    .unwrap()
    .bytes;
    let mut revised = ds_grid_exchange::unpack(&source).unwrap();
    revised.snapshot.structure_types[0].description = Some("Reviewed release two".into());
    let revised = pack(
        &revised.snapshot,
        &PackOptions {
            model_id: revised.manifest.model.model_id,
            model_revision: revised.manifest.model.model_revision + 1,
            presentation: revised.manifest.model.presentation,
            coordinate_system: revised.manifest.model.coordinate_system,
            library_pins: vec![],
            library_needs: vec![],
            assets: revised.assets,
            exchange_bindings: revised.exchange_bindings,
        },
    )
    .unwrap();
    let r2 = release(&revised, "r2");
    let follower_path = dir.path().join("pinned.dsgrid");
    let release_path = dir.path().join("shared-poles-r2.dsgrid-library");
    std::fs::write(&follower_path, &follower).unwrap();
    std::fs::write(&release_path, &r2).unwrap();
    let release_arg = release_path.to_str().unwrap();
    let release_digest = bundle_digest(&r2);
    let follower_arg = format!(
        "{}={}",
        bundle_digest(&follower),
        follower_path.to_str().unwrap()
    );
    for name in ["impact-plan", "impact-apply"] {
        let descriptor = invoke(&["capabilities", &format!("library.model.{name}")], true);
        assert_eq!(descriptor["data"]["command"]["chapter"], "grid-model");
        assert_eq!(descriptor["data"]["command"]["authority"], "none");
    }

    let plan = invoke(
        &[
            "library",
            "model",
            "impact-plan",
            "--release",
            release_arg,
            "--expected-library-sha256",
            &release_digest,
            "--follower",
            &follower_arg,
        ],
        true,
    );
    assert_eq!(plan["data"]["applicable"], 1);
    assert_eq!(plan["data"]["cloud_write"], false);
    let planned = &plan["data"]["followers"][0];
    assert_eq!(planned["status"], "applicable");
    assert_eq!(planned["current_pin"]["revision_id"], "r1");
    assert_eq!(planned["proposed_pin"]["revision_id"], "r2");
    assert!(
        planned["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|element| element["change"] == "changed")
    );
    let plan_id = plan["data"]["plan_id"].as_str().unwrap().to_owned();

    let apply = |plan_id: &str, out: &std::path::Path, ok: bool| {
        invoke(
            &[
                "library",
                "model",
                "impact-apply",
                "--release",
                release_arg,
                "--expected-library-sha256",
                &release_digest,
                "--follower",
                &follower_arg,
                "--plan-id",
                plan_id,
                "--out-dir",
                out.to_str().unwrap(),
            ],
            ok,
        )
    };
    let unreviewed = dir.path().join("unreviewed");
    let refused = apply(&format!("sha256:{}", "0".repeat(64)), &unreviewed, false);
    assert_eq!(refused["error"]["code"], "impact_plan_mismatch");
    assert!(!unreviewed.exists());

    let applied = apply(&plan_id, &dir.path().join("following-r2"), true);
    assert_eq!(applied["data"]["complete"], true);
    assert_eq!(applied["data"]["applied"], 1);
    let row = &applied["data"]["followers"][0];
    assert_eq!(row["outcome"], "applied");
    let bytes = std::fs::read(row["written"].as_str().unwrap()).unwrap();
    assert_eq!(row["resulting_digest"], bundle_digest(&bytes));
    let after = ds_grid_exchange::unpack(&bytes).unwrap();
    assert_eq!(
        after.manifest.model.library_pins[0].revision_id.as_str(),
        "r2"
    );
    assert_eq!(
        after.snapshot.structure_types[0].description.as_deref(),
        Some("Reviewed release two")
    );
    assert_eq!(std::fs::read(&follower_path).unwrap(), follower);

    // An intervening edit moves the follower off its reviewed head.
    std::fs::write(&follower_path, &bytes).unwrap();
    let stale = apply(&plan_id, &dir.path().join("stale"), false);
    assert_eq!(stale["error"]["code"], "impact_apply_incomplete");
    assert_eq!(stale["error"]["detail"]["complete"], false);
    assert_eq!(
        stale["error"]["detail"]["followers"][0]["outcome"],
        "refused"
    );
    assert_eq!(
        stale["error"]["detail"]["followers"][0]["planned_status"],
        "stale_head"
    );
    assert!(
        std::fs::read_dir(dir.path().join("stale"))
            .unwrap()
            .next()
            .is_none()
    );
}

/// A project-native type resolves to the canonical member carrying the same
/// bytes, by bytes alone; an edited file resolves to nothing.
#[test]
fn project_native_types_resolve_to_canonical_members_by_exact_bytes() {
    use ds_grid_exchange::model_library::create_model_library;
    use ds_grid_model::EntityId;

    let scratch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out");
    let dir = tempfile::tempdir_in(scratch).unwrap();
    let native = include_bytes!(
        "../../../../ds-network/fixtures/pls-public/humble-pole/workspace/structures/hp-m1-strain.012"
    );
    let imported = |name: &str, bytes: &[u8]| {
        let blank = create_blank_model(&BlankModelRequest::default()).unwrap();
        import_structure_package(&blank.bytes, name, bytes, None, None)
            .unwrap()
            .bytes
    };
    let canonical_model = imported("hp-m1-strain.012", native);
    let release = create_model_library(
        &canonical_model,
        &bundle_digest(&canonical_model),
        EntityId::new("canonical-structures").unwrap(),
        EntityId::new("r1").unwrap(),
    )
    .unwrap();
    let release_path = dir.path().join("canonical.dsgrid-library");
    std::fs::write(&release_path, &release).unwrap();
    let mut edited_bytes = native.to_vec();
    edited_bytes.extend_from_slice(b"\r\n");
    for (name, bytes, status, member) in [
        (
            "S190_1p_strain_12.012",
            native.to_vec(),
            "exact_member",
            serde_json::json!("hp-m1-strain.012"),
        ),
        (
            "hp-m1-strain.012",
            edited_bytes,
            "no_exact_member",
            Value::Null,
        ),
    ] {
        let project = imported(name, &bytes);
        let path = dir.path().join(format!("{}.dsgrid", status));
        std::fs::write(&path, &project).unwrap();
        let matched = invoke(
            &[
                "library",
                "model",
                "match",
                "--model",
                path.to_str().unwrap(),
                "--expected-sha256",
                &bundle_digest(&project),
                "--release",
                release_path.to_str().unwrap(),
                "--expected-library-sha256",
                &bundle_digest(&release),
            ],
            true,
        );
        assert_eq!(matched["data"]["solver_approval"], false);
        let row = matched["data"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["media"] == "structure_definition")
            .unwrap()
            .clone();
        assert_eq!(row["invariant_leaf"], name);
        assert_eq!(row["status"], status);
        assert_eq!(row["members"][0]["invariant_leaf"], member);
    }
}
