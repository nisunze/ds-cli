//! UI installation runtime tests, kept beside their owner so ui.rs counts production code.
use super::*;
use serde_json::json;
use std::cell::Cell;

fn local_inputs(
    component: UiComponent,
    directory: &Path,
) -> (PathBuf, String, String, Vec<(&'static str, Vec<u8>)>) {
    let (_, files) = fixture(component, 1);
    let identity: Value = serde_json::from_str(match component {
        UiComponent::Interactions => include_str!("../../../ds-command-kernel/src/module_release/fixtures/interactions-ui-engine-build.json"),
        UiComponent::CommandKernel => include_str!("../../../ds-command-kernel/src/module_release/fixtures/command-kernel-ui-engine-build.json"),
    }).unwrap();
    let identity_path = directory.join("identity.json");
    fs::write(&identity_path, serde_json::to_vec(&identity).unwrap()).unwrap();
    for (name, bytes) in &files {
        fs::write(directory.join(name), bytes).unwrap();
    }
    let wasm = files
        .iter()
        .find(|(name, _)| name.ends_with("_bg.wasm"))
        .unwrap();
    (
        identity_path,
        identity["source_sha"].as_str().unwrap().into(),
        format!("{:x}", Sha256::digest(&wasm.1)),
        files,
    )
}

#[test]
fn local_stage_uses_same_owned_bytes_and_keeps_release_selection_independent() {
    for component in [UiComponent::Interactions, UiComponent::CommandKernel] {
        let base = private_base();
        let input = private_base();
        let (identity, source, wasm_sha, files) = local_inputs(component, input.path());
        let local =
            UiModuleStore::new_scoped(base.path(), component, "canary", UiScope::LocalDev).unwrap();
        let release = UiModuleStore::new(base.path(), component, "canary").unwrap();
        let ready = local
            .stage_local(input.path(), &identity, &source, &wasm_sha, &|| false)
            .unwrap();
        assert!(local.selection().unwrap().is_none());
        assert!(release.selection().unwrap().is_none());
        assert!(
            release
                .stage_local(input.path(), &identity, &source, &wasm_sha, &|| false)
                .is_err()
        );
        assert!(
            local
                .activate(&ready.receipt_id, 1, ActivationGuard::default(), &|| false)
                .is_err()
        );
        local
            .activate(&ready.receipt_id, 0, ActivationGuard::default(), &|| false)
            .unwrap();
        let captured = local.capture(&|| false).unwrap();
        assert_eq!(captured.receipt.unit.scope(), UiScope::LocalDev);
        assert!(release.selection().unwrap().is_none());
        fs::write(
            input.path().join(files[0].0),
            b"source changed after staging",
        )
        .unwrap();
        for (name, bytes) in &files {
            let (held, _) = local
                .member(
                    captured.receipt.unit.unit_sha256(),
                    name,
                    0,
                    MAX_MEMBER_CHUNK,
                    &|| false,
                )
                .unwrap();
            assert_eq!(&held, bytes, "capture retains the already staged bytes");
        }
        let selected = local.selection().unwrap().unwrap();
        // A release store cannot consume a local ready receipt or selection.
        assert!(release.admit_receipt(&captured.receipt).is_err());
        assert!(
            policy::activate(
                Some(&selected),
                policy::prepare(
                    policy::inspect(component.as_str(), "canary", &fixture(component, 2).0)
                        .unwrap(),
                    UiAuthority::Published(PublishedUiAuthority {
                        gateway_origin: "https://gateway.example".into(),
                        route_manifest_sha256: "e".repeat(64)
                    }),
                    &fixture(component, 2)
                        .1
                        .iter()
                        .map(|(name, bytes)| VerifiedFile {
                            filename: (*name).into(),
                            sha256: format!("{:x}", Sha256::digest(bytes)),
                            size_bytes: bytes.len() as i64
                        })
                        .collect::<Vec<_>>(),
                    true,
                )
                .unwrap(),
                selected.generation,
                true,
                ActivationGuard::default()
            )
            .is_err()
        );
        fs::write(
            local
                .unit_directory(captured.receipt.unit.unit_sha256())
                .unwrap()
                .join(files[0].0),
            b"corrupt retained",
        )
        .unwrap();
        assert!(local.capture(&|| false).is_err());
    }
}

#[test]
fn local_stage_rejects_wrong_scope_source_digest_and_linked_inputs_before_selection() {
    let base = private_base();
    let input = private_base();
    let component = UiComponent::CommandKernel;
    let (identity, source, wasm_sha, files) = local_inputs(component, input.path());
    assert!(
        UiModuleStore::new_scoped(base.path(), component, "stable", UiScope::LocalDev).is_err()
    );
    let store =
        UiModuleStore::new_scoped(base.path(), component, "canary", UiScope::LocalDev).unwrap();
    assert!(
        store
            .stage_local(input.path(), &identity, &"f".repeat(40), &wasm_sha, &|| {
                false
            })
            .is_err()
    );
    assert!(
        store
            .stage_local(input.path(), &identity, &source, &"f".repeat(64), &|| false)
            .is_err()
    );
    let file = input.path().join(files[0].0);
    fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&identity, &file).unwrap();
    assert!(
        store
            .stage_local(input.path(), &identity, &source, &wasm_sha, &|| false)
            .is_err()
    );
    assert!(store.selection().unwrap().is_none());
}

