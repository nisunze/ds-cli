//! Mechanical native consumer qualification against existing producer binaries.
//! Selection receipts below are isolated fixtures, never installation authority.
#![cfg(unix)]
use ds_cli_exec::{External, addons};
use ds_command_kernel::{module_install::*, module_release::*};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn probe(owner: &'static External, path: &Path) -> Value {
    owner
        .capture_pinned(path)
        .unwrap()
        .call_json("build-info", &[], Duration::from_secs(20))
        .unwrap()
}

fn select(
    base: &Path,
    component: ModuleComponent,
    lane: &str,
    binary: &Path,
    info: &Value,
    generation: u64,
    trailer: &[u8],
) -> PathBuf {
    let mut bytes = fs::read(binary).unwrap();
    bytes.extend_from_slice(trailer);
    let sha256 = ds_compute_runtime::digest(&bytes);
    let root = base.join(component.as_str()).join(lane);
    let directory = root.join("versions").join(&sha256);
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(component.executable());
    fs::write(&path, &bytes).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let source = info["source_sha"].as_str().unwrap();
    let version = info[if component == ModuleComponent::Solar {
        "version"
    } else {
        "package_version"
    }]
    .as_str()
    .unwrap();
    let release = validate_module_release(component.as_str(), lane, &json!({
        "schema":"ds.module-release/v1", "component":component, "channel":lane,
        "product_id":format!("rw.datasolutions.module.{}{}",component.as_str(), if lane == "canary" { ".canary" } else { "" }),
        "version":format!("{version}-r{generation}"), "protocol":component.protocol(),
        "released_at":"2026-10-14T10:00:00Z", "status":"active", "assets":[{
            "platform":"linux", "arch":"x86_64", "kind":"module-executable",
            "protocol":component.protocol(),
            "url":format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/{}/{lane}/{version}-r{generation}/{}",component.as_str(), component.executable()),
            "size_bytes":bytes.len(), "sha256":sha256, "source_sha":source,
            "built_at":"2026-10-14T09:00:00Z", "source_revisions":{component.engine_owner():source},
            "engines":[{"engine":component.engine_owner(),
                "release":format!("{}@{version}+{source}",component.engine_owner()),
                "manifest_sha256":info["build_manifest_sha256"]}]
        }]
    })).unwrap();
    let installed = InstalledModule {
        schema: INSTALLED_SCHEMA.into(),
        authority: ReleaseAuthorityReceipt {
            gateway_origin: "https://synthetic-module-authority.ue.gateway.dev".into(),
            native_catalog_sha256: "a".repeat(64),
            release_sha256: ds_compute_runtime::digest(&serde_json::to_vec(&release).unwrap()),
        },
        release,
        platform: "linux".into(),
        engine_identity: info.clone(),
    };
    let selected = ModuleSelection {
        schema: SELECTION_SCHEMA.into(),
        component,
        channel: lane.into(),
        generation,
        active: installed,
        previous: None,
    };
    validate_selection(&selected, component, lane, "linux").unwrap();
    let staged = root.join("selection.next");
    fs::write(&staged, serde_json::to_vec(&selected).unwrap()).unwrap();
    fs::rename(staged, root.join("selection.json")).unwrap();
    path
}

