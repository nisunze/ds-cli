//! Native module updater tests, beside their owner so modules.rs counts production code.
use super::*;
use serde_json::json;
use std::cell::Cell;
fn identity(component: ModuleComponent, source: &str, manifest: &str) -> Value {
    match component {
        ModuleComponent::Network => {
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0","sourceSha":source,"dependencyClosureSha256":manifest,"environment":"native","runtime":{"environment":"native","logicalCpus":8,"internalWorkerThreads":1},"jsonObjectOrder":"insertion","capabilities":["lv_processing_v1","lv_publication_v1","lv_analysis_validation_v1","grid_query_v1","grid_inspect_v1","grid_apply_v1","grid_publication_v1","profile_session_v1","report_engineering_v1","report_sizing_v1","report_feeder_selection_v1"]},"build":{"target":"x86_64-unknown-linux-gnu","profile":"release","rustc":"synthetic owner process fixture","cargoLockSha256":"a".repeat(64)}})
        }
        ModuleComponent::Solar => {
            json!({"schema":"ds.engine-build/v1","name":"ds-solar-engine","version":"0.1.0","source_sha":source,"cargo_lock_sha256":"a".repeat(64),"rustc":"synthetic owner process fixture","target":"x86_64-unknown-linux-gnu","profile":"release","schemas":["ds-solar.prepared-calculation-result/v1","ds-solar.result/v1","ds-solar.prepared-city-input/v1","ds-solar.worker-budget/v1"],"build_manifest_sha256":manifest})
        }
        ModuleComponent::Reporter => {
            json!({"schema":"ds.engine-build/v1","service":"ds-network-reporter","binary":"ds-report","package_version":"0.1.0","engine_version":format!("ds-network-reporter@0.1.0+{source}"),"source_sha":source,"cargo_lock_sha256":"a".repeat(64),"features":[],"rustc":"synthetic owner process fixture","target":"x86_64-unknown-linux-gnu","profile":"release","schemas":["ds.engine-build/v1","ds-network-reporter.task-result/v1"],"build_manifest_sha256":manifest})
        }
    }
}
fn fixture(component: ModuleComponent, n: u64) -> (ModuleRelease, Vec<u8>) {
    let source = format!("{n:040x}");
    let manifest = format!("{n:064x}");
    let info = serde_json::to_string(&identity(component, &source, &manifest)).unwrap();
    assert!(!info.contains('\''));
    let bytes=format!("#!/bin/sh\ncase \"$1\" in\n build-info|work) printf '%s\\n' '{info}';;\n *) exit 2;;\nesac\n").into_bytes();
    let release=validate_module_release(component.as_str(),"canary",&json!({"schema":"ds.module-release/v1","component":component,"channel":"canary","product_id":format!("rw.datasolutions.module.{}.canary",component.as_str()),"version":format!("0.1.0-r{n}"),"protocol":component.protocol(),"released_at":"2026-10-14T10:00:00Z","status":"active","assets":[{"platform":"linux","arch":"x86_64","kind":"module-executable","protocol":component.protocol(),"url":format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/{}/canary/0.1.0-r{n}/{}",component.as_str(),component.executable()),"size_bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"source_sha":source,"built_at":"2026-10-14T09:00:00Z","source_revisions":{component.engine_owner():source},"engines":[{"engine":component.engine_owner(),"release":format!("{}@0.1.0+{}",component.engine_owner(),source),"manifest_sha256":manifest}]}]})).unwrap();
    (release, bytes)
}
fn authority() -> CapturedReleaseAuthority {
    CapturedReleaseAuthority {
        gateway_origin: "https://synthetic-module-authority.ue.gateway.dev".into(),
        channel: "canary",
        catalog_sha256: "a".repeat(64),
        gateway_key: "synthetic public test key".into(),
    }
}
fn grant(release: ModuleRelease) -> TrustedModuleGrant {
    let component = release.component;
    let digest = release.assets[0].sha256.clone();
    authority()
        .acquire_with(component, &digest, &|| false, |_| {
            Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
        })
        .unwrap()
}
fn stage(store: &ModuleStore, release: ModuleRelease, bytes: &[u8]) -> StagedModule {
    store
        .stage_with(grant(release), &|| false, |_, path| write_new(path, bytes))
        .unwrap()
}
fn owner(component: ModuleComponent) -> &'static ds_cli_exec::External {
    match component {
        ModuleComponent::Solar => &ds_cli_exec::addons::SOLAR,
        ModuleComponent::Reporter => &ds_cli_exec::addons::REPORTER,
        ModuleComponent::Network => &ds_cli_exec::addons::NETWORK,
    }
}
/// Capture exactly what the relocated store selected. These tests are about the
/// managed store, so a developer override such as `DS_SOLAR_ENGINE_BIN` in the
/// shell must not answer for it; it did, and two of them failed there. Errors
/// are their refusal codes.
fn managed(
    owner: &'static ds_cli_exec::External,
    base: &Path,
) -> Result<ds_cli_exec::CapturedExternal, String> {
    owner
        .capture_managed_for_lane_in("canary", base)
        .map(|captured| captured.expect("the store selected an executable"))
        .map_err(|failure| failure.code().to_owned())
}
#[test]
fn host_connectivity_and_pre_cancel_stop_before_transfer_or_probe() {
    for component in [
        ModuleComponent::Solar,
        ModuleComponent::Reporter,
        ModuleComponent::Network,
    ] {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), component, "canary").unwrap();
        let (release, _) = fixture(component, 1);
        let admissions = Cell::new(0);
        let error = store
            .stage_with_admission(
                grant(release.clone()),
                |_| {},
                &|| false,
                || {
                    admissions.set(admissions.get() + 1);
                    Err("offline: operator selected offline mode".into())
                },
            )
            .unwrap_err();
        assert!(error.contains("offline:"));
        assert_eq!(admissions.get(), 1);
        assert!(store.selection().unwrap().is_none());
        assert_eq!(
            fs::read_dir(store.root.join("versions")).unwrap().count(),
            0
        );
        let error = store
            .stage_with_admission(
                grant(release),
                |_| {},
                &|| true,
                || panic!("pre-cancelled work must not consume connectivity or transport"),
            )
            .unwrap_err();
        assert!(error.contains("module_update_cancelled"));
        assert!(store.selection().unwrap().is_none());
    }
}

