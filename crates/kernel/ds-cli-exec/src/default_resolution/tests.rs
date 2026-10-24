use super::*;
use ds_module_contracts::{module_install::*, module_release::*};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, sync::atomic::AtomicUsize};
static NEXT: AtomicUsize = AtomicUsize::new(0);
static OWNER: External = External {
    name: "ds-network",
    owner: "ds-network",
    env_override: "DS_EXEC_DEFAULT_FIXTURE",
    missing_code: "fixture_missing",
    remedy: "repair fixture",
};

struct Fixture {
    root: PathBuf,
    base: PathBuf,
    host: PathBuf,
    fallback: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ds-default-resolution-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let base = root.join("addons");
        let host = root.join("bin/ds-canary");
        fs::create_dir_all(host.parent().unwrap()).unwrap();
        let fallback = root.join("bin/ds-network");
        executable(&fallback, b"#!/bin/sh\nexit 97\n");
        executable(&root.join("bin/ds-network-canary"), b"#!/bin/sh\nexit 98\n");
        Self {
            root,
            base,
            host,
            fallback,
        }
    }
    fn resolve(&self, lane: Option<&str>) -> Result<Resolved, Failure> {
        OWNER.resolve_default_for_paths(
            Some(&self.host),
            self.fallback.parent().map(Path::as_os_str),
            lane,
            || Ok(self.base.clone()),
        )
    }
    fn selection(&self) -> PathBuf {
        self.base.join("network/canary/selection.json")
    }
    fn install(&self) -> PathBuf {
        let marker = self
            .root
            .join("executed")
            .to_string_lossy()
            .replace('\'', "'\\''");
        let bytes =
            format!("#!/bin/sh\nprintf launched > '{marker}'\nprintf 'managed-canary\\n'\n")
                .into_bytes();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let source = "a".repeat(40);
        let closure = "b".repeat(64);
        let release: ModuleRelease = serde_json::from_value(json!({
            "schema":MODULE_RELEASE_SCHEMA,"component":"network","channel":"canary",
            "product_id":"rw.datasolutions.module.network.canary","version":"0.1.0-r1","protocol":NETWORK_PROTOCOL,
            "released_at":"2026-10-17T00:00:00Z","status":"active","assets":[{
                "platform":"linux","arch":"x86_64","kind":"module-executable","protocol":NETWORK_PROTOCOL,
                "url":"https://storage.googleapis.com/test/modules/network/canary/0.1.0-r1/ds-network",
                "size_bytes":bytes.len(),"sha256":digest,"source_sha":source,"built_at":"2026-10-17T00:00:00Z",
                "source_revisions":{"ds-network":source},"engines":[{"engine":"ds-network",
                "release":format!("ds-network@0.1.0+{source}"),"manifest_sha256":closure}]
            }]
        })).unwrap();
        let active = InstalledModule {
            schema: INSTALLED_SCHEMA.into(),
            platform: "linux".into(),
            authority: ReleaseAuthorityReceipt {
                gateway_origin: "https://test.ue.gateway.dev/".into(),
                native_catalog_sha256: "c".repeat(64),
                release_sha256: format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(&release).unwrap())
                ),
            },
            release,
            engine_identity: json!({"schema":"ds.engine-build/v1","module":"network",
            "engineIdentity":{"schema":NETWORK_PROTOCOL,"protocolMajor":1,"environment":"native","coreVersion":"0.1.0",
                "sourceSha":source,"dependencyClosureSha256":closure,"jsonObjectOrder":"sorted",
                "runtime":{"environment":"native","logicalCpus":2,"internalWorkerThreads":1},
                "capabilities":["lv_processing_v1","lv_publication_v1","lv_analysis_validation_v1","grid_query_v1","grid_inspect_v1","grid_apply_v1","grid_publication_v1","profile_session_v1","report_engineering_v1","report_sizing_v1","report_feeder_selection_v1"]},
            "build":{"profile":"release","target":"x86_64-unknown-linux-gnu","cargoLockSha256":"d".repeat(64),"rustc":"test"}}),
        };
        let selection = ModuleSelection {
            schema: SELECTION_SCHEMA.into(),
            component: ModuleComponent::Network,
            channel: "canary".into(),
            generation: 1,
            active,
            previous: None,
        };
        validate_selection(&selection, ModuleComponent::Network, "canary", "linux").unwrap();
        let selected = self
            .base
            .join("network/canary/versions")
            .join(&digest)
            .join("ds-network");
        fs::create_dir_all(selected.parent().unwrap()).unwrap();
        executable(&selected, &bytes);
        fs::write(self.selection(), serde_json::to_vec(&selection).unwrap()).unwrap();
        selected
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn executable(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn packaged_lane_matches_native_authority_without_inventing_local_stable() {
    for (debug, root, path, windows, expected) in [
        (
            true,
            "rw.datasolutions.canary",
            "/usr/bin/ds-canary",
            false,
            None,
        ),
        (
            false,
            "rw.datasolutions.canary",
            "/target/release/ds",
            false,
            Some("canary"),
        ),
        (
            false,
            "rw.datasolutions",
            "/usr/bin/ds",
            false,
            Some("stable"),
        ),
        (false, "", "/usr/bin/ds-canary", false, Some("canary")),
        (false, "", "/target/release/ds-canary", false, None),
        (false, "", "/target/release/ds", false, None),
        (false, "", "/usr/bin/ds", false, None),
        (
            false,
            "",
            "C:/DataSolutions Canary/ds.exe",
            true,
            Some("canary"),
        ),
        (false, "", "C:/DataSolutions/ds.exe", true, Some("stable")),
    ] {
        assert_eq!(
            packaged_lane(debug, root, Some(Path::new(path)), windows),
            expected,
            "{path}"
        );
    }
}

#[test]
fn packaged_canary_captures_managed_bytes_without_discovery_execution_and_retains_them() {
    let fixture = Fixture::new();
    let selected = fixture.install();
    let resolved = fixture.resolve(Some("canary")).unwrap();
    assert_eq!(resolved.executable, selected);
    assert!(resolved.managed_identity.is_some());
    assert_eq!(resolved.lane, Some("canary"));
    let captured = OWNER.capture_resolved(resolved).unwrap();
    assert!(
        !fixture.root.join("executed").exists(),
        "discovery must not probe the engine"
    );
    fs::write(fixture.selection(), b"{changed-pointer").unwrap();
    assert_eq!(captured.executable(), selected);
    assert_eq!(
        captured
            .call("build-info", &[], Duration::from_secs(2))
            .unwrap()
            .stdout,
        "managed-canary\n"
    );
    assert!(fixture.root.join("executed").exists());
    let mut corrupt = fs::read(&selected).unwrap();
    corrupt[0] ^= 1;
    fs::write(&selected, corrupt).unwrap();
    assert_eq!(
        captured
            .call("build-info", &[], Duration::from_secs(2))
            .err()
            .unwrap()
            .code(),
        "module_receipt_invalid"
    );
}

#[test]
fn packaged_missing_removed_crosslane_and_corrupt_never_fall_back() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.resolve(Some("canary")).err().unwrap().code(),
        "fixture_missing"
    );
    let selected = fixture.install();
    assert_eq!(
        fixture.resolve(Some("stable")).err().unwrap().code(),
        "fixture_missing"
    );
    let valid = fs::read(fixture.selection()).unwrap();
    fs::write(fixture.selection(), b"{torn").unwrap();
    assert_eq!(
        fixture.resolve(Some("canary")).err().unwrap().code(),
        "module_receipt_invalid"
    );
    fs::write(fixture.selection(),serde_json::to_vec(&json!({"schema":REMOVED_SELECTION_SCHEMA,"component":"network","channel":"canary","generation":2,"state":"removed"})).unwrap()).unwrap();
    assert_eq!(
        fixture.resolve(Some("canary")).err().unwrap().code(),
        "module_removed"
    );
    fs::write(fixture.selection(), valid).unwrap();
    fs::write(&selected, b"changed").unwrap();
    assert_eq!(
        fixture.resolve(Some("canary")).err().unwrap().code(),
        "module_receipt_invalid"
    );
}

