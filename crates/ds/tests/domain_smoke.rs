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
use serde_json::{Value, json};

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
    let manifest =
        parse_standards_library_manifest(&std::fs::read(version.join("manifest.json")).unwrap())
            .unwrap();
    let native_path = manifest.members[0].pls_cadd_path.clone();
    assert_eq!(native_path, "pls-cadd/structures/source/pole.012");
    assert!(version.join(&native_path).is_file());
    let release =
        unpack_library(&std::fs::read(version.join("dsgrid/library.dsgrid-library")).unwrap())
            .unwrap();
    assert!(
        release.assets.is_empty(),
        "DS Grid seed must not embed PLS bytes"
    );

    let second = ok(&args);
    assert_eq!(second["idempotent"], true);

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
    std::fs::create_dir_all(misrouted.join(&native_path).parent().unwrap()).unwrap();
    std::fs::copy(
        version.join("manifest.json"),
        misrouted.join("manifest.json"),
    )
    .unwrap();
    std::fs::copy(version.join(&native_path), misrouted.join(&native_path)).unwrap();
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
fn dsgrid_run_executes_native_reads_and_never_admits_a_mutation() {
    let model = common::fixture();
    let data = ok(&[
        "dsgrid",
        "run",
        "--model",
        &model,
        "--operation",
        "project_plan",
        "--limit",
        "2",
        "--output",
        "json",
    ]);
    assert_eq!(data["operation"]["id"], "project_plan");
    assert_eq!(data["operation"]["effect"], "read");
    assert_eq!(data["operation"]["journaled"], false);
    assert_eq!(data["staged"], false);
    assert_eq!(data["persisted"], false);
    assert!(
        data["source"]["authored_revision"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(data["result"].as_array().expect("plan rows").len(), 2);
    assert!(data["result"].as_array().unwrap().iter().all(|row| {
        row["entity_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    }));
    assert!(data["more"]["truncated"].as_array().is_some());

    let feature_codes = ok(&[
        "dsgrid",
        "run",
        "--model",
        &model,
        "--operation",
        "feature_code_report",
        "--output",
        "json",
    ]);
    assert_eq!(
        feature_codes["operation"]["result_type"],
        "FeatureCodeResolution"
    );

    let refused = ds(&[
        "dsgrid",
        "run",
        "--model",
        &model,
        "--operation",
        "create_alignment",
        "--output",
        "json",
    ]);
    assert_ne!(refused.code, 0);
    assert_eq!(refused.envelope["error"]["code"], "operation_not_read_only");
}

#[test]
fn dsgrid_inspect_and_validate_expose_the_authored_revision() {
    let model = common::fixture();
    let inspected = ok(&[
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "authored-revision",
        "--output",
        "json",
    ]);
    assert_eq!(inspected["decoded"], true);
    assert!(
        inspected["authored_revision"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let validated = ok(&["dsgrid", "validate", "--model", &model, "--output", "json"]);
    assert_eq!(
        validated["model"]["authored_revision"],
        inspected["authored_revision"]
    );
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

// ---------------------------------------------------------------------------
// dsgrid model — the paired application's local model lifecycle
// ---------------------------------------------------------------------------
//
// A local model lives inside a running application, so there is no fixture
// that can stand in for one: an assertion about which model occupies Profile
// needs the desktop, and the desktop is not present on CI.
//
// What *is* assertable everywhere is the boundary this family exists to hold,
// and it is a vocabulary boundary — the one a reverted `dsgrid model
// create/import/convert` family got wrong. Each check below is a negative
// control for one thing that family conflated, and every one of them holds on
// a machine with no application at all.

#[test]
fn the_local_dsgrid_model_family_neither_requires_nor_accepts_a_project() {
    // The load-bearing property. A local model is the application's own state;
    // asking for a project to list, create, acquire or open one would make a
    // projectless session — an ordinary session for this family — unable to do
    // any of it. `--project` must be an unknown flag, not an optional one.
    for path in [
        vec!["dsgrid", "model", "list"],
        vec!["dsgrid", "model", "create-local"],
        vec!["dsgrid", "model", "import-external"],
        vec!["dsgrid", "model", "set-active"],
    ] {
        let mut args = path.clone();
        args.extend(["--project", "ds-project-1", "--output", "json"]);
        let run = ds(&args);
        assert_eq!(
            run.envelope["error"]["code"],
            "unknown_flag",
            "`ds {}` accepted a project; the local family must be project-independent",
            path.join(" ")
        );

        let descriptor = ok(&[
            "capabilities",
            &format!("dsgrid.model.{}", path[2]),
            "--output",
            "json",
        ]);
        let command = &descriptor["command"];
        assert_eq!(
            command["authority"], "desktop_pairing",
            "`{}` must prove a transport, never a project",
            command["id"]
        );
        for input in command["inputs"].as_array().expect("inputs") {
            let name = input["name"].as_str().unwrap_or("");
            assert!(
                !name.contains("project"),
                "`{}` declares the project input `{name}`",
                command["id"]
            );
        }
    }
}

#[test]
fn local_dsgrid_model_commands_reach_the_bridge_with_a_well_formed_call() {
    // The other half of the same claim: a well-formed local call must end in a
    // pairing state, never in an input refusal. Without this, the checks above
    // could pass on a command that refuses everything.
    for args in [
        vec!["dsgrid", "model", "list", "--output", "json"],
        vec!["dsgrid", "model", "create-local", "--output", "json"],
        vec![
            "dsgrid",
            "model",
            "set-active",
            "--model",
            "gm-local-7",
            "--output",
            "json",
        ],
    ] {
        let code = refusal(&args);
        assert!(
            PAIRING_CODES.contains(&code.as_str()),
            "`ds {}` ended in `{code}`, which is not a pairing state",
            args.join(" ")
        );
    }
}

#[test]
fn external_import_is_dsgrid_only_and_routes_conversion_to_the_exchange_boundary() {
    // The conflation the revert reversed, refused locally by name. A second
    // convert-and-project verb would start exactly here: by accepting a PLS
    // workspace or a `.bak` as a model source.
    let root = temp_root("dsgrid-import");
    std::fs::create_dir_all(&root).expect("temp directory is writable");
    let backup = root.join("delivery.bak").display().to_string();
    let folder = root.join("workspace").display().to_string();
    let package = root.join("route.dsgrid").display().to_string();

    for source in [&backup, &folder, &workspace_file("humble-pole.don")] {
        for args in [
            vec![
                "dsgrid",
                "model",
                "import-external",
                "--path",
                source,
                "--name",
                "Route",
                "--output",
                "json",
            ],
            // Publication is confirmed here on purpose: the source check must
            // still be the thing that refuses, and the confirmation gate would
            // otherwise answer first and hide it.
            vec![
                "dsgrid",
                "publish-version",
                "--path",
                source,
                "--name",
                "Route",
                "--kind",
                "mv_line",
                "--yes",
                "--output",
                "json",
            ],
        ] {
            let run = ds(&args);
            assert_eq!(
                run.envelope["error"]["code"], "unsupported_model_source",
                "`{source}` was accepted as a DS Grid package"
            );
            assert!(
                run.envelope["error"]["remedy"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("ds dsgrid-exchange"),
                "the refusal must name where conversion actually lives"
            );
        }
    }

    // A relative path is refused too: the application resolves it in its own
    // working directory, where it means a different file or none.
    assert_eq!(
        refusal(&[
            "dsgrid",
            "model",
            "import-external",
            "--path",
            "route.dsgrid",
            "--output",
            "json",
        ]),
        "absolute_path_required"
    );
    // And a well-formed one reaches the bridge rather than an input refusal.
    let code = refusal(&[
        "dsgrid",
        "model",
        "import-external",
        "--path",
        &package,
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&code.as_str()),
        "a valid .dsgrid path ended in `{code}` instead of a pairing state"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn publishing_a_version_is_confirmation_gated_and_never_implies_local_activation() {
    // Publication writes a governed project revision, so the gate is in
    // dispatch, before the handler — it holds with no application at all.
    let run = ds(&[
        "dsgrid",
        "publish-version",
        "--name",
        "Kamonyi MV",
        "--kind",
        "mv_line",
        "--output",
        "json",
    ]);
    assert_eq!(
        run.envelope["error"]["code"], "confirmation_required",
        "`ds dsgrid publish-version` reached past the confirmation gate"
    );
    assert_ne!(run.code, 0);

    // There is no durable exclusive "activate this revision for the project"
    // authority anywhere in this stack, so no command may be named or
    // documented as though there were, and publication must say what it left
    // alone rather than leaving it to be inferred.
    let commands = ok(&["capabilities", "dsgrid", "--output", "json"]);
    for command in commands["commands"].as_array().expect("commands") {
        let id = command["id"].as_str().expect("id");
        assert!(
            !id.contains("activate"),
            "`{id}` names an activation this stack has no authority for"
        );
    }
    let publish =
        ok(&["capabilities", "dsgrid.publish-version", "--output", "json"])["command"].clone();
    assert_eq!(publish["effect"], "global_write");
    assert_eq!(publish["authority"], "project");
    assert_eq!(publish["confirmation_required"], true);
    let output = publish["output"].as_str().expect("output");
    assert!(
        output.contains("active_model_changed"),
        "the publication receipt must report what it left alone, so `published` \
         is never read as `now current`"
    );
}

#[test]
fn publication_takes_one_source_and_refuses_to_become_a_rename() {
    // Two selectors would leave the application to guess which source the
    // operator meant, and it cannot.
    assert_eq!(
        refusal(&[
            "dsgrid",
            "publish-version",
            "--model",
            "gm-local-7",
            "--path",
            "/models/route.dsgrid",
            "--project-model",
            "gm-9",
            "--yes",
            "--output",
            "json",
        ]),
        "ambiguous_publish_source"
    );
    // Project resource ids are generated, not authored: naming one and a
    // display name together is a rename request wearing a publication's
    // clothes, and ds-web refuses the same pairing.
    assert_eq!(
        refusal(&[
            "dsgrid",
            "publish-version",
            "--project-model",
            "gm-9",
            "--name",
            "Renamed",
            "--yes",
            "--output",
            "json",
        ]),
        "project_model_rename_unsupported"
    );
    // A new project model needs both an authored name and a kind; refusing
    // locally saves a capture and an upload that would be discarded.
    for args in [
        vec!["dsgrid", "publish-version", "--name", "Kamonyi MV"],
        vec!["dsgrid", "publish-version", "--kind", "mv_line"],
        vec!["dsgrid", "publish-version"],
    ] {
        let mut args = args;
        args.extend(["--yes", "--output", "json"]);
        assert_eq!(
            refusal(&args),
            "new_project_model_incomplete",
            "`ds {}` was accepted without a complete new project model",
            args.join(" ")
        );
    }
    assert_eq!(
        refusal(&[
            "dsgrid",
            "publish-version",
            "--name",
            "Kamonyi MV",
            "--kind",
            "substation",
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_choice",
        "--kind must be one of the project catalogue's own kinds"
    );
    // An existing project model needs neither, and the call reaches the bridge.
    let code = refusal(&[
        "dsgrid",
        "publish-version",
        "--project-model",
        "gm-9",
        "--expected-head",
        "r-7",
        "--reason",
        "Spotted route",
        "--yes",
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&code.as_str()),
        "a complete publication ended in `{code}` instead of a pairing state"
    );
}

#[test]
fn no_dsgrid_model_command_can_carry_model_content() {
    // Bytes never travel in CLI or MCP JSON: a source is a filesystem path the
    // shell prepares or an opaque local model id, and a receipt addresses
    // content by digest and byte count. The declared inputs are the only place
    // a content field could enter.
    let commands = ok(&["capabilities", "dsgrid", "--output", "json"]);
    let mut checked = 0usize;
    for command in commands["commands"].as_array().expect("commands") {
        let id = command["id"].as_str().expect("id");
        if !(id.starts_with("dsgrid.model.") || id == "dsgrid.publish-version") {
            continue;
        }
        checked += 1;
        let descriptor = ok(&["capabilities", id, "--output", "json"])["command"].clone();
        for input in descriptor["inputs"].as_array().expect("inputs") {
            let name = input["name"].as_str().unwrap_or("");
            assert!(
                !matches!(name, "bytes" | "content" | "base64" | "blob" | "data"),
                "`{id}` declares the content-carrying input `{name}`"
            );
        }
    }
    assert_eq!(
        checked, 5,
        "the DS Grid model family must be five commands; this check would \
         otherwise silently stop covering one"
    );
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
fn pls_backup_create_frames_every_real_workspace_member_and_refuses_overwrite() {
    let source = workspace();
    let root = temp_root("pls-backup-create");
    std::fs::create_dir_all(&root).unwrap();
    let backup = root.join("humble-submission.bak");
    let backup_text = backup.display().to_string();

    let unconfirmed = ds(&[
        "pls",
        "backup-create",
        "--workspace",
        &source,
        "--out",
        &backup_text,
        "--output",
        "json",
    ]);
    assert_eq!(unconfirmed.code, 2);
    assert_eq!(
        unconfirmed.envelope["error"]["code"],
        "confirmation_required"
    );
    assert!(!backup.exists(), "an unconfirmed call must write nothing");

    let created = ok(&[
        "pls",
        "backup-create",
        "--workspace",
        &source,
        "--out",
        &backup_text,
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(created["file_members"], 15);
    assert_eq!(created["member_bytes_preserved"], true);
    assert_eq!(created["path_healing_performed"], false);
    assert_eq!(created["native_restore_reopen_required"], true);
    assert_eq!(created["native_restore_reopen_accepted"], false);
    assert!(backup.is_file());

    let inspected = ok(&[
        "dsgrid-exchange",
        "inspect",
        "--source",
        &backup_text,
        "--output",
        "json",
    ]);
    assert_eq!(inspected["sources"][0]["kind"], "PlsBackupContainer");
    assert_eq!(inspected["sources"][0]["counts"]["members"], 15);

    let overwrite = ds(&[
        "pls",
        "backup-create",
        "--workspace",
        &source,
        "--out",
        &backup_text,
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(overwrite.envelope["error"]["code"], "output_exists");
    let _ = std::fs::remove_dir_all(&root);
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
// data elevation
// ---------------------------------------------------------------------------

#[test]
fn native_elevation_validates_absolute_paths_before_pairing() {
    let absolute_source = temp_root("elevation-source")
        .join("points.csv")
        .display()
        .to_string();
    let absolute_out = temp_root("elevation-out")
        .join("points.geojson")
        .display()
        .to_string();
    for args in [
        vec![
            "data",
            "elevation",
            "attach",
            "--source",
            "points.csv",
            "--out",
            absolute_out.as_str(),
            "--output",
            "json",
        ],
        vec![
            "data",
            "elevation",
            "attach",
            "--source",
            absolute_source.as_str(),
            "--out",
            "points.geojson",
            "--output",
            "json",
        ],
    ] {
        let run = ds(&args);
        assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
        assert_eq!(run.envelope["error"]["code"], "absolute_path_required");
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
fn map_design_set_refuses_non_scalar_and_duplicate_values_before_pairing() {
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--set",
            "tags:=[1]",
            "--output",
            "json",
        ]),
        "invalid_property_value"
    );
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--set",
            "enabled=true",
            "--set",
            "enabled:=true",
            "--output",
            "json",
        ]),
        "duplicate_property"
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
        "map.design.open",
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
        "map.design.version.list",
        "map.design.version.play",
        "map.design.version.compare",
        "map.design.process",
        "map.design.batch.process",
        "map.design.batch.report",
        "map.design.batch.save",
        "map.design.save",
        "map.design.list",
        "map.design.pin",
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
fn map_design_open_is_the_one_discoverable_visible_context_entry() {
    let descriptor = ok(&["capabilities", "map.design.open", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["authority"], "project");
    assert_eq!(command["effect"], "local_ui");
    assert_eq!(command["confirmation_required"], false);
    let inputs = command["inputs"].as_array().expect("inputs");
    assert_eq!(
        inputs
            .iter()
            .map(|input| input["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["transformer", "desktop-descriptor"]
    );
    assert_eq!(inputs[0]["required"], true);
    let refusals = command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .map(|refusal| refusal["code"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        refusals,
        BTreeSet::from([
            "desktop_not_paired",
            "desktop_ambiguous",
            "desktop_unreachable",
            "pairing_rejected",
            "desktop_signed_out",
            "desktop_refused",
            "transformer_not_found",
            "project_mismatch",
            "dirty_room",
            "desktop_operation_unsupported",
            "desktop_unreadable",
        ])
    );

    for query in [
        "open transformer",
        "transformer edit context",
        "activate design",
    ] {
        let result = ok(&["capabilities", "--search", query, "--output", "json"]);
        let matches = result["results"].as_array().expect("search results");
        assert!(
            matches.iter().any(|row| row["id"] == "map.design.open"),
            "`{query}` did not find the canonical context-entry command: {matches:?}"
        );
    }
}

#[test]
fn map_design_version_history_is_discoverable_and_governed() {
    let expected = [
        (
            "map.design.version.list",
            "read_only",
            vec!["transformer", "desktop-descriptor"],
        ),
        (
            "map.design.version.play",
            "local_ui",
            vec!["transformer", "version", "desktop-descriptor"],
        ),
        (
            "map.design.version.compare",
            "local_ui",
            vec!["transformer", "from", "to", "desktop-descriptor"],
        ),
    ];
    for (id, effect, inputs) in expected {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["authority"], "project", "{id}");
        assert_eq!(command["effect"], effect, "{id}");
        assert_eq!(command["confirmation_required"], false, "{id}");
        assert_eq!(
            command["inputs"]
                .as_array()
                .expect("inputs")
                .iter()
                .map(|input| input["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            inputs,
            "{id}",
        );
    }

    for (query, id) in [
        (
            "list retained transformer versions",
            "map.design.version.list",
        ),
        ("play transformer version", "map.design.version.play"),
        ("compare design version head", "map.design.version.compare"),
    ] {
        let result = ok(&["capabilities", "--search", query, "--output", "json"]);
        assert!(
            result["results"]
                .as_array()
                .expect("results")
                .iter()
                .any(|row| row["id"] == id),
            "`{query}` did not discover {id}",
        );
    }
}

#[test]
fn a_well_formed_map_call_stops_at_confirmation_or_pairing() {
    // This test intentionally never passes --yes. A real paired desktop could
    // otherwise execute a shared write while running a smoke suite. Reads
    // reach the pairing boundary; writes must stop at confirmation first.
    let descriptor = temp_root("map-smoke-unreachable")
        .join("session.json")
        .display()
        .to_string();
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
            "pin",
            "--transformer",
            "T-1042",
            "--mode",
            "add",
        ],
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
        argv.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
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
// design
// ---------------------------------------------------------------------------
//
// Fast LV has two file-owned commands in this domain: a governed project
// snapshot export and the project-free native process. They are intentionally
// exercised here before the paired collaboration refusals below.

#[test]
fn design_lv_process_runs_the_native_batch_without_project_or_desktop_state() {
    let root = temp_root("native-fast-lv");
    std::fs::create_dir_all(&root).unwrap();
    let input_path = root.join("request.json");
    let output_path = root.join("result.json");
    let job = |name: &str| {
        json!({
            "transformer_name": name,
            "gdfs": {
                "tr": {
                    "type": "FeatureCollection",
                    "features": [{
                        "type": "Feature",
                        "id": format!("{name}-tr"),
                        "geometry": { "type": "Point", "coordinates": [30.0, -2.0] },
                        "properties": { "name": name, "names": name }
                    }]
                },
                "lv_lines": {
                    "type": "FeatureCollection",
                    "features": [{
                        "type": "Feature",
                        "id": format!("{name}-line"),
                        "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], [30.0004, -2.0]] },
                        "properties": {}
                    }]
                },
                "customers": { "type": "FeatureCollection", "features": [] }
            },
            "settings": {}
        })
    };
    std::fs::write(
        &input_path,
        serde_json::to_vec(&json!({
            "schema": "ds.fast-lv.request/v1",
            "jobs": [job("T2"), job("T1")]
        }))
        .unwrap(),
    )
    .unwrap();

    let input = input_path.display().to_string();
    let output = output_path.display().to_string();
    let run = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "design", "lv", "process", "--input", &input, "--out", &output, "--output", "json",
        ])
        // A stale explicit Desktop descriptor would fail any bridge-backed
        // command. Native Fast LV must never inspect it.
        .env("DS_DESKTOP_DESCRIPTOR", root.join("stale-desktop.json"))
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let envelope: Value = serde_json::from_slice(&run.stdout).unwrap();
    let receipt = &envelope["data"];
    assert_eq!(receipt["execution_environment"], "native");
    assert_eq!(receipt["jobs"], 2);
    assert_eq!(receipt["succeeded"], 2);
    assert_eq!(receipt["failed"], 0);
    assert_eq!(receipt["results"][0]["transformer_name"], "T2");
    assert_eq!(receipt["results"][1]["transformer_name"], "T1");

    let result: Value = serde_json::from_slice(&std::fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(result["schema"], "ds.fast-lv.result/v1");
    assert_eq!(result["jobs"][0]["transformer_name"], "T2");
    assert!(result.get("ds_project").is_none());
    assert!(result["jobs"][0].get("ds_project").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn design_lv_project_export_refuses_an_existing_artifact_before_auth_or_desktop() {
    let root = temp_root("native-fast-lv-project-export");
    std::fs::create_dir_all(&root).unwrap();
    let output_path = root.join("request.json");
    std::fs::write(&output_path, b"operator-owned").unwrap();
    let profile_path = root.join("catalog.json");
    let profile = |project: &str, gateway: &str, digest: &str| {
        json!({
            "firebase": { "project_id": project, "api_key": "firebase-public" },
            "gateway": { "origin": gateway, "api_key": "gateway-public" },
            "auth_link_begin": { "method": "POST", "path": "/api/v1/auth/device/begin" },
            "auth_link_status": { "method": "POST", "path": "/api/v1/auth/device/status" },
            "auth_link_complete": { "method": "POST", "path": "/api/v1/auth/device/complete" },
            "auth_device_refresh": { "method": "POST", "path": "/api/v1/auth/device/refresh" },
            "auth_device_list": { "method": "GET", "path": "/api/v1/auth/devices" },
            "auth_device_read": { "method": "GET", "path_template": "/api/v1/auth/devices/{device_id}" },
            "auth_device_revoke": { "method": "DELETE", "path_template": "/api/v1/auth/devices/{device_id}" },
            "projects_read": { "method": "GET", "path": "/api/v1/user/projects" },
            "transformer_context": {
                "method": "POST",
                "path": "/api/v1/data",
                "action": "get_transformers_data",
                "fields": "context"
            },
            "project_forms": {
                "method": "POST",
                "path": "/api/v1/project-forms",
                "action": "activate",
                "settings_editor_action": "settings_editor"
            },
            "solar_snapshot": {
                "method": "POST",
                "path": "/api/v1/solar",
                "action": "desktop_snapshot"
            },
            "survey_query": {
                "method": "POST",
                "path": "/api/v1/survey/query"
            },
            "survey_entries_select": {
                "method": "POST",
                "path": "/api/v1/survey/entries/select"
            },
            "survey_entries_changes": {
                "method": "POST",
                "path": "/api/v1/survey/entries/changes"
            },
            "survey_entry_create": {
                "method": "POST",
                "path": "/api/v1/entries/mutate",
                "operation": "create"
            },
            "provenance": { "source_revision": "abc123", "descriptor_sha256": digest }
        })
    };
    std::fs::write(
        &profile_path,
        serde_json::to_vec(&json!({
            "schema_version": "ds.native-client-profiles/v10",
            "development": true,
            "profiles": {
                "stable": profile(
                    "stable-project",
                    "https://stable.ue.gateway.dev",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                ),
                "canary": profile(
                    "canary-project",
                    "https://ds-canary.ue.gateway.dev",
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                )
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let output = output_path.display().to_string();
    let run = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "design",
            "lv",
            "project-export",
            "--transformer",
            "T-1",
            "--out",
            &output,
            "--output",
            "json",
        ])
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &profile_path)
        .env("DS_DESKTOP_DESCRIPTOR", root.join("stale-desktop.json"))
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    assert_eq!(run.status.code(), Some(5));
    let envelope: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(envelope["error"]["code"], "fast_lv_request_output_exists");
    assert_eq!(std::fs::read(&output_path).unwrap(), b"operator-owned");
    let _ = std::fs::remove_dir_all(&root);
}

//
// Design collaboration lives behind ds-brain and is reached through the paired
// application, so no fixture can stand in for a saved selection or a comment
// thread. What IS assertable on every machine, and where this domain's real
// bugs would live, is the ordering claim: every handler validates its own
// inputs and stops at the confirmation gate BEFORE it opens the bridge. If a
// handler were rewritten to resolve the paired session first, none of these
// codes would ever be seen by anyone without the desktop installed.

#[test]
fn design_validates_its_own_inputs_before_it_opens_the_bridge() {
    assert_eq!(
        refusal(&[
            "design",
            "tag",
            "define",
            "--definition",
            "scope",
            "--name",
            "Scope",
            "--values",
            " , ,",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_value_list",
        "a list flag with nothing in it must be refused before a project round trip"
    );
    let many = (0..600)
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        refusal(&[
            "design",
            "selection",
            "save",
            "--name",
            "Everything",
            "--transformers",
            &many,
            "--output",
            "json",
            "--yes",
        ]),
        "too_many_values",
        "a selection over the member bound must be refused locally, not by a rejected write"
    );
    assert_eq!(
        refusal(&[
            "design",
            "comment",
            "post",
            "--body",
            "Looks short.",
            "--output",
            "json",
            "--yes",
        ]),
        "missing_comment_target",
        "posting with neither a thread nor a complete anchor must name the choice, not a key"
    );
    assert_eq!(
        refusal(&[
            "design",
            "attachment",
            "list",
            "--kind",
            "survey_entry",
            "--object",
            "x",
            "--output",
            "json",
        ]),
        "invalid_choice",
        "an object kind outside the closed set must be refused by the parser"
    );
    // The governed set is closed and declared on the descriptor, so a third
    // name is refused here rather than after a round trip.
    assert_eq!(
        refusal(&[
            "design",
            "group",
            "preview",
            "--group",
            "region",
            "--transformers",
            "kigali_a",
            "--output",
            "json",
        ]),
        "invalid_choice",
        "a group outside the closed set must be refused by the parser"
    );
    // The batch bound is one transaction's write budget; the projection's is a
    // whole project's export. 300 is over the first and well inside the second,
    // which is the distinction a single shared bound would have erased.
    let three_hundred = (0..300)
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        refusal(&[
            "design",
            "group",
            "preview",
            "--group",
            "city",
            "--transformers",
            &three_hundred,
            "--output",
            "json",
        ]),
        "too_many_values",
        "an over-large batch must be refused locally, not by a rejected write"
    );
    let over_projection = (0..(ds_cli_design::group::MAX_PROJECTION_TRANSFORMERS + 1))
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        refusal(&[
            "design",
            "group",
            "export",
            "--transformers",
            &over_projection,
            "--output",
            "json",
        ]),
        "too_many_values",
        "an over-large projection must be refused locally, not by a rejected read"
    );
}

#[test]
fn a_projection_covers_a_whole_project_where_a_batch_covers_one_transaction() {
    // A live project already carries 202 transformers. Bounding the export at
    // the batch's 200 would split the one report it exists to serve into two
    // documents with two digests, which the report consumer pins separately and
    // will not join.
    let batch = ds_cli_design::group::MAX_GROUP_BATCH;
    let projection = ds_cli_design::group::MAX_PROJECTION_TRANSFORMERS;
    assert_eq!(
        batch, 200,
        "the batch bound moved; it is a transaction budget, not a page size"
    );
    assert!(
        projection >= 2_000,
        "the projection bound is {projection}; it must cover at least 2000 explicit transformers"
    );
    // 202 and the bound itself both reach the bridge rather than a local
    // refusal, so the whole-project export is genuinely available.
    for count in [202, ds_cli_design::group::MAX_PROJECTION_TRANSFORMERS] {
        let names = (0..count)
            .map(|index| format!("t{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let code = refusal(&[
            "design",
            "group",
            "export",
            "--transformers",
            &names,
            "--output",
            "json",
        ]);
        assert!(
            PAIRING_CODES.contains(&code.as_str()),
            "a {count}-transformer export ended in `{code}`, not a pairing state"
        );
    }
}

#[test]
fn a_governed_group_batch_cannot_be_applied_without_the_plan_it_was_previewed_as() {
    // The digest is the fence. `ds` carries the one the server returned and
    // never mints one, so a caller that skips the preview has no digest to
    // send and dispatch refuses on the missing input rather than committing a
    // batch nobody approved.
    assert_eq!(
        refusal(&[
            "design",
            "group",
            "apply",
            "--group",
            "city",
            "--transformers",
            "kigali_a",
            "--value",
            "kigali",
            "--output",
            "json",
            "--yes",
        ]),
        "missing_input",
        "apply without --digest must be refused before the bridge opens"
    );
    // Clearing is its own command, so forgetting --value can never silently
    // become an unassign.
    assert_eq!(
        refusal(&[
            "design",
            "group",
            "apply",
            "--group",
            "city",
            "--transformers",
            "kigali_a",
            "--digest",
            "abc123",
            "--output",
            "json",
            "--yes",
        ]),
        "missing_input",
        "apply with no --value must refuse rather than clear the group"
    );
    // Both refusals come from the parser because both flags are DECLARED
    // required on apply. `--value` is deliberately not shared with `preview`,
    // where omitting it is the meaningful way to plan an unassign.
}

#[test]
fn every_design_write_refuses_without_confirmation() {
    // Every write in this domain mutates governed project state, so dispatch
    // must stop it before the bridge opens.
    for args in [
        vec![
            "design",
            "selection",
            "save",
            "--name",
            "Week 32",
            "--transformers",
            "kigali_a",
        ],
        vec!["design", "selection", "archive", "--selection", "sel-1"],
        vec![
            "design",
            "selection",
            "assign",
            "--selection",
            "sel-1",
            "--title",
            "Review",
        ],
        vec![
            "design",
            "attachment",
            "publish",
            "--kind",
            "lv_transformer",
            "--object",
            "kigali_a",
            "--path",
            "/tmp/a.bak",
        ],
        vec!["design", "attachment", "retire", "--attachment", "att-1"],
        vec![
            "design",
            "tag",
            "define",
            "--definition",
            "scope",
            "--name",
            "Scope",
            "--values",
            "a,b",
        ],
        vec![
            "design",
            "tag",
            "set",
            "--kind",
            "lv_transformer",
            "--object",
            "kigali_a",
            "--definition",
            "scope",
            "--values",
            "a",
        ],
        vec![
            "design", "comment", "post", "--thread", "thread-1", "--body", "Agreed.",
        ],
        vec!["design", "comment", "resolve", "--thread", "thread-1"],
        vec!["design", "comment", "promote", "--thread", "thread-1"],
        vec![
            "design",
            "group",
            "apply",
            "--group",
            "city",
            "--transformers",
            "kigali_a",
            "--value",
            "kigali",
            "--digest",
            "abc123",
        ],
        vec![
            "design",
            "group",
            "unassign",
            "--group",
            "phasing",
            "--transformers",
            "kigali_a",
            "--digest",
            "abc123",
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
fn every_design_command_is_discoverable_without_the_desktop_installed() {
    // Bridge collaboration remains available before pairing. The native
    // feature read is separately discoverable and honestly unavailable when
    // this test build has no digest-pinned release catalog.
    let index = ok(&["capabilities", "design", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    assert_eq!(
        commands.len(),
        28,
        "the design domain should expose its whole family: {commands:?}"
    );
    for command in commands {
        let id = command["id"].as_str().unwrap_or("?");
        assert_eq!(
            command["availability"],
            if matches!(id, "design.features.select" | "design.lv.project-export") {
                "unavailable"
            } else {
                "available"
            },
            "`{id}` has the wrong packaging or Desktop availability"
        );
    }
    // A well-formed read gets as far as pairing and no further.
    let descriptor = temp_root("design-discovery-unreachable")
        .join("session.json")
        .display()
        .to_string();
    let code = refusal(&[
        "design",
        "selection",
        "list",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&code.as_str()),
        "a well-formed design read ended in `{code}`, not a pairing state"
    );
}

#[test]
fn design_reads_are_reads_and_design_writes_are_governed_writes() {
    // The effect classification is what a caller plans around, and it is the
    // one thing a reviewer cannot infer from a command's name: `retire` sounds
    // destructive and is a soft, reversible governed write, while `download`
    // sounds like it moves bytes and changes nothing shared.
    for (id, effect) in [
        ("design.selection.list", "read_only"),
        ("design.selection.read", "read_only"),
        ("design.attachment.list", "read_only"),
        ("design.attachment.download", "read_only"),
        ("design.tag.list", "read_only"),
        ("design.tag.query", "read_only"),
        ("design.comment.list", "read_only"),
        ("design.comment.read", "read_only"),
        ("design.selection.save", "global_write"),
        ("design.selection.assign", "global_write"),
        ("design.attachment.publish", "global_write"),
        ("design.attachment.retire", "global_write"),
        ("design.tag.define", "global_write"),
        ("design.tag.set", "global_write"),
        ("design.comment.post", "global_write"),
        ("design.comment.promote", "global_write"),
        // A governed group's preview and its report projection are reads: they
        // decide and describe, and neither writes a byte. That is what keeps
        // both usable on a project that accepts no changes.
        ("design.group.list", "read_only"),
        ("design.group.preview", "read_only"),
        ("design.group.export", "read_only"),
        ("design.group.apply", "global_write"),
        ("design.group.unassign", "global_write"),
        ("design.consumer-grouping.preview", "read_only"),
        ("design.consumer-grouping.apply", "global_write"),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        assert_eq!(
            descriptor["command"]["effect"], effect,
            "`{id}` declares the wrong blast radius"
        );
        assert_eq!(
            descriptor["command"]["authority"], "project",
            "`{id}` must require a verified principal bound to a project"
        );
    }
    let descriptor = ok(&["capabilities", "design.features.select", "--output", "json"]);
    assert_eq!(descriptor["command"]["effect"], "local_auth_state");
    assert_eq!(descriptor["command"]["authority"], "headless_project");
    let descriptor = ok(&[
        "capabilities",
        "design.lv.project-export",
        "--output",
        "json",
    ]);
    assert_eq!(descriptor["command"]["effect"], "local_file_write");
    assert_eq!(descriptor["command"]["authority"], "headless_project");
}

// ---------------------------------------------------------------------------
// work
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// design collaboration
// ---------------------------------------------------------------------------

#[test]
fn design_collaboration_is_a_complete_headless_project_surface() {
    // Saved Transformer Status selections, versioned attachments, tags and
    // comment threads are project records, not map-owned state. All commands
    // must therefore be discoverable without an open map; reads reach only
    // the paired application and writes stop at the global confirmation gate.
    let index = ok(&["capabilities", "design", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("id"))
        .filter(|id| !id.starts_with("design.lv."))
        .collect();
    let expected: BTreeSet<&str> = [
        "design.selection.list",
        "design.features.select",
        "design.selection.read",
        "design.selection.save",
        "design.selection.archive",
        "design.selection.assign",
        "design.attachment.list",
        "design.attachment.publish",
        "design.attachment.download",
        "design.attachment.retire",
        "design.tag.list",
        "design.tag.query",
        "design.tag.define",
        "design.tag.set",
        "design.group.list",
        "design.group.preview",
        "design.group.apply",
        "design.group.unassign",
        "design.group.export",
        "design.consumer-grouping.preview",
        "design.consumer-grouping.apply",
        "design.comment.list",
        "design.comment.read",
        "design.comment.post",
        "design.comment.resolve",
        "design.comment.promote",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "design collaboration must expose every governed record operation"
    );

    let writes: BTreeSet<&str> = [
        "design.selection.save",
        "design.selection.archive",
        "design.selection.assign",
        "design.attachment.publish",
        "design.attachment.retire",
        "design.tag.define",
        "design.tag.set",
        "design.comment.post",
        "design.comment.resolve",
        "design.comment.promote",
        // The governed group's two committing actions. `list`, `preview` and
        // `export` are reads: they decide and describe, and neither writes a
        // byte, which is what keeps all three usable on a project that accepts
        // no changes.
        "design.group.apply",
        "design.group.unassign",
        "design.consumer-grouping.apply",
    ]
    .into_iter()
    .collect();
    for command in commands {
        let id = command["id"].as_str().expect("id");
        if id.starts_with("design.lv.") {
            continue;
        }
        if id == "design.features.select" {
            assert_eq!(command["availability"], "unavailable");
            assert_eq!(command["effect"], "local_auth_state");
            assert_eq!(command["authority"], "headless_project");
            continue;
        }
        assert_eq!(
            command["availability"], "available",
            "`{id}` must not require an open map"
        );
        assert_eq!(
            command["effect"],
            if writes.contains(id) {
                "global_write"
            } else {
                "read_only"
            }
        );
        if writes.contains(id) {
            let mut args = match id {
                "design.selection.save" => vec![
                    "design",
                    "selection",
                    "save",
                    "--name",
                    "smoke",
                    "--transformers",
                    "T-smoke",
                ],
                "design.selection.archive" => {
                    vec!["design", "selection", "archive", "--selection", "sel-smoke"]
                }
                "design.selection.assign" => vec![
                    "design",
                    "selection",
                    "assign",
                    "--selection",
                    "sel-smoke",
                    "--title",
                    "Smoke",
                ],
                "design.attachment.publish" => vec![
                    "design",
                    "attachment",
                    "publish",
                    "--kind",
                    "lv_transformer",
                    "--object",
                    "T-smoke",
                    "--path",
                    "smoke.bak",
                ],
                "design.attachment.retire" => vec![
                    "design",
                    "attachment",
                    "retire",
                    "--attachment",
                    "att-smoke",
                ],
                "design.tag.define" => vec![
                    "design",
                    "tag",
                    "define",
                    "--definition",
                    "scope",
                    "--name",
                    "Scope",
                    "--values",
                    "smoke",
                ],
                "design.tag.set" => vec![
                    "design",
                    "tag",
                    "set",
                    "--kind",
                    "lv_transformer",
                    "--object",
                    "T-smoke",
                    "--definition",
                    "scope",
                ],
                "design.comment.post" => vec![
                    "design",
                    "comment",
                    "post",
                    "--kind",
                    "lv_transformer",
                    "--object",
                    "T-smoke",
                    "--body",
                    "Smoke",
                ],
                "design.comment.resolve" => {
                    vec!["design", "comment", "resolve", "--thread", "thread-smoke"]
                }
                "design.comment.promote" => {
                    vec!["design", "comment", "promote", "--thread", "thread-smoke"]
                }
                "design.group.apply" => vec![
                    "design",
                    "group",
                    "apply",
                    "--group",
                    "city",
                    "--transformers",
                    "T-smoke",
                    "--value",
                    "kigali",
                    "--digest",
                    "digest-smoke",
                ],
                "design.group.unassign" => vec![
                    "design",
                    "group",
                    "unassign",
                    "--group",
                    "phasing",
                    "--transformers",
                    "T-smoke",
                    "--digest",
                    "digest-smoke",
                ],
                "design.consumer-grouping.apply" => vec![
                    "design",
                    "consumer-grouping",
                    "apply",
                    "--transformers",
                    "T-smoke",
                    "--digest",
                    "digest-smoke",
                ],
                _ => unreachable!("write inventory and command surface diverged: {id}"),
            };
            args.extend(["--output", "json"]);
            assert_eq!(
                refusal(&args),
                "confirmation_required",
                "`{id}` reached past confirmation"
            );
        }
    }

    let descriptor = temp_root("design-collaboration-unreachable")
        .join("session.json")
        .display()
        .to_string();

    // Well-formed reads reach the paired bridge rather than silently asking
    // the map for local state or rejecting a valid shared-record request.
    for args in [
        vec!["design", "selection", "list"],
        vec!["design", "selection", "read", "--selection", "sel-smoke"],
        vec![
            "design",
            "attachment",
            "list",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
        ],
        vec![
            "design",
            "attachment",
            "download",
            "--attachment",
            "att-smoke",
        ],
        vec![
            "design",
            "tag",
            "list",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
        ],
        vec!["design", "tag", "query", "--choice", "city:equals:kigali"],
        vec![
            "design",
            "comment",
            "list",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
        ],
        vec!["design", "comment", "read", "--thread", "thread-smoke"],
    ] {
        let mut argv = args;
        argv.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
        let code = refusal(&argv);
        assert!(
            PAIRING_CODES.contains(&code.as_str()),
            "`ds {}` stopped at `{code}`, not the paired application",
            argv.join(" ")
        );
    }

    assert_eq!(
        refusal(&[
            "design",
            "selection",
            "save",
            "--name",
            "smoke",
            "--transformers",
            ",,",
            "--yes",
            "--output",
            "json"
        ]),
        "invalid_value_list",
        "empty member lists must be refused before pairing",
    );
    assert_eq!(
        refusal(&[
            "design",
            "tag",
            "define",
            "--definition",
            "score",
            "--name",
            "Score",
            "--value-type",
            "number",
            "--values",
            "one",
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_tag_input",
        "a numeric definition must not disguise strings as a vocabulary",
    );
    assert_eq!(
        refusal(&[
            "design",
            "tag",
            "set",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
            "--definition",
            "score",
            "--values",
            "one",
            "--number",
            "1",
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_tag_input",
        "an assignment must carry exactly one value representation",
    );
    assert_eq!(
        refusal(&[
            "design",
            "tag",
            "query",
            "--number",
            "completion:gte:NaN",
            "--output",
            "json",
        ]),
        "invalid_number",
        "numeric query predicates must be finite before pairing",
    );
    let mut too_many_filters = vec!["design", "tag", "query"];
    for _ in 0..=ds_cli_design::MAX_TAG_QUERY_FILTERS {
        too_many_filters.extend(["--presence", "city:exists"]);
    }
    too_many_filters.extend(["--output", "json"]);
    assert_eq!(
        refusal(&too_many_filters),
        "too_many_tag_filters",
        "a query must enforce the backend's predicate-read bound before pairing",
    );
    assert_eq!(
        refusal(&[
            "design",
            "tag",
            "query",
            "--presence",
            "city:exists",
            "--limit",
            "2001",
            "--output",
            "json",
        ]),
        "invalid_number",
        "a result bound past the backend's complete-project limit must refuse locally",
    );
}

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

    // An explicit unreachable descriptor makes the test deterministic even
    // when a production Desktop is open. The previous ambient-discovery form
    // could submit its synthetic report to the real backlog during a local
    // test run.
    let descriptor = temp_root("feedback-smoke-unreachable")
        .join("session.json")
        .display()
        .to_string();
    let base = vec![
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
        "--desktop-descriptor",
        &descriptor,
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
    let descriptor = temp_root("work-smoke-unreachable")
        .join("session.json")
        .display()
        .to_string();
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
        argv.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
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
        ("backup restore workspace", "pls.backup-create"),
        ("exact-byte native backup", "pls.backup-create"),
        ("submission bak", "pls.backup-create"),
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
    let descriptor = temp_root("sre-smoke-unreachable")
        .join("session.json")
        .display()
        .to_string();
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
        argv.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
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
    let descriptor = temp_root("feedback-triage-unreachable")
        .join("session.json")
        .display()
        .to_string();
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
        let mut argv = args.clone();
        argv.extend(["--desktop-descriptor", &descriptor]);
        let code = refusal(&argv);
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

#[test]
fn project_forms_native_reads_preserve_the_explicit_project_desktop_surface() {
    let native = ok(&[
        "capabilities",
        "survey.project-forms.list",
        "--output",
        "json",
    ]);
    assert_eq!(native["command"]["authority"], "headless_project");
    assert_eq!(native["command"]["effect"], "local_auth_state");
    let names = native["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["lane", "limit"]));
    let settings = ok(&[
        "capabilities",
        "survey.project-form.settings",
        "--output",
        "json",
    ]);
    assert_eq!(settings["command"]["authority"], "headless_project");
    assert_eq!(settings["command"]["effect"], "local_auth_state");
    let names = settings["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["form", "lane"]));
    assert!(
        settings["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|arg| arg["name"] != "project" && arg["name"] != "desktop-descriptor")
    );

    let query = ok(&["capabilities", "survey.query", "--output", "json"]);
    assert_eq!(query["command"]["authority"], "headless_project");
    assert_eq!(query["command"]["effect"], "local_auth_state");
    let names = query["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "distinct-field",
            "filter",
            "form",
            "group-by",
            "lane",
            "limit",
            "metric",
            "order",
        ])
    );
    for forbidden in [
        "project",
        "url",
        "body",
        "token",
        "raw",
        "entry",
        "media",
        "desktop-descriptor",
    ] {
        assert!(
            query["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.query exposed --{forbidden}"
        );
    }

    let entries = ok(&["capabilities", "survey.entries.select", "--output", "json"]);
    assert_eq!(entries["command"]["authority"], "headless_project");
    assert_eq!(entries["command"]["effect"], "local_auth_state");
    let names = entries["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["bbox", "form", "lane", "limit"]));
    for forbidden in [
        "project",
        "url",
        "method",
        "body",
        "token",
        "cursor",
        "wkt",
        "geojson",
        "fields",
        "media",
        "deleted",
        "include-deleted",
        "force",
        "authority",
        "desktop-descriptor",
    ] {
        assert!(
            entries["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.select exposed --{forbidden}"
        );
    }
    let invalid = ds(&[
        "survey",
        "entries",
        "select",
        "--form",
        "poles",
        "--bbox",
        "not-a-bbox",
        "--output",
        "json",
    ]);
    assert_eq!(invalid.code, 2);
    assert_eq!(invalid.envelope["error"]["code"], "survey_entries_invalid");

    let changes = ok(&["capabilities", "survey.entries.changes", "--output", "json"]);
    assert_eq!(changes["command"]["authority"], "headless_project");
    assert_eq!(changes["command"]["effect"], "local_auth_state");
    let names = changes["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from(["cursor", "form", "lane", "limit", "updated-after"])
    );
    for forbidden in [
        "project",
        "url",
        "method",
        "body",
        "token",
        "fields",
        "media",
        "deleted",
        "include-deleted",
        "force",
        "authority",
        "desktop-descriptor",
        "auto-pagination",
    ] {
        assert!(
            changes["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.changes exposed --{forbidden}"
        );
    }
    let invalid_changes = ds(&[
        "survey",
        "entries",
        "changes",
        "--form",
        "poles",
        "--updated-after",
        "not-a-time",
        "--output",
        "json",
    ]);
    assert_eq!(invalid_changes.code, 2);
    assert_eq!(
        invalid_changes.envelope["error"]["code"],
        "survey_entries_changes_invalid"
    );

    let create = ok(&["capabilities", "survey.entries.create", "--output", "json"]);
    assert_eq!(create["command"]["authority"], "headless_project");
    assert_eq!(create["command"]["effect"], "global_write");
    assert_eq!(create["command"]["execution"], "sync");
    assert_eq!(create["command"]["confirmation_required"], true);
    let create_help = ds(&["survey", "entries", "create", "--help"]);
    assert_eq!(create_help.code, 0);
    assert!(
        create_help
            .stdout
            .contains("--idempotency-key <opaque-key>")
    );
    assert!(create_help.stdout.contains("Firestore committed"));
    assert!(create_help.stdout.contains("BigQuery mirror unconfirmed"));
    let names = create["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "context-key",
            "created-at",
            "doc-id",
            "document",
            "form",
            "idempotency-key",
            "lane",
        ])
    );
    for forbidden in [
        "project",
        "url",
        "method",
        "body",
        "token",
        "origin",
        "operation",
        "retry",
        "force",
        "authority",
        "desktop-descriptor",
    ] {
        assert!(
            create["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.create exposed --{forbidden}"
        );
    }
    let unconfirmed_create = ds(&[
        "survey",
        "entries",
        "create",
        "--form",
        "poles",
        "--doc-id",
        "pole-1",
        "--idempotency-key",
        "opaque-1",
        "--created-at",
        "2026-08-30T00:00:00Z",
        "--document",
        "missing.json",
        "--output",
        "json",
    ]);
    assert_eq!(unconfirmed_create.code, 2);
    assert_eq!(
        unconfirmed_create.envelope["error"]["code"],
        "confirmation_required"
    );
    let confirmed_invalid_create = ds(&[
        "survey",
        "entries",
        "create",
        "--form",
        "poles",
        "--doc-id",
        "pole-1",
        "--idempotency-key",
        "opaque-1",
        "--created-at",
        "2026-08-30T00:00:00Z",
        "--document",
        "missing.json",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(confirmed_invalid_create.code, 2);
    assert_eq!(
        confirmed_invalid_create.envelope["error"]["code"],
        "survey_entry_create_document_invalid"
    );

    let import = ok(&["capabilities", "survey.entries.import", "--output", "json"]);
    assert_eq!(import["command"]["authority"], "headless_project");
    assert_eq!(import["command"]["effect"], "global_write");
    assert_eq!(import["command"]["confirmation_required"], true);
    let names = import["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from(["checkpoint", "file", "form", "lane", "on-error", "receipt"])
    );
    for forbidden in [
        "project",
        "concurrency",
        "retry",
        "origin",
        "operation",
        "created-by",
        "token",
        "url",
        "method",
        "source-provenance",
    ] {
        assert!(
            import["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.import exposed --{forbidden}"
        );
    }
    let root = temp_root("survey-import-preauth");
    std::fs::create_dir_all(&root).unwrap();
    let missing = root.join("missing.ndjson").display().to_string();
    let checkpoint = root.join("state.json").display().to_string();
    let receipt = root.join("receipt.ndjson").display().to_string();
    let invalid_import = ds(&[
        "survey",
        "entries",
        "import",
        "--form",
        "poles",
        "--file",
        &missing,
        "--checkpoint",
        &checkpoint,
        "--receipt",
        &receipt,
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(invalid_import.code, 2);
    assert_eq!(
        invalid_import.envelope["error"]["code"], "survey_entries_import_source_invalid",
        "the complete local source contract must fail before profile or auth access"
    );
    assert!(!PathBuf::from(checkpoint).exists());
    assert!(!PathBuf::from(receipt).exists());
    std::fs::remove_dir_all(root).unwrap();

    let legacy = ok(&[
        "capabilities",
        "survey.project-forms.read",
        "--output",
        "json",
    ]);
    assert_eq!(legacy["command"]["authority"], "desktop_user");
    assert!(
        legacy["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["name"] == "project" && arg["required"] == true)
    );

    let legacy_editor = ok(&[
        "capabilities",
        "survey.project-form.editor",
        "--output",
        "json",
    ]);
    assert_eq!(legacy_editor["command"]["authority"], "desktop_user");
    assert!(
        legacy_editor["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["name"] == "project" && arg["required"] == true)
    );
}
