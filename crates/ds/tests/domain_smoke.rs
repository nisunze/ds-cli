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

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ds_grid_engine::{CommandEnvelope, GridCommand, GridSession};
use ds_grid_exchange::{parse_standards_library_manifest, unpack, unpack_library};
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

fn temp_root(label: &str) -> PathBuf {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "ds-cli-{label}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

// ---------------------------------------------------------------------------
// workstation
// ---------------------------------------------------------------------------

#[test]
fn workstation_windows_libreoffice_plan_is_explicitly_mutation_free() {
    let data = ok(&[
        "workstation",
        "plan",
        "--component",
        "libreoffice",
        "--platform",
        "windows",
        "--output",
        "json",
    ]);
    assert_eq!(data["mutated"], false);
    assert_eq!(data["authorized"], false);
    assert_eq!(data["implementation"], "available");
    assert_eq!(
        data["constraints"]["windows_libreoffice_lifecycle_proven"],
        true
    );
    assert_eq!(data["evidence"]["state"], "proven");
    assert!(
        data["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap_or("").contains("SHA-256"))
    );
}

#[test]
fn workstation_rwanda_acquisition_plan_names_the_receipt_proof() {
    let data = ok(&[
        "workstation",
        "plan",
        "--component",
        "rwanda-reference",
        "--output",
        "json",
    ]);
    assert_eq!(data["implementation"], "available");
    assert!(data["steps"].as_array().unwrap().iter().any(|step| {
        let step = step.as_str().unwrap_or("");
        step.contains("license") && step.contains("SHA-256")
    }));
}

#[cfg(not(windows))]
#[test]
fn workstation_install_refuses_an_unproven_platform_component() {
    let run = ds(&[
        "workstation",
        "install",
        "--component",
        "git-bash",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 3, "{}{}", run.stdout, run.stderr);
    assert_eq!(
        run.envelope["error"]["code"],
        "workstation_mutation_unsupported"
    );
}

#[cfg(not(windows))]
#[test]
fn workstation_configure_refuses_outside_native_windows() {
    let run = ds(&[
        "workstation",
        "configure",
        "--component",
        "git-bash",
        "--target",
        "vscode",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 3, "{}{}", run.stdout, run.stderr);
    assert_eq!(
        run.envelope["error"]["code"],
        "workstation_mutation_unsupported"
    );
}

// On the platform where these commands can change a machine, their smoke
// coverage must not disappear. Deliberately omit --yes: this exercises the
// same dispatch choke point without installing or configuring anything.
#[cfg(windows)]
#[test]
fn workstation_install_is_confirmation_gated_on_native_windows() {
    let run = ds(&[
        "workstation",
        "install",
        "--component",
        "git-bash",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert_eq!(run.envelope["error"]["code"], "confirmation_required");
}

#[cfg(windows)]
#[test]
fn workstation_configure_is_confirmation_gated_on_native_windows() {
    let run = ds(&[
        "workstation",
        "configure",
        "--component",
        "git-bash",
        "--target",
        "vscode",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert_eq!(run.envelope["error"]["code"], "confirmation_required");
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

// ---------------------------------------------------------------------------
// library
// ---------------------------------------------------------------------------

#[test]
fn library_seed_materializes_two_native_families_idempotently() {
    let root = temp_root("library-seed");
    let _ = std::fs::remove_dir_all(&root);
    let source = root.join("source");
    let out = root.join("out");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("pole.012"),
        b"TYPE='STRUCT FILE' VERSION='13' UNITS='SI' SOURCE='PLS-CADD Version 16.81'\n\
          SYNTHETIC POLE\n\
          10.2 1.8 0\n\
          1\n\
          'gw' 'C' 'N' 'N' 0 1\n\
          0 0.2 0 'GW' 'GW' 0 0 0 0\n",
    )
    .unwrap();
    let source_text = source.display().to_string();
    let out_text = out.display().to_string();
    let args = [
        "library",
        "seed",
        "--source",
        &source_text,
        "--out",
        &out_text,
        "--library-id",
        "new-design",
        "--library-version",
        "2026-08-27-v1",
        "--role",
        "new_design",
        "--provenance",
        "synthetic-test",
        "--yes",
        "--output",
        "json",
    ];
    let first = ok(&args);
    assert_eq!(first["cloud_write"], false);
    assert_eq!(first["idempotent"], false);
    assert_eq!(first["execution_owner"], "ds");
    let version = out.join("library/new-design/2026-08-27-v1");
    assert!(version.join("manifest.json").is_file());
    assert!(version.join("pls-cadd/pole.012").is_file());
    let release =
        unpack_library(&std::fs::read(version.join("dsgrid/library.dsgrid-library")).unwrap())
            .unwrap();
    assert!(
        release.assets.is_empty(),
        "DS Grid seed must not embed PLS bytes"
    );

    let second = ok(&args);
    assert_eq!(second["idempotent"], true);

    let manifest =
        parse_standards_library_manifest(&std::fs::read(version.join("manifest.json")).unwrap())
            .unwrap();
    let expected_root = format!("sha256:{}", manifest.content_root_sha256);
    let native_kind = manifest.members[0].native_kind.as_str();
    let resolved = ok(&[
        "library",
        "resolve-native",
        "--store",
        &out_text,
        "--library-id",
        "new-design",
        "--library-version",
        "2026-08-27-v1",
        "--expect-digest",
        &expected_root,
        "--native-name",
        "pole.012",
        "--native-kind",
        native_kind,
        "--output",
        "json",
    ]);
    assert_eq!(resolved["canonical_typed_name"], "pole.012");
    assert_eq!(resolved["execution_owner"], "ds");
    assert!(resolved["sha256"].as_str().unwrap().starts_with("sha256:"));

    let misrouted = out.join("library/new-design/wrong-version");
    std::fs::create_dir_all(misrouted.join("pls-cadd")).unwrap();
    std::fs::copy(
        version.join("manifest.json"),
        misrouted.join("manifest.json"),
    )
    .unwrap();
    std::fs::copy(
        version.join("pls-cadd/pole.012"),
        misrouted.join("pls-cadd/pole.012"),
    )
    .unwrap();
    let wrong_version = ds(&[
        "library",
        "resolve-native",
        "--store",
        &out_text,
        "--library-id",
        "new-design",
        "--library-version",
        "wrong-version",
        "--expect-digest",
        &expected_root,
        "--native-name",
        "pole.012",
        "--native-kind",
        native_kind,
        "--output",
        "json",
    ]);
    assert_ne!(wrong_version.code, 0);
    assert_eq!(
        wrong_version.envelope["error"]["code"],
        "library_version_mismatch"
    );
    std::fs::remove_dir_all(&root).unwrap();
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

fn terrain_xyz(points: &[(f64, f64, f64, &str)]) -> Vec<u8> {
    let mut bytes = b"TYPE='XYZ FILE' VERSION='5' UNITS='INTERNAL' SOURCE='ds smoke'\n\0".to_vec();
    for (x, y, z, code) in points {
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
        bytes.extend_from_slice(&z.to_le_bytes());
        bytes.extend_from_slice(&0f64.to_le_bytes());
        bytes.push(0xc8);
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(code.as_bytes());
        bytes.push(0);
        bytes.push(0);
    }
    bytes
}

#[test]
fn pls_terrain_reconcile_then_deviation_labels_is_dry_run_first_and_code_only() {
    let root = temp_root("pls-terrain-reconcile");
    let _ = std::fs::remove_dir_all(&root);
    let baseline = root.join("baseline");
    std::fs::create_dir_all(&baseline).unwrap();
    let survey = (0..9)
        .flat_map(|row| {
            (0..9).map(move |column| {
                (
                    500_000.0 + column as f64 * 20.0,
                    5_000_000.0 + row as f64 * 20.0,
                    1_500.0 + column as f64 + row as f64,
                    "GP",
                )
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(baseline.join("A Project.xyz"), terrain_xyz(&survey)).unwrap();

    let points = root.join("points.json");
    let rows = (0..17)
        .map(|index| {
            serde_json::json!({
                "x_m": 500_000.0 + index as f64 * 10.0,
                "y_m": 5_000_000.0 + index as f64 * 10.0,
                "z_m": 1_512.5 + index as f64,
                "code": "GP"
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(&points, serde_json::to_vec(&rows).unwrap()).unwrap();
    let routes = root.join("routes.geojson");
    std::fs::write(
        &routes,
        serde_json::to_vec(&serde_json::json!({
            "type":"FeatureCollection",
            "features":[{
                "type":"Feature",
                "properties":{"id":"route-a"},
                "geometry":{"type":"LineString","coordinates":[[500000.0,5000000.0],[500160.0,5000160.0]]}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let reconciled = root.join("reconciled");
    let strings = [
        baseline.display().to_string(),
        points.display().to_string(),
        routes.display().to_string(),
        reconciled.display().to_string(),
    ];
    let preview = ok(&[
        "pls",
        "terrain-reconcile",
        "--workspace",
        &strings[0],
        "--points",
        &strings[1],
        "--routes",
        &strings[2],
        "--horizontal-crs",
        "EDCL Rwanda TM",
        "--vertical-datum",
        "project surveyed TIN",
        "--out",
        &strings[3],
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_eq!(preview["persisted"], false);
    assert_eq!(preview["residual_distribution"]["pair_count"], 17);
    assert_eq!(preview["coordinate_change_count"], 0);
    assert!(!reconciled.exists());

    let committed = ok(&[
        "pls",
        "terrain-reconcile",
        "--workspace",
        &strings[0],
        "--points",
        &strings[1],
        "--routes",
        &strings[2],
        "--horizontal-crs",
        "EDCL Rwanda TM",
        "--vertical-datum",
        "project surveyed TIN",
        "--out",
        &strings[3],
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(committed["persisted"], true);
    assert_eq!(committed["output_point_count"], 98);

    let labelled = root.join("labelled");
    let labelled_text = labelled.display().to_string();
    let labels = ok(&[
        "pls",
        "deviation-labels",
        "--workspace",
        &strings[3],
        "--points",
        &strings[1],
        "--routes",
        &strings[2],
        "--internal-code",
        "angle-point-new",
        "--start-code",
        "deviation-start",
        "--end-code",
        "deviation-end",
        "--preserve-occupied-endpoints",
        "--out",
        &labelled_text,
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_eq!(labels["labels"]["start_count"], 1);
    assert_eq!(labels["labels"]["end_count"], 1);
    assert_eq!(
        labels["labels"]["changed_fields"],
        serde_json::json!(["code"])
    );
    assert_eq!(labels["labels"]["unchanged_xyz"], true);
    assert_eq!(labels["labels"]["unchanged_flags"], true);
    assert_eq!(labels["point_count_before"], labels["point_count_after"]);
    assert!(!labelled.exists());
    let _ = std::fs::remove_dir_all(&root);
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

    let mut checked = 0usize;
    for command in envelope["data"]["commands"].as_array().expect("commands") {
        let id = command["id"].as_str().unwrap_or("");
        if id.starts_with("dsgrid.") || id.starts_with("dsgrid-exchange.") || id.starts_with("pls.")
        {
            checked += 1;
            assert_eq!(
                command["availability"], "available",
                "`{id}` links its engine and must not depend on an installed binary"
            );
        }
    }
    assert!(
        checked > 0,
        "doctor returned no linked-engine commands; this availability check would be vacuous"
    );
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
    let root = temp_root("map-line");
    std::fs::create_dir_all(&root).expect("temp directory is writable");
    let path = root.join("line.geojson");
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
        // `ds map ui open` is a panel door, not a UI driver. A selector, an
        // expression and a URL each have to be refused here — after the
        // bridge is open it is too late for the distinction to mean anything,
        // because the argument would already have been sent.
        (
            &[
                "map",
                "ui",
                "open",
                "--target",
                "style-center",
                "--ref",
                "#legend",
            ],
            "ui_ref_not_semantic",
        ),
        (
            &[
                "map",
                "ui",
                "open",
                "--target",
                "attribute-table",
                "--ref",
                "http://127.0.0.1/panel",
            ],
            "ui_ref_not_semantic",
        ),
        // A relative path and a non-PNG are the two ways `--out` goes wrong,
        // and both are cheaper to name here than after a full render.
        (
            &["map", "evidence", "capture", "--out", "step-1.png"],
            "evidence_out_invalid",
        ),
        (
            &[
                "map",
                "evidence",
                "capture",
                "--out",
                "/evidence/step-1.jpg",
            ],
            "evidence_out_invalid",
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
fn map_design_attach_print_validates_before_pairing() {
    let unconfirmed = ds(&[
        "map",
        "design",
        "attach-print",
        "--path",
        "/deliverables/T-1042-A3.pdf",
        "--transformer",
        "T-1042",
        "--map-family",
        "lv-atlas",
        "--layout",
        "LV A3",
        "--paper-size",
        "A3",
        "--orientation",
        "landscape",
        "--output",
        "json",
    ]);
    assert_eq!(
        unconfirmed.envelope["error"]["code"], "confirmation_required",
        "a durable QGIS attachment must require --yes"
    );

    assert_eq!(
        refusal(&[
            "map",
            "design",
            "attach-print",
            "--path",
            "/deliverables/project-A1.pdf",
            "--layout",
            "Project A1",
            "--map-family",
            "mv-map",
            "--paper-size",
            "A1",
            "--orientation",
            "landscape",
            "--yes",
            "--output",
            "json",
        ]),
        "transformer_required",
        "transformer scope must name its transformer before bridge discovery"
    );
}

#[test]
fn map_evidence_capture_will_not_overwrite_a_frame_without_being_asked_and_confirmed() {
    // The gate that matters for this command. `local_file_write` is not in
    // dispatch's confirmation set, so the whole of what stands between a model
    // and an overwritten piece of evidence is the handler — and it has to hold
    // on a machine with no desktop, which is every CI machine.
    let root = temp_root("map-evidence");
    std::fs::create_dir_all(&root).expect("temp directory is writable");
    let existing = root.join("step-3.png");
    std::fs::write(&existing, b"an earlier frame").expect("temp file is writable");
    let existing = existing.display().to_string();

    assert_eq!(
        refusal(&[
            "map", "evidence", "capture", "--out", &existing, "--output", "json"
        ]),
        "evidence_exists",
        "an existing frame must be refused rather than silently re-shot"
    );
    assert_eq!(
        refusal(&[
            "map",
            "evidence",
            "capture",
            "--out",
            &existing,
            "--replace",
            "--output",
            "json",
        ]),
        "confirmation_required",
        "--replace without --yes must stop before the bridge is opened"
    );

    // Confirmed, the local checks are satisfied and the call proceeds to the
    // desktop — which is not here, so it ends in a pairing state rather than
    // in either local refusal. That ordering is the claim: the gate is not
    // merely reachable, it is the only thing that was stopping the call.
    let confirmed = refusal(&[
        "map",
        "evidence",
        "capture",
        "--out",
        &existing,
        "--replace",
        "--yes",
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&confirmed.as_str()),
        "a confirmed replace ended in `{confirmed}`, not at the pairing boundary"
    );
    assert_eq!(
        std::fs::read(&existing).expect("the frame is still there"),
        b"an earlier frame",
        "no invocation in this test may touch the file; the application writes it"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn map_evidence_capture_declares_a_fixed_receipt_and_no_way_to_record() {
    // Two claims a caller reads before it ever runs the command, held to the
    // descriptor rather than to prose: the receipt is the seven fields, and
    // there is no recording door. Video is a third-party tool's job, and the
    // way that stays true is that no flag here could ever start or bound one.
    let descriptor = ok(&["capabilities", "map.evidence.capture", "--output", "json"]);
    let command = &descriptor["command"];
    let output = command["output"].as_str().expect("output");
    for field in [
        "path",
        "bytes",
        "sha256",
        "dimensions",
        "scope",
        "view",
        "ui",
    ] {
        assert!(
            output.contains(field),
            "the declared output no longer names the receipt field `{field}`"
        );
    }

    let inputs: BTreeSet<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect();
    assert_eq!(
        inputs,
        ["out", "scope", "replace", "desktop-descriptor"]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "a flag appeared on the capture contract; a recorder would arrive as one"
    );

    let scope: BTreeSet<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "scope")
        .expect("--scope is declared")["choices"]
        .as_array()
        .expect("choices")
        .iter()
        .map(|choice| choice.as_str().expect("choice"))
        .collect();
    assert_eq!(
        scope,
        ["map", "app"].into_iter().collect::<BTreeSet<_>>(),
        "--scope is a closed pair: the canvas, or the whole window"
    );
}

#[test]
fn map_ui_open_offers_three_panels_and_no_way_to_address_the_interface() {
    // The reason this command is safe to have at all: the target is a closed
    // set of published panels, and there is no second input that could carry a
    // selector, a coordinate or a script. If a fourth target or a new flag
    // appears, someone is turning a panel door into a UI driver.
    let command = ok(&["capabilities", "map.ui.open", "--output", "json"])["command"].clone();
    let inputs: BTreeSet<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect();
    assert_eq!(
        inputs,
        ["target", "ref", "desktop-descriptor"]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "`ds map ui open` grew an input; it takes a published panel and a published ref"
    );

    let targets: Vec<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "target")
        .expect("--target is declared")["choices"]
        .as_array()
        .expect("choices")
        .iter()
        .map(|choice| choice.as_str().expect("choice"))
        .collect();
    assert_eq!(
        targets,
        vec!["attribute-table", "style-center", "selection-properties"],
    );

    // A ref the application publishes reaches the bridge; a selector does not.
    let published = refusal(&[
        "map",
        "ui",
        "open",
        "--target",
        "selection-properties",
        "--ref",
        "TX-1042.lv_lines/17",
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&published.as_str()),
        "a published reference ended in `{published}`, not at the pairing boundary"
    );
}

#[test]
fn map_layer_management_keeps_project_and_desktop_local_effects_separate() {
    let add = ok(&["capabilities", "map.layer.add", "--output", "json"])["command"].clone();
    assert_eq!(add["authority"], "desktop_pairing");
    assert_eq!(add["effect"], "local_ui");
    let list = ok(&["capabilities", "map.layer.list", "--output", "json"])["command"].clone();
    assert_eq!(list["authority"], "project");
    assert_eq!(list["effect"], "read_only");
    let reorder = ok(&["capabilities", "map.layer.reorder", "--output", "json"])["command"].clone();
    assert_eq!(reorder["authority"], "project");
    assert_eq!(reorder["effect"], "global_write");

    let confirmation = refusal(&[
        "map",
        "layer",
        "reorder",
        "--order",
        "roads=100",
        "--output",
        "json",
    ]);
    assert_eq!(confirmation, "confirmation_required");

    let bad_visibility = refusal(&[
        "map",
        "layer",
        "visibility",
        "--layer",
        "tile-1",
        "--visible",
        "toggle",
        "--output",
        "json",
    ]);
    assert_eq!(bad_visibility, "invalid_choice");
}

#[test]
fn every_map_command_is_reachable_without_the_desktop_installed() {
    // Availability here is deliberately unconditional: dispatch checks it
    // before parsing, so a gate would make `--desktop-descriptor` — the flag
    // that names a descriptor discovery did not find — unreachable, and would
    // put every input refusal above out of reach on a machine with no app.
    let index = ok(&["capabilities", "map", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = [
        "map.view",
        "map.draw",
        "map.remove",
        "map.zoom",
        "map.layer.list",
        "map.layer.reorder",
        "map.layer.remote-list",
        "map.layer.add",
        "map.layer.remove",
        "map.layer.visibility",
        "map.ui.open",
        "map.evidence.capture",
        "map.points-along",
        "map.random-points",
        "map.outliers",
        "map.line-difference",
        "map.survey.download",
        "map.survey.migrate.plan",
        "map.survey.migrate.apply",
        "map.design.read",
        "map.design.discard",
        "map.design.layer-to-local",
        "map.design.upload-to-local",
        "map.design.select",
        "map.design.set",
        "map.design.create",
        "map.design.delete",
        "map.design.geometry",
        "map.design.setup",
        "map.design.version.begin",
        "map.design.process",
        "map.design.batch.process",
        "map.design.batch.report",
        "map.design.batch.save",
        "map.design.save",
        "map.design.list",
        "map.design.report",
        "map.design.attach-print",
        "map.design.upload.inspect",
        "map.design.upload.stage",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "map command coverage list changed; add a specific smoke assertion for the new command before accepting it"
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
fn a_well_formed_map_call_stops_at_confirmation_or_pairing() {
    // This test intentionally never passes --yes. A real paired desktop could
    // otherwise execute a shared write while running a smoke suite. Reads
    // reach the pairing boundary; writes must stop at confirmation first.
    for args in [
        vec!["map", "view"],
        vec!["map", "zoom", "--bbox", "29.9,-2.1,30.2,-1.85"],
        vec![
            "map",
            "ui",
            "open",
            "--target",
            "attribute-table",
            "--ref",
            "master/customers",
        ],
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
        vec!["map", "design", "batch", "save", "--transformer", "T-1042"],
        vec!["map", "design", "upload", "inspect", "--path", "survey.zip"],
        vec![
            "map",
            "design",
            "upload",
            "stage",
            "--source",
            "T-1042=survey.zip",
        ],
        vec!["map", "design", "save", "--transformer", "T-1042"],
        vec!["map", "design", "report", "--transformer", "T-1042"],
        vec![
            "map",
            "design",
            "attach-print",
            "--path",
            "/deliverables/T-1042-A3.pdf",
            "--transformer",
            "T-1042",
            "--map-family",
            "lv-atlas",
            "--layout",
            "LV A3",
            "--paper-size",
            "A3",
            "--orientation",
            "landscape",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        let code = refusal(&argv);
        assert!(
            code == "confirmation_required" || PAIRING_CODES.contains(&code.as_str()),
            "`ds {}` failed with `{code}`, which is neither the confirmation \
             gate for a write nor a pairing outcome for a read.",
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
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = [
        "work.plan",
        "work.task.list",
        "work.task.read",
        "work.task.create",
        "work.task.update",
        "work.task.assign",
        "work.task.respond",
        "work.record.list",
        "work.record.read",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "work command coverage list changed; add a specific smoke assertion for the new command before accepting it"
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
    // The domain is the loop: report a gap, find it again, close it. Reading
    // the backlog must stay outside the confirmation gate and both writes
    // inside it — getting that backwards is silent until an unattended
    // session either cannot read or closes something without being asked.
    let effects: Vec<(&str, &str)> = commands
        .iter()
        .map(|command| {
            (
                command["id"].as_str().expect("id"),
                command["effect"].as_str().expect("effect"),
            )
        })
        .collect();
    assert_eq!(
        effects,
        [
            ("feedback.submit", "global_write"),
            ("feedback.list", "read_only"),
            ("feedback.close", "global_write"),
        ]
    );
    for command in commands {
        assert_eq!(
            command["availability"], "available",
            "`{}` gates on discovery, which puts --desktop-descriptor out of reach",
            command["id"]
        );
    }

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
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = ["sre.overview", "sre.events"].into_iter().collect();
    assert_eq!(
        actual, expected,
        "SRE command coverage list changed; add a specific smoke assertion for the new command before accepting it"
    );
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
fn capability_search_finds_library_seeding_and_native_resolution() {
    for (query, expected) in [
        ("seed library", "library.seed"),
        ("as-built library", "library.seed"),
        ("new-design library", "library.seed"),
        ("pinned native assets", "library.resolve-native"),
        ("differential handoff", "library.resolve-native"),
    ] {
        let data = ok(&["capabilities", "--search", query, "--output", "json"]);
        let results = data["results"].as_array().expect("search results");
        assert!(
            results.iter().any(|row| row["id"] == expected),
            "`{query}` did not find {expected}: {results:?}"
        );
    }
}

#[test]
fn capability_search_finds_terrain_waterfall_and_visible_deviation_labels() {
    for (query, expected) in [
        ("terrain waterfall", "pls.terrain-reconcile"),
        ("terrain datum", "pls.terrain-reconcile"),
        ("surveyed route seams", "pls.terrain-reconcile"),
        ("deviation labels", "pls.deviation-labels"),
        ("occupied endpoints", "pls.deviation-labels"),
        ("feature-code vertices", "pls.deviation-labels"),
        ("delivery verification", "pls.delivery-verify"),
        ("OPGW support-chain readback", "pls.delivery-verify"),
    ] {
        let data = ok(&["capabilities", "--search", query, "--output", "json"]);
        let results = data["results"].as_array().expect("search results");
        assert!(
            results.iter().any(|row| row["id"] == expected),
            "`{query}` did not find {expected}: {results:?}"
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

/// `ds feedback` is a paired domain, so a machine without DS GridDesign
/// cannot prove what a close does to the backlog. What it *can* prove — and
/// what a caller depends on — is that every bound is enforced here, before the
/// bridge is touched, so a malformed triage call costs one local refusal
/// rather than a round trip and an application error.
#[test]
fn feedback_triage_bounds_are_enforced_before_the_bridge() {
    let over_limit = ds(&["feedback", "list", "--limit", "999", "--output", "json"]);
    assert_ne!(over_limit.code, 0);
    assert_eq!(over_limit.envelope["error"]["code"], "invalid_number");
    assert_eq!(over_limit.envelope["error"]["detail"]["max"], 50);

    // The resolution is the record a reader sees instead of reopening the
    // investigation; the bound is ds-brain's own, held here by hand.
    let long_resolution = "x".repeat(1_001);
    let over_resolution = ds(&[
        "feedback",
        "close",
        "--id",
        "feedback-1",
        "--resolution",
        &long_resolution,
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(over_resolution.code, 0);
    assert_eq!(over_resolution.envelope["error"]["code"], "invalid_text");

    // Closing names the two addressed statuses only. Reopening a report stays
    // a human triage decision in the application.
    let reopen = ds(&[
        "feedback",
        "close",
        "--id",
        "feedback-1",
        "--resolution",
        "Reopening.",
        "--status",
        "open",
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(reopen.code, 0);
    assert_eq!(reopen.envelope["error"]["code"], "invalid_choice");

    // A triage decision on shared state is never taken from an unconfirmed
    // invocation, whatever else is right about it.
    let unconfirmed = ds(&[
        "feedback",
        "close",
        "--id",
        "feedback-1",
        "--resolution",
        "Fixed by this session.",
        "--output",
        "json",
    ]);
    assert_ne!(unconfirmed.code, 0);
    assert_eq!(
        unconfirmed.envelope["error"]["code"],
        "confirmation_required"
    );

    // Whatever this machine's desktop situation, a well-formed triage call
    // must reach the bridge and stop there — never an input refusal, and
    // never `undeclared_bridge_argument`, which would mean a handler built an
    // argument key its own BridgeOp does not declare. The id below belongs to
    // no report, so a paired machine refuses it rather than closing anything.
    for args in [
        vec!["feedback", "list", "--view", "all", "--output", "json"],
        vec![
            "feedback",
            "close",
            "--id",
            "ds-cli-smoke-no-such-report",
            "--resolution",
            "Verified against the acceptance condition.",
            "--yes",
            "--output",
            "json",
        ],
    ] {
        let code = refusal(&args);
        assert!(
            code.is_empty()
                || PAIRING_CODES.contains(&code.as_str())
                || matches!(
                    code.as_str(),
                    "feedback_not_found" | "feedback_not_permitted" | "feedback_conflict"
                ),
            "`ds {}` failed with `{code}`, which is neither a pairing outcome \
             nor a named triage refusal",
            args.join(" ")
        );
    }
}

// ---------------------------------------------------------------------------
// style — the cartographic axis
// ---------------------------------------------------------------------------

/// Everything `ds style cartography` can decide without the application.
///
/// The axis is deliberately field-free, so unlike the second dimension there
/// is no `ds style read` value list to check a flag against. That makes what
/// *is* locally decidable worth pinning: the bounds, the two closed
/// vocabularies, the seamless tile sizes, and the two contradictions that
/// cannot be right under any document state.
#[test]
fn style_cartography_validates_its_own_inputs_before_it_opens_the_bridge() {
    for (case, args, expected) in [
        (
            "a call that asks for nothing",
            vec!["style", "cartography", "plan", "--ref", "master/mv_lines"],
            "invalid_cartography",
        ),
        (
            "arrow size sent with a dashed line in the same call",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/mv_lines",
                "--line-type",
                "dashed",
                "--direction-size",
                "14",
            ],
            "invalid_cartography",
        ),
        (
            "hatch detail sent with the pattern that removes the hatch",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/service_areas",
                "--fill-pattern",
                "solid",
                "--pattern-color",
                "#B45309",
            ],
            "invalid_cartography",
        ),
        (
            "an arrow smaller than the marker bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/water_mains",
                "--line-type",
                "directional",
                "--direction-size",
                "5",
            ],
            "invalid_number",
        ),
        (
            "arrows spaced beyond the bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/water_mains",
                "--line-type",
                "directional",
                "--direction-spacing",
                "1001",
            ],
            "invalid_number",
        ),
        (
            "a casing wider than the bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/mv_lines",
                "--casing-width",
                "21",
            ],
            "invalid_number",
        ),
        (
            "a hatch stroke wider than the bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/service_areas",
                "--pattern-stroke",
                "7",
            ],
            "invalid_number",
        ),
        (
            "a casing colour that is not a colour",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/mv_lines",
                "--casing-color",
                "navy",
            ],
            "invalid_color",
        ),
        (
            "a line type outside the closed vocabulary",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/mv_lines",
                "--line-type",
                "dash_dot",
            ],
            "invalid_choice",
        ),
        (
            "a fill pattern outside the closed vocabulary",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/service_areas",
                "--fill-pattern",
                "hatch",
            ],
            "invalid_choice",
        ),
        (
            // MapLibre repeats a pattern by tiling its image, so only the
            // powers of two meet without a visible seam at every tile edge.
            "a pattern tile size that would seam",
            vec![
                "style",
                "cartography",
                "plan",
                "--ref",
                "master/service_areas",
                "--fill-pattern",
                "crosshatch",
                "--pattern-spacing",
                "12",
            ],
            "invalid_choice",
        ),
        (
            "a publish with no confirmation",
            vec![
                "style",
                "cartography",
                "set",
                "--ref",
                "master/mv_lines",
                "--line-type",
                "dashed",
            ],
            "confirmation_required",
        ),
    ] {
        let mut args = args;
        args.extend(["--output", "json"]);
        assert_eq!(
            refusal(&args),
            expected,
            "{case}: `ds {}` did not refuse with `{expected}`",
            args.join(" ")
        );
    }

    // Every seamless size is accepted, so the closed choice is a real bound
    // rather than one spelling that happens to work.
    for spacing in ["4", "8", "16", "32"] {
        let code = refusal(&[
            "style",
            "cartography",
            "plan",
            "--ref",
            "master/service_areas",
            "--fill-pattern",
            "crosshatch",
            "--pattern-spacing",
            spacing,
            "--output",
            "json",
        ]);
        assert!(
            code.is_empty() || PAIRING_CODES.contains(&code.as_str()),
            "`--pattern-spacing {spacing}` is seamless but was refused with `{code}`"
        );
    }

    // Adjusting arrow spacing on a ref that already carries the directional
    // type is legitimate: `ds` has not read the document, so it must not
    // invent a contradiction. This one is well formed and may only end at the
    // bridge.
    let adjust = refusal(&[
        "style",
        "cartography",
        "plan",
        "--ref",
        "master/water_mains",
        "--direction-spacing",
        "140",
        "--output",
        "json",
    ]);
    assert!(
        adjust.is_empty() || PAIRING_CODES.contains(&adjust.as_str()),
        "adjusting arrow spacing alone failed with `{adjust}`, which is not a pairing outcome"
    );
}

/// Each of the three scenarios the cartography commands exist for is one
/// well-formed call that reaches the bridge and stops there — never an input
/// refusal, and never `undeclared_bridge_argument`, which would mean a
/// handler built an argument key its own BridgeOp does not declare.
#[test]
fn a_well_formed_cartography_call_stops_at_confirmation_or_pairing() {
    for args in [
        // Directional water-flow lines.
        vec![
            "style",
            "cartography",
            "plan",
            "--ref",
            "master/water_mains",
            "--line-type",
            "directional",
            "--direction-size",
            "14",
            "--direction-spacing",
            "140",
            "--output",
            "json",
        ],
        // Satellite-contrast casing, on the fractional bound.
        vec![
            "style",
            "cartography",
            "plan",
            "--ref",
            "master/mv_lines",
            "--casing-color",
            "#0F172A",
            "--casing-width",
            "2.5",
            "--output",
            "json",
        ],
        // Crosshatched proposed service areas, every pattern flag at once so
        // every declared key is built at least once by a real invocation.
        vec![
            "style",
            "cartography",
            "plan",
            "--ref",
            "master/service_areas",
            "--fill-pattern",
            "crosshatch",
            "--pattern-color",
            "#B45309",
            "--pattern-background",
            "#FFFFFF00",
            "--pattern-spacing",
            "8",
            "--pattern-stroke",
            "1",
            "--output",
            "json",
        ],
        // The same, published.
        vec![
            "style",
            "cartography",
            "set",
            "--ref",
            "master/service_areas",
            "--fill-pattern",
            "crosshatch",
            "--pattern-color",
            "#B45309",
            "--pattern-spacing",
            "8",
            "--yes",
            "--output",
            "json",
        ],
    ] {
        let code = refusal(&args);
        assert!(
            code.is_empty() || PAIRING_CODES.contains(&code.as_str()),
            "`ds {}` failed with `{code}`, which is not a pairing outcome",
            args.join(" ")
        );
    }
}

/// The three scenarios are what an operator says, not what the flags are
/// called. Discovery has to survive that gap, or the axis stays unreachable
/// to anyone who did not already know it exists.
#[test]
fn capability_search_finds_cartography_by_operator_vocabulary() {
    for (query, expected) in [
        ("water flow direction", "style.cartography.plan"),
        ("directional arrows", "style.cartography.plan"),
        ("satellite contrast casing", "style.cartography.plan"),
        ("dashed line type", "style.cartography.plan"),
        (
            "crosshatch proposed service areas",
            "style.cartography.plan",
        ),
        ("hatching a fill", "style.cartography.plan"),
        ("publish casing satellite", "style.cartography.set"),
    ] {
        let data = ok(&["capabilities", "--search", query, "--output", "json"]);
        let results = data["results"].as_array().expect("search results");
        assert!(
            results.iter().any(|row| row["id"] == expected),
            "`{query}` did not find {expected}: {results:?}"
        );
    }
}

/// Each scenario is also a declared example, so an agent that has found the
/// command is handed the exact invocation instead of a set of flags to
/// assemble. A summary check would not catch this: an example that stops
/// carrying its scenario's flags is how the vocabulary a caller searches for
/// and the contract they then run drift apart.
#[test]
fn every_cartography_scenario_is_a_declared_example_of_its_own_command() {
    for id in ["style.cartography.plan", "style.cartography.set"] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        let examples: Vec<String> = command["examples"]
            .as_array()
            .expect("examples")
            .iter()
            .filter_map(|example| example["command"].as_str())
            .map(str::to_string)
            .collect();
        for scenario in [
            "--line-type directional",
            "--casing-color",
            "--fill-pattern crosshatch",
        ] {
            assert!(
                examples.iter().any(|example| example.contains(scenario)),
                "`{id}` declares no example for `{scenario}`: {examples:?}"
            );
        }
        let path = command["path"]
            .as_array()
            .expect("path")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        for example in &examples {
            assert!(
                example.starts_with(&format!("ds {path} ")),
                "`{id}` example `{example}` does not invoke `ds {path}`"
            );
        }
    }
}