#[test]
fn local_and_exact_unmanaged_owners_keep_their_legacy_lookup() {
    let fixture = Fixture::new();
    // Existing ds-canary sibling rule remains deliberate for a local host.
    assert_eq!(
        fixture.resolve(None).unwrap().executable,
        fixture.root.join("bin/ds-network-canary")
    );
    for (name, owner) in [("ds-network", "other"), ("other", "ds-network")] {
        let external = External {
            name,
            owner,
            env_override: "DS_EXEC_NONMANAGED_UNUSED",
            missing_code: "fixture_missing",
            remedy: "repair",
        };
        let sibling = fixture
            .host
            .parent()
            .unwrap()
            .join(format!("{name}-canary"));
        executable(&sibling, b"fixture");
        let resolved = external
            .resolve_default_for_paths(Some(&fixture.host), None, Some("canary"), || {
                panic!("unmanaged owner must not read managed base")
            })
            .unwrap();
        assert_eq!(resolved.executable, sibling);
        assert!(resolved.managed_identity.is_none());
    }
}

#[test]
fn packaged_override_remains_deliberate_and_lane_override_does_not_recurse() {
    static EXPLICIT: External = External {
        name: "ds-network",
        owner: "ds-network",
        env_override: "DS_EXEC_DEFAULT_EXPLICIT",
        missing_code: "fixture_missing",
        remedy: "repair",
    };
    let fixture = Fixture::new();
    unsafe {
        std::env::set_var(EXPLICIT.env_override, &fixture.fallback);
    }
    let resolved = EXPLICIT
        .resolve_default_for_paths(Some(&fixture.host), None, Some("canary"), || {
            panic!("explicit override must not read managed base")
        })
        .unwrap();
    assert_eq!(resolved.executable, fixture.fallback);
    assert!(resolved.managed_identity.is_none());
    assert_eq!(
        EXPLICIT
            .capture_for_lane_in("canary", &fixture.base)
            .unwrap()
            .executable(),
        fixture.fallback
    );
    unsafe {
        std::env::set_var(EXPLICIT.env_override, fixture.root.join("absent"));
    }
    assert_eq!(
        EXPLICIT
            .capture_for_lane_in("canary", &fixture.base)
            .err()
            .unwrap()
            .code(),
        "fixture_missing"
    );
    unsafe {
        std::env::remove_var(EXPLICIT.env_override);
    }
}

