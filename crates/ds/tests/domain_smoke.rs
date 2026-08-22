//! Semantic smoke: every offline command, against real domain data.
//!
//! The other suites prove the *shape* of the surface — that help matches the
//! descriptor, that codes are documented, that budgets hold. None of them can
//! catch the failure that actually happened twice while this domain was being
//! built:
//!
//! * `ds pls reference-closure` defaulted `--limit` to 50 against a task that
//!   bounds it at 32, so every call refused;
//! * `ds dsgrid-exchange inspect` filtered capabilities by comparing a
//!   `Debug` spelling against `"Available"` — a variant that does not exist —
//!   so a source set with ten ready conversions reported none.
//!
//! Both compiled. Both had correct help, documented refusals and a passing
//! contract suite. What they did not have was anyone running them against a
//! real file and looking at the answer.
//!
//! So each command here is invoked exactly as a caller would, against the
//! fixtures in `ds-network`, and the answer is asserted to be *specifically*
//! right — not merely well-formed. An assertion like "at least one capability
//! is offered" is what makes this suite worth having; an assertion that the
//! response parses is not.

use std::path::PathBuf;
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

/// The real PLS-CADD workspace that ships with `ds-network`.
fn workspace() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-network/fixtures/pls-public/humble-pole/workspace");
    let path = path.canonicalize().unwrap_or(path);
    assert!(
        path.is_dir(),
        "fixture workspace missing at {}",
        path.display()
    );
    path.display().to_string()
}

/// Join a leaf onto the fixture workspace with the platform's own separator.
///
/// Not `format!("{}/{leaf}")`. `workspace()` canonicalizes, and on Windows
/// that returns an extended-length `\\?\C:\…` path — a form the OS passes to
/// the filesystem *without* normalization, so a forward slash in it is an
/// ordinary character rather than a separator. The interpolated version
/// therefore asked for a file whose name literally contained
/// `workspace/structures/…`, and every PLS test failed `source_not_found`
/// against a fixture that was sitting right there.
fn workspace_file(leaf: &str) -> String {
    let mut path = PathBuf::from(workspace());
    for part in leaf.split('/') {
        path.push(part);
    }
    path.display().to_string()
}

fn ok(args: &[&str]) -> Value {
    let run = ds(args);
    assert_eq!(
        run.code,
        0,
        "`ds {}` exited {}: {}{}",
        args.join(" "),
        run.code,
        run.stdout,
        run.stderr
    );
    run.envelope["data"].clone()
}

// ---------------------------------------------------------------------------
// dsgrid
// ---------------------------------------------------------------------------

#[test]
fn dsgrid_validate_finds_the_fixture_sound() {
    let model = common::fixture();
    let data = ok(&["dsgrid", "validate", "--model", &model, "--output", "json"]);

    assert_eq!(
        data["container"]["verified"], true,
        "the fixture package must verify"
    );
    assert_eq!(
        data["model"]["valid"], true,
        "the fixture model must be sound"
    );
    assert_eq!(data["model"]["issue_count"], 0);
    // The container's member count is the manifest's, not a guess.
    assert!(data["container"]["members"].as_u64().unwrap_or(0) > 1);
}

#[test]
fn dsgrid_describe_returns_a_real_engine_catalog() {
    for (kind, expect) in [
        ("operations", "create_alignment"),
        ("commands", "create_alignment"),
        ("projections", "project_plan"),
    ] {
        let data = ok(&["dsgrid", "describe", "--kind", kind, "--output", "json"]);
        let entries = data["entries"].as_array().expect("entries");
        assert!(
            entries.len() > 5,
            "the `{kind}` catalog came back with {} entries; the engine publishes far more",
            entries.len()
        );
        assert!(
            entries.iter().any(|entry| entry["id"] == expect),
            "the `{kind}` catalog does not contain `{expect}`"
        );
        // Every entry must carry an effect. A null here means the field was
        // read under the wrong name.
        for entry in entries {
            assert!(
                entry["effect"].is_string(),
                "`{kind}` entry {} has no effect; the descriptor field was misread",
                entry["id"]
            );
        }
    }

    // And one full descriptor, by id.
    let data = ok(&[
        "dsgrid",
        "describe",
        "--id",
        "create_alignment",
        "--output",
        "json",
    ]);
    assert_eq!(data["descriptor"]["operation_id"], "create_alignment");
    assert!(data["descriptor"]["params"].is_array());
}

