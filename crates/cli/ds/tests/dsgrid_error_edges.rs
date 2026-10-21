//! Error edges of the MV file CLI: one class and code for a corrupt package on
//! every door, no false clean answers, malformed pins refused as input, and
//! stale pins named for what they are. Run with `DS_NETWORK_ENGINE_BIN`
//! naming a compatible engine, as the other restored Grid CLI tests do.

use serde_json::{Value, json};
use std::path::Path;

mod common;

fn run(args: &[&str]) -> (Value, i32) {
    common::json(args)
}

fn refused(args: &[&str]) -> Value {
    let (envelope, code) = run(args);
    assert_ne!(code, 0, "{envelope}");
    assert_eq!(envelope["status"], "error", "{envelope}");
    envelope["error"].clone()
}

fn ok(args: &[&str]) -> Value {
    let (envelope, code) = run(args);
    assert_eq!(code, 0, "{envelope}");
    envelope["data"].clone()
}

fn write(directory: &Path, name: &str, bytes: &[u8]) -> String {
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.display().to_string()
}

fn params(directory: &Path, name: &str, value: Value) -> String {
    write(directory, name, &serde_json::to_vec(&value).unwrap())
}

/// A real model, plus the three corrupt files a user can hand a command.
struct Files {
    _directory: tempfile::TempDir,
    dir: std::path::PathBuf,
    model: String,
    empty: String,
    truncated: String,
    flipped: String,
}

fn files() -> Files {
    let model = common::fixture();
    let bytes = std::fs::read(&model).unwrap();
    let directory = tempfile::tempdir_in(std::env::var_os("TMPDIR").unwrap_or_else(|| "/var/tmp".into()))
        .unwrap();
    let dir = directory.path().to_path_buf();
    let empty = write(&dir, "empty.dsgrid", b"");
    let truncated = write(&dir, "trunc.dsgrid", &bytes[..bytes.len() / 2]);
    let mut flip = bytes.clone();
    let at = flip.len() / 3;
    flip[at] ^= 0xff;
    let flipped = write(&dir, "flip.dsgrid", &flip);
    Files {
        _directory: directory,
        dir,
        model,
        empty,
        truncated,
        flipped,
    }
}

