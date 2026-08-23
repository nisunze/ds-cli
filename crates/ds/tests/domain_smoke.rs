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

// ---------------------------------------------------------------------------
// map
// ---------------------------------------------------------------------------
//
// The map lives inside a running application, so there is no fixture that can
// stand in for it: an assertion about what a layer looks like on screen needs
// the desktop, and the desktop is not present on CI.
//
// What *is* assertable everywhere, and is where this domain's real bugs would
// live, is the input contract. Every handler here validates its own flags
// before it opens the bridge, so a malformed invocation must refuse with the
// same typed code whether or not an application is running. That is not a
// shape test: it is the ordering claim the whole domain rests on, and it is
// exactly what would break if a handler were rewritten to resolve the paired
// session first — after which none of these codes would ever be seen by
// anyone without the desktop installed.

/// A features file holding one line, for the geometry checks.
fn line_geojson() -> String {
    let path = std::env::temp_dir().join("ds-map-smoke-line.geojson");
    std::fs::write(
        &path,
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"a"},
             "geometry":{"type":"LineString","coordinates":[[30.0,-1.9],[30.1,-1.95]]}}]}"#,
    )
    .expect("temp geojson is writable");
    path.display().to_string()
}

/// The refusal code a call came back with, or "" if it succeeded.
fn refusal(args: &[&str]) -> String {
    let run = ds(args);
    if run.envelope["status"] == "ok" {
        return String::new();
    }
    run.envelope["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// The pairing states a well-formed map call may legitimately end in. Which
/// one depends on the machine; that it is one of these does not.
const PAIRING_CODES: &[&str] = &[
    "desktop_not_paired",
    "desktop_ambiguous",
    "desktop_unreachable",
    "desktop_unreadable",
    "desktop_refused",
    "desktop_operation_unsupported",
    "desktop_signed_out",
    "pairing_rejected",
];

#[test]
fn map_validates_its_own_inputs_before_it_opens_the_bridge() {
    // Each of these is malformed in exactly one way, and the expected code is
    // the one that names that way — never a pairing code. A handler that
    // resolved the desktop first would return `desktop_not_paired` for every
    // row here on any machine without the application.
    let cases: &[(&[&str], &str)] = &[
        // A transposed box is the commonest way `zoom` goes wrong, and the
        // application would refuse it too — one round trip later.
        (
            &["map", "zoom", "--bbox", "30.2,-1.85,29.9,-2.1"],
            "invalid_bbox",
        ),
        (&["map", "zoom", "--bbox", "29.9,-2.1,30.2"], "invalid_bbox"),
        (
            &[
                "map",
                "zoom",
                "--bbox",
                "29.9,-2.1,30.2,-1.85",
                "--padding",
                "900",
            ],
            "invalid_number",
        ),
        (&["map", "view", "--limit", "0"], "invalid_number"),
        (
            &[
                "map",
                "points-along",
                "--layer",
                "sketch:x",
                "--interval-m",
                "0",
            ],
            "invalid_number",
        ),
        (
            &[
                "map",
                "outliers",
                "--layer",
                "sketch:x",
                "--threshold",
                "40",
            ],
            "invalid_number",
        ),
        (
            &["map", "outliers", "--layer", "sketch:x", "--limit", "500"],
            "invalid_number",
        ),
        (
            &[
                "map",
                "random-points",
                "--layer",
                "sketch:x",
                "--min-spacing-m",
                "0",
            ],
            "invalid_number",
        ),
        (
            &[
                "map",
                "draw",
                "--name",
                "n",
                "--geometry",
                "Point",
                "--features",
                "/definitely/not/here.geojson",
            ],
            "features_not_found",
        ),
        (
            &[
                "map",
                "design",
                "select",
                "--transformer",
                "T",
                "--where",
                "nope",
            ],
            "invalid_pair",
        ),
        (
            &[
                "map",
                "design",
                "select",
                "--transformer",
                "T",
                "--bbox",
                "1,2,3",
            ],
            "invalid_bbox",
        ),
        (
            &[
                "map",
                "design",
                "process",
                "--transformer",
                "T",
                "--differential-where",
                "nope",
            ],
            "invalid_pair",
        ),
    ];

    for (args, expected) in cases {
        let mut argv: Vec<&str> = args.to_vec();
        argv.extend(["--output", "json"]);
        let code = refusal(&argv);
        assert_eq!(
            &code,
            expected,
            "`ds {}` refused with `{code}`, not `{expected}`. Either the \
             validation moved after the bridge call, or the code changed.",
            args.join(" ")
        );
    }

    // The geometry check reads the caller's own file and names the feature
    // that is wrong — the answer the application cannot give, because by the
    // time it looks the file is a payload.
    let line = line_geojson();
    let code = refusal(&[
        "map",
        "draw",
        "--name",
        "n",
        "--geometry",
        "Polygon",
        "--features",
        &line,
        "--output",
        "json",
    ]);
    assert_eq!(
        code, "geometry_mismatch",
        "a LineString file declared as a Polygon layer must be refused locally"
    );
}

#[test]
fn map_draw_names_the_feature_whose_geometry_is_wrong() {
    // "The response parses" is not a test. This asserts the refusal carries
    // the index of the offending feature and the types actually found, which
    // is the difference between fixing a file and re-exporting it blind.
    let line = line_geojson();
    let run = ds(&[
        "map",
        "draw",
        "--name",
        "n",
        "--geometry",
        "Point",
        "--features",
        &line,
        "--output",
        "json",
    ]);
    assert_eq!(run.envelope["error"]["code"], "geometry_mismatch");
    let detail = &run.envelope["error"]["detail"];
    assert_eq!(detail["index"], 0, "the refusal must name which feature");
    assert_eq!(detail["declared"], "Point");
    assert_eq!(
        detail["found"],
        serde_json::json!(["LineString"]),
        "the refusal must report the geometry the file actually holds"
    );
    assert_eq!(run.code, 2, "a malformed input is exit class 2");
}

#[test]
fn map_design_set_refuses_an_edit_with_nothing_to_write() {
    // `--set` is declared required, so the parser catches an absent one; this
    // is the other way to arrive with nothing, and it must not reach the
    // bridge and stage an empty change.
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--set",
            "=value",
            "--output",
            "json",
        ]),
        "invalid_pair"
    );
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--output",
            "json"
        ]),
        "missing_input",
        "--set is required and the parser must say so by name"
    );
}