#[test]
fn retained_selection_read_is_read_only_and_checks_existing_ancestors() {
    use std::os::unix::{fs::symlink, fs::PermissionsExt};
    let scratch = tempfile::tempdir().unwrap();
    let base = scratch.path().join("absent");
    assert!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
            .unwrap()
            .is_none()
    );
    assert!(!base.exists());
    fs::create_dir(&base).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fs::metadata(&base).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert_eq!(fs::read_dir(&base).unwrap().count(), 0);
    fs::write(base.join("network"), b"wrong type").unwrap();
    assert!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
            .unwrap_err()
            .contains("module_store_invalid")
    );
    fs::remove_file(base.join("network")).unwrap();
    symlink(scratch.path().join("missing"), base.join("network")).unwrap();
    assert!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
            .unwrap_err()
            .contains("module_store_invalid")
    );
    fs::remove_file(base.join("network")).unwrap();
    let store = ModuleStore::new(&base, ModuleComponent::Network, "canary").unwrap();
    let (release, bytes) = fixture(ModuleComponent::Network, 1);
    let staged = stage(&store, release, &bytes);
    let selected = store
        .activate(&staged, ActivationGuard::default(), &|| false)
        .unwrap();
    let path = store.root.join("selection.json");
    let held = fs::read(&path).unwrap();
    let lock_modified = fs::metadata(store.root.join("update.lock"))
        .unwrap()
        .modified()
        .unwrap();
    let root_modified = fs::metadata(&store.root).unwrap().modified().unwrap();
    assert_eq!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "canary").unwrap(),
        Some(selected.clone())
    );
    assert_eq!(fs::read(&path).unwrap(), held);
    assert_eq!(
        fs::metadata(store.root.join("update.lock"))
            .unwrap()
            .modified()
            .unwrap(),
        lock_modified
    );
    assert_eq!(
        fs::metadata(&store.root).unwrap().modified().unwrap(),
        root_modified
    );
    assert!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "stable")
            .unwrap()
            .is_none()
    );
    assert!(!base.join("network/stable").exists());
    let mut crossed = selected;
    crossed.channel = "stable".into();
    fs::write(&path, serde_json::to_vec(&crossed).unwrap()).unwrap();
    assert!(ModuleStore::selection_from(&base, ModuleComponent::Network, "canary").is_err());
    fs::write(&path, b"{broken").unwrap();
    assert!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
            .unwrap_err()
            .contains("module_receipt_invalid")
    );
    fs::remove_file(&path).unwrap();
    symlink(scratch.path().join("missing-receipt"), &path).unwrap();
    assert!(
        ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
            .unwrap_err()
            .contains("module_store_invalid")
    );
}

#[test]
fn stage_fixture_reads_one_authority_downloads_once_and_probes_only_selected_owner() {
    for component in [
        ModuleComponent::Solar,
        ModuleComponent::Reporter,
        ModuleComponent::Network,
    ] {
        let scratch = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(scratch.path(), component, "canary").unwrap();
        let probe_log = scratch.path().join("probe-count");
        let log_path = probe_log.to_str().unwrap();
        assert!(!log_path.contains('\''));
        let (mut release, original) = fixture(component, 1);
        let bytes = String::from_utf8(original)
            .unwrap()
            .replacen(
                "#!/bin/sh\n",
                &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log_path}'\n"),
                1,
            )
            .into_bytes();
        release.assets[0].size_bytes = bytes.len() as _;
        release.assets[0].sha256 = format!("{:x}", Sha256::digest(&bytes));
        let authority_reads = Cell::new(0);
        let downloads = Cell::new(0);
        let grant = authority().acquire_with(component, &release.assets[0].sha256, &|| false, |url| {
            authority_reads.set(authority_reads.get() + 1);
            assert_eq!(url, format!("https://synthetic-module-authority.ue.gateway.dev/api/v1/app_releases?component={}&channel=canary", component.as_str()));
            Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
        }).unwrap();
        let ready = store
            .stage_with(grant, &|| false, |_, path| {
                downloads.set(downloads.get() + 1);
                write_new(path, &bytes)
            })
            .unwrap();
        assert_eq!(authority_reads.get(), 1);
        assert_eq!(downloads.get(), 1);
        assert_eq!(fs::read_to_string(&probe_log).unwrap().trim(), "build-info");
        assert_eq!(ready.component, component);
        assert_eq!(ready.state, "ready");
        assert!(store.selection().unwrap().is_none());
        assert!(!store.root.join("selection.json").exists());
        for unused in [
            ModuleComponent::Solar,
            ModuleComponent::Reporter,
            ModuleComponent::Network,
        ] {
            if unused != component {
                assert!(!scratch.path().join(unused.as_str()).exists());
            }
        }
    }
}

