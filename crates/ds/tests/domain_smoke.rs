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

use ds_grid_engine::{CommandEnvelope, GridCommand, GridSession};
use ds_grid_exchange::unpack;
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
fn dsgrid_apply_dry_runs_then_writes_one_revision_without_overwriting_source() {
    let model = common::fixture();
    let bytes = std::fs::read(&model).expect("read fixture package");
    let package = unpack(&bytes).expect("decode fixture package");
    let alignment = package
        .snapshot
        .alignments
        .first()
        .expect("fixture has an alignment")
        .id
        .clone();
    let session = GridSession::open(package.snapshot);
    let expected_revision = session.current_revision().revision_id.clone();
    let envelope = CommandEnvelope::new(
        "ds-cli-domain-smoke-survey-facts",
        expected_revision,
        GridCommand::SetAlignmentSurveyFacts {
            alignment_id: alignment,
            terrain_corridor_half_width_m: None,
            route_buffer_half_width_m: None,
            terrain_gap_tolerance_m: None,
            survey_note: Some("ds-cli domain smoke".to_string()),
        },
    );

    let root = std::env::temp_dir().join(format!(
        "ds-cli-apply-smoke-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("worker")
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create smoke dir");
    let envelope_path = root.join("command.json");
    let out_path = root.join("revised.dsgrid");
    std::fs::write(
        &envelope_path,
        serde_json::to_vec_pretty(&envelope).expect("encode envelope"),
    )
    .expect("write envelope");

    let envelope_text = envelope_path.display().to_string();
    let out_text = out_path.display().to_string();
    let preview = ok(&[
        "dsgrid",
        "apply",
        "--model",
        &model,
        "--envelope",
        &envelope_text,
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["would_apply"], true);
    assert_eq!(preview["persisted"], false);
    assert!(!out_path.exists(), "dry-run must not create the output");

    let applied = ok(&[
        "dsgrid",
        "apply",
        "--model",
        &model,
        "--envelope",
        &envelope_text,
        "--out",
        &out_text,
        "--output",
        "json",
    ]);
    assert_eq!(applied["persisted"], true);
    assert_eq!(applied["command_kind"], "set_alignment_survey_facts");
    assert!(
        out_path.is_file(),
        "apply must write the requested new package"
    );
    assert!(
        applied["artifact"]["sha256"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:")
    );

    let validation = ok(&[
        "dsgrid", "validate", "--model", &out_text, "--output", "json",
    ]);
    assert_eq!(validation["container"]["verified"], true);
    assert_eq!(validation["model"]["valid"], true);

    std::fs::remove_dir_all(&root).expect("remove smoke dir");
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
        (&["map", "zoom"], "zoom_target"),
        (
            &[
                "map",
                "zoom",
                "--bbox",
                "29.9,-2.1,30.2,-1.85",
                "--layer",
                "sketch:x",
            ],
            "zoom_target",
        ),
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
fn map_design_version_begin_cannot_run_without_confirmation() {
    let run = ds(&[
        "map",
        "design",
        "version",
        "begin",
        "--transformer",
        "agasharu",
        "--reason",
        "Approved drafting baseline",
        "--output",
        "json",
    ]);
    assert_eq!(
        run.envelope["error"]["code"], "confirmation_required",
        "design version creation reached past the confirmation gate"
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
        31,
        "the map domain should register thirty-one commands"
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
        vec!["map", "zoom", "--layer", "sketch-does-not-exist"],
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
        vec![
            "map",
            "line-difference",
            "--source-layer",
            "sketch:incoming",
            "--base-layer",
            "sketch:base",
            "--name",
            "difference",
        ],
        vec!["map", "design", "read", "--transformer", "T-1042"],
        vec![
            "map",
            "design",
            "layer-to-local",
            "--transformer",
            "T-1042",
            "--layer",
            "lv_lines",
            "--name",
            "base",
        ],
        vec![
            "map",
            "design",
            "upload-to-local",
            "--path",
            "survey.zip",
            "--source-layer",
            "lv_lines",
            "--name",
            "incoming",
        ],
        vec!["map", "design", "select", "--transformer", "T-1042"],
        vec!["map", "design", "set", "--transformer", "T", "--set", "a=b"],
        vec![
            "map",
            "design",
            "create",
            "--transformer",
            "T",
            "--source-layer",
            "sketch:difference",
            "--target-layer",
            "lv_lines",
        ],
        vec!["map", "design", "process", "--transformer", "T-1042"],
        vec![
            "map",
            "design",
            "setup",
            "--survey-layer",
            "edcl_customers_survey",
            "--preset",
            "drafting",
            "--dry-run",
        ],
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

// ---------------------------------------------------------------------------
// work
// ---------------------------------------------------------------------------

#[test]
fn work_validates_its_own_inputs_before_it_opens_the_bridge() {
    // Every refusal below must be reachable on a machine with no application
    // running, because that is every CI machine — and because a caller that
    // transposed a date should hear which flag was wrong, not that no session
    // was found.
    assert_eq!(
        refusal(&[
            "work", "task", "update", "--task", "T-1", "--output", "json", "--yes"
        ]),
        "nothing_to_update",
        "an update with no change must be refused before a project round trip"
    );
    assert_eq!(
        refusal(&[
            "work",
            "task",
            "create",
            "--title",
            "Stake the route",
            "--start",
            "01-09-2026",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_date",
        "a transposed day and month must be refused locally; the engine would accept it"
    );
    assert_eq!(
        refusal(&[
            "work", "task", "create", "--title", "Orphan", "--kind", "child", "--output", "json",
            "--yes",
        ]),
        "invalid_task_shape",
        "a child with no parent must be refused before the bridge opens"
    );
    assert_eq!(
        refusal(&[
            "work",
            "task",
            "update",
            "--task",
            "T-1",
            "--progress",
            "140",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_number",
        "progress is a percent and must be held to 0..100"
    );
    assert_eq!(
        refusal(&[
            "work", "task", "assign", "--task", "T-1", "--output", "json", "--yes"
        ]),
        "invalid_assignment",
        "an assign that names nobody has no intent to send"
    );
    assert_eq!(
        refusal(&[
            "work",
            "task",
            "assign",
            "--task",
            "T-1",
            "--owner",
            "a@example.com",
            "--request",
            "b@example.com",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_assignment",
        "asking and transferring are two different intents"
    );
    assert_eq!(
        refusal(&[
            "work",
            "task",
            "assign",
            "--task",
            "T-1",
            "--request",
            "not-an-address",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_email",
        "a person flag that is not an address must be refused locally"
    );
    assert_eq!(
        refusal(&["work", "task", "list", "--limit", "500", "--output", "json"]),
        "invalid_number",
        "a page larger than the application returns must be refused by the bound it names"
    );
    assert_eq!(
        refusal(&[
            "work",
            "task",
            "respond",
            "--task",
            "T-1",
            "--response",
            "maybe",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_choice",
        "the assignment loop has two answers, and `maybe` is not one of them"
    );
}

#[test]
fn every_work_write_refuses_without_confirmation() {
    // Project Work is shared state governed by ds-brain. Every write here is
    // `global_write`, so dispatch must stop it before the bridge opens —
    // including `respond`, which is the one a person is most likely to script.
    for args in [
        vec!["work", "task", "create", "--title", "Stake the route"],
        vec![
            "work",
            "task",
            "update",
            "--task",
            "T-1",
            "--delivery",
            "in_progress",
        ],
        vec![
            "work",
            "task",
            "assign",
            "--task",
            "T-1",
            "--request",
            "pilot@example.com",
        ],
        vec![
            "work",
            "task",
            "respond",
            "--task",
            "T-1",
            "--response",
            "accept",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        assert_eq!(
            refusal(&argv),
            "confirmation_required",
            "`ds {}` reached past the confirmation gate",
            args.join(" ")
        );
    }
}

#[test]
fn every_work_command_is_reachable_without_the_desktop_installed() {
    // Same reasoning as the map domain: dispatch checks availability before
    // parsing, so a discovery gate would put `--desktop-descriptor` and every
    // input refusal above out of reach on a machine with no application.
    let index = ok(&["capabilities", "work", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    assert_eq!(
        commands.len(),
        9,
        "the work domain should register nine commands"
    );
    for command in commands {
        assert_eq!(
            command["availability"], "available",
            "`{}` gates on discovery, which puts --desktop-descriptor out of reach",
            command["id"]
        );
    }
    // Reads must never be behind the confirmation gate, and writes must never
    // be outside it. Getting this backwards is silent until an operator runs
    // an unattended session.
    for command in commands {
        let effect = command["effect"].as_str().expect("effect");
        let id = command["id"].as_str().expect("id");
        let write = matches!(
            id,
            "work.task.create" | "work.task.update" | "work.task.assign" | "work.task.respond"
        );
        assert_eq!(
            effect,
            if write { "global_write" } else { "read_only" },
            "`{id}` declares the wrong effect class for its blast radius"
        );
    }
}

#[test]
fn feedback_is_one_confirmed_shared_write() {
    let index = ok(&["capabilities", "feedback", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    assert_eq!(commands.len(), 1, "feedback has one narrow operation");
    assert_eq!(commands[0]["id"], "feedback.submit");
    assert_eq!(commands[0]["effect"], "global_write");
    assert_eq!(commands[0]["availability"], "available");

    let base = [
        "feedback",
        "submit",
        "--title",
        "Missing export",
        "--detail",
        "Live discovery found no export. Acceptance: expose one typed export.",
        "--component",
        "ds-cli",
        "--agent",
        "test-agent",
    ];
    let mut unconfirmed = base.to_vec();
    unconfirmed.extend(["--output", "json"]);
    assert_eq!(
        refusal(&unconfirmed),
        "confirmation_required",
        "feedback reached the shared backlog without explicit confirmation"
    );

    let mut confirmed = base.to_vec();
    confirmed.extend(["--yes", "--output", "json"]);
    let code = refusal(&confirmed);
    assert!(
        PAIRING_CODES.contains(&code.as_str()),
        "a valid feedback report ended in `{code}`, not a pairing outcome"
    );
}

#[test]
fn feedback_validates_context_before_pairing() {
    let code = refusal(&[
        "feedback",
        "submit",
        "--title",
        "Missing export",
        "--detail",
        "Live discovery found no export.",
        "--component",
        "ds-cli",
        "--agent",
        "test-agent",
        "--context",
        "not-a-pair",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(code, "invalid_context");
}

#[test]
fn a_well_formed_work_call_only_ever_fails_on_the_pairing_state() {
    // Whatever this machine's desktop situation, a correct invocation must end
    // in a pairing outcome — never an input refusal, and never an internal
    // error. `undeclared_bridge_argument` in particular would mean a handler
    // built an argument key its own BridgeOp does not declare, which no other
    // suite can see.
    for args in [
        vec!["work", "plan"],
        vec!["work", "task", "list"],
        vec![
            "work",
            "task",
            "list",
            "--state",
            "blocked",
            "--assignee",
            "pilot@example.com",
            "--placement",
            "inbox",
            "--limit",
            "25",
            "--page",
            "1",
        ],
        vec!["work", "task", "read", "--task", "T-1"],
        vec![
            "work",
            "task",
            "create",
            "--title",
            "Stake the route",
            "--kind",
            "milestone",
            "--start",
            "2026-09-01",
            "--yes",
        ],
        vec![
            "work",
            "task",
            "update",
            "--task",
            "T-1",
            "--title",
            "Stake the MV route",
            "--priority",
            "high",
            "--delivery",
            "in_progress",
            "--progress",
            "40",
            "--finish",
            "2026-09-20",
            "--yes",
        ],
        vec![
            "work",
            "task",
            "assign",
            "--task",
            "T-1",
            "--request",
            "pilot@example.com",
            "--yes",
        ],
        vec![
            "work",
            "task",
            "assign",
            "--task",
            "T-1",
            "--withdraw",
            "--yes",
        ],
        vec![
            "work",
            "task",
            "respond",
            "--task",
            "T-1",
            "--response",
            "accept",
            "--yes",
        ],
        vec!["work", "record", "list", "--category", "review"],
        vec!["work", "record", "read", "--record", "R-1"],
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

// ---------------------------------------------------------------------------
// sre
// ---------------------------------------------------------------------------

#[test]
fn sre_is_two_global_read_only_commands_with_stable_defaults() {
    let index = ok(&["capabilities", "sre", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    assert_eq!(commands.len(), 2, "the SRE domain has two bounded reads");
    for command in commands {
        assert_eq!(command["effect"], "read_only");
        assert_eq!(
            command["authority"], "desktop_user",
            "SRE is global to the signed-in user, not project authority"
        );
        assert_eq!(command["availability"], "available");
    }

    let events = ok(&["capabilities", "sre.events", "--output", "json"]);
    let inputs = events["command"]["inputs"].as_array().expect("inputs");
    let input = |name: &str| {
        inputs
            .iter()
            .find(|input| input["name"] == name)
            .unwrap_or_else(|| panic!("missing --{name}"))
    };
    assert_eq!(input("days")["default"], "3");
    assert_eq!(input("limit")["default"], "50");
    assert_eq!(input("scan-limit")["default"], "1000");
    assert_eq!(input("outcome")["default"], "failure");
    assert_eq!(
        input("outcome")["choices"],
        serde_json::json!(["failure", "success", "all"])
    );
    assert_eq!(input("project")["required"], false);
}

#[test]
fn capability_search_finds_sre_by_operator_vocabulary() {
    for query in ["recent errors", "diagnostic logs", "reliability events"] {
        let data = ok(&["capabilities", "--search", query, "--output", "json"]);
        let results = data["results"].as_array().expect("search results");
        assert!(
            results.iter().any(|row| row["id"] == "sre.events"),
            "`{query}` did not find sre.events: {results:?}"
        );
    }
}

#[test]
fn sre_validates_bounds_and_filters_before_pairing() {
    for (flag, value, expected) in [
        ("days", "0", "invalid_number"),
        ("days", "366", "invalid_number"),
        ("limit", "0", "invalid_number"),
        ("limit", "251", "invalid_number"),
        ("scan-limit", "0", "invalid_number"),
        ("scan-limit", "5001", "invalid_number"),
        ("service", " ds-brain", "invalid_text"),
    ] {
        assert_eq!(
            refusal(&[
                "sre",
                "events",
                &format!("--{flag}"),
                value,
                "--output",
                "json"
            ]),
            expected,
            "--{flag} {value} reached the bridge"
        );
    }
    assert_eq!(
        refusal(&["sre", "events", "--outcome", "unknown", "--output", "json"]),
        "invalid_choice"
    );
}

#[test]
fn well_formed_sre_reads_reach_only_the_global_runtime_boundary() {
    for args in [
        vec!["sre", "overview"],
        vec![
            "sre",
            "events",
            "--days",
            "7",
            "--limit",
            "25",
            "--scan-limit",
            "500",
            "--service",
            "ds-brain",
            "--outcome",
            "all",
            "--category",
            "timeout",
            "--lane",
            "stable",
            "--action",
            "query_table",
            "--project",
            "project-1",
            "--source",
            "forwarded_compute",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        let code = refusal(&argv);
        assert!(
            code.is_empty()
                || PAIRING_CODES.contains(&code.as_str())
                || code == "sre_not_permitted",
            "`ds {}` failed with `{code}` before or beyond its paired read boundary",
            args.join(" ")
        );
    }
}

// ---------------------------------------------------------------------------
// shell
// ---------------------------------------------------------------------------

#[test]
fn shell_status_tells_this_shell_from_a_new_one() {
    // The PATH is the fixture here. With the built binary's own directory
    // first, `ds` resolves to this executable; with nothing on it, the answer
    // is a remedy — never a guess.
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_ds"));
    let directory = executable.parent().expect("binary directory");
    let leading = std::env::join_paths([directory.to_path_buf()]).expect("path");

    let status = |path: &std::ffi::OsStr| {
        let output = Command::new(&executable)
            .args(["shell", "status", "--output", "json"])
            .env("NO_COLOR", "1")
            .env("PATH", path)
            .output()
            .expect("ds runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value =
            serde_json::from_slice(&output.stdout).expect("shell status emits JSON");
        envelope["data"].clone()
    };

    let data = status(leading.as_os_str());
    assert_eq!(
        data["reachable"], true,
        "the binary's own directory leads the PATH: {data}"
    );
    assert!(
        data["executable"]
            .as_str()
            .is_some_and(|path| PathBuf::from(path).is_file()),
        "the executable is a real file: {data}"
    );
    assert!(data["resolves_to"].as_str().is_some());
    assert_eq!(data["others"].as_array().map_or(0, Vec::len), 0);
    assert!(
        data["remedy"].is_null(),
        "nothing to fix when this shell already finds it: {data}"
    );
    assert!(
        data["registration"]["kind"]
            .as_str()
            .is_some_and(|kind| kind == "windows_user_path" || kind == "unix_local_bin"),
        "the registration names its platform: {data}"
    );

    let data = status(std::ffi::OsStr::new("/nonexistent"));
    assert_eq!(data["reachable"], false);
    assert!(data["resolves_to"].is_null());
    assert!(
        data["remedy"].as_str().is_some_and(
            |remedy| remedy.contains("ds shell register") || remedy.contains("new terminal")
        ),
        "an unreachable ds names the fix: {data}"
    );

    // Doctor folds the two answers into one word, and never fails on them.
    let output = Command::new(&executable)
        .args(["doctor", "--output", "json"])
        .env("NO_COLOR", "1")
        .env("PATH", &leading)
        .output()
        .expect("ds runs");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("doctor emits JSON");
    assert_eq!(envelope["data"]["shell"]["status"], "reachable");
}
