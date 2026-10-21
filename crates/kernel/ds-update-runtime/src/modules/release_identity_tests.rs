//! Main application and Core release reads and installation records.
use super::*;
use serde_json::json;
use std::cell::RefCell;

fn authority() -> CapturedReleaseAuthority {
    CapturedReleaseAuthority {
        gateway_origin: "https://gateway.example".into(),
        channel: "canary",
        catalog_sha256: "e".repeat(64),
        gateway_key: "public-fixture-key".into(),
    }
}

fn read(application: Application, body: Value) -> (Result<Value, String>, String) {
    let asked = RefCell::new(String::new());
    let result = authority().read_application_release_with(application, |url| {
        *asked.borrow_mut() = url.to_owned();
        Ok(serde_json::to_vec(&body).unwrap())
    });
    (result, asked.into_inner())
}

#[test]
fn each_application_reads_only_its_own_lane_member() {
    let member = json!({"channel":"canary","version":"0.2.0-r118"});
    let (shell, url) = read(Application::Shell, json!({"success":true,"release":member}));
    assert_eq!(shell.unwrap(), member);
    assert_eq!(
        url,
        "https://gateway.example/api/v1/app_releases?channel=canary"
    );
    let (core, url) = read(Application::Core, json!({"success":true,"release":null}));
    assert_eq!(core.unwrap(), Value::Null);
    assert_eq!(
        url,
        "https://gateway.example/api/v1/app_releases?component=core&channel=canary"
    );
    for body in [
        json!({"success":false,"release":null}),
        json!({"success":true,"release":null,"redirect":"https://other.example"}),
        json!({"release":member}),
        json!("x".repeat(MAX_RELEASE_BYTES as usize)),
    ] {
        assert!(read(Application::Shell, body).0.is_err());
    }
    let failed = authority().read_application_release_with(Application::Core, |_| {
        Err("module_release_unavailable: offline".into())
    });
    assert_eq!(failed.unwrap_err(), "module_release_unavailable: offline");
}

fn base() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn record(application: Application, channel: &str) -> InstallationRecord {
    serde_json::from_value(json!({"schema":INSTALLATION_SCHEMA,"component":application,
        "channel":channel,"platform":"linux","build":{"source_sha":"6".repeat(64),
        "built_at":"2026-10-20T08:06:40.396Z"},"release_version":"0.2.0-r118",
        "released_at":"2026-10-20T08:20:57.054388Z","recorded_at":"2026-10-20T12:00:00Z"}))
    .unwrap()
}

#[test]
fn a_record_is_read_back_exactly_and_reading_creates_nothing() {
    let base = base();
    let store = InstallationStore::new(base.path(), Application::Shell, "canary").unwrap();
    assert_eq!(store.read().unwrap(), None);
    assert!(!base.path().join("installations").exists());
    let shell = record(Application::Shell, "canary");
    store.write(&shell).unwrap();
    assert_eq!(
        store.read().unwrap(),
        Some(serde_json::to_value(&shell).unwrap())
    );
    let path = base.path().join("installations/canary/shell.json");
    assert!(path.is_file());
    // A replacement is whole; Core's record is its own file.
    let mut next = shell.clone();
    next.release_version = "0.2.0-r119".into();
    store.write(&next).unwrap();
    assert_eq!(
        store.read().unwrap().unwrap()["release_version"],
        "0.2.0-r119"
    );
    let core = InstallationStore::new(base.path(), Application::Core, "canary").unwrap();
    assert_eq!(core.read().unwrap(), None);
    // Unreadable JSON is no record; the next proof replaces it.
    std::fs::write(&path, b"{torn").unwrap();
    assert_eq!(store.read().unwrap(), None);
    store.write(&shell).unwrap();
    assert!(store.read().unwrap().is_some());
}

#[test]
fn a_crossed_record_local_lane_or_relative_base_refuses() {
    let base = base();
    let store = InstallationStore::new(base.path(), Application::Shell, "canary").unwrap();
    assert!(store.write(&record(Application::Core, "canary")).is_err());
    assert!(store.write(&record(Application::Shell, "stable")).is_err());
    let mut other = record(Application::Shell, "canary");
    other.schema = "ds.release-installation/v0".into();
    assert!(store.write(&other).is_err());
    assert!(InstallationStore::new(base.path(), Application::Shell, "local").is_err());
    assert!(InstallationStore::new(Path::new("relative"), Application::Core, "stable").is_err());
    assert!(!base.path().join("installations").exists());
}

#[cfg(unix)]
#[test]
fn a_linked_record_store_refuses_to_be_read() {
    let base = base();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), base.path().join("installations")).unwrap();
    let store = InstallationStore::new(base.path(), Application::Core, "canary").unwrap();
    assert!(store.read().is_err());
    let mut core = record(Application::Core, "canary");
    core.build = [
        ("source_sha", "3".repeat(40)),
        ("executable_sha256", "5".repeat(64)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    assert!(store.write(&core).is_err());
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}