#[test]
fn all_three_exact_managed_owners_require_the_packaged_lane() {
    let fixture = Fixture::new();
    for (name, owner) in [
        ("solar", "ds-solar"),
        ("reporter", "ds-network-reporter"),
        ("ds-network", "ds-network"),
    ] {
        let external = External {
            name,
            owner,
            env_override: "DS_EXEC_THREE_MANAGED_UNUSED",
            missing_code: "fixture_missing",
            remedy: "repair",
        };
        assert_eq!(
            external
                .resolve_default_for_paths(
                    Some(&fixture.host),
                    Some(fixture.root.as_os_str()),
                    Some("canary"),
                    || Ok(fixture.base.clone())
                )
                .err()
                .unwrap()
                .code(),
            "fixture_missing"
        );
    }
}

#[test]
fn default_reporter_capture_and_fixed_launches_preserve_only_the_closed_host_lane() {
    static REPORTER: External = External {
        name: "reporter",
        owner: "ds-network-reporter",
        env_override: "DS_EXEC_DEFAULT_REPORTER_LANE",
        missing_code: "fixture_missing",
        remedy: "repair",
    };
    let fixture = Fixture::new();
    let reporter = fixture.root.join("reporter-fixture");
    executable(
        &reporter,
        b"#!/bin/sh\nif [ \"$1\" = \"typed-input\" ]; then cat >/dev/null; fi\nprintf '%s' \"${DS_REPORTER_NETWORK_LANE-unset}\"\n",
    );
    unsafe {
        std::env::set_var(REPORTER.env_override, &reporter);
    }
    for lane in [Some("canary"), Some("stable"), None] {
        let resolved = REPORTER
            .resolve_default_for_paths(Some(&fixture.host), None, lane, || {
                panic!("explicit fixture must not read managed base")
            })
            .unwrap();
        assert_eq!(resolved.lane, lane);
        let captured = REPORTER.capture_resolved(resolved).unwrap();
        let expected = lane.unwrap_or("unset");
        assert_eq!(
            captured
                .call("build-info", &[], Duration::from_secs(2))
                .unwrap()
                .stdout,
            expected
        );
        let resolved = REPORTER
            .resolve_default_for_paths(Some(&fixture.host), None, lane, || {
                panic!("explicit fixture must not read managed base")
            })
            .unwrap();
        assert_eq!(
            resolved
                .verified(|| REPORTER.call_at(
                    &resolved.executable,
                    "build-info",
                    &[],
                    Duration::from_secs(2),
                    resolved.lane
                ))
                .unwrap()
                .stdout,
            expected
        );
        assert_eq!(
            resolved
                .verified(|| REPORTER.call_with_stdin_at(
                    &resolved.executable,
                    "typed-input",
                    &[],
                    b"{}",
                    Duration::from_secs(2),
                    resolved.lane
                ))
                .unwrap()
                .stdout,
            expected
        );
    }
    assert_eq!(
        REPORTER
            .resolve_default_for_paths(None, None, Some("other"), || panic!(
                "invalid lane must refuse before lookup"
            ))
            .err()
            .unwrap()
            .code(),
        "module_lane_invalid"
    );
    unsafe {
        std::env::remove_var(REPORTER.env_override);
    }
}

/// a1964c7d: discovery of an addon whose lane is a call input admits any
/// installed lane, reads only metadata and receipts, and otherwise keeps the
/// packaged default's answer with its remedy.
#[test]
fn any_lane_discovery_admits_an_installed_lane_without_executing_it() {
    let fixture = Fixture::new();
    assert_eq!(
        OWNER.availability_for_any_lane_in(Ok(fixture.base.clone())),
        OWNER.availability()
    );
    fixture.install();
    assert_eq!(
        OWNER.availability_for_any_lane_in(Ok(fixture.base.clone())),
        Availability::Available
    );
    assert!(!fixture.root.join("executed").exists());
    assert_eq!(
        OWNER.availability_for_any_lane_in(Err(Failure::invalid("base", "absent"))),
        OWNER.availability()
    );
}