#[test]
#[ignore = "requires explicitly selected existing qualified Solar and Reporter binaries"]
fn qualified_addons_capture_exact_lane_and_keep_old_program_without_task_or_network() {
    let base = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let binaries = [
        (
            ModuleComponent::Solar,
            &addons::SOLAR,
            "DS_MODULE_TEST_SOLAR_BIN",
        ),
        (
            ModuleComponent::Reporter,
            &addons::REPORTER,
            "DS_MODULE_TEST_REPORTER_BIN",
        ),
    ];
    for (component, owner, variable) in binaries {
        assert!(
            std::env::var_os(owner.env_override).is_none(),
            "test selects managed receipts"
        );
        let binary = PathBuf::from(std::env::var_os(variable).expect("qualified artifact"));
        let info = probe(owner, &binary);
        assert!(
            owner
                .capture_managed_for_lane_in("canary", base.path())
                .unwrap()
                .is_none()
        );
        let stable = select(base.path(), component, "stable", &binary, &info, 1, b"");
        let canary = select(base.path(), component, "canary", &binary, &info, 1, b"");
        if component == ModuleComponent::Solar {
            for lane in ["stable", "canary"] {
                let host = ds_solar_native::NativeHost::headless(scratch.path().join(lane))
                    .unwrap()
                    .with_engine_lane(lane)
                    .unwrap();
                assert_eq!(
                    host.selected_engine_path_in(base.path()).unwrap(),
                    if lane == "stable" { &stable } else { &canary }.to_path_buf()
                );
            }
        }
        let old = owner
            .capture_managed_for_lane_in("canary", base.path())
            .unwrap()
            .unwrap();
        assert_eq!(old.executable(), canary);
        assert_eq!(
            owner
                .capture_for_lane_in("stable", base.path())
                .unwrap()
                .executable(),
            stable
        );
        // A fixture-only ELF trailer yields another immutable byte identity
        // while retaining the same compatible producer protocol/build identity.
        let next = select(
            base.path(),
            component,
            "canary",
            &binary,
            &info,
            2,
            b"\nmanaged-consumer-fixture-v2\n",
        );
        assert_ne!(canary, next);
        assert_eq!(
            owner
                .capture_for_lane_in("canary", base.path())
                .unwrap()
                .executable(),
            next
        );
        assert_eq!(old.executable(), canary);
        assert_eq!(
            old.call_json("build-info", &[], Duration::from_secs(20))
                .unwrap(),
            info
        );
        if component == ModuleComponent::Solar {
            let cancel = Default::default();
            assert_eq!(
                serde_json::to_value(
                    ds_solar_process::PreparedEngine::capture(
                        old.executable(),
                        scratch.path(),
                        1,
                        &cancel
                    )
                    .unwrap()
                    .identity()
                )
                .unwrap()["source_sha"],
                info["source_sha"]
            );
            ds_solar_process::BatchEngine::capture(old.executable(), scratch.path(), 1, &cancel)
                .unwrap();
            ds_solar_process::PortfolioEngine::capture(
                old.executable(),
                scratch.path(),
                1,
                &cancel,
            )
            .unwrap();
        }
        // Corrupt managed state cannot borrow the intact stable lane or PATH.
        fs::write(
            base.path()
                .join(component.as_str())
                .join("canary/selection.json"),
            b"{}",
        )
        .unwrap();
        assert_eq!(
            owner
                .capture_for_lane_in("canary", base.path())
                .err()
                .unwrap()
                .code(),
            "module_receipt_invalid"
        );
        if component == ModuleComponent::Solar {
            let host = ds_solar_native::NativeHost::headless(scratch.path().join("corrupt"))
                .unwrap()
                .with_engine_lane("canary")
                .unwrap();
            assert!(
                host.selected_engine_path_in(base.path())
                    .unwrap_err()
                    .starts_with("module_receipt_invalid")
            );
        }
        assert_eq!(
            owner
                .capture_for_lane_in("stable", base.path())
                .unwrap()
                .executable(),
            stable
        );
        assert_eq!(
            old.call_json("build-info", &[], Duration::from_secs(20))
                .unwrap(),
            info
        );
    }
}