#[test]
fn dsgrid_exchange_inspect_classifies_and_offers_real_capabilities() {
    let workspace = workspace();
    let data = ok(&[
        "dsgrid-exchange",
        "inspect",
        "--source",
        &workspace,
        "--output",
        "json",
    ]);

    let sources = data["sources"].as_array().expect("sources");
    assert_eq!(sources.len(), 1, "one directory is one folder source");
    let source = &sources[0];
    assert_eq!(
        source["kind"], "PlsWorkspaceFolder",
        "the engine must recognise this as a PLS-CADD workspace folder"
    );
    assert!(
        source["digest"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:"),
        "every source carries its exact digest"
    );
    assert!(
        source["members"].as_u64().unwrap_or(0) > 5,
        "the workspace folder has many members"
    );
    assert_eq!(
        source["version_evidence"], "PLS-CADD Version 16.81",
        "the engine recovers the workspace's declared version"
    );

    // The assertion that matters. A filter bug made this list empty while
    // every other check still passed.
    let capabilities = data["capabilities"].as_array().expect("capabilities");
    assert!(
        !capabilities.is_empty(),
        "a real PLS-CADD workspace must offer at least one conversion; \
         an empty list here means the capability filter is wrong, not that \
         the engine has nothing to offer"
    );
    for capability in capabilities {
        assert!(
            matches!(
                capability["state"].as_str(),
                Some("ready") | Some("unverified")
            ),
            "an offered capability must be ready or unverified, not {}",
            capability["state"]
        );
    }
}

#[test]
fn dsgrid_exchange_inspect_is_deterministic_over_a_directory() {
    // The engine digests the member list, so directory iteration order must
    // not reach it. Two runs over the same tree must agree byte for byte.
    let workspace = workspace();
    let args = [
        "dsgrid-exchange",
        "inspect",
        "--source",
        &workspace,
        "--output",
        "json",
    ];
    let first = ds(&args);
    let second = ds(&args);
    assert_eq!(first.code, 0, "{}{}", first.stdout, first.stderr);
    assert_eq!(
        first.stdout, second.stdout,
        "two inspections of the same directory disagreed; member order is leaking"
    );
}

// ---------------------------------------------------------------------------
// pls
// ---------------------------------------------------------------------------

#[test]
fn pls_reference_closure_reads_the_real_workspace() {
    let workspace = workspace();
    // Deliberately with no --limit: the default must be inside the task's own
    // bound. A default over that bound made every call refuse.
    let data = ok(&[
        "pls",
        "reference-closure",
        "--workspace",
        &workspace,
        "--output",
        "json",
    ]);

    assert!(
        data["workspace_file_count"].as_u64().unwrap_or(0) > 5,
        "the fixture workspace has many files"
    );
    assert!(data["workspace_bytes"].as_u64().unwrap_or(0) > 1_000);
    assert!(
        data["component_identity_preserved"].is_boolean(),
        "the task's verdict must be present"
    );
}

#[test]
fn pls_pole_capacity_reads_a_real_structure() {
    let structure = workspace_file("structures/hp-m1-strain.012");
    let data = ok(&[
        "pls",
        "pole-capacity",
        "read",
        "--structure",
        &structure,
        "--limit",
        "2",
        "--output",
        "json",
    ]);

    assert_eq!(data["source_leaf"], "hp-m1-strain.012");
    assert!(
        data["source_sha256"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:")
    );
    assert_eq!(data["declared_units"], "SI");
    assert_eq!(data["returned"], 2);
    let items = data["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "--limit must be honoured");
    assert!(
        items[0]["allowable_wind_span"].is_number(),
        "a capacity row must carry its engineering values"
    );
}

#[test]
fn pls_pole_capacity_limit_default_is_inside_the_task_bound() {
    // No --limit at all. The task refuses anything over 64, so a default
    // above that would make the bare command unusable.
    let structure = workspace_file("structures/hp-m1-strain.012");
    let data = ok(&[
        "pls",
        "pole-capacity",
        "read",
        "--structure",
        &structure,
        "--output",
        "json",
    ]);
    assert!(data["returned"].as_u64().is_some());
}

#[test]
fn pls_compare_don_is_digest_pinned_and_reconciles_a_file_with_itself() {
    let don = workspace_file("A Project.don");

    // Unpinned: refused, and the refusal must hand back a usable pin.
    let unpinned = ds(&[
        "pls",
        "compare-don",
        "--baseline",
        &don,
        "--candidate",
        &don,
        "--output",
        "json",
    ]);
    assert_eq!(unpinned.code, 2, "an unpinned comparison must refuse");
    assert_eq!(unpinned.envelope["error"]["code"], "missing_digest_pin");
    let observed = &unpinned.envelope["error"]["detail"]["observed"];
    let digest = observed["baseline"]
        .as_str()
        .expect("the refusal reports the digest");
    assert!(digest.starts_with("sha256:") && digest.len() == 71);
    assert_eq!(
        observed["candidate"], observed["baseline"],
        "same file, same digest"
    );

    // Pinned with what the refusal reported: it must now run, and a file
    // compared with itself must agree at every position.
    let data = ok(&[
        "pls",
        "compare-don",
        "--baseline",
        &don,
        "--candidate",
        &don,
        "--baseline-sha256",
        digest,
        "--candidate-sha256",
        digest,
        "--output",
        "json",
    ]);
    assert_eq!(
        data["differing"], 0,
        "a file compared with itself cannot differ"
    );
    assert_eq!(data["name_equivalent"], 0);
    assert!(
        data["agreeing"].as_u64().unwrap_or(0) > 0,
        "the fixture has structures, so some must agree"
    );
    assert_eq!(
        data["baseline_structure_count"],
        data["candidate_structure_count"]
    );
}

#[test]
fn pls_compare_don_refuses_a_wrong_digest() {
    // The pin is only worth having if it is actually checked.
    let don = workspace_file("A Project.don");
    let wrong = format!("sha256:{}", "0".repeat(64));
    let run = ds(&[
        "pls",
        "compare-don",
        "--baseline",
        &don,
        "--candidate",
        &don,
        "--baseline-sha256",
        &wrong,
        "--candidate-sha256",
        &wrong,
        "--output",
        "json",
    ]);
    assert_ne!(run.code, 0, "a wrong digest must not be accepted");
    assert_eq!(run.envelope["error"]["code"], "task_refused");
}

#[test]
fn pls_section_orientation_publishes_the_task_schema() {
    let data = ok(&["pls", "section-orientation", "--schema", "--output", "json"]);
    let schema = &data["schema"];
    assert_eq!(schema["type"], "object");
    let properties = schema["properties"].as_object().expect("schema properties");
    for required in ["source_path", "block_index", "section_number", "alignment"] {
        assert!(
            properties.contains_key(required),
            "the published schema is missing `{required}`"
        );
    }
}

// ---------------------------------------------------------------------------
// Cross-cutting
// ---------------------------------------------------------------------------

#[test]
fn every_offline_command_is_available_without_any_engine_binary() {
    // The `dsgrid`, `dsgrid-exchange` and `pls` domains link their engine, so they must work on
    // a machine with no sidecar installed at all — that is the difference
    // between a linked crate and a called binary, and it should be visible in
    // `doctor`.
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["doctor", "--all", "--output", "json"])
        .env("NO_COLOR", "1")
        .env_remove("DS_REPORT_BIN")
        .env_remove("DS_SOLAR_BIN")
        .env("PATH", "/nonexistent")
        .output()
        .expect("ds runs");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("doctor emits JSON");

    for command in envelope["data"]["commands"].as_array().expect("commands") {
        let id = command["id"].as_str().unwrap_or("");
        if id.starts_with("dsgrid.") || id.starts_with("pls.") {
            assert_eq!(
                command["availability"], "available",
                "`{id}` links its engine and must not depend on an installed binary"
            );
        }
    }
}