#[test]
fn map_design_save_cannot_run_without_confirmation() {
    // The only command in the domain that writes to the project. The gate is
    // in dispatch, before the handler, so this holds no matter what the
    // handler does — and it must hold on a machine with no desktop at all.
    let run = ds(&[
        "map",
        "design",
        "save",
        "--transformer",
        "T-1042",
        "--output",
        "json",
    ]);
    assert_eq!(
        run.envelope["error"]["code"], "confirmation_required",
        "`ds map design save` reached past the confirmation gate"
    );
    assert_ne!(run.code, 0);
}

#[test]
fn map_design_delete_requires_an_explicit_selector() {
    // An empty selector would match the whole room; "delete everything" must
    // never be the accident default of forgetting a flag — refused locally,
    // by name, before the bridge is even opened.
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "delete",
            "--transformer",
            "T-1042",
            "--output",
            "json",
        ]),
        "selector_required",
        "an unselected delete must be refused before it reaches the bridge"
    );
}

#[test]
fn map_design_geometry_rejects_a_non_object_geometry_locally() {
    // The geometry contract is validated on this side too: a string that is
    // not JSON must fail by name without a desktop present.
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "geometry",
            "--transformer",
            "T-1042",
            "--id",
            "lv_poles#1",
            "--geometry",
            "not-json",
            "--output",
            "json",
        ]),
        "invalid_geometry",
        "a malformed --geometry must be refused before the bridge is opened"
    );
}

