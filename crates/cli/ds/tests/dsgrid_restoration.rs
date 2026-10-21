//! DS Grid file commands restored through the captured Network addon
//! (DECOUPLING-003): discoverable in the Grid model chapter, engine work
//! refuses before staging on a missing or older addon, and Core-only
//! catalogue reads need no addon at all.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const RESTORED: &[&str] = &[
    "dsgrid.package.migrate",
    "dsgrid.project.list",
    "dsgrid.project.show",
    "dsgrid.project.versions",
    "dsgrid.project.download",
    "dsgrid.project.exports.list",
    "dsgrid.project.exports.publish",
    "dsgrid.project.exports.download",
    "dsgrid.project.bump-version",
    "dsgrid.project.update",
    "dsgrid.project.set-approval",
    "dsgrid.project.backup.download",
    "dsgrid.project.retire",
    "dsgrid.project.restore",
    "dsgrid.create",
    "dsgrid.inspect",
    "dsgrid.validate",
    "dsgrid.describe",
    "dsgrid.run",
    "dsgrid.spotting.preview-receipt",
    "dsgrid.spotting.apply-receipt",
    "dsgrid.apply",
    "dsgrid.apply-batch",
    "dsgrid.apply-correction",
    "dsgrid.profile.open",
    "dsgrid.model.list",
    "dsgrid.model.show",
    "dsgrid.model.create-local",
    "dsgrid.model.import-external",
    "dsgrid.model.link",
    "dsgrid.model.unlink",
    "dsgrid.model.set-active",
    "dsgrid.model.forget",
    "dsgrid.model.prepare-project",
    "dsgrid.structure.describe",
    "dsgrid.structure.retype",
    "dsgrid.report.structures",
    "dsgrid.report.staking",
    "dsgrid.feature-codes.report",
    "dsgrid.criteria.show",
    "dsgrid.criteria.clearance.set",
    "dsgrid.analyse.clearance",
];

fn ds(args: &[&str], engine: Option<&Path>, home: &Path) -> (bool, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_LAYER_HOME", home)
        .env("TMPDIR", home);
    match engine {
        Some(engine) => command.env("DS_NETWORK_ENGINE_BIN", engine),
        None => command.env_remove("DS_NETWORK_ENGINE_BIN"),
    };
    let output = command.output().unwrap();
    let envelope = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: {error}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), envelope)
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid")
}

/// A protocol-only executable that answers `build-info` as an older Network
/// release without the Grid file operations, and records every invocation.
fn older_engine(dir: &Path) -> (PathBuf, PathBuf) {
    let log = dir.join("calls.log");
    let engine = dir.join("ds-network");
    let script = format!(
        r#"#!/bin/sh
echo "$1" >> '{log}'
if [ "$1" = build-info ]; then
  printf '{{"schema":"ds.engine-build/v1","module":"network","engineIdentity":{{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"older-fixture","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{{"environment":"native","logicalCpus":%s,"internalWorkerThreads":%s}},"jsonObjectOrder":"sorted","capabilities":["lv_processing_v1","grid_read_v1","grid_inspect_v1","grid_manifest_v1","grid_query_v1","grid_apply_v1","grid_apply_batch_v1","grid_write_admission_v1"]}}}}\n' "$3" "$3"
  exit 0
fi
exit 70
"#,
        log = log.display()
    );
    std::fs::write(&engine, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (engine, log)
}

#[test]
fn restored_commands_are_discoverable_in_the_grid_model_chapter() {
    let home = tempfile::tempdir().unwrap();
    for id in RESTORED {
        let (success, descriptor) =
            ds(&["capabilities", id, "--output", "json"], None, home.path());
        assert!(success, "{id}: {descriptor}");
        let command = &descriptor["data"]["command"];
        assert_eq!(command["id"], *id);
        assert_eq!(command["chapter"], "grid-model", "{id}");
    }
}

#[cfg(unix)]
#[test]
fn engine_work_refuses_on_an_older_addon_before_any_task() {
    let home = tempfile::tempdir().unwrap();
    let (engine, log) = older_engine(home.path());
    let out = home.path().join("described.dsgrid");
    let fixture = fixture();
    let fixture = fixture.to_str().unwrap();
    for args in [
        vec![
            "dsgrid",
            "structure",
            "describe",
            "--package",
            fixture,
            "--structure",
            "1",
            "--text",
            "restored",
            "--dry-run",
        ],
        vec!["dsgrid", "criteria", "show", "--package", fixture],
        vec!["dsgrid", "report", "structures", "--package", fixture],
    ] {
        let mut args = args;
        args.extend(["--output", "json"]);
        let (success, refused) = ds(&args, Some(&engine), home.path());
        assert!(!success, "{args:?}: {refused}");
        assert_eq!(
            refused["error"]["code"], "network_engine_incompatible",
            "{args:?}: {refused}"
        );
    }
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.lines().all(|line| line == "build-info"),
        "an older addon is only identified, never asked to work: {calls}"
    );
    assert!(!out.exists());
}

#[test]
fn engine_work_refuses_when_the_addon_is_missing() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("absent").join("ds-network");
    let fixture = fixture();
    let (success, refused) = ds(
        &[
            "dsgrid",
            "validate",
            "--model",
            fixture.to_str().unwrap(),
            "--output",
            "json",
        ],
        Some(&missing),
        home.path(),
    );
    assert!(!success, "{refused}");
    let code = refused["error"]["code"].as_str().unwrap();
    assert!(
        matches!(
            code,
            "network_engine_missing" | "network_engine_path_invalid"
        ),
        "{refused}"
    );
}

#[test]
fn the_working_copy_catalogue_needs_no_addon() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("absent").join("ds-network");
    let (success, listed) = ds(
        &[
            "dsgrid",
            "model",
            "list",
            "--lane",
            "canary",
            "--account",
            "uid-1",
            "--output",
            "json",
        ],
        Some(&missing),
        home.path(),
    );
    assert!(success, "{listed}");
    assert_eq!(listed["data"]["models"].as_array().map(Vec::len), Some(0));
}
