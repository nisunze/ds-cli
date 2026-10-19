//! `ds dsgrid inspect` through the Network process port (DECOUPLING-003):
//! the end-to-end tests the linked command carried in `cli.rs` before the
//! 2026-10-17 retirement, unchanged. They need an engine that advertises the
//! restored Grid file operations, selected with `DS_NETWORK_ENGINE_BIN`.

use std::process::Command;

use serde_json::Value;

mod common;

struct Run {
    envelope: Value,
    stdout: String,
    stderr: String,
    code: i32,
}

fn ds(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_SRE_REPORTING", "off")
        .output()
        .expect("ds binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Run {
        envelope: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn inspect_reports_the_engine_s_own_identity() {
    let model = common::fixture();
    let run = ds(&["dsgrid", "inspect", "--model", &model, "--output", "json"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let data = &run.envelope["data"];

    // These are the fixture's real values, produced by ds-grid-exchange. They
    // are asserted verbatim so a change in the engine's answer is a visible
    // test failure rather than a silent change in what `ds` reports.
    assert_eq!(data["format"], "dsgrid");
    assert_eq!(data["model"]["crs"], "EPSG:32735");
    assert_eq!(data["model"]["schema_version"], 5);
    assert_eq!(data["model"]["format_version"], 1);
    // Canonical ds-network granular-tag schema regenerated this self-authored fixture.
    // Regenerated again on 2026-09-20 when the FEA member became engineering
    // (program contract 03): the fixture carries its feature-code definition
    // and its survey points resolve by number.
    // Native axis normalization and the spotting-warnings schema re-emitted
    // the fixture on 2026-09-21 and 2026-09-22, respectively; on 2026-09-26
    // its cables were re-read in the units PLS-CADD's Cable Data Report prints.
    // On 2026-10-04 ds-network re-emitted it at strict schema 4 (all 47 current
    // tables; ds-network 5b86e73e), which moved its fingerprint.
    // The test-local native schema-5 migration now preserves those source
    // identities while adding the explicit ground-profile basis table. The
    // committed predecessor remains unchanged and its receipt is retained.
    // Keep the source fingerprint and its derived model id coupled: changing
    // one while leaving the other stale would conceal an identity drift.
    const HUMBLE_FINGERPRINT: &str = "fnv1a64:fc2cb1fa44bb0d96";
    assert_eq!(data["model"]["fingerprint"], HUMBLE_FINGERPRINT);
    assert_eq!(
        data["model"]["id"],
        format!(
            "pls-import-fnv1a64:{}",
            HUMBLE_FINGERPRINT
                .strip_prefix("fnv1a64:")
                .expect("fixture fingerprint prefix")
                .get(..8)
                .expect("fingerprint has the id digest prefix")
        )
    );

    // The default answer decodes nothing. This is the cost contract, not a
    // detail: it is why calling inspect first is cheap.
    assert_eq!(data["decoded"], false);
    assert!(
        data.get("tables").is_none(),
        "default answer carried a projection"
    );
    assert!(
        data.get("library").is_none(),
        "default answer carried a projection"
    );
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn projections_are_opt_in_and_report_their_cost() {
    let model = common::fixture();

    let manifest_only = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "tables",
        "--output",
        "json",
    ]);
    assert_eq!(manifest_only.code, 0);
    assert_eq!(
        manifest_only.envelope["data"]["decoded"], false,
        "the tables projection is answerable from the manifest and must not decode"
    );
    assert!(manifest_only.envelope["data"]["tables"].is_object());

    let decoded = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "library",
        "--output",
        "json",
    ]);
    assert_eq!(decoded.code, 0);
    assert_eq!(
        decoded.envelope["data"]["decoded"], true,
        "the library projection needs the tables and must say so"
    );
    let library = &decoded.envelope["data"]["library"];
    assert!(
        !library["structure_types"]
            .as_array()
            .expect("structure types")
            .is_empty()
    );
    assert!(!library["cables"].as_array().expect("cables").is_empty());
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn truncation_is_always_visible() {
    // A silently shortened list reads as a complete one. Ask for a limit
    // below the real count and the response must say what it withheld.
    let model = common::fixture();
    let run = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "tables",
        "--limit",
        "2",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 0);
    let tables = run.envelope["data"]["tables"].as_object().expect("tables");
    assert_eq!(tables.len(), 2, "limit was not applied");

    let truncated = run.envelope["data"]["more"]["truncated"]
        .as_array()
        .expect("truncation must be reported");
    let entry = truncated.iter().find(|entry| entry["field"] == "tables");
    let entry = entry.expect("the truncated tables projection must be named");
    assert!(entry["withheld"].as_u64().expect("withheld count") > 0);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn continuation_names_what_else_exists() {
    // The response tells a caller what it did not ask for, so discovering the
    // rest never costs another round-trip through help.
    let model = common::fixture();
    let run = ds(&["dsgrid", "inspect", "--model", &model, "--output", "json"]);
    let available = run.envelope["data"]["more"]["available_projections"]
        .as_array()
        .expect("unrequested projections are named");
    let names: Vec<&str> = available.iter().filter_map(Value::as_str).collect();
    assert!(names.contains(&"tables"));
    assert!(names.contains(&"library"));
}