#[test]
fn activation_requires_callers_generation_as_well_as_staging_generation() {
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    let (release, bytes) = fixture(ModuleComponent::Network, 1);
    let ready = stage(&store, release, &bytes);
    let error = store
        .activate_at_generation(&ready, 1, ActivationGuard::default(), &|| false)
        .unwrap_err();
    assert!(error.starts_with("module_selection_changed:"));
    assert!(store.selection().unwrap().is_none());
    assert_eq!(
        store
            .activate_at_generation(&ready, 0, ActivationGuard::default(), &|| false)
            .unwrap()
            .generation,
        1
    );
    let before = std::fs::read(store.root.join("selection.json")).unwrap();
    assert!(store
        .activate_at_generation(&ready, 0, ActivationGuard::default(), &|| false)
        .unwrap_err()
        .starts_with("module_selection_changed:"));
    assert_eq!(
        std::fs::read(store.root.join("selection.json")).unwrap(),
        before
    );
}

#[test]
fn native_three_component_stage_activate_restart_rollback_keeps_captured_old_job() {
    for component in [
        ModuleComponent::Solar,
        ModuleComponent::Reporter,
        ModuleComponent::Network,
    ] {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), component, "canary").unwrap();
        assert!(store.selected_executable(&|| false).unwrap().is_none());
        let (one, bytes) = fixture(component, 1);
        let first = stage(&store, one, &bytes);
        let mut incomplete = first.installed.clone();
        let compiler = if component == ModuleComponent::Network {
            &mut incomplete.engine_identity["build"]["rustc"]
        } else {
            &mut incomplete.engine_identity["rustc"]
        };
        *compiler = json!("");
        assert!(module_install::validate_engine_identity(&incomplete).is_err());
        assert!(store.selection().unwrap().is_none());
        let first_selection = store
            .activate(&first, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(first_selection.generation, 1);
        let (old_path, _) = store.selected_executable(&|| false).unwrap().unwrap();
        let captured = managed(owner(component), base.path()).unwrap();
        let (two, bytes) = fixture(component, 2);
        let second = stage(&store, two, &bytes);
        let recovered = ModuleStore::new(base.path(), component, "canary")
            .unwrap()
            .recover_stage(second.receipt_id())
            .unwrap();
        let second_selection = store
            .activate(&recovered, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(second_selection.generation, 2);
        assert!(old_path.is_file());
        let (new_path, _) = store.selected_executable(&|| false).unwrap().unwrap();
        assert_ne!(old_path, new_path);
        let old_result = captured
            .call_json("work", &[], Duration::from_secs(2))
            .unwrap();
        let new_result = managed(owner(component), base.path())
            .unwrap()
            .call_json("work", &[], Duration::from_secs(2))
            .unwrap();
        assert_ne!(old_result, new_result);
        assert_eq!(old_result, first_selection.active.engine_identity);
        assert_eq!(new_result, second_selection.active.engine_identity);
        let rolled = store
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(rolled.generation, 3);
        assert_eq!(
            store.selected_executable(&|| false).unwrap().unwrap().0,
            old_path
        );
        assert!(new_path.is_file());
        assert!(store
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap_err()
            .contains("module_selection_changed"));
    }
}
#[test]
fn removal_restart_stale_stage_reinstall_and_captured_jobs_preserve_one_authority() {
    for component in [
        ModuleComponent::Solar,
        ModuleComponent::Reporter,
        ModuleComponent::Network,
    ] {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), component, "canary").unwrap();
        let (release, bytes) = fixture(component, 1);
        let first = stage(&store, release.clone(), &bytes);
        let installed = store
            .activate(&first, ActivationGuard::default(), &|| false)
            .unwrap();
        let v1 = fs::read(store.root.join("selection.json")).unwrap();
        assert_eq!(
            serde_json::from_slice::<ModuleSelection>(&v1).unwrap(),
            installed
        );
        assert!(matches!(
            serde_json::from_slice::<ModuleSelectionRecord>(&v1).unwrap(),
            ModuleSelectionRecord::Installed(_)
        ));
        let captured = managed(owner(component), base.path()).unwrap();
        let (candidate, candidate_bytes) = fixture(component, 2);
        let stale = stage(&store, candidate, &candidate_bytes);
        let removed = store
            .remove(1, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(removed.generation, 2);
        let removed_bytes = fs::read(store.root.join("selection.json")).unwrap();
        assert!(serde_json::from_slice::<ModuleSelection>(&removed_bytes).is_err());
        let restarted = ModuleStore::new(base.path(), component, "canary").unwrap();
        assert_eq!(
            restarted.selection_record().unwrap(),
            Some(ModuleSelectionRecord::Removed(removed))
        );
        assert!(restarted
            .selection()
            .unwrap_err()
            .starts_with("module_removed:"));
        assert!(restarted
            .selected_executable(&|| false)
            .unwrap_err()
            .starts_with("module_removed:"));
        assert_eq!(
            managed(owner(component), base.path())
                .err()
                .unwrap()
                .as_str(),
            "module_removed"
        );
        let rollback_error = restarted
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap_err();
        assert!(
            rollback_error.starts_with("module_removed:"),
            "{rollback_error}"
        );
        let stale = restarted.recover_stage(stale.receipt_id()).unwrap();
        assert!(restarted
            .activate(&stale, ActivationGuard::default(), &|| false)
            .unwrap_err()
            .starts_with("module_selection_changed:"));
        assert_eq!(
            fs::read(store.root.join("selection.json")).unwrap(),
            removed_bytes
        );
        assert_eq!(
            captured
                .call_json("work", &[], Duration::from_secs(2))
                .unwrap(),
            installed.active.engine_identity
        );
        let fresh = stage(&restarted, release, &bytes);
        assert_eq!(fresh.expected_generation, 2);
        let restored = restarted
            .activate(&fresh, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(restored.generation, 3);
        assert!(restored.previous.is_none());
        assert!(managed(owner(component), base.path()).is_ok());
    }
}
#[test]
fn removal_admission_fences_busy_dirty_cancelled_changed_and_exhausted_generations() {
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    let (release, bytes) = fixture(ModuleComponent::Network, 1);
    let staged = stage(&store, release, &bytes);
    store
        .activate(&staged, ActivationGuard::default(), &|| false)
        .unwrap();
    let held = fs::read(store.root.join("selection.json")).unwrap();
    for guard in [
        ActivationGuard {
            unsaved_edits: true,
            ..Default::default()
        },
        ActivationGuard {
            uncaptured_sessions: 1,
            ..Default::default()
        },
        ActivationGuard {
            host_restart_required: true,
            ..Default::default()
        },
    ] {
        assert!(store
            .remove(1, guard, &|| false)
            .unwrap_err()
            .starts_with("module_activation_deferred:"));
    }
    assert!(store
        .remove(1, ActivationGuard::default(), &|| true)
        .unwrap_err()
        .contains("cancel"));
    assert!(store
        .remove(0, ActivationGuard::default(), &|| false)
        .unwrap_err()
        .starts_with("module_selection_changed:"));
    let lock = store.lock().unwrap();
    assert!(store
        .remove(1, ActivationGuard::default(), &|| false)
        .unwrap_err()
        .starts_with("module_update_busy:"));
    drop(lock);
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    let checks = Cell::new(0);
    assert!(store
        .remove(1, ActivationGuard::default(), &|| {
            checks.set(checks.get() + 1);
            checks.get() == 3
        })
        .is_err());
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    let mut installed = store.selection().unwrap().unwrap();
    installed.generation = u64::MAX;
    store
        .write_selection(&ModuleSelectionRecord::Installed(installed), &|| false)
        .unwrap();
    assert!(store
        .remove(u64::MAX, ActivationGuard::default(), &|| false)
        .unwrap_err()
        .contains("generation exhausted"));
}
#[test]
fn removal_and_removed_automatic_admission_need_no_executable_authority_or_transfer() {
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    assert!(store.admit_automatic_acquisition().is_ok());
    let (release, bytes) = fixture(ModuleComponent::Network, 1);
    let pre_removal_grant = grant(release.clone());
    let staged = stage(&store, release, &bytes);
    store
        .activate(&staged, ActivationGuard::default(), &|| false)
        .unwrap();
    let (path, _) = store.selected_executable(&|| false).unwrap().unwrap();
    // A broken executable must not be probed or reopened by removal/admission.
    fs::write(&path, b"damaged executable").unwrap();
    store
        .remove(1, ActivationGuard::default(), &|| false)
        .unwrap();
    let reads = Cell::new(0);
    let automatic = || -> Result<(), String> {
        store.admit_automatic_acquisition()?;
        reads.set(reads.get() + 1);
        let _ = authority();
        Ok(())
    };
    assert!(automatic().unwrap_err().starts_with("module_removed:"));
    assert_eq!(reads.get(), 0);
    assert!(store
        .stage_automatic_with_admission(
            pre_removal_grant,
            |_| panic!("unexpected transfer"),
            &|| false,
            || panic!("unexpected transport")
        )
        .unwrap_err()
        .starts_with("module_removed:"));
    let stable = ModuleStore::new(base.path(), ModuleComponent::Network, "stable").unwrap();
    assert!(stable.admit_automatic_acquisition().is_ok());
    assert!(owner(ModuleComponent::Network)
        .capture_managed_for_lane_in("stable", base.path())
        .unwrap()
        .is_none());
    let solar = ModuleStore::new(base.path(), ModuleComponent::Solar, "canary").unwrap();
    assert!(solar.admit_automatic_acquisition().is_ok());
    assert!(store.admit_automatic_acquisition().is_err());
}
#[test]
fn removed_record_read_and_removal_refuse_corrupt_linked_and_crossed_authority() {
    use std::os::unix::fs::symlink;
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    store
        .remove(0, ActivationGuard::default(), &|| false)
        .unwrap();
    let path = store.root.join("selection.json");
    let held = fs::read(&path).unwrap();
    assert!(matches!(
        ModuleStore::selection_record_from(base.path(), ModuleComponent::Network, "canary")
            .unwrap(),
        Some(ModuleSelectionRecord::Removed(_))
    ));
    for value in [
        json!({"schema":REMOVED_SELECTION_SCHEMA,"component":"network","channel":"stable","generation":1,"state":"removed"}),
        json!({"schema":REMOVED_SELECTION_SCHEMA,"component":"network","channel":"canary","generation":1,"state":"removed","active":null}),
    ] {
        let corrupted = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &corrupted).unwrap();
        assert!(store
            .remove(1, ActivationGuard::default(), &|| false)
            .is_err());
        assert!(store.admit_automatic_acquisition().is_err());
        assert_eq!(fs::read(&path).unwrap(), corrupted);
    }
    let linked = base.path().join("removed-record.json");
    fs::write(&linked, &held).unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&linked, &path).unwrap();
    assert!(store
        .remove(1, ActivationGuard::default(), &|| false)
        .is_err());
    assert!(store.admit_automatic_acquisition().is_err());
    assert_eq!(fs::read(&linked).unwrap(), held);
    fs::remove_file(&path).unwrap();
    fs::write(&path, &held).unwrap();
    let replaced = store.root.with_file_name("retained-canary");
    fs::rename(&store.root, &replaced).unwrap();
    symlink(&replaced, &store.root).unwrap();
    assert!(store
        .remove(1, ActivationGuard::default(), &|| false)
        .is_err());
    assert!(store.selection_record().is_err());
    assert_eq!(fs::read(replaced.join("selection.json")).unwrap(), held);
}
#[test]
fn corrupt_incompatible_interrupted_cancelled_or_changed_candidate_preserves_current() {
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    let (one, bytes) = fixture(ModuleComponent::Network, 1);
    let first = stage(&store, one, &bytes);
    store
        .activate(&first, ActivationGuard::default(), &|| false)
        .unwrap();
    let held = fs::read(store.root.join("selection.json")).unwrap();
    let (two, bytes) = fixture(ModuleComponent::Network, 2);
    assert!(store
        .stage_with(grant(two.clone()), &|| false, |_, path| {
            write_new(path, b"corrupt")?;
            Ok(())
        })
        .is_err());
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    assert!(store
        .stage_with(grant(two.clone()), &|| false, |_, path| {
            write_new(path, &bytes[..bytes.len() / 2])?;
            Err("synthetic interrupted transfer".into())
        })
        .is_err());
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    let mut crossed = two.clone();
    crossed.assets[0].engines[0].manifest_sha256 = "f".repeat(64);
    assert!(store
        .stage_with(grant(crossed), &|| false, |_, path| write_new(path, &bytes))
        .unwrap_err()
        .contains("module_engine_incompatible"));
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    let staged = stage(&store, two, &bytes);
    assert!(store
        .activate(
            &staged,
            ActivationGuard {
                unsaved_edits: true,
                ..Default::default()
            },
            &|| false
        )
        .unwrap_err()
        .contains("module_activation_deferred"));
    assert!(store
        .activate(
            &staged,
            ActivationGuard {
                uncaptured_sessions: 1,
                ..Default::default()
            },
            &|| false
        )
        .is_err());
    assert!(store
        .activate(
            &staged,
            ActivationGuard {
                host_restart_required: true,
                ..Default::default()
            },
            &|| false
        )
        .is_err());
    assert!(store
        .activate(&staged, ActivationGuard::default(), &|| true)
        .unwrap_err()
        .contains("module_update_cancelled"));
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    let candidate = store.path(&staged.installed).unwrap();
    fs::write(&candidate, b"changed candidate").unwrap();
    assert!(store
        .activate(&staged, ActivationGuard::default(), &|| false)
        .is_err());
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    assert!(store.selected_executable(&|| false).unwrap().is_some());
}
#[test]
fn authority_read_is_one_fixed_request_and_refusals_launch_nothing() {
    let authority = authority();
    let (release, _) = fixture(ModuleComponent::Network, 1);
    let digest = release.assets[0].sha256.clone();
    let reads = Cell::new(0);
    let read = |url: &str| {
        reads.set(reads.get() + 1);
        assert_eq!(url,"https://synthetic-module-authority.ue.gateway.dev/api/v1/app_releases?component=network&channel=canary");
        Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
    };
    let grant = authority
        .acquire_with(ModuleComponent::Network, &digest, &|| false, read)
        .unwrap();
    assert_eq!(reads.get(), 1);
    assert_eq!(grant.release, release);
    assert!(authority
        .acquire_with(ModuleComponent::Network, "invalid", &|| false, |_| panic!(
            "invalid intent must not read"
        ))
        .is_err());
    assert!(authority
        .acquire_with(ModuleComponent::Network, &digest, &|| true, |_| panic!(
            "cancelled intent must not read"
        ))
        .is_err());
    for status in ["blocked", "stopped", "rolled_back"] {
        let mut inactive = release.clone();
        inactive.status = status.into();
        assert!(authority
            .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| Ok(
                serde_json::to_vec(&json!({"success":true,"release":inactive})).unwrap()
            ))
            .unwrap_err()
            .contains("module_release_inactive"));
    }
    assert!(authority
        .acquire_with(ModuleComponent::Network, &"f".repeat(64), &|| false, read)
        .unwrap_err()
        .contains("module_release_changed"));
    assert!(authority
        .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| Ok(
            vec![b' '; MAX_RELEASE_BYTES as usize + 1]
        ))
        .is_err());
    assert!(authority
        .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| Ok(
            br#"{"success":true,"release":null}"#.to_vec()
        ))
        .unwrap_err()
        .contains("module_release_unpublished"));
}
#[test]
fn atomic_pointer_recovery_and_generation_fence_do_not_need_network_or_probes() {
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Solar, "canary").unwrap();
    let (one, bytes) = fixture(ModuleComponent::Solar, 1);
    let first = stage(&store, one, &bytes);
    store
        .activate(&first, ActivationGuard::default(), &|| false)
        .unwrap();
    // Power loss before atomic replacement leaves this incomplete sibling
    // ignored. Only selection.json is ever a reader authority.
    fs::write(store.root.join("interrupted-selection.tmp"), b"{torn").unwrap();
    let restarted = ModuleStore::new(base.path(), ModuleComponent::Solar, "canary").unwrap();
    assert_eq!(restarted.selection().unwrap().unwrap().generation, 1);
    let (two, bytes) = fixture(ModuleComponent::Solar, 2);
    let second = stage(&store, two, &bytes);
    let (three, bytes) = fixture(ModuleComponent::Solar, 3);
    let third = stage(&store, three, &bytes);
    store
        .activate(&second, ActivationGuard::default(), &|| false)
        .unwrap();
    let held = fs::read(store.root.join("selection.json")).unwrap();
    let stale_error = store
        .activate(&third, ActivationGuard::default(), &|| false)
        .unwrap_err();
    assert!(
        stale_error.contains("module_selection_changed"),
        "{stale_error}"
    );
    assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
    let active = store.selected_executable(&|| false).unwrap().unwrap().0;
    fs::write(&active, b"damaged active unit").unwrap();
    assert!(store.selected_executable(&|| false).is_err());
    assert_eq!(
        managed(owner(ModuleComponent::Solar), base.path())
            .err()
            .unwrap()
            .as_str(),
        "module_receipt_invalid"
    );
    let recovered = store
        .rollback(2, ActivationGuard::default(), &|| false)
        .unwrap();
    assert!(recovered.previous.is_none());
    assert!(store.selected_executable(&|| false).unwrap().is_some());
}
#[test]
fn linked_receipts_and_version_paths_and_crossed_component_or_lane_refuse() {
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Reporter, "canary").unwrap();
    let (one, bytes) = fixture(ModuleComponent::Reporter, 1);
    let first = stage(&store, one, &bytes);
    store
        .activate(&first, ActivationGuard::default(), &|| false)
        .unwrap();
    let path = store.root.join("selection.json");
    let backup = store.root.join("untouched-original");
    fs::rename(&path, &backup).unwrap();
    std::os::unix::fs::symlink(&backup, &path).unwrap();
    assert!(store.selection().is_err());
    fs::remove_file(&path).unwrap();
    fs::rename(&backup, &path).unwrap();
    let (other, bytes) = fixture(ModuleComponent::Solar, 2);
    assert!(store
        .stage_with(grant(other), &|| false, |_, _| panic!(
            "crossed owner must not transfer"
        ))
        .is_err());
    let _ = bytes;
    assert!(store.recover_stage("../selection").is_err());
    let stage_path = store.receipt_path(first.receipt_id()).unwrap();
    let real = stage_path.with_extension("original");
    fs::rename(&stage_path, &real).unwrap();
    std::os::unix::fs::symlink(&real, &stage_path).unwrap();
    assert!(store.recover_stage(first.receipt_id()).is_err());
    let mut selected = store.selection().unwrap().unwrap();
    selected.channel = "stable".into();
    assert!(module_install::validate_selection(
        &selected,
        ModuleComponent::Reporter,
        "canary",
        "linux"
    )
    .is_err());
}
fn qualified_network_fixture(
    binary_key: &str,
    receipt_key: &str,
    version: &str,
) -> (ModuleRelease, Vec<u8>, Value) {
    let binary = PathBuf::from(std::env::var_os(binary_key).expect("qualified Network artifact"));
    let producer: Value = serde_json::from_slice(
        &fs::read(std::env::var_os(receipt_key).expect("producer receipt")).unwrap(),
    )
    .unwrap();
    let bytes = fs::read(&binary).unwrap();
    assert_eq!(producer["contract"], "ds.module-artifact/v1");
    assert_eq!(producer["component"], "network");
    assert_eq!(producer["lane"], "canary");
    assert_eq!(producer["platform"], "linux");
    assert_eq!(producer["protocol"], "ds.network-engine/v1");
    assert_eq!(producer["filename"], "ds-network");
    assert_eq!(producer["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(producer["size_bytes"], bytes.len());
    let mut asset = producer.as_object().unwrap().clone();
    for field in [
        "contract",
        "component",
        "lane",
        "version",
        "platform",
        "arch",
        "filename",
    ] {
        asset.remove(field);
    }
    asset.insert("platform".into(), json!("linux"));
    asset.insert("arch".into(), json!("x86_64"));
    asset.insert("kind".into(), json!("module-executable"));
    asset.insert("url".into(),json!(format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/network/canary/{version}/ds-network")));
    let release=validate_module_release("network","canary",&json!({"schema":"ds.module-release/v1","component":"network","channel":"canary","product_id":"rw.datasolutions.module.network.canary","version":version,"protocol":"ds.network-engine/v1","released_at":"2026-10-14T10:00:00Z","status":"active","assets":[asset]})).unwrap();
    (release, bytes, producer)
}
/// Real producer bytes, isolated authority/staging. Run explicitly after
/// supplying the owning qualified artifact/receipt; this is not a public
/// installation or cloud publication assertion.
#[test]
#[ignore = "requires the explicitly selected qualified Network producer artifact"]
fn qualified_network_artifact_runs_lv_from_native_managed_selection() {
    let (release, bytes, producer) = qualified_network_fixture(
        "DS_MODULE_TEST_NETWORK_BIN",
        "DS_MODULE_TEST_NETWORK_RECEIPT",
        "0.1.0-r1",
    );
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    let ready = store
        .stage_with(grant(release), &|| false, |_, path| {
            use std::os::unix::fs::OpenOptionsExt;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|e| e.to_string())?;
            output
                .write_all(&bytes)
                .and_then(|_| output.sync_all())
                .map_err(|e| e.to_string())
        })
        .unwrap();
    let selected = store
        .activate(&ready, ActivationGuard::default(), &|| false)
        .unwrap();
    assert_eq!(
        selected.active.engine_identity["engineIdentity"]["sourceSha"],
        producer["source_sha"]
    );
    assert_eq!(
        selected.active.engine_identity["engineIdentity"]["dependencyClosureSha256"],
        producer["engines"][0]["manifest_sha256"]
    );
    let captured = managed(&ds_cli_exec::addons::NETWORK, base.path()).unwrap();
    assert_eq!(fs::read(captured.executable()).unwrap(), bytes);
    let request = PathBuf::from(
        std::env::var_os("DS_MODULE_TEST_LV_INPUT").expect("owning immutable LV fixture"),
    );
    let output = base.path().join("lv-output");
    fs::create_dir(&output).unwrap();
    let args = [
        "--request".into(),
        request.as_os_str().to_owned(),
        "--out".into(),
        output.clone().into_os_string(),
        "--workers".into(),
        "1".into(),
    ];
    let result = captured
        .call_cancelled("lv-process", &args, Duration::from_secs(30), 1, &|| false)
        .unwrap();
    assert!(
        result.succeeded(),
        "status={:?} stdout={} stderr={}",
        result.status,
        result.stdout,
        result.stderr
    );
    let receipt: Value = serde_json::from_str(&result.stdout).unwrap();
    assert_eq!(receipt["outcome"], "completed");
    assert_eq!(
        receipt["inputSha256"],
        format!("{:x}", Sha256::digest(fs::read(&request).unwrap()))
    );
    let payload = fs::read(output.join("result.json")).unwrap();
    let expected = PathBuf::from(
        std::env::var_os("DS_MODULE_TEST_LV_EXPECTED").expect("owning immutable LV result bytes"),
    );
    assert_eq!(payload, fs::read(expected).unwrap());
    assert_eq!(receipt["artifact"]["byteCount"], payload.len());
    assert_eq!(
        receipt["artifact"]["sha256"],
        format!("{:x}", Sha256::digest(&payload))
    );
    assert_eq!(
        receipt["engineIdentity"]["sourceSha"],
        producer["source_sha"]
    );
    assert_eq!(
        receipt["engineIdentity"]["dependencyClosureSha256"],
        producer["engines"][0]["manifest_sha256"]
    );
    assert!(store.selection().unwrap().unwrap().previous.is_none());
}

/// Two exact producer artifacts exercise retained old work, a compatible
/// activation and rollback under private synthetic authority. No public
/// catalog, installed product selection or unrelated engine is changed.
#[test]
#[ignore = "requires exact qualified Network A/B artifacts, receipts and retained host/Solar/Reporter artifacts"]
fn qualified_network_update_keeps_held_a_runs_b_and_rolls_back_to_a() {
    let (release_a, bytes_a, producer_a) = qualified_network_fixture(
        "DS_MODULE_TEST_NETWORK_BIN_A",
        "DS_MODULE_TEST_NETWORK_RECEIPT_A",
        "0.1.0-r1",
    );
    let (release_b, bytes_b, producer_b) = qualified_network_fixture(
        "DS_MODULE_TEST_NETWORK_BIN_B",
        "DS_MODULE_TEST_NETWORK_RECEIPT_B",
        "0.1.0-r2",
    );
    assert_ne!(producer_a["sha256"], producer_b["sha256"]);
    assert_ne!(producer_a["source_sha"], producer_b["source_sha"]);
    assert_ne!(
        producer_a["engines"][0]["manifest_sha256"],
        producer_b["engines"][0]["manifest_sha256"]
    );
    let mut untouched = Vec::new();
    for key in [
        "DS_MODULE_TEST_HOST_BIN",
        "DS_MODULE_TEST_SOLAR_BIN",
        "DS_MODULE_TEST_REPORTER_BIN",
    ] {
        let path = PathBuf::from(std::env::var_os(key).expect("retained unrelated artifact"));
        let sha256 = format!("{:x}", Sha256::digest(fs::read(&path).unwrap()));
        untouched.push((key, path, sha256));
    }
    let host = std::env::current_exe().unwrap();
    untouched.push((
        "test-host",
        host.clone(),
        format!("{:x}", Sha256::digest(fs::read(host).unwrap())),
    ));
    let request = PathBuf::from(
        std::env::var_os("DS_MODULE_TEST_LV_INPUT").expect("owning immutable LV fixture"),
    );
    let expected = fs::read(
        std::env::var_os("DS_MODULE_TEST_LV_EXPECTED").expect("owning immutable LV result"),
    )
    .unwrap();
    let base = tempfile::tempdir().unwrap();
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    let authority_reads = Cell::new(0);
    let acquire = |release: ModuleRelease| {
        let digest = release.assets[0].sha256.clone();
        authority()
            .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| {
                authority_reads.set(authority_reads.get() + 1);
                Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
            })
            .unwrap()
    };
    let stage_real = |release: ModuleRelease, bytes: &[u8]| {
        store
            .stage_with(acquire(release), &|| false, |_, path| {
                use std::os::unix::fs::OpenOptionsExt;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                output
                    .write_all(bytes)
                    .and_then(|_| output.sync_all())
                    .map_err(|e| e.to_string())
            })
            .unwrap()
    };
    let run = |label: &str, captured: &ds_cli_exec::CapturedExternal, producer: &Value| {
        eprintln!("module-update-phase {label} begin");
        let out = base.path().join(label);
        fs::create_dir(&out).unwrap();
        let args = [
            "--request".into(),
            request.as_os_str().to_owned(),
            "--out".into(),
            out.clone().into_os_string(),
            "--workers".into(),
            "1".into(),
        ];
        let result = captured
            .call_cancelled("lv-process", &args, Duration::from_secs(30), 1, &|| false)
            .unwrap();
        assert!(result.succeeded() && !result.truncated, "{}", result.stdout);
        let receipt: Value = serde_json::from_str(&result.stdout).unwrap();
        assert_eq!(receipt["outcome"], "completed");
        assert_eq!(
            receipt["inputSha256"],
            format!("{:x}", Sha256::digest(fs::read(&request).unwrap()))
        );
        let payload = fs::read(out.join("result.json")).unwrap();
        assert_eq!(payload, expected);
        assert_eq!(receipt["artifact"]["byteCount"], payload.len());
        assert_eq!(
            receipt["artifact"]["sha256"],
            format!("{:x}", Sha256::digest(&payload))
        );
        assert_eq!(
            receipt["engineIdentity"]["sourceSha"],
            producer["source_sha"]
        );
        assert_eq!(
            receipt["engineIdentity"]["dependencyClosureSha256"],
            producer["engines"][0]["manifest_sha256"]
        );
        assert_eq!(fs::read_dir(out).unwrap().count(), 1);
        eprintln!("module-update-phase {label} end");
    };
    eprintln!("module-update-phase stage-a begin");
    let ready_a = stage_real(release_a, &bytes_a);
    eprintln!("module-update-phase stage-a end");
    eprintln!("module-update-phase activate-a begin");
    let selected_a = store
        .activate(&ready_a, ActivationGuard::default(), &|| false)
        .unwrap();
    let held_a = managed(&ds_cli_exec::addons::NETWORK, base.path()).unwrap();
    assert_eq!(selected_a.generation, 1);
    assert_eq!(fs::read(held_a.executable()).unwrap(), bytes_a);
    assert_eq!(authority_reads.get(), 1);
    eprintln!("module-update-phase activate-a end");
    eprintln!("module-update-phase stage-b begin");
    let ready_b = stage_real(release_b, &bytes_b);
    eprintln!("module-update-phase stage-b end");
    eprintln!("module-update-phase activate-b begin");
    let selected_b = store
        .activate(&ready_b, ActivationGuard::default(), &|| false)
        .unwrap();
    let new_b = managed(&ds_cli_exec::addons::NETWORK, base.path()).unwrap();
    assert_eq!(selected_b.generation, 2);
    assert_eq!(selected_b.previous.as_ref().unwrap(), &selected_a.active);
    assert_eq!(fs::read(new_b.executable()).unwrap(), bytes_b);
    assert_ne!(held_a.executable(), new_b.executable());
    assert_eq!(fs::read(held_a.executable()).unwrap(), bytes_a);
    assert_eq!(authority_reads.get(), 2);
    eprintln!("module-update-phase activate-b end");
    run("held-a", &held_a, &producer_a);
    run("new-b", &new_b, &producer_b);
    eprintln!("module-update-phase rollback begin");
    let rolled_back = store
        .rollback(2, ActivationGuard::default(), &|| false)
        .unwrap();
    let new_a = managed(&ds_cli_exec::addons::NETWORK, base.path()).unwrap();
    assert_eq!(rolled_back.generation, 3);
    assert_eq!(rolled_back.active, selected_a.active);
    assert_eq!(rolled_back.previous.as_ref().unwrap(), &selected_b.active);
    assert_eq!(new_a.executable(), held_a.executable());
    assert_eq!(authority_reads.get(), 2);
    eprintln!("module-update-phase rollback end");
    run("rollback-a", &new_a, &producer_a);
    assert_eq!(fs::read(held_a.executable()).unwrap(), bytes_a);
    for (key, path, before) in &untouched {
        assert_eq!(
            &format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
            before,
            "{key}"
        );
    }
    eprintln!(
        "MODULE_QUALIFIED_NETWORK_UPDATE_PROOF {}",
        json!({
            "artifact_a":producer_a,"artifact_b":producer_b,
            "authority_fixture_reads":authority_reads.get(),
            "unrelated_artifacts":untouched.iter().map(|(key,path,sha)|json!({"role":key,"path":path,"sha256":sha})).collect::<Vec<_>>(),
            "exact_lv_results":3,"held_a_preserved":true,"rollback_generation":rolled_back.generation,
            "authority":"isolated synthetic authority; no public or installed-product update claim"
        })
    );
}