fn assert_corrupt(error: &Value, context: &str) {
    assert_eq!(error["class"], "invalid_input", "{context}: {error}");
    assert_eq!(error["code"], "package_decode_failed", "{context}: {error}");
    assert_eq!(error["retryable"], false, "{context}: {error}");
    let hints = error["next"].to_string();
    assert!(
        !hints.contains("feedback"),
        "{context}: a corrupt user file is not a Core defect: {error}"
    );
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn a_corrupt_package_is_the_same_invalid_input_on_every_door() {
    let f = files();
    let ops = params(&f.dir, "p.json", json!({}));
    for (name, path) in [("empty", &f.empty), ("truncated", &f.truncated)] {
        assert_corrupt(
            &refused(&["dsgrid", "inspect", "--model", path, "--output", "json"]),
            &format!("inspect {name}"),
        );
        assert_corrupt(
            &refused(&["dsgrid", "validate", "--model", path, "--output", "json"]),
            &format!("validate {name}"),
        );
        assert_corrupt(
            &refused(&[
                "dsgrid", "run", "--model", path, "--operation", "project_plan", "--params", &ops,
                "--output", "json",
            ]),
            &format!("run {name}"),
        );
        assert_corrupt(
            &refused(&[
                "dsgrid", "report", "structures", "--package", path, "--output", "json",
            ]),
            &format!("report structures {name}"),
        );
    }
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn validate_refuses_a_flipped_byte_but_still_exits_zero_for_model_issues() {
    let f = files();
    assert_corrupt(
        &refused(&["dsgrid", "validate", "--model", &f.flipped, "--output", "json"]),
        "validate flipped",
    );
    let readable = ok(&["dsgrid", "validate", "--model", &f.model, "--output", "json"]);
    assert_eq!(readable["container"]["verified"], true);
    assert!(readable["model"]["valid"].is_boolean());
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn inspect_says_loudly_when_it_did_not_read_the_tables() {
    let f = files();
    let manifest_only = ok(&["dsgrid", "inspect", "--model", &f.model, "--output", "json"]);
    assert_eq!(manifest_only["decoded"], false);
    assert_eq!(manifest_only["tables_verified"], false);
    assert!(
        manifest_only["help"]
            .as_str()
            .is_some_and(|help| help.contains("ds dsgrid validate")),
        "{manifest_only}"
    );
    let decoded = ok(&[
        "dsgrid", "inspect", "--model", &f.model, "--include", "authored-revision", "--output",
        "json",
    ]);
    assert_eq!(decoded["tables_verified"], true);
    assert!(decoded.get("help").is_none(), "{decoded}");
    // A flipped byte never reads as a verified model: either the manifest
    // read refuses it, or the answer says its tables were not verified, and
    // decoding them refuses.
    let (flipped, code) = run(&["dsgrid", "inspect", "--model", &f.flipped, "--output", "json"]);
    if code == 0 {
        assert_eq!(flipped["data"]["tables_verified"], false, "{flipped}");
    } else {
        assert_corrupt(&flipped["error"], "inspect flipped");
    }
    assert_corrupt(
        &refused(&[
            "dsgrid", "inspect", "--model", &f.flipped, "--include", "authored-revision",
            "--output", "json",
        ]),
        "inspect flipped decoded",
    );
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn a_nonpositive_clearance_corridor_is_params_invalid_not_a_clean_report() {
    let f = files();
    for width in [json!(-5), json!(0)] {
        let p = params(&f.dir, "c.json", json!({ "corridor_half_width_m": width }));
        let error = refused(&[
            "dsgrid", "run", "--model", &f.model, "--operation", "clearance_report", "--params",
            &p, "--output", "json",
        ]);
        assert_eq!(error["class"], "invalid_input", "{error}");
        assert_eq!(error["code"], "params_invalid", "{error}");
    }
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn a_malformed_revision_is_input_and_a_stale_one_is_not_retryable() {
    let f = files();
    let head = ok(&[
        "dsgrid", "inspect", "--model", &f.model, "--include", "authored-revision", "--output",
        "json",
    ])["authored_revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = f.dir.join("out.dsgrid");
    let out = out.to_str().unwrap();
    let retype = |revision: &str| {
        refused(&[
            "dsgrid", "structure", "retype", "--package", &f.model, "--out", out, "--structure",
            "1", "--type", "x", "--revision", revision, "--dry-run", "--output", "json",
        ])
    };
    for malformed in ["nonsense", "rev:", "rev:xyz", "REV:00"] {
        let error = retype(malformed);
        assert_eq!(error["class"], "invalid_input", "{malformed}: {error}");
        assert_eq!(error["code"], "revision_invalid", "{malformed}: {error}");
    }
    let stale = retype(&format!("rev:{}", "0".repeat(64)));
    assert_eq!(stale["code"], "revision_conflict", "{stale}");
    assert_eq!(stale["class"], "conflict", "{stale}");
    assert_eq!(stale["retryable"], false, "{stale}");
    // The real head passes the pin check (it may still refuse the made-up type).
    let real = retype(&head);
    assert_ne!(real["code"], "revision_invalid", "{real}");
    assert_ne!(real["code"], "revision_conflict", "{real}");
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn a_stale_engineering_root_surfaces_under_its_own_code_with_a_reread_remedy() {
    let f = files();
    let criteria = ok(&[
        "dsgrid", "run", "--model", &f.model, "--operation", "project_criteria_workbench",
        "--output", "json",
    ]);
    let p = params(
        &f.dir,
        "a.json",
        json!({ "request": {
            "expected_revision": criteria["result"]["model_revision"],
            "expected_engineering_input_root": "stale-root",
            "max_rows": 2,
        }}),
    );
    let error = refused(&[
        "dsgrid", "run", "--model", &f.model, "--operation", "analyze_model_defaults", "--params",
        &p, "--output", "json",
    ]);
    assert_eq!(error["code"], "engineering_input_root_mismatch", "{error}");
    assert_eq!(error["class"], "conflict", "{error}");
    assert_eq!(error["retryable"], false, "{error}");
    assert!(error["remedy"].as_str().unwrap().contains("re-read"), "{error}");
    assert_eq!(
        error["detail"]["refusal"]["code"], "engineering_input_root_mismatch",
        "{error}"
    );
    assert!(
        !error["next"].to_string().contains("feedback"),
        "a stale pin is not a defect: {error}"
    );
}

fn first_structure(model: &str) -> (String, Value, Value) {
    let report = ok(&["dsgrid", "report", "structures", "--package", model, "--output", "json"]);
    let row = report["rows"][0].clone();
    (
        row["structure_id"].as_str().unwrap().to_owned(),
        row["x_m"].clone(),
        row["y_m"].clone(),
    )
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn an_envelope_with_a_bad_orientation_or_schema_version_is_refused() {
    let f = files();
    let head = ok(&[
        "dsgrid", "inspect", "--model", &f.model, "--include", "authored-revision", "--output",
        "json",
    ])["authored_revision"]
        .clone();
    let (id, x, y) = first_structure(&f.model);
    let envelope = |name: &str, version: u64, orientation: f64| {
        params(
            &f.dir,
            name,
            json!({
                "command_id": name, "command_schema_version": version,
                "expected_revision": head,
                "command": { "command_kind": "move_structure", "id": id, "x_m": x, "y_m": y,
                             "z_m": 0.0, "orientation_rad": orientation },
            }),
        )
    };
    let apply = |path: &str| {
        refused(&[
            "dsgrid", "apply", "--model", &f.model, "--envelope", path, "--dry-run", "--output",
            "json",
        ])
    };
    for (name, orientation) in [("o99", 99.0), ("oneg", -3.0)] {
        let error = apply(&envelope(name, 11, orientation));
        assert_eq!(error["code"], "command_invalid", "{name}: {error}");
        assert_eq!(error["class"], "invalid_input", "{name}: {error}");
    }
    let error = apply(&envelope("v999", 999, 0.0));
    assert_eq!(error["code"], "envelope_invalid", "{error}");
    assert_eq!(error["class"], "invalid_input", "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("command_schema_version 999"),
        "{error}"
    );
}
