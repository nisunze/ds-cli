//! `ds dsgrid-exchange inspect|plan|convert` restored through the Network
//! addon's closed exchange calls (DECOUPLING-003): discoverable in the Grid
//! model chapter, refused before staging on a missing or older addon, and —
//! with an addon advertising `grid_exchange_operations_v1`, selected by
//! `DS_NETWORK_ENGINE_BIN` — the linked commands' answers and files.
//! `sync` stays retired.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

const RESTORED: &[&str] = &[
    "dsgrid-exchange.inspect",
    "dsgrid-exchange.plan",
    "dsgrid-exchange.convert",
];

fn ds(args: &[&str], engine: Option<&Path>, home: &Path) -> (bool, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_SRE_REPORTING", "off")
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

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-network/fixtures/pls-public/humble-pole")
        .join(name)
}

/// The compatible addon the caller selected, or `None` when the test runs
/// without one (it is then `#[ignore]`d unless asked for).
fn selected_engine() -> PathBuf {
    PathBuf::from(
        std::env::var_os("DS_NETWORK_ENGINE_BIN")
            .expect("DS_NETWORK_ENGINE_BIN names a compatible Network executable"),
    )
}

/// A protocol-only executable that answers `build-info` as a Network release
/// with the Grid file operations but without the exchange calls, and records
/// every invocation.
fn older_engine(dir: &Path) -> (PathBuf, PathBuf) {
    let log = dir.join("calls.log");
    let engine = dir.join("ds-network");
    let script = format!(
        r#"#!/bin/sh
echo "$1" >> '{log}'
if [ "$1" = build-info ]; then
  printf '{{"schema":"ds.engine-build/v1","module":"network","engineIdentity":{{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"older-fixture","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{{"environment":"native","logicalCpus":%s,"internalWorkerThreads":%s}},"jsonObjectOrder":"sorted","capabilities":["grid_read_v1","grid_inspect_v1","grid_file_operations_v1"]}}}}\n' "$3" "$3"
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
fn restored_exchange_commands_are_discoverable_and_sync_stays_retired() {
    let home = tempfile::tempdir().unwrap();
    for id in RESTORED {
        let (success, descriptor) =
            ds(&["capabilities", id, "--output", "json"], None, home.path());
        assert!(success, "{id}: {descriptor}");
        let command = &descriptor["data"]["command"];
        assert_eq!(command["id"], *id);
        assert_eq!(command["chapter"], "grid-model", "{id}");
        let refusals: Vec<&str> = command["refusals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|refusal| refusal["code"].as_str().unwrap())
            .collect();
        for code in ["source_not_found", "network_engine_incompatible", "network_engine_missing"] {
            assert!(refusals.contains(&code), "{id} declares {code}");
        }
    }
    let (success, refused) = ds(
        &["dsgrid-exchange", "sync", "--output", "json"],
        None,
        home.path(),
    );
    assert!(!success, "{refused}");
    assert_eq!(refused["error"]["code"], "unknown_command", "{refused}");
}

#[cfg(unix)]
#[test]
fn an_addon_without_the_exchange_calls_is_only_identified() {
    let home = tempfile::tempdir().unwrap();
    let (engine, log) = older_engine(home.path());
    let source = fixture("humble-pole.dsgrid");
    let source = source.to_str().unwrap();
    let out = home.path().join("out");
    let out_arg = out.to_str().unwrap().to_owned();
    for args in [
        vec!["dsgrid-exchange", "inspect", "--source", source],
        vec!["dsgrid-exchange", "plan", "--source", source, "--target", "kmz"],
        vec![
            "dsgrid-exchange",
            "convert",
            "--source",
            source,
            "--target",
            "kmz",
            "--out",
            &out_arg,
        ],
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
fn a_missing_addon_refuses_and_a_missing_source_refuses_first() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("absent").join("ds-network");
    let source = fixture("humble-pole.dsgrid");
    let (success, refused) = ds(
        &[
            "dsgrid-exchange",
            "inspect",
            "--source",
            source.to_str().unwrap(),
            "--output",
            "json",
        ],
        Some(&missing),
        home.path(),
    );
    assert!(!success, "{refused}");
    let code = refused["error"]["code"].as_str().unwrap();
    assert!(
        matches!(code, "network_engine_missing" | "network_engine_path_invalid"),
        "{refused}"
    );
    let absent = home.path().join("absent.dsgrid");
    let (success, refused) = ds(
        &[
            "dsgrid-exchange",
            "inspect",
            "--source",
            absent.to_str().unwrap(),
            "--output",
            "json",
        ],
        Some(&missing),
        home.path(),
    );
    assert!(!success, "{refused}");
    assert_eq!(refused["error"]["code"], "source_not_found", "{refused}");
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn inspect_plan_and_convert_run_in_the_addon_and_write_verified_outputs() {
    let engine = selected_engine();
    let home = tempfile::tempdir().unwrap();
    let source = fixture("humble-pole.dsgrid");
    let source_arg = source.to_str().unwrap();
    let before = std::fs::read(&source).unwrap();

    let (success, inspected) = ds(
        &["dsgrid-exchange", "inspect", "--source", source_arg, "--output", "json"],
        Some(&engine),
        home.path(),
    );
    assert!(success, "{inspected}");
    let data = &inspected["data"];
    assert_eq!(data["byte_len"], before.len());
    assert_eq!(data["sources"][0]["name"], "humble-pole.dsgrid");
    assert!(
        data["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .all(|capability| matches!(
                capability["state"].as_str(),
                Some("ready" | "unverified")
            ))
    );

    let (success, planned) = ds(
        &[
            "dsgrid-exchange",
            "plan",
            "--source",
            source_arg,
            "--target",
            "kmz",
            "--include-layer",
            "tension_sections",
            "--output",
            "json",
        ],
        Some(&engine),
        home.path(),
    );
    assert!(success, "{planned}");
    let plan = &planned["data"];
    assert_eq!(plan["executable"], true, "{plan}");
    assert_eq!(plan["include_layers"], serde_json::json!(["tension_sections"]));
    let plan_id = plan["plan_id"].as_str().unwrap().to_owned();

    let out = home.path().join("out");
    let out_arg = out.to_str().unwrap().to_owned();
    let convert = [
        "dsgrid-exchange",
        "convert",
        "--source",
        source_arg,
        "--target",
        "kmz",
        "--include-layer",
        "tension_sections",
        "--out",
        &out_arg,
        "--output",
        "json",
    ];
    let (success, converted) = ds(&convert, Some(&engine), home.path());
    assert!(success, "{converted}");
    let data = &converted["data"];
    assert_eq!(data["plan_id"], plan_id.as_str());
    assert_eq!(data["status"], "completed");
    let written = data["written"].as_array().unwrap();
    assert_eq!(data["written_count"], written.len());
    assert!(!written.is_empty());
    for row in written {
        let bytes = std::fs::read(out.join(row["path"].as_str().unwrap())).unwrap();
        assert_eq!(row["sha256"], sha256(&bytes));
        assert_eq!(row["byte_len"], bytes.len());
    }
    let report: Value =
        serde_json::from_slice(&std::fs::read(out.join("exchange-report.json")).unwrap()).unwrap();
    assert_eq!(report["plan"]["plan_id"], plan_id.as_str());
    assert_eq!(report["sources"][0]["path"], source_arg);
    assert_eq!(report["status"], "completed");

    // Never overwrites: the same conversion into the same directory refuses
    // and leaves the first run's evidence as it was.
    let kept = std::fs::read(out.join("exchange-report.json")).unwrap();
    let (success, refused) = ds(&convert, Some(&engine), home.path());
    assert!(!success, "{refused}");
    assert_eq!(refused["error"]["code"], "output_exists", "{refused}");
    assert_eq!(std::fs::read(out.join("exchange-report.json")).unwrap(), kept);
    assert_eq!(std::fs::read(&source).unwrap(), before, "the source is read only");
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn owner_refusals_cross_the_port_unchanged() {
    let engine = selected_engine();
    let home = tempfile::tempdir().unwrap();
    let source = fixture("humble-pole.dsgrid");
    let source_arg = source.to_str().unwrap();
    for (extra, code) in [
        (vec!["--expect-lon", "30"], "incomplete_expected_location"),
        (
            vec!["--network-source-layer", "sources"],
            "incomplete_gis_network_selection",
        ),
        (
            vec!["--select-project", "absent.don"],
            "project_not_resolvable",
        ),
    ] {
        let mut args = vec![
            "dsgrid-exchange",
            "plan",
            "--source",
            source_arg,
            "--target",
            "dsgrid",
        ];
        args.extend(extra);
        args.extend(["--output", "json"]);
        let (success, refused) = ds(&args, Some(&engine), home.path());
        assert!(!success, "{args:?}: {refused}");
        assert_eq!(refused["error"]["code"], code, "{args:?}: {refused}");
    }
}
