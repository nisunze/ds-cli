//! Engine unit store tests, kept beside their owner so engine_unit.rs counts
//! production code.
use super::*;
use serde_json::json;
use std::cell::Cell;

const NETWORK: ModuleComponent = ModuleComponent::Network;

fn private_base() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    directory
}
fn authority() -> CapturedReleaseAuthority {
    CapturedReleaseAuthority {
        gateway_origin: "https://gateway.example".into(),
        channel: "canary",
        catalog_sha256: "c".repeat(64),
        gateway_key: "key".into(),
    }
}
/// The published Network catalog fixture rebound to small fixture bytes.
fn fixture(revision: usize) -> (Value, Vec<(&'static str, Vec<u8>)>) {
    let mut release: Value = serde_json::from_str::<Value>(include_str!(
        "../../../ds-command-kernel/src/module_release/fixtures/network-wasm-canary.json"
    ))
    .unwrap()["release"]
        .clone();
    let version = format!("0.1.0-r{revision}");
    release["version"] = json!(version);
    let files = policy::member_names(NETWORK)
        .unwrap()
        .into_iter()
        .map(|name| (name, format!("fixture:{name}:{revision}\n").into_bytes()))
        .collect::<Vec<_>>();
    for asset in release["assets"].as_array_mut().unwrap() {
        let (name, bytes) = files
            .iter()
            .find(|(name, _)| asset["url"].as_str().unwrap().ends_with(&format!("/{name}")))
            .unwrap();
        asset["url"] = json!(format!("https://storage.googleapis.com/dedicated-public-releases/modules/network/canary/wasm/{version}/{name}"));
        asset["size_bytes"] = json!(bytes.len());
        asset["sha256"] = json!(format!("{:x}", Sha256::digest(bytes)));
    }
    (release, files)
}
fn grant(release: &Value, expected: Option<&str>, calls: &Cell<usize>) -> Result<TrustedEngineGrant, String> {
    authority().acquire_engine_unit_with(NETWORK, expected, &|| false, |url| {
        calls.set(calls.get() + 1);
        assert!(url.ends_with("/api/v1/app_releases?component=network&channel=canary&runtime=wasm"));
        Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
    })
}
fn stage_published(store: &EngineUnitStore, revision: usize) -> StagedEngineUnit {
    let (release, files) = fixture(revision);
    let calls = Cell::new(0);
    store
        .stage_with(grant(&release, None, &calls).unwrap(), &|| false, |asset, path| {
            let (_, bytes) = files
                .iter()
                .find(|(name, _)| asset.url.ends_with(&format!("/{name}")))
                .unwrap();
            fs::write(path, bytes).map_err(|e| e.to_string())
        })
        .unwrap()
}
fn local_inputs(directory: &Path) -> (PathBuf, String, String, Vec<(&'static str, Vec<u8>)>) {
    let source = "7".repeat(40);
    let identity = json!({"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0",
        "sourceSha":source,"dependencyClosureSha256":"8".repeat(64),"environment":"wasm",
        "runtime":{"environment":"wasm","logicalCpus":1,"internalWorkerThreads":1},
        "jsonObjectOrder":"sorted","capabilities":["lv_processing_v1"]});
    let identity_path = directory.join("engine-identity.json");
    fs::write(&identity_path, serde_json::to_vec(&identity).unwrap()).unwrap();
    let files = policy::member_names(NETWORK)
        .unwrap()
        .into_iter()
        .map(|name| (name, format!("local:{name}\n").into_bytes()))
        .collect::<Vec<_>>();
    for (name, bytes) in &files {
        fs::write(directory.join(name), bytes).unwrap();
    }
    let wasm = format!("{:x}", Sha256::digest(&files[1].1));
    (identity_path, source, wasm, files)
}

#[test]
fn local_build_stages_activates_and_serves_its_own_bytes_beside_an_independent_release_scope() {
    let base = private_base();
    let input = private_base();
    let (identity, source, wasm, files) = local_inputs(input.path());
    let local = EngineUnitStore::new(base.path(), NETWORK, "canary", UiScope::LocalDev).unwrap();
    let release = EngineUnitStore::new(base.path(), NETWORK, "canary", UiScope::Release).unwrap();
    // A release store never takes a working-tree build.
    assert!(release
        .stage_local(input.path(), &identity, &source, &wasm, &|| false)
        .unwrap_err()
        .starts_with("module_scope_invalid"));
    assert!(EngineUnitStore::new(base.path(), NETWORK, "stable", UiScope::LocalDev)
        .err()
        .unwrap()
        .starts_with("module_scope_invalid"));
    assert!(local
        .stage_local(input.path(), &identity, &source, &"0".repeat(64), &|| false)
        .unwrap_err()
        .starts_with("module_artifact_invalid"));
    assert!(local
        .stage_local(input.path(), &identity, &"6".repeat(40), &wasm, &|| false)
        .unwrap_err()
        .starts_with("module_engine_incompatible"));
    let ready = local
        .stage_local(input.path(), &identity, &source, &wasm, &|| false)
        .unwrap();
    assert_eq!(ready.scope, UiScope::LocalDev);
    assert_eq!(ready.expected_generation, 0);
    assert!(local.selection().unwrap().is_none());
    assert!(local
        .activate(&ready.receipt_id, 1, ActivationGuard::default(), &|| false)
        .unwrap_err()
        .starts_with("module_selection_changed"));
    local
        .activate(&ready.receipt_id, 0, ActivationGuard::default(), &|| false)
        .unwrap();
    assert!(release.selection().unwrap().is_none());
    assert_eq!(
        EngineUnitStore::selection_from(base.path(), NETWORK, "canary", UiScope::LocalDev)
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    let captured = local.capture(&|| false).unwrap();
    assert_eq!(captured.receipt.unit.scope(), UiScope::LocalDev);
    // Changing the working tree after staging never changes the held unit.
    fs::write(input.path().join(files[0].0), b"rebuilt after staging").unwrap();
    for (name, bytes) in &files {
        let (held, fact) = local
            .member(captured.receipt.unit.unit_sha256(), name, 0, MAX_MEMBER_CHUNK, &|| false)
            .unwrap();
        assert_eq!(&held, bytes);
        assert_eq!(fact.size_bytes as usize, bytes.len());
    }
    let (chunk, _) = local
        .member(captured.receipt.unit.unit_sha256(), files[1].0, 3, 4, &|| false)
        .unwrap();
    assert_eq!(chunk, files[1].1[3..7]);
    for (name, limit) in [("ds_reporter_wasm.js", 16), (files[0].0, 0), (files[0].0, MAX_MEMBER_CHUNK + 1)] {
        assert!(local
            .member(captured.receipt.unit.unit_sha256(), name, 0, limit, &|| false)
            .unwrap_err()
            .starts_with("module_artifact_invalid"));
    }
    // Corrupt retained bytes refuse every later capture; nothing is repaired.
    fs::write(
        local
            .unit_directory(captured.receipt.unit.unit_sha256())
            .unwrap()
            .join(files[1].0),
        b"corrupt retained",
    )
    .unwrap();
    assert!(local
        .capture(&|| false)
        .unwrap_err()
        .starts_with("module_artifact_invalid"));
}

#[test]
fn published_unit_is_granted_only_for_the_confirmed_active_catalog_and_follows_its_channel() {
    let base = private_base();
    let store = EngineUnitStore::new(base.path(), NETWORK, "canary", UiScope::Release).unwrap();
    let (release, _) = fixture(1);
    let calls = Cell::new(0);
    let unit = policy::inspect(NETWORK, "canary", &release).unwrap();
    assert!(grant(&release, Some(&"0".repeat(64)), &calls)
        .unwrap_err()
        .starts_with("module_release_changed"));
    grant(&release, Some(unit.unit_sha256()), &calls).unwrap();
    let mut blocked = release.clone();
    blocked["status"] = json!("blocked");
    assert!(grant(&blocked, None, &calls)
        .unwrap_err()
        .starts_with("module_release_inactive"));
    let reporter: Value = serde_json::from_str::<Value>(include_str!(
        "../../../ds-command-kernel/src/module_release/fixtures/reporter-wasm-canary.json"
    ))
    .unwrap()["release"]
        .clone();
    assert!(grant(&reporter, None, &calls)
        .unwrap_err()
        .starts_with("module_release_identity_mismatch"));
    let unpublished = authority().acquire_engine_unit_with(NETWORK, None, &|| false, |_| {
        Ok(br#"{"success":true,"release":null}"#.to_vec())
    });
    assert!(unpublished
        .unwrap_err()
        .starts_with("module_release_unpublished"));
    assert_eq!(calls.get(), 4);

    // A bad download leaves nothing installed.
    let (release, _) = fixture(1);
    assert!(store
        .stage_with(grant(&release, None, &calls).unwrap(), &|| false, |_, path| {
            fs::write(path, b"wrong bytes").map_err(|e| e.to_string())
        })
        .unwrap_err()
        .starts_with("module_artifact_invalid"));
    assert!(store.selection().unwrap().is_none());

    let first = stage_published(&store, 1);
    assert_eq!(first.version, "0.1.0-r1");
    let selected = store
        .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
        .unwrap();
    let captured = store.capture(&|| false).unwrap();
    assert!(matches!(captured.receipt.authority, UiAuthority::Published(_)));
    assert_eq!(captured.generation, selected.generation);
    // Staging the same unit again reuses the verified retained bytes.
    let again = stage_published(&store, 1);
    assert_eq!(again.sha256, first.sha256);
    assert!(store
        .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
        .unwrap_err()
        .starts_with("module_selection_changed"));
    let held = ActivationGuard {
        uncaptured_sessions: 1,
        ..ActivationGuard::default()
    };
    assert!(store
        .activate(&again.receipt_id, 1, held, &|| false)
        .unwrap_err()
        .starts_with("module_activation_deferred"));

    // Convergence: current, then a newer active release, then removal is kept.
    let (current, _) = fixture(1);
    let outcome = store
        .converge(grant(&current, None, &calls).unwrap(), ActivationGuard::default(), |_| {}, &|| false)
        .unwrap();
    assert_eq!(outcome.outcome, EngineConvergenceOutcome::Current);
    let (next, next_files) = fixture(2);
    let next_unit = policy::inspect(NETWORK, "canary", &next).unwrap();
    assert_eq!(
        convergence_plan(store.selection().unwrap().as_ref(), next_unit.unit_sha256()),
        EngineConvergenceOutcome::Updated
    );
    let staged = store
        .stage_with(grant(&next, None, &calls).unwrap(), &|| false, |asset, path| {
            let (_, bytes) = next_files
                .iter()
                .find(|(name, _)| asset.url.ends_with(&format!("/{name}")))
                .unwrap();
            fs::write(path, bytes).map_err(|e| e.to_string())
        })
        .unwrap();
    let updated = store
        .activate(&staged.receipt_id, 1, ActivationGuard::default(), &|| false)
        .unwrap();
    assert_eq!(updated.generation, 2);
    // A capture taken before the update still reads its own bytes.
    let (bytes, _) = store
        .member(captured.receipt.unit.unit_sha256(), "ds_network_wasm.js", 0, 64, &|| false)
        .unwrap();
    assert_eq!(bytes, b"fixture:ds_network_wasm.js:1\n");
    let restored = store.rollback(2, ActivationGuard::default(), &|| false).unwrap();
    assert_eq!(restored.generation, 3);
    assert_eq!(
        store.capture(&|| false).unwrap().receipt.unit.unit_sha256(),
        captured.receipt.unit.unit_sha256()
    );
    let removed = store.remove(3, ActivationGuard::default(), &|| false).unwrap();
    assert_eq!(removed.generation, 4);
    assert!(store.capture(&|| false).unwrap_err().starts_with("module_removed"));
    assert_eq!(
        convergence_plan(Some(&removed), next_unit.unit_sha256()),
        EngineConvergenceOutcome::Removed
    );
    assert_eq!(convergence_plan(None, next_unit.unit_sha256()), EngineConvergenceOutcome::Absent);
    // Absent and removed units read no catalog; an installed one follows its channel.
    assert_eq!(retained_convergence(NETWORK, "canary", None).unwrap().outcome, EngineConvergenceOutcome::Absent);
    let kept = retained_convergence(NETWORK, "canary", Some(&removed)).unwrap();
    assert_eq!((kept.outcome, kept.generation, kept.release_version), (EngineConvergenceOutcome::Removed, 4, None));
    assert!(retained_convergence(NETWORK, "canary", Some(&updated)).is_none());
}

#[test]
fn absent_stores_read_offline_without_creating_state() {
    let base = private_base();
    assert!(
        EngineUnitStore::selection_from(base.path(), NETWORK, "canary", UiScope::Release)
            .unwrap()
            .is_none()
    );
    assert!(
        EngineUnitStore::selection_from(base.path(), NETWORK, "canary", UiScope::LocalDev)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_dir(base.path()).unwrap().count(), 0);
    // Owner 24 Oct: Solar is a page engine unit; its absent store reads the same.
    assert!(
        EngineUnitStore::selection_from(base.path(), ModuleComponent::Solar, "canary", UiScope::Release)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_dir(base.path()).unwrap().count(), 0);
    // The native addon store keys its directories by component; engine units
    // never share them.
    let store = EngineUnitStore::new(base.path(), NETWORK, "canary", UiScope::Release).unwrap();
    assert!(store.root.starts_with(base.path().join("engine-units")));
    assert!(!base.path().join("network").exists());
}