fn private_base() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
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
        catalog_sha256: "e".repeat(64),
        gateway_key: "public-fixture-key".into(),
    }
}
fn fixture(component: UiComponent, revision: usize) -> (Value, Vec<(&'static str, Vec<u8>)>) {
    let mut release: Value = serde_json::from_str(match component {
        UiComponent::Interactions => {
            include_str!("../../../ds-command-kernel/src/module_release/fixtures/interactions-ui-wasm-canary.json")
        }
        UiComponent::CommandKernel => include_str!(
            "../../../ds-command-kernel/src/module_release/fixtures/command-kernel-ui-wasm-canary.json"
        ),
    })
    .unwrap();
    let version = format!("0.1.0-r{revision}");
    release["version"] = json!(version);
    let files = component
        .files()
        .into_iter()
        .map(|(name, _)| (name, format!("fixture:{name}:{revision}\n").into_bytes()))
        .collect::<Vec<_>>();
    for asset in release["assets"].as_array_mut().unwrap() {
        let (name, bytes) = files
            .iter()
            .find(|(name, _)| asset["url"].as_str().unwrap().ends_with(name))
            .unwrap();
        asset["url"] = json!(format!("https://storage.googleapis.com/dedicated-public-releases/modules/{}/canary/wasm/{version}/{name}", component.as_str()));
        asset["size_bytes"] = json!(bytes.len());
        asset["sha256"] = json!(format!("{:x}", Sha256::digest(bytes)));
    }
    (release, files)
}
fn grant(
    component: UiComponent,
    release: Value,
    calls: &Cell<usize>,
) -> Result<TrustedUiGrant, String> {
    let unit = policy::inspect(component.as_str(), "canary", &release).unwrap();
    authority().acquire_ui_with(component, Some(unit.unit_sha256()), &|| false, |url| {
        calls.set(calls.get() + 1);
        assert!(url.ends_with(&format!(
            "?component={}&channel=canary&runtime=wasm",
            component.as_str()
        )));
        Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
    })
}
fn stage(
    store: &UiModuleStore,
    revision: usize,
    reads: &Cell<usize>,
    downloads: &Cell<usize>,
) -> StagedUiModule {
    let (release, files) = fixture(store.component, revision);
    let grant = grant(store.component, release, reads).unwrap();
    store
        .stage_with(grant, &|| false, |asset, path| {
            downloads.set(downloads.get() + 1);
            let bytes = &files
                .iter()
                .find(|(name, _)| asset.url.ends_with(name))
                .unwrap()
                .1;
            fs::write(path, bytes).map_err(|e| e.to_string())
        })
        .unwrap()
}
#[test]
fn both_ui_units_stage_four_opaque_members_and_capture_survives_update_removal_rollback() {
    for component in [UiComponent::Interactions, UiComponent::CommandKernel] {
        let base = private_base();
        let store = UiModuleStore::new(base.path(), component, "canary").unwrap();
        let reads = Cell::new(0);
        let downloads = Cell::new(0);
        let first = stage(&store, 1, &reads, &downloads);
        assert!(store.selection().unwrap().is_none());
        let selected = store
            .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(selected.generation, 1);
        let captured = store.capture(&|| false).unwrap();
        let name = component.files()[0].0;
        let digest = captured.receipt.unit.unit_sha256().to_owned();
        let expected = fixture(component, 1).1[0].1.clone();
        let mut body = store.member(&digest, name, 0, 4, &|| false).unwrap().0;
        body.extend(
            store
                .member(&digest, name, 4, MAX_MEMBER_CHUNK, &|| false)
                .unwrap()
                .0,
        );
        assert_eq!(body, expected);
        let second = stage(&store, 2, &reads, &downloads);
        let next = store
            .activate(&second.receipt_id, 1, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(next.generation, 2);
        assert_eq!(
            store
                .member(&digest, name, 0, MAX_MEMBER_CHUNK, &|| false)
                .unwrap()
                .0,
            expected
        );
        let restarted = UiModuleStore::new(base.path(), component, "canary").unwrap();
        assert_eq!(restarted.selection().unwrap().unwrap(), next);
        let restored = restarted
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(restored.generation, 3);
        assert_eq!(
            restarted.capture(&|| false).unwrap().receipt,
            captured.receipt
        );
        restarted
            .remove(3, ActivationGuard::default(), &|| false)
            .unwrap();
        assert!(restarted
            .capture(&|| false)
            .unwrap_err()
            .contains("module_removed"));
        assert_eq!(
            restarted
                .member(&digest, name, 0, MAX_MEMBER_CHUNK, &|| false)
                .unwrap()
                .0,
            expected
        );
        let restored = restarted
            .rollback(4, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(restored.generation, 5);
        assert_eq!(reads.get(), 2);
        assert_eq!(downloads.get(), 8);
        assert_eq!(
            fs::read_dir(store.root.join("versions")).unwrap().count(),
            2
        );
        assert!(store
            .member(&digest, "../selection.json", 0, 10, &|| false)
            .unwrap_err()
            .contains("module_artifact_invalid"));
        assert!(store
            .member(&digest, name, 0, MAX_MEMBER_CHUNK + 1, &|| false)
            .is_err());
        assert!(store
            .member(&digest, name, u64::MAX, 10, &|| false)
            .is_err());
    }
}
#[test]
fn authority_admits_complete_source_compiler_config_and_members_before_any_transfer() {
    let (release, _) = fixture(UiComponent::Interactions, 1);
    let unit = policy::inspect("interactions", "canary", &release).unwrap();
    let reads = Cell::new(0);
    for field in [
        "source_sha",
        "cargo_lock_sha256",
        "rustc",
        "target",
        "profile",
        "config_sha256",
    ] {
        let mut bad = release.clone();
        bad["build_manifest"][field] = json!("tampered");
        assert!(authority()
            .acquire_ui_with(
                UiComponent::Interactions,
                Some(unit.unit_sha256()),
                &|| false,
                |_| {
                    reads.set(reads.get() + 1);
                    Ok(serde_json::to_vec(&json!({"success":true,"release":bad})).unwrap())
                }
            )
            .is_err());
    }
    let mut incomplete = release.clone();
    incomplete["assets"].as_array_mut().unwrap().pop();
    assert!(authority()
        .acquire_ui_with(
            UiComponent::Interactions,
            Some(unit.unit_sha256()),
            &|| false,
            |_| Ok(serde_json::to_vec(&json!({"success":true,"release":incomplete})).unwrap())
        )
        .is_err());
    assert!(authority()
        .acquire_ui_with(
            UiComponent::Interactions,
            Some(unit.unit_sha256()),
            &|| true,
            |_| panic!("cancelled acquisition must not read")
        )
        .is_err());
    let mut withdrawn = release;
    withdrawn["status"] = json!("blocked");
    assert!(authority()
        .acquire_ui_with(
            UiComponent::Interactions,
            Some(unit.unit_sha256()),
            &|| false,
            |_| Ok(serde_json::to_vec(&json!({"success":true,"release":withdrawn})).unwrap())
        )
        .unwrap_err()
        .contains("module_release_inactive"));
    assert_eq!(reads.get(), 6);
}
#[test]
fn a_new_core_profile_reuses_verified_unit_bytes_and_keeps_each_acquisition_authority() {
    let base = private_base();
    let store = UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
    let first = stage(&store, 1, &Cell::new(0), &Cell::new(0));
    store
        .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
        .unwrap();
    store
        .remove(1, ActivationGuard::default(), &|| false)
        .unwrap();
    let (release, _) = fixture(UiComponent::Interactions, 1);
    let unit = policy::inspect("interactions", "canary", &release).unwrap();
    let mut next_authority = authority();
    next_authority.catalog_sha256 = "f".repeat(64);
    let grant = next_authority
        .acquire_ui_with(
            UiComponent::Interactions,
            Some(unit.unit_sha256()),
            &|| false,
            |_| Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap()),
        )
        .unwrap();
    let ready = store
        .stage_with(grant, &|| false, |_, _| {
            panic!("unchanged verified bytes need no transfer")
        })
        .unwrap();
    let historical: InstalledUiModule = serde_json::from_slice(
        &fs::read(
            store
                .unit_directory(unit.unit_sha256())
                .unwrap()
                .join("installation.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(historical.authority.route_manifest_sha256, "e".repeat(64));
    let selected = store
        .activate(&ready.receipt_id, 2, ActivationGuard::default(), &|| false)
        .unwrap();
    assert_eq!(selected.generation, 3);
    assert_eq!(
        store
            .capture(&|| false)
            .unwrap()
            .receipt
            .authority
            .route_manifest_sha256,
        "f".repeat(64)
    );
    assert!(store
        .member(
            unit.unit_sha256(),
            "ds_interactions_wasm.js",
            0,
            100,
            &|| false
        )
        .is_ok());
}
#[test]
fn cancellation_corruption_stale_generation_and_lifecycle_guard_preserve_selection() {
    let base = private_base();
    let store = UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
    let reads = Cell::new(0);
    let downloads = Cell::new(0);
    let first = stage(&store, 1, &reads, &downloads);
    store
        .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
        .unwrap();
    let original = fs::read(store.root.join("selection.json")).unwrap();
    let second = stage(&store, 2, &reads, &downloads);
    let third = stage(&store, 3, &reads, &downloads);
    let dirty = ActivationGuard {
        unsaved_edits: true,
        ..ActivationGuard::default()
    };
    assert!(store
        .activate(&second.receipt_id, 1, dirty, &|| false)
        .unwrap_err()
        .contains("module_activation_deferred"));
    assert!(store
        .activate(&second.receipt_id, 1, ActivationGuard::default(), &|| true)
        .unwrap_err()
        .contains("module_update_cancelled"));
    assert_eq!(
        fs::read(store.root.join("selection.json")).unwrap(),
        original
    );
    store
        .activate(&second.receipt_id, 1, ActivationGuard::default(), &|| false)
        .unwrap();
    assert!(store
        .activate(&third.receipt_id, 1, ActivationGuard::default(), &|| false)
        .unwrap_err()
        .contains("module_selection_changed"));
    let held = store.capture(&|| false).unwrap();
    fs::write(
        store
            .unit_directory(held.receipt.unit.unit_sha256())
            .unwrap()
            .join("ds_interactions_wasm_bg.wasm"),
        b"damaged",
    )
    .unwrap();
    assert!(store
        .capture(&|| false)
        .unwrap_err()
        .contains("module_artifact_invalid"));
    let repaired = store
        .rollback(2, ActivationGuard::default(), &|| false)
        .unwrap();
    assert!(matches!(
        repaired.selection,
        UiSelectionState::Installed { previous: None, .. }
    ));
    assert!(store.capture(&|| false).is_ok());
    let (release, _) = fixture(UiComponent::Interactions, 4);
    let candidate = grant(UiComponent::Interactions, release, &reads).unwrap();
    assert!(store
        .stage_with(candidate, &|| false, |_, path| {
            fs::write(path, b"partial").unwrap();
            Err("module_stage_failed: interrupted".into())
        })
        .is_err());
    assert_eq!(store.selection().unwrap().unwrap(), repaired);
    assert_eq!(
        fs::read_dir(store.root.join("versions")).unwrap().count(),
        3
    );
}
#[cfg(unix)]
#[test]
fn linked_or_nonprivate_store_and_receipts_refuse() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let base = private_base();
    let store = UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
    let ready = stage(&store, 1, &Cell::new(0), &Cell::new(0));
    let path = store.receipt_path(&ready.receipt_id).unwrap();
    let real = path.with_extension("original");
    fs::rename(&path, &real).unwrap();
    symlink(&real, &path).unwrap();
    assert!(store
        .activate(&ready.receipt_id, 0, ActivationGuard::default(), &|| false)
        .is_err());
    assert!(store.selection().unwrap().is_none());
    fs::set_permissions(&store.root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store
        .selection()
        .unwrap_err()
        .contains("module_store_invalid"));
}
fn active_grant(component: UiComponent, release: Value) -> Result<TrustedUiGrant, String> {
    authority().acquire_ui_with(component, None, &|| false, |url| {
        assert!(url.ends_with("&runtime=wasm"));
        Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
    })
}
fn converge_to(
    store: &UiModuleStore,
    revision: usize,
    downloads: &Cell<usize>,
) -> Result<UiConvergence, String> {
    let (release, files) = fixture(store.component, revision);
    let grant = active_grant(store.component, release)?;
    store.converge_with(grant, ActivationGuard::default(), &|| false, |grant| {
        store.stage_with(grant, &|| false, |asset, path| {
            downloads.set(downloads.get() + 1);
            let bytes = &files
                .iter()
                .find(|(name, _)| asset.url.ends_with(name))
                .unwrap()
                .1;
            fs::write(path, bytes).map_err(|e| e.to_string())
        })
    })
}
#[test]
fn an_installed_unit_follows_the_active_release_and_rollback_is_just_another_release() {
    for component in [UiComponent::Interactions, UiComponent::CommandKernel] {
        let base = private_base();
        let store = UiModuleStore::new(base.path(), component, "canary").unwrap();
        let first = stage(&store, 1, &Cell::new(0), &Cell::new(0));
        store
            .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
            .unwrap();
        let loaded = store.capture(&|| false).unwrap();
        let downloads = Cell::new(0);
        let updated = converge_to(&store, 2, &downloads).unwrap();
        assert_eq!(updated.outcome, UiConvergenceOutcome::Updated);
        assert_eq!(updated.generation, 2);
        assert_eq!(updated.release_version.as_deref(), Some("0.1.0-r2"));
        assert_eq!(downloads.get(), 4);
        let captured = store.capture(&|| false).unwrap();
        assert_eq!(
            Some(captured.receipt.unit.unit_sha256()),
            updated.unit_sha256.as_deref()
        );
        // A loaded Desktop keeps reading the unit it captured.
        let name = component.files()[0].0;
        assert!(store
            .member(loaded.receipt.unit.unit_sha256(), name, 0, 8, &|| false)
            .is_ok());
        let current = converge_to(&store, 2, &downloads).unwrap();
        assert_eq!(current.outcome, UiConvergenceOutcome::Current);
        assert_eq!(current.generation, 2);
        assert_eq!(downloads.get(), 4);
        // Publishing the earlier unit again is a release; retained bytes are reused.
        let restored = converge_to(&store, 1, &downloads).unwrap();
        assert_eq!(restored.outcome, UiConvergenceOutcome::Updated);
        assert_eq!(restored.generation, 3);
        assert_eq!(downloads.get(), 4);
        assert_eq!(
            store.capture(&|| false).unwrap().receipt.unit,
            loaded.receipt.unit
        );
    }
}
#[test]
fn absence_and_removal_are_retained_and_need_no_catalog_read() {
    let base = private_base();
    let store = UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
    let absent = retained_convergence(UiComponent::Interactions, "canary", None).unwrap();
    assert_eq!(absent.outcome, UiConvergenceOutcome::Absent);
    assert_eq!((absent.generation, absent.release_version), (0, None));
    let (release, _) = fixture(UiComponent::Interactions, 1);
    let grant = active_grant(UiComponent::Interactions, release.clone()).unwrap();
    let untouched = store
        .converge_with(grant, ActivationGuard::default(), &|| false, |_| {
            panic!("an absent unit is never installed by the updater")
        })
        .unwrap();
    assert_eq!(untouched.outcome, UiConvergenceOutcome::Absent);
    assert!(store.selection().unwrap().is_none());
    let first = stage(&store, 1, &Cell::new(0), &Cell::new(0));
    store
        .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
        .unwrap();
    assert!(retained_convergence(
        UiComponent::Interactions,
        "canary",
        store.selection().unwrap().as_ref()
    )
    .is_none());
    store
        .remove(1, ActivationGuard::default(), &|| false)
        .unwrap();
    let selection = store.selection().unwrap();
    let removed =
        retained_convergence(UiComponent::Interactions, "canary", selection.as_ref()).unwrap();
    assert_eq!(
        (removed.outcome, removed.generation),
        (UiConvergenceOutcome::Removed, 2)
    );
    let (newer, _) = fixture(UiComponent::Interactions, 2);
    let grant = active_grant(UiComponent::Interactions, newer).unwrap();
    let kept = store
        .converge_with(grant, ActivationGuard::default(), &|| false, |_| {
            panic!("a removed unit is never reinstalled by the updater")
        })
        .unwrap();
    assert_eq!(
        (kept.outcome, kept.generation),
        (UiConvergenceOutcome::Removed, 2)
    );
    assert_eq!(store.selection().unwrap(), selection);
}
#[test]
fn a_change_after_the_plan_refuses_instead_of_reinstalling() {
    let base = private_base();
    let store = UiModuleStore::new(base.path(), UiComponent::CommandKernel, "canary").unwrap();
    let first = stage(&store, 1, &Cell::new(0), &Cell::new(0));
    store
        .activate(&first.receipt_id, 0, ActivationGuard::default(), &|| false)
        .unwrap();
    let (release, files) = fixture(UiComponent::CommandKernel, 2);
    let grant = active_grant(UiComponent::CommandKernel, release).unwrap();
    let refused = store
        .converge_with(grant, ActivationGuard::default(), &|| false, |grant| {
            let staged = store.stage_with(grant, &|| false, |asset, path| {
                let bytes = &files
                    .iter()
                    .find(|(name, _)| asset.url.ends_with(name))
                    .unwrap()
                    .1;
                fs::write(path, bytes).map_err(|e| e.to_string())
            });
            // Someone removes the unit while the update is in flight.
            store
                .remove(1, ActivationGuard::default(), &|| false)
                .unwrap();
            staged
        })
        .unwrap_err();
    assert!(refused.contains("module_selection_changed"), "{refused}");
    assert!(matches!(
        store.selection().unwrap().unwrap().selection,
        UiSelectionState::Removed { .. }
    ));
}
#[test]
fn active_acquisition_admits_the_whole_release_and_refuses_a_withdrawn_one() {
    let (release, _) = fixture(UiComponent::Interactions, 3);
    let unit = policy::inspect("interactions", "canary", &release).unwrap();
    let grant = active_grant(UiComponent::Interactions, release.clone()).unwrap();
    assert_eq!(grant.unit.unit_sha256(), unit.unit_sha256());
    let mut withdrawn = release.clone();
    withdrawn["status"] = json!("blocked");
    assert!(active_grant(UiComponent::Interactions, withdrawn)
        .unwrap_err()
        .contains("module_release_inactive"));
    let mut tampered = release;
    tampered["build_manifest"]["rustc"] = json!("tampered");
    assert!(active_grant(UiComponent::Interactions, tampered).is_err());
    assert!(authority()
        .acquire_ui_with(UiComponent::Interactions, None, &|| false, |_| {
            Ok(serde_json::to_vec(&json!({"success":true,"release":null})).unwrap())
        })
        .unwrap_err()
        .contains("module_release_unpublished"));
    assert!(authority()
        .acquire_ui_with(UiComponent::Interactions, Some("short"), &|| false, |_| {
            panic!("an invalid confirmed digest refuses before any read")
        })
        .unwrap_err()
        .contains("module_release_invalid"));
}
