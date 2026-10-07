use ds_grid_exchange::blank_model::{BlankModelRequest, create_blank_model};
use ds_grid_exchange::library::bundle_digest;
use ds_grid_exchange::structure_import::import_structure_package;
use serde_json::Value;
use std::process::Command;

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
    let detached = ds_grid_exchange::unpack(&std::fs::read(&detached).unwrap()).unwrap();
    assert!(detached.manifest.model.library_pins.is_empty());
    assert_eq!(detached.assets, after.assets);
    assert_eq!(std::fs::read(&model).unwrap(), source);
    std::fs::remove_dir_all(&dir).unwrap();
}