#[test]
fn map_design_report_cannot_run_without_confirmation() {
    // Report export writes durable artifacts, so dispatch requires --yes
    // exactly like save — and the gate must hold with no desktop at all.
    let run = ds(&[
        "map",
        "design",
        "report",
        "--transformer",
        "T-1042",
        "--output",
        "json",
    ]);
    assert_eq!(
        run.envelope["error"]["code"], "confirmation_required",
        "`ds map design report` reached past the confirmation gate"
    );
    assert_ne!(run.code, 0);
}

#[test]
fn every_map_command_is_reachable_without_the_desktop_installed() {
    // Availability here is deliberately unconditional: dispatch checks it
    // before parsing, so a gate would make `--desktop-descriptor` — the flag
    // that names a descriptor discovery did not find — unreachable, and would
    // put every input refusal above out of reach on a machine with no app.
    let index = ok(&["capabilities", "map", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    assert_eq!(
        commands.len(),
        21,
        "the map domain should register twenty-one commands"
    );
    for command in commands {
        assert_eq!(
            command["availability"], "available",
            "`{}` gates on discovery, which puts --desktop-descriptor out of reach",
            command["id"]
        );
    }
}

#[test]
fn a_well_formed_map_call_only_ever_fails_on_the_pairing_state() {
    // Whatever this machine's desktop situation, a correct invocation must
    // end in a pairing outcome — never an input refusal, and never an
    // internal error. `undeclared_bridge_argument` in particular would mean a
    // handler built an argument key its own BridgeOp does not declare, which
    // no other suite can see.
    for args in [
        vec!["map", "view"],
        vec!["map", "zoom", "--bbox", "29.9,-2.1,30.2,-1.85"],
        vec!["map", "remove", "--layer", "sketch-does-not-exist"],
        vec![
            "map",
            "points-along",
            "--layer",
            "sketch:x",
            "--interval-m",
            "25",
        ],
        vec!["map", "random-points", "--layer", "sketch:x"],
        vec!["map", "outliers", "--layer", "sketch:x"],
        vec!["map", "design", "read", "--transformer", "T-1042"],
        vec!["map", "design", "select", "--transformer", "T-1042"],
        vec!["map", "design", "set", "--transformer", "T", "--set", "a=b"],
        vec!["map", "design", "process", "--transformer", "T-1042"],
        vec![
            "map",
            "design",
            "delete",
            "--transformer",
            "T-1042",
            "--where",
            "pole_status=duplicate",
        ],
        vec![
            "map",
            "design",
            "geometry",
            "--transformer",
            "T-1042",
            "--id",
            "lv_poles#1",
            "--geometry",
            r#"{"type":"Point","coordinates":[30.06,-1.95]}"#,
        ],
        vec!["map", "design", "list"],
        vec![
            "map",
            "design",
            "batch",
            "process",
            "--transformer",
            "T-1042",
        ],
        vec![
            "map",
            "design",
            "batch",
            "save",
            "--transformer",
            "T-1042",
            "--yes",
        ],
        vec!["map", "design", "upload", "inspect", "--path", "survey.zip"],
        vec![
            "map",
            "design",
            "upload",
            "stage",
            "--source",
            "T-1042=survey.zip",
        ],
        vec!["map", "design", "save", "--transformer", "T-1042", "--yes"],
        vec![
            "map",
            "design",
            "report",
            "--transformer",
            "T-1042",
            "--yes",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        let code = refusal(&argv);
        assert!(
            code.is_empty() || PAIRING_CODES.contains(&code.as_str()),
            "`ds {}` failed with `{code}`, which is not a pairing outcome. \
             A well-formed call must reach the bridge and stop there.",
            args.join(" ")
        );
    }
}