fn isolated_solar_host(override_path: Option<&Path>) {
    // Hard-links this test binary, so the root shares its (disk) filesystem.
    let root = ds_cli_contract::testing::private_tempdir("solar-host-");
    let child = root.path().join("host-selection-test");
    fs::hard_link(std::env::current_exe().unwrap(), &child).unwrap();
    use std::os::unix::fs::PermissionsExt;
    for name in ["solar", "ds-solar"] {
        let trap = root.path().join(name);
        fs::write(
            &trap,
            format!(
                "#!/bin/sh\necho called > '{}'\nexit 99\n",
                root.path().join("called").display()
            ),
        )
        .unwrap();
        fs::set_permissions(trap, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut command = std::process::Command::new(&child);
    command
        .args([
            "--exact",
            "solar_native_selection_child",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_SOLAR_SELECTION_ROOT", root.path())
        .env("PATH", root.path())
        .env_remove("DS_SOLAR_ENGINE_BIN");
    if let Some(path) = override_path {
        command.env("DS_SOLAR_ENGINE_BIN", path);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.path().join("called").exists());
}

#[test]
fn solar_native_missing_and_corrupt_selection_refuse_sibling_and_path_traps() {
    isolated_solar_host(None);
}

#[test]
#[ignore = "isolated native selection control"]
fn solar_native_selection_child() {
    let root = PathBuf::from(std::env::var_os("DS_SOLAR_SELECTION_ROOT").unwrap());
    let base = root.join("addons");
    let data = root.join("native");
    let host = ds_solar_native::NativeHost::headless(data.clone())
        .unwrap()
        .with_engine_lane("canary")
        .unwrap();
    if let Some(path) = std::env::var_os("DS_SOLAR_ENGINE_BIN") {
        fs::create_dir_all(base.join("solar/canary")).unwrap();
        fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
        assert_eq!(
            host.selected_engine_path_in(&base).unwrap(),
            fs::canonicalize(path).unwrap()
        );
    } else {
        assert!(
            host.selected_engine_path_in(&base)
                .unwrap_err()
                .starts_with("solar_engine_missing")
        );
        fs::create_dir_all(base.join("solar/canary")).unwrap();
        fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
        assert!(
            host.selected_engine_path_in(&base)
                .unwrap_err()
                .starts_with("module_receipt_invalid")
        );
        // The deliberately unmanaged development host retains its old sibling policy.
        let unmanaged = ds_solar_native::NativeHost::headless(root.join("unmanaged")).unwrap();
        assert_eq!(
            unmanaged.selected_engine_path_in(&base).unwrap(),
            root.join("solar")
        );
    }
    assert!(!data.join("solar-jobs").exists());
    assert!(!root.join("called").exists());
}

#[test]
#[ignore = "requires an explicitly qualified retained Solar executable"]
fn solar_native_managed_and_explicit_override_selection_preserve_exact_program() {
    assert!(std::env::var_os("DS_SOLAR_ENGINE_BIN").is_none());
    let binary = PathBuf::from(std::env::var_os("DS_MODULE_TEST_SOLAR_BIN").unwrap());
    let info = probe(&addons::SOLAR, &binary);
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("addons");
    let host = ds_solar_native::NativeHost::headless(root.path().join("native"))
        .unwrap()
        .with_engine_lane("canary")
        .unwrap();
    select(
        &base,
        ModuleComponent::Solar,
        "stable",
        &binary,
        &info,
        1,
        b"",
    );
    assert!(
        host.selected_engine_path_in(&base)
            .unwrap_err()
            .starts_with("solar_engine_missing")
    );
    let first = select(
        &base,
        ModuleComponent::Solar,
        "canary",
        &binary,
        &info,
        1,
        b"",
    );
    let retained = host.selected_engine_path_in(&base).unwrap();
    assert_eq!(retained, first);
    let second = select(
        &base,
        ModuleComponent::Solar,
        "canary",
        &binary,
        &info,
        2,
        b"\nselection-control\n",
    );
    assert_eq!(host.selected_engine_path_in(&base).unwrap(), second);
    fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
    assert!(
        host.selected_engine_path_in(&base)
            .unwrap_err()
            .starts_with("module_receipt_invalid")
    );
    assert_eq!(probe(&addons::SOLAR, &retained), info);
    isolated_solar_host(Some(&binary));
}
