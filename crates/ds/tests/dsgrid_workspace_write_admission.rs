//! Real CLI workspace consumers, exact protected messages and request counts.
use ds_command_kernel::local_models::{Op, Origin, Scope};
use ds_layer_store::local_models::{execute_at, package_path, read_at, scope_dir};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn root() -> tempfile::TempDir {
    let base = PathBuf::from(env::var_os("DS_NETWORK_WRITE_FIXTURES").unwrap()).join("cli-scratch");
    fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}
fn scope() -> Scope {
    Scope {
        lane: "stable".into(),
        uid: "write-guard-test".into(),
    }
}
fn seed(root: &Path, id: &str, bytes: &[u8]) {
    execute_at(
        root,
        &scope(),
        Op::Register {
            id: id.into(),
            display_name: id.into(),
            origin: Origin::Imported,
            crs: "EPSG:32735".into(),
            model_revision: 1,
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            created_at: None,
            project: None,
            head_revision: None,
            activate: false,
        },
        Some(bytes),
    )
    .unwrap();
}
fn call(root: &Path, args: &[&str], missing: bool) -> (Value, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ds"));
    cmd.args(["dsgrid", "model"])
        .args(args)
        .args([
            "--account",
            "write-guard-test",
            "--lane",
            "stable",
            "--output",
            "json",
        ])
        .env("TMPDIR", root)
        .env("DS_LAYER_HOME", root);
    if missing {
        cmd.env("DS_NETWORK_ENGINE_BIN", root.join("missing-network"));
    }
    let output = cmd.output().unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().unwrap())
}
fn calls(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("requests.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "requires exact native owner behind the request-count adapter"]
fn create_reuses_one_capture_and_ordinary_historical_metadata_stays_offline() {
    let root = root();
    let (created, status) = call(
        root.path(),
        &["create-local", "--name", "Guard capture"],
        false,
    );
    assert_eq!(status, 0, "{created}");
    assert_eq!(
        calls(root.path()),
        ["build-info", "grid-create", "grid-write-admission"]
    );
    let id = created["data"]["model"]["model"].as_str().unwrap();
    let (listed, status) = call(root.path(), &["list"], true);
    assert_eq!(status, 0, "{listed}");
    let (forgotten, status) = call(root.path(), &["forget", "--model", id], true);
    assert_eq!(status, 0, "{forgotten}");
    assert_eq!(
        calls(root.path()),
        ["build-info", "grid-create", "grid-write-admission"]
    );

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-network/crates/ds-grid-exchange/tests/fixtures/ground-basis-schema4/schema4.dsgrid");
    let historical = fs::read(source).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&historical)),
        "7ccadc264cf9225ba87ae984bd17b5e1e218ca39c14f6831ec3c17165112b21d"
    );
    seed(root.path(), "historical-schema4", &historical);
    let (listed, status) = call(root.path(), &["list"], true);
    assert_eq!(status, 0, "{listed}");
    let (forgotten, status) = call(
        root.path(),
        &["forget", "--model", "historical-schema4"],
        true,
    );
    assert_eq!(status, 0, "{forgotten}");
    assert_eq!(
        calls(root.path()),
        ["build-info", "grid-create", "grid-write-admission"]
    );
}

#[test]
#[ignore = "requires canonical protected fixtures and exact native count adapter"]
fn protected_catalogue_mutations_keep_short_and_long_owner_messages_and_bytes() {
    let fixtures = PathBuf::from(env::var_os("DS_NETWORK_WRITE_FIXTURES").unwrap());
    for name in ["protected", "long-protected"] {
        let root = root();
        let bytes = fs::read(fixtures.join(format!("{name}.dsgrid"))).unwrap();
        let expected: Value = serde_json::from_slice(
            &fs::read(fixtures.join(format!("{name}-delete.json"))).unwrap(),
        )
        .unwrap();
        seed(root.path(), name, &bytes);
        let before = read_at(root.path(), &scope()).unwrap();
        let (value, status) = call(root.path(), &["forget", "--model", name], false);
        assert_eq!(status, 2, "{value}");
        assert_eq!(value["error"]["code"], "composite_model_protected");
        assert_eq!(value["error"]["message"], expected["message"]);
        assert!(value["error"].get("detail").is_none());
        assert_eq!(calls(root.path()), ["build-info", "grid-write-admission"]);
        assert_eq!(read_at(root.path(), &scope()).unwrap(), before);
        let package = package_path(&scope_dir(root.path(), &scope()).unwrap(), name).unwrap();
        assert_eq!(fs::read(package).unwrap(), bytes);
    }
}
