//! The DS Grid file commands restored through the Network process port
//! (Architecture DECOUPLING-003), proved by the smoke tests their linked
//! implementations carried before the 2026-10-17 retirement.
//!
//! Each test is the retired `domain_smoke.rs` test of the same name. The
//! assertions are unchanged except where a command now reaches its engine
//! through the addon: those tests carry the crate's usual
//! `requires the explicitly selected compatible Network executable` gate and
//! run with `DS_NETWORK_ENGINE_BIN` naming an engine that advertises
//! `grid_file_operations_v1`, so they prove the process port against the same
//! answers the linked commands gave.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ds_grid_engine::{CommandEnvelope, GridCommand, GridSession};
use ds_grid_exchange::unpack;
use serde_json::{Value, json};

mod common;

struct Run {
    envelope: Value,
    stdout: String,
    stderr: String,
    code: i32,
}

fn ds(args: &[&str]) -> Run {
    run_ds(args, false)
}

fn native_ds(args: &[&str]) -> Run {
    run_ds(args, true)
}

fn native_refusal(args: &[&str]) -> String {
    native_ds(args).envelope["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn run_ds(args: &[&str], native: bool) -> Run {
    let config = temp_root("native-smoke-auth");
    // The development catalogue is the reviewed action vocabulary written out
    // literally, not a hashed release artefact: it carries no profile digest
    // and no producer stages it.
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    // Smoke tests must never discover or contact the operator's running app.
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_SRE_REPORTING", "off")
        .env("DS_DESKTOP_DESCRIPTOR", config.join("no-desktop.json"));
    if native {
        command
            .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &bundle)
            .env("DS_CONFIG_HOME", &config);
    }
    let output = command.output().expect("ds binary runs");
    let _ = std::fs::remove_dir_all(config);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Run {
        envelope: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
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

fn temp_root(label: &str) -> PathBuf {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "ds-cli-{label}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The real PLS-CADD workspace that ships with `ds-network`.
fn workspace() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-network/fixtures/pls-public/humble-pole/workspace");
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

/// A value for one schema, built only from what `describe --kind types`
/// published: required fields, the first variant of every enum, and the
/// `known` fields a caller chose from the published field descriptions.
fn from_published_shape(schema: &Value, defs: &Value, known: &Value) -> Value {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference.trim_start_matches("#/$defs/");
        return from_published_shape(&defs[name], defs, known);
    }
    if let Some(values) = schema["enum"].as_array() {
        return values[0].clone();
    }
    if let Some(value) = schema.get("const") {
        return value.clone();
    }
    if let Some(options) = schema["oneOf"].as_array().or(schema["anyOf"].as_array()) {
        return from_published_shape(&options[0], defs, known);
    }
    let kind = match &schema["type"] {
        Value::Array(kinds) => kinds[0].as_str().unwrap_or("null"),
        other => other.as_str().unwrap_or("null"),
    };
    match kind {
        "object" => {
            let mut out: serde_json::Map<String, Value> = schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|field| {
                    let value = known.get(field).cloned().unwrap_or_else(|| {
                        from_published_shape(&schema["properties"][field], defs, known)
                    });
                    (field.to_string(), value)
                })
                .collect();
            for (field, value) in known.as_object().into_iter().flatten() {
                if schema["properties"].get(field).is_some() {
                    out.insert(field.clone(), value.clone());
                }
            }
            Value::Object(out)
        }
        "string" => json!("smoke"),
        "integer" => json!(0),
        "number" => json!(0.0),
        "boolean" => json!(false),
        _ => Value::Null,
    }
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn a_machine_holds_creates_opens_and_lists_its_own_working_copies() {
    // The whole point of the 2026-09-18 move: this runs with no application,
    // no sign-in and no project, and the answers are about THIS machine. The
    // store root is redirected so the test never touches the operator's own
    // catalogue.
    let home = temp_root("dsgrid-local-workspace");
    let account = ["--lane", "stable", "--account", "uid-smoke"];
    let with_home = |args: &[&str]| -> Run {
        let mut argv: Vec<&str> = args.to_vec();
        argv.extend(account);
        argv.extend(["--output", "json"]);
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(&argv)
            .env("DS_LAYER_HOME", &home)
            .env("NO_COLOR", "1")
            .output()
            .expect("ds binary runs");
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| json!({}));
        Run {
            envelope,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code().unwrap_or(-1),
        }
    };

    // A machine that has never held a working copy answers with an empty
    // catalogue rather than a refusal.
    let empty = with_home(&["dsgrid", "model", "list"]);
    assert_eq!(
        empty.code, 0,
        "an empty catalogue refused: {:?}",
        empty.envelope
    );
    assert_eq!(empty.envelope["data"]["total"], 0);
    assert!(empty.envelope["data"]["active_model"].is_null());

    let created = with_home(&["dsgrid", "model", "create-local", "--name", "Feeder A"]);
    assert_eq!(created.code, 0, "create refused: {:?}", created.envelope);
    let model = created.envelope["data"]["model"]["model"]
        .as_str()
        .expect("the new model's id")
        .to_owned();
    assert_eq!(
        created.envelope["data"]["became_active"], true,
        "a machine's first working copy is the one it opens"
    );
    // The engine's own reading, not the operator's description of it.
    assert!(
        created.envelope["data"]["model"]["crs"]
            .as_str()
            .is_some_and(|crs| crs.starts_with("EPSG:")),
        "the package's coordinate system was not recorded"
    );
    assert_eq!(created.envelope["data"]["model"]["origin"], "created");

    // Creating opens what it created: this is the one local command that
    // changes which copy an editing session starts from, and the receipt says
    // so rather than leaving it to be discovered.
    let second = with_home(&["dsgrid", "model", "create-local", "--name", "Feeder B"]);
    assert_eq!(second.code, 0);
    assert_eq!(second.envelope["data"]["became_active"], true);
    let opened = second.envelope["data"]["model"]["model"]
        .as_str()
        .expect("the second model's id")
        .to_owned();

    // Two copies may not share the name an operator reads.
    let clash = with_home(&["dsgrid", "model", "create-local", "--name", "feeder a"]);
    assert_eq!(clash.envelope["error"]["code"], "local_model_name_taken");

    let listed = with_home(&["dsgrid", "model", "list"]);
    assert_eq!(listed.envelope["data"]["total"], 2);
    assert_eq!(listed.envelope["data"]["active_model"], opened);

    // Opening the earlier copy moves the pointer; asking again changes
    // nothing, so a retry after a lost answer is safe.
    let moved = with_home(&["dsgrid", "model", "set-active", "--model", &model]);
    assert_eq!(moved.envelope["data"]["changed"], true);
    assert_eq!(moved.envelope["data"]["active_model"], model);
    let reopen = with_home(&["dsgrid", "model", "set-active", "--model", &model]);
    assert_eq!(reopen.envelope["data"]["changed"], false);
    assert_eq!(reopen.envelope["data"]["status"], "unchanged");
    let unknown = with_home(&["dsgrid", "model", "set-active", "--model", "local-nope"]);
    assert_eq!(unknown.envelope["error"]["code"], "local_model_not_found");

    // Another account on the same machine sees its own catalogue.
    let theirs = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "dsgrid",
            "model",
            "list",
            "--lane",
            "stable",
            "--account",
            "uid-other",
            "--output",
            "json",
        ])
        .env("DS_LAYER_HOME", &home)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let envelope: Value = serde_json::from_slice(&theirs.stdout).expect("json");
    assert_eq!(
        envelope["data"]["total"], 0,
        "one account read another's working copies"
    );
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn dsgrid_apply_batch_matches_sequential_apply_and_preserves_package_members() {
    let model = common::fixture();
    let source = std::fs::read(&model).unwrap();
    let package = unpack(&source).unwrap();
    let mut session = GridSession::open(package.snapshot.clone());
    let head = session.current_revision().revision_id.clone();
    let alignment_id = package.snapshot.alignments[0].id.clone();
    let commands: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|note| GridCommand::SetAlignmentSurveyFacts {
            alignment_id: alignment_id.clone(),
            terrain_corridor_half_width_m: None,
            route_buffer_half_width_m: None,
            terrain_gap_tolerance_m: None,
            survey_note: Some(note.to_string()),
        })
        .collect();
    let root = tempfile::tempdir().unwrap();
    let batch_path = root.path().join("batch.json");
    let out = root.path().join("batch.dsgrid");
    let payload = json!({"expected_revision": head, "commands": commands.iter().enumerate()
        .map(|(i, command)| json!({"command_id": format!("batch-{i}"), "command": command})).collect::<Vec<_>>()});
    std::fs::write(&batch_path, serde_json::to_vec(&payload).unwrap()).unwrap();
    let preview = ok(&[
        "dsgrid",
        "apply-batch",
        "--model",
        &model,
        "--batch",
        batch_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_eq!(preview["commands"], 2);
    assert_eq!(preview["operations"]["set_alignment_survey_facts"], 2);
    assert_eq!(preview["persisted"], false);
    assert!(!out.exists());
    let applied = ok(&[
        "dsgrid",
        "apply-batch",
        "--model",
        &model,
        "--batch",
        batch_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(applied["resulting_revision"], preview["resulting_revision"]);
    assert_eq!(applied["package_revision_increment"], 1);
    assert_eq!(
        applied["artifact"]["package_revision"],
        package.manifest.model.model_revision + 1
    );
    let mut previous = model.clone();
    for (i, command) in commands.into_iter().enumerate() {
        let envelope = CommandEnvelope::new(
            format!("batch-{i}"),
            session.current_revision().revision_id.clone(),
            command,
        );
        session.apply_command(envelope.clone()).unwrap();
        let envelope_path = root.path().join(format!("envelope-{i}.json"));
        std::fs::write(&envelope_path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        let next = root.path().join(format!("sequential-{i}.dsgrid"));
        ok(&[
            "dsgrid",
            "apply",
            "--model",
            &previous,
            "--envelope",
            envelope_path.to_str().unwrap(),
            "--out",
            next.to_str().unwrap(),
            "--output",
            "json",
        ]);
        previous = next.display().to_string();
    }
    let revised = unpack(&std::fs::read(&out).unwrap()).unwrap();
    let sequential = unpack(&std::fs::read(previous).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&revised.snapshot).unwrap(),
        serde_json::to_value(&sequential.snapshot).unwrap()
    );
    assert_eq!(
        revised
            .assets
            .iter()
            .map(|a| (&a.invariant_leaf, &a.bytes))
            .collect::<Vec<_>>(),
        package
            .assets
            .iter()
            .map(|a| (&a.invariant_leaf, &a.bytes))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        serde_json::to_value(&revised.exchange_bindings).unwrap(),
        serde_json::to_value(&package.exchange_bindings).unwrap()
    );
    assert_eq!(std::fs::read(&model).unwrap(), source);
    let retained = std::fs::read(&out).unwrap();
    let refused = ds(&[
        "dsgrid",
        "apply-batch",
        "--model",
        &model,
        "--batch",
        batch_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["error"]["code"], "output_exists");
    assert_eq!(std::fs::read(&out).unwrap(), retained);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn dsgrid_apply_batch_refuses_late_failure_stale_head_and_mixed_pins_without_output() {
    let model = common::fixture();
    let source = std::fs::read(&model).unwrap();
    let package = unpack(&source).unwrap();
    let mut session = GridSession::open(package.snapshot.clone());
    let head = session.current_revision().revision_id.clone();
    let command = GridCommand::SetAlignmentSurveyFacts {
        alignment_id: package.snapshot.alignments[0].id.clone(),
        terrain_corridor_half_width_m: None,
        route_buffer_half_width_m: None,
        terrain_gap_tolerance_m: None,
        survey_note: Some("must roll back".into()),
    };
    let mut bad_command = serde_json::to_value(&command).unwrap();
    bad_command["alignment_id"] = json!("missing-batch-alignment");
    let bad_command: GridCommand = serde_json::from_value(bad_command).unwrap();
    assert!(
        session
            .apply_transaction_at_head(
                head.clone(),
                vec![
                    ("first".into(), command.clone()),
                    ("bad".into(), bad_command.clone())
                ]
            )
            .is_err()
    );
    assert_eq!(session.current_revision().revision_id, head);
    assert_eq!(
        serde_json::to_value(session.snapshot()).unwrap(),
        serde_json::to_value(&package.snapshot).unwrap()
    );
    let root = tempfile::tempdir().unwrap();
    let batch_path = root.path().join("batch.json");
    let out = root.path().join("rejected.dsgrid");
    let good = json!({"command_id":"first", "command":command});
    let bad = json!({"command_id":"bad", "command":bad_command});
    let same_pin_envelope =
        serde_json::to_value(CommandEnvelope::new("first", head.clone(), command.clone())).unwrap();
    let cases = [
        (
            json!({"expected_revision":head, "commands":[good.clone(), bad]}),
            "target_not_found",
        ),
        (
            json!({"expected_revision":"rev:stale", "commands":[good.clone()]}),
            "revision_conflict",
        ),
        (
            json!([same_pin_envelope.clone(), {"command_id":"second", "command_schema_version":ds_grid_engine::COMMAND_SCHEMA_VERSION, "expected_revision":"rev:stale", "command":command}]),
            "batch_invalid",
        ),
        (
            json!({"expected_revision":head, "commands":[good.clone(), good]}),
            "batch_invalid",
        ),
        (
            json!({"expected_revision":head, "commands":[]}),
            "batch_invalid",
        ),
    ];
    for (payload, code) in cases {
        std::fs::write(&batch_path, serde_json::to_vec(&payload).unwrap()).unwrap();
        for dry_run in [false, true] {
            let mut args = vec![
                "dsgrid",
                "apply-batch",
                "--model",
                &model,
                "--batch",
                batch_path.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--output",
                "json",
            ];
            if dry_run {
                args.push("--dry-run");
            }
            let rejected = ds(&args);
            assert_eq!(
                rejected.envelope["error"]["code"], code,
                "{}",
                rejected.stdout
            );
            assert!(!out.exists());
            assert_eq!(std::fs::read(&model).unwrap(), source);
        }
    }
    // A valid retry proves the failed transaction left no file/session residue;
    // the array input is the same-head convenience form, never a rebasing path.
    std::fs::write(
        &batch_path,
        serde_json::to_vec(&json!([same_pin_envelope])).unwrap(),
    )
    .unwrap();
    let retry = ok(&[
        "dsgrid",
        "apply-batch",
        "--model",
        &model,
        "--batch",
        batch_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(retry["commands"], 1);
    assert_eq!(retry["persisted"], true);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
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
#[ignore = "requires the explicitly selected compatible Network executable"]
fn dsgrid_command_descriptors_inline_the_four_authoring_shapes() {
    let mut authored = Vec::new();
    for (id, param, ty, field) in [
        ("create_route_node", "row", "RouteNodeRow", "role"),
        ("create_structure", "row", "StructureRow", "orientation_rad"),
        (
            "author_terrain_source",
            "row",
            "TerrainSourceRow",
            "vertical_datum",
        ),
        (
            "create_tension_section_set_path",
            "members",
            "TensionSectionSetMember",
            "slot",
        ),
    ] {
        let data = ok(&[
            "dsgrid", "describe", "--kind", "commands", "--id", id, "--output", "json",
        ]);
        let param = data["descriptor"]["params"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == param)
            .unwrap();
        assert!(param["type_schemas"][ty]["properties"][field].is_object());
        assert!(
            data.to_string().len() < 32 * 1024,
            "selected authoring descriptor stays bounded"
        );
        if matches!(id, "create_route_node" | "author_terrain_source") {
            let schema = &param["type_schemas"][ty];
            let row = from_published_shape(
                schema,
                &schema["$defs"],
                &json!({
                    "id":format!("schema-smoke-{id}"), "provider":"field_survey",
                }),
            );
            authored.push(json!({"command_id":format!("smoke-{id}"),
                "command":{"command_kind":id,"row":row}}));
        }
    }
    // The inline row shapes alone suffice to author valid native mutations.
    let model = common::fixture();
    let head = ok(&["dsgrid", "validate", "--model", &model, "--output", "json"])["model"]["authored_revision"].clone();
    let dir = tempfile::tempdir().unwrap();
    let batch = dir.path().join("commands.json");
    let output = dir.path().join("authored.dsgrid");
    std::fs::write(
        &batch,
        serde_json::to_vec(&json!({"expected_revision":head,"commands":authored})).unwrap(),
    )
    .unwrap();
    let applied = ok(&[
        "dsgrid",
        "apply-batch",
        "--model",
        &model,
        "--batch",
        batch.to_str().unwrap(),
        "--out",
        output.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(applied["operations"]["create_route_node"], 1);
    assert_eq!(applied["operations"]["author_terrain_source"], 1);
    let validated = ok(&[
        "dsgrid",
        "validate",
        "--model",
        output.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(validated["model"]["valid"], true);
    assert_ne!(validated["model"]["authored_revision"], head);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn dsgrid_create_edit_and_reopen_require_no_typescript_or_pairing() {
    let root = temp_root("dsgrid-native-create");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("new.dsgrid");
    let source_text = source.to_str().unwrap();
    let created = ok(&[
        "dsgrid",
        "create",
        "--out",
        source_text,
        "--model-id",
        "native-mv",
        "--crs",
        "EPSG:32735",
        "--output",
        "json",
    ]);
    assert_eq!(created["model_id"], "native-mv");
    assert_eq!(created["coordinate_system"], "EPSG:32735");
    assert_eq!(created["package_revision"], 0);
    let bytes = std::fs::read(&source).unwrap();
    let blank = unpack(&bytes).unwrap();
    assert!(blank.snapshot.alignments.is_empty());
    let session = GridSession::open(blank.snapshot);
    assert_eq!(
        created["authored_revision"],
        session.current_revision().revision_id.as_str()
    );
    let command: GridCommand = serde_json::from_value(json!({
        "command_kind": "create_alignment",
        "row": {"id": "native-route", "parent_id": null, "label": "Native MV route"}
    }))
    .unwrap();
    let envelope = CommandEnvelope::new(
        "native-create-route",
        session.current_revision().revision_id.clone(),
        command,
    );
    let request = root.join("route.json");
    std::fs::write(&request, serde_json::to_vec(&envelope).unwrap()).unwrap();
    let revised = root.join("revised.dsgrid");
    let result = ok(&[
        "dsgrid",
        "apply",
        "--model",
        source_text,
        "--envelope",
        request.to_str().unwrap(),
        "--out",
        revised.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(result["persisted"], true);
    let reopened = unpack(&std::fs::read(&revised).unwrap()).unwrap();
    assert_eq!(reopened.manifest.model.model_id.as_str(), "native-mv");
    assert_eq!(reopened.manifest.model.model_revision, 1);
    assert_eq!(reopened.snapshot.alignments[0].label, "Native MV route");
    assert_eq!(
        std::fs::read(&source).unwrap(),
        bytes,
        "editing preserves the source"
    );
    let validation = ok(&[
        "dsgrid",
        "validate",
        "--model",
        revised.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(validation["model"]["valid"], true);
    let collision = ds(&["dsgrid", "create", "--out", source_text, "--output", "json"]);
    assert_eq!(collision.envelope["error"]["code"], "output_exists");
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn dsgrid_create_uses_native_crs_and_template_admission() {
    let root = temp_root("dsgrid-native-create-refusals");
    std::fs::create_dir_all(&root).unwrap();
    let out = root.join("new.dsgrid");
    let template = root.join("standards.dsgrid-template");
    std::fs::write(
        &template,
        ds_grid_exchange::blank_model::empty_standards_template().unwrap(),
    )
    .unwrap();
    let valid = ok(&[
        "dsgrid",
        "create",
        "--out",
        out.to_str().unwrap(),
        "--standards",
        template.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(
        valid["coordinate_system"],
        ds_grid_exchange::blank_model::DEFAULT_BLANK_MODEL_CRS
    );
    assert!(unpack(&std::fs::read(&out).unwrap()).is_ok());
    let rejected = root.join("rejected.dsgrid");
    for (flag, value, code) in [
        ("--crs", "EPSG:4326", "unsupported_coordinate_system"),
        ("--model-id", "", "invalid_id"),
        ("--standards", out.to_str().unwrap(), "standards_refused"),
        (
            "--standards",
            root.to_str().unwrap(),
            "standards_unreadable",
        ),
    ] {
        let result = ds(&[
            "dsgrid",
            "create",
            "--out",
            rejected.to_str().unwrap(),
            flag,
            value,
            "--output",
            "json",
        ]);
        assert_eq!(result.envelope["error"]["code"], code, "{}", result.stdout);
        assert!(!rejected.exists());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
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
#[ignore = "requires the explicitly selected compatible Network executable"]
fn dsgrid_describe_types_publishes_rows_a_batch_can_be_written_from() {
    // Feedback 778bb573: `author_design_policy` named Vec<StructureDutyProfileRow>
    // and nothing said what that row was. The index lists the declared types
    // with the operations that take them.
    let index = ok(&["dsgrid", "describe", "--kind", "types", "--output", "json"]);
    let entries = index["entries"].as_array().expect("entries");
    let profile_entry = entries
        .iter()
        .find(|entry| entry["id"] == "StructureDutyProfileRow")
        .expect("the duty profile row is a declared type");
    assert_eq!(
        profile_entry["declared_by"],
        json!(["author_design_policy"])
    );
    assert!(entries.iter().all(|entry| {
        entry["declared_by"]
            .as_array()
            .is_some_and(|operations| !operations.is_empty())
    }));
    assert!(index["more"]["nested_types"].as_u64().unwrap() > 0);

    let described = |id: &str| {
        ok(&[
            "dsgrid", "describe", "--kind", "types", "--id", id, "--output", "json",
        ])["descriptor"]
            .clone()
    };
    let profile = described("StructureDutyProfileRow");
    assert_eq!(
        profile["schema"]["required"],
        json!([
            "policy_id",
            "sequence",
            "structure_resource_id",
            "family",
            "material",
            "portal_configuration",
            "verification"
        ])
    );
    assert!(
        profile["schema"]["$defs"]["StructureDuty"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("strain_dead_end"))
    );
    // A type reached only through another still answers --id.
    assert!(described("StructureMaterialClass")["schema"]["oneOf"].is_array());

    // Write the policy batch from the published shapes alone, against the
    // real fixture, and the engine accepts it.
    let model = common::fixture();
    let root = tempfile::tempdir().unwrap();
    let params = root.path().join("resources.json");
    std::fs::write(&params, br#"{"table_kind":"resources"}"#).unwrap();
    let resources = ok(&[
        "dsgrid",
        "run",
        "--model",
        &model,
        "--operation",
        "project_table",
        "--params",
        params.to_str().unwrap(),
        "--limit",
        "1",
        "--output",
        "json",
    ]);
    let resource = resources["result"][0]["payload"]["id"].clone();
    assert!(resource.is_string(), "the fixture carries a resource");
    let head = ok(&["dsgrid", "validate", "--model", &model, "--output", "json"])["model"]
        ["authored_revision"]
        .clone();
    let policy_schema = described("DesignPolicyRow")["schema"].clone();
    let row = from_published_shape(
        &policy_schema,
        &policy_schema["$defs"],
        &json!({"id": "policy-smoke"}),
    );
    // The `duties` description says at least one: take the first published
    // StructureDuty value.
    assert!(
        profile["schema"]["properties"]["duties"]["description"]
            .as_str()
            .is_some_and(|text| text.contains("at least one"))
    );
    let first_duty = profile["schema"]["$defs"]["StructureDuty"]["enum"][0].clone();
    let duty_profile = from_published_shape(
        &profile["schema"],
        &profile["schema"]["$defs"],
        &json!({
            "policy_id": "policy-smoke",
            "structure_resource_id": resource,
            "duties": [first_duty],
        }),
    );
    let batch = |profiles: Value| {
        let path = root
            .path()
            .join(format!("batch-{}.json", profiles.as_array().unwrap().len()));
        let payload = json!({"expected_revision": head, "commands": [{
            "command_id": "smoke-author-policy",
            "command": {"command_kind": "author_design_policy", "row": row, "duty_profiles": profiles},
        }]});
        std::fs::write(&path, serde_json::to_vec(&payload).unwrap()).unwrap();
        path.display().to_string()
    };
    let accepted = ok(&[
        "dsgrid",
        "apply-batch",
        "--model",
        &model,
        "--batch",
        &batch(json!([duty_profile])),
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_eq!(accepted["would_apply"], true);
    assert_eq!(accepted["operations"]["author_design_policy"], 1);

    // Without a profile, the refusal itself names the row and its fields.
    let refused = ds(&[
        "dsgrid",
        "apply-batch",
        "--model",
        &model,
        "--batch",
        &batch(json!([])),
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_ne!(refused.code, 0);
    let engine = refused.envelope["error"]["detail"]["engine"]
        .as_str()
        .expect("the engine's refusal is carried");
    assert!(engine.contains("StructureDutyProfileRow"), "{engine}");
    assert!(engine.contains("structure_resource_id"), "{engine}");
}

/// The restored part of the family over a blank package: report it and see
/// the refusals a caller plans for — the missing clearance criteria — without
/// any working copy or window. `feature-codes import|migrate|export` and
/// `cleared-forest-offset set` stay retired (DECOUPLING-003); their half of
/// the retired test returns with them.
#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn dsgrid_feature_codes_family_runs_over_a_package_and_refuses_by_name() {
    let root = temp_root("dsgrid-feature-codes");
    std::fs::create_dir_all(&root).unwrap();
    let blank = root.join("blank.dsgrid");
    let blank_text = blank.to_str().unwrap();
    ok(&[
        "dsgrid",
        "create",
        "--out",
        blank_text,
        "--model-id",
        "fc-smoke",
        "--crs",
        "EPSG:32735",
        "--output",
        "json",
    ]);

    // Baseline: no codes, nothing unresolved, no window, no account.
    let baseline = ok(&[
        "dsgrid",
        "feature-codes",
        "report",
        "--package",
        blank_text,
        "--output",
        "json",
    ]);
    assert_eq!(baseline["code_count"], 0);
    assert_eq!(baseline["unresolved_token_count"], 0);

    // Criteria: a blank model has no criterion set to fill and no clearance
    // cases to analyse; both say so by name.
    let shown = ok(&[
        "dsgrid",
        "criteria",
        "show",
        "--package",
        blank_text,
        "--output",
        "json",
    ]);
    assert_eq!(shown["sets"].as_array().map_or(0, Vec::len), 0);
    assert_eq!(
        refusal(&[
            "dsgrid",
            "criteria",
            "clearance",
            "set",
            "--package",
            blank_text,
            "--voltage-class",
            "MV",
            "--vertical-case",
            "Maximum Conductor Temperature",
            "--horizontal-case",
            "High wind",
            "--dry-run",
            "--output",
            "json"
        ]),
        "criterion_set_missing"
    );
    assert_eq!(
        refusal(&[
            "dsgrid",
            "analyse",
            "clearance",
            "--package",
            blank_text,
            "--output",
            "json"
        ]),
        "criterion_set_missing"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
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
fn dsgrid_live_checkpoint_refuses_unreviewed_history_without_catalogue_lookup() {
    let root = temp_root("live-profile-checkpoint-history");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("new.dsgrid");
    for history in [None, Some(r#"{"project":"untrusted"}"#)] {
        let mut args = vec![
            "dsgrid",
            "profile",
            "open",
            "--model",
            "browser-only",
            "--live",
            "--checkpoint-out",
            path.to_str().unwrap(),
            "--output",
            "json",
        ];
        if let Some(history) = history {
            args.extend_from_slice(&["--expect-history", history]);
        }
        let result = ds(&args);
        assert_eq!(
            result.envelope["error"]["code"],
            "checkpoint_history_invalid"
        );
        assert!(!path.exists());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dsgrid_profile_checkpoint_validates_output_before_touching_the_window() {
    for args in [
        vec![
            "dsgrid",
            "profile",
            "open",
            "--model",
            "any",
            "--account",
            "test-account",
            "--lane",
            "stable",
            "--checkpoint-out",
            "relative.dsgrid",
            "--output",
            "json",
        ],
        vec![
            "dsgrid",
            "profile",
            "open",
            "--model",
            "any",
            "--account",
            "test-account",
            "--lane",
            "stable",
            "--expect-revision",
            "rev:old",
            "--output",
            "json",
        ],
    ] {
        let result = ds(&args);
        assert_eq!(
            result.envelope["error"]["code"],
            "checkpoint_output_invalid"
        );
    }
    let root = temp_root("profile-checkpoint-existing");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("existing.dsgrid");
    std::fs::write(&path, b"preserve").unwrap();
    let result = ds(&[
        "dsgrid",
        "profile",
        "open",
        "--model",
        "any",
        "--account",
        "test-account",
        "--lane",
        "stable",
        "--checkpoint-out",
        path.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(result.envelope["error"]["code"], "output_exists");
    assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
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
#[ignore = "requires the explicitly selected compatible Network executable"]
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
#[ignore = "requires the explicitly selected compatible Network executable"]
fn explicit_package_migration_keeps_source_and_revision_and_refuses_wrong_digest() {
    use sha2::{Digest, Sha256};
    let root = temp_root("explicit-package-migration");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("source.dsgrid");
    let source_path = source.display().to_string();
    let created = ok(&[
        "dsgrid",
        "create",
        "--out",
        &source_path,
        "--model-id",
        "migration-smoke",
        "--output",
        "json",
    ]);
    let bytes = std::fs::read(&source).unwrap();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let result = ok(&[
        "dsgrid",
        "package",
        "migrate",
        "--path",
        &source_path,
        "--expected-source-sha256",
        &digest,
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_eq!(result["source_modified"], false);
    assert_eq!(result["published"], false);
    assert_eq!(result["files"], Value::Null);
    assert_eq!(result["receipt"]["already_current"], true);
    assert_eq!(result["receipt"]["source_sha256"], digest);
    assert_eq!(result["receipt"]["output_sha256"], digest);
    assert_eq!(
        result["receipt"]["source_schema_version"],
        result["receipt"]["target_schema_version"]
    );
    assert!(created["authored_revision"].is_string());
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    assert!(created["artifact"]["sha256"].is_string());
    assert_eq!(
        refusal(&[
            "dsgrid",
            "package",
            "migrate",
            "--path",
            &source_path,
            "--expected-source-sha256",
            &"0".repeat(64),
            "--dry-run",
            "--output",
            "json"
        ]),
        "package_migration_source_conflict"
    );
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    assert_eq!(
        refusal(&[
            "dsgrid",
            "package",
            "migrate",
            "--path",
            &source_path,
            "--expected-source-sha256",
            &digest,
            "--output",
            "json"
        ]),
        "package_migration_confirmation"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn exports_and_version_markers_refuse_malformed_requests_before_authentication() {
    let root = temp_root("dsgrid-exports-refusals");
    std::fs::create_dir_all(&root).expect("create smoke dir");
    let file = root.join("delivered.bak");
    std::fs::write(&file, b"bak").expect("write export file");
    let file = file.display().to_string();
    assert_eq!(
        native_refusal(&[
            "dsgrid",
            "project",
            "exports",
            "publish",
            "--project",
            "project-smoke",
            "--model",
            "model-1",
            "--revision",
            "rev-1",
            "--file",
            &file,
            "--output-id",
            "Delivered Workspace",
            "--format",
            "pls_cadd_bak",
            "--yes",
            "--output",
            "json",
        ]),
        "export_id_invalid"
    );
    assert_eq!(
        native_refusal(&[
            "dsgrid",
            "project",
            "exports",
            "download",
            "--project",
            "project-smoke",
            "--model",
            "model-1",
            "--revision",
            "rev-1",
            "--export",
            "submission-2",
            "--output-id",
            "pls-delivered-workspace",
            "--out",
            &file,
            "--output",
            "json",
        ]),
        "grid_project_output_invalid",
        "an existing file is never overwritten"
    );
    // Governance writes are checked with the catalog's own rules before a
    // credential is read: a decision names its level, a fence is a catalog
    // id, and an update names what it changes.
    let governance = |extra: &[&str]| {
        let mut args = vec!["dsgrid", "project"];
        args.extend_from_slice(extra);
        args.extend_from_slice(&[
            "--project",
            "project-smoke",
            "--model",
            "model-1",
            "--yes",
            "--output",
            "json",
        ]);
        native_refusal(&args)
    };
    assert_eq!(
        governance(&[
            "set-approval",
            "--revision",
            "rev-1",
            "--expected-head",
            "rev-2",
            "--status",
            "approved",
            "--reason",
            "Reviewed",
        ]),
        "grid_request_invalid"
    );
    assert_eq!(
        governance(&[
            "bump-version",
            "--expected-head",
            "Rev Two",
            "--reason",
            "Submitted",
        ]),
        "grid_request_invalid"
    );
    assert_eq!(
        governance(&["update", "--expected-head", "rev-2"]),
        "grid_project_output_invalid"
    );
    // An MV fence is a revision id or `-`; the shared Rust validation refuses
    // anything else before a credential is read.
    let fenced = native_ds(&[
        "design",
        "version",
        "begin",
        "--project",
        "project-smoke",
        "--kind",
        "mv_model",
        "--object",
        "model-1",
        "--reason",
        "Submitted",
        "--idempotency-key",
        "k-1",
        "--expected-source",
        "Rev ABC",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(fenced.envelope["error"]["code"], "design_version_refused");
    assert_eq!(
        fenced.envelope["error"]["detail"]["cause"], "invalid_input",
        "{:?}",
        fenced.envelope
    );
    // A batch that names one object twice is refused rather than half-applied.
    let batch = root.join("batch.json");
    std::fs::write(
        &batch,
        serde_json::to_vec(&json!({"reason":"Submission 2","items":[
            {"object":{"kind":"mv_model","id":"model-1"},"idempotency_key":"a"},
            {"object":{"kind":"mv_model","id":"model-1"},"idempotency_key":"b"}]}))
        .unwrap(),
    )
    .unwrap();
    let repeated = native_ds(&[
        "design",
        "version",
        "begin-batch",
        "--project",
        "project-smoke",
        "--file",
        &batch.display().to_string(),
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(repeated.envelope["error"]["code"], "design_version_refused");
    std::fs::write(&batch, br#"{"items":[],"extra":1}"#).unwrap();
    assert_eq!(
        native_refusal(&[
            "design",
            "version",
            "begin-batch",
            "--project",
            "project-smoke",
            "--file",
            &batch.display().to_string(),
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_input"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn external_import_is_dsgrid_only_and_routes_conversion_to_the_exchange_boundary() {
    // The conflation the revert reversed, refused locally by name. A second
    // convert-and-project verb would start exactly here: by accepting a PLS
    // workspace or a `.bak` as a model source.
    let root = temp_root("dsgrid-import");
    std::fs::create_dir_all(&root).expect("temp directory is writable");
    let backup = root.join("delivery.bak").display().to_string();
    let folder = root.join("workspace").display().to_string();
    let package = root.join("route.dsgrid").display().to_string();

    // `publish-version` made the same refusal; it stays retired
    // (DECOUPLING-003) and returns to this loop with its own arguments.
    for source in [&backup, &folder, &workspace_file("humble-pole.don")] {
        let run = ds(&[
            "dsgrid",
            "model",
            "import-external",
            "--path",
            source,
            "--name",
            "Route",
            // The catalogue this would join is named explicitly, so the
            // source check is what refuses rather than a missing flag.
            "--account",
            "uid-smoke",
            "--output",
            "json",
        ]);
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

    // A relative path is refused too: it means a different file depending on
    // where the command was run, or none at all.
    assert_eq!(
        refusal(&[
            "dsgrid",
            "model",
            "import-external",
            "--path",
            "route.dsgrid",
            "--account",
            "uid-smoke",
            "--output",
            "json",
        ]),
        "absolute_path_required"
    );
    // A path that names nothing says so by name rather than as a bound.
    assert_eq!(
        refusal(&[
            "dsgrid",
            "model",
            "import-external",
            "--path",
            &package,
            "--account",
            "uid-smoke",
            "--output",
            "json",
        ]),
        "model_not_found"
    );

    // And a real package is acquired: `ds dsgrid create` writes one with no
    // application in sight, and importing it reads the identity those bytes
    // declare.
    let created = ds(&["dsgrid", "create", "--out", &package, "--output", "json"]);
    assert_eq!(
        created.code, 0,
        "ds dsgrid create refused: {}",
        created.stdout
    );
    let home = temp_root("dsgrid-import-workspace");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "dsgrid",
            "model",
            "import-external",
            "--path",
            &package,
            "--account",
            "uid-smoke",
            "--output",
            "json",
        ])
        .env("DS_LAYER_HOME", &home)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(
        output.status.code(),
        Some(0),
        "a valid .dsgrid was not acquired: {envelope}"
    );
    assert_eq!(envelope["data"]["status"], "imported");
    assert_eq!(envelope["data"]["model"]["origin"], "imported");
    assert_eq!(
        envelope["data"]["became_active"], true,
        "the first working copy a machine holds is the one it opens"
    );
    assert!(
        envelope["data"]["model"]["content_digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "the package's digest was not recorded"
    );
    std::fs::remove_dir_all(&root).ok();
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
    // Six since `dsgrid.model.prepare-project` joined the family in 7a92551;
    // eight since `dsgrid.model.show` and `dsgrid.model.link` joined on
    // 2026-09-20 (contract 02 §1) — `link` names a workspace by path and
    // Forget takes only a local id and account scope; no package bytes cross the command.
    // records digests, never bytes. The count is here so a new command cannot
    // join unexamined — it did its job: each command above was read against
    // the content rule before this number moved.
    // Ten since `dsgrid.model.unlink` joined on 2026-09-25: it takes a local
    // id and account scope and removes a link row; no bytes cross it.
    // Eleven after automatic graph commands were replaced by deliberate
    // combination: source and output paths, with a reviewed plan id.
    // Nine since the 2026-10-19 restoration (DECOUPLING-003): `model combine`
    // and `publish-version` stay retired until their engine halves have a
    // process port; each is read against the content rule when it returns.
    assert_eq!(
        checked, 9,
        "review the DS Grid model family when its command count changes"
    );
}

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
            command["authority"], "none",
            "`{}` is a fact about this machine: no project, no principal, no window",
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
fn unlink_removes_exactly_the_recorded_link_and_refuses_an_unlinked_copy() {
    use ds_command_kernel::local_models::{Op, Origin, PlsSourceLink, Scope};
    let home = temp_root("dsgrid-unlink");
    std::fs::create_dir_all(&home).expect("create store root");
    let scope = Scope {
        lane: "stable".into(),
        uid: "uid-unlink".into(),
    };
    ds_layer_store::local_models::execute_at(
        &home,
        &scope,
        Op::Register {
            id: "local-unlinksmoke".into(),
            display_name: "Nyamagabe".into(),
            origin: Origin::Imported,
            crs: "EPSG:32735".into(),
            model_revision: 0,
            bytes: 4,
            sha256: "c".repeat(64),
            created_at: None,
            project: None,
            head_revision: None,
            activate: false,
        },
        Some(b"pkg1"),
    )
    .expect("register a working copy");
    ds_layer_store::local_models::execute_at(
        &home,
        &scope,
        Op::Link {
            id: "local-unlinksmoke".into(),
            pls_source: PlsSourceLink {
                path: "/srv/pls/Nyamagabe".into(),
                digest: format!("sha256:{}", "7".repeat(64)),
                pls_version: "16.81".into(),
                member_versions: [("DON".to_string(), "57".to_string())].into(),
                member_count: 42,
                linked_at: "2026-09-20T10:00:00Z".into(),
            },
        },
        None,
    )
    .expect("link the working copy");
    let unlink = || {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
                "dsgrid",
                "model",
                "unlink",
                "--model",
                "local-unlinksmoke",
                "--lane",
                "stable",
                "--account",
                "uid-unlink",
                "--output",
                "json",
            ])
            .env("DS_LAYER_HOME", &home)
            .env("NO_COLOR", "1")
            .output()
            .expect("ds binary runs");
        serde_json::from_slice::<Value>(&output.stdout).expect("json envelope")
    };
    let first = unlink();
    assert_eq!(first["data"]["status"], "unlinked", "{first}");
    assert_eq!(first["data"]["removed"]["path"], "/srv/pls/Nyamagabe");
    assert!(first["data"]["model"]["pls_source"].is_null());
    // Nothing is left to remove: refused by name, not reported as done.
    let second = unlink();
    assert_eq!(second["error"]["code"], "local_model_request_invalid");
    std::fs::remove_dir_all(&home).ok();
}

/// A reviewed correction runs end to end through the process port with a
/// minimal guard file: the owner admits the guard (defaults filled) with the
/// batch, the task echoes that admitted form, and the receipt names the exact
/// guard bytes. Unknown guard fields and an unapproved write still refuse.
#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn a_guarded_correction_dry_runs_with_a_minimal_guard_and_refuses_widening() {
    use sha2::{Digest, Sha256};
    let model = common::fixture();
    let bytes = std::fs::read(&model).expect("read fixture package");
    let package = unpack(&bytes).expect("decode fixture package");
    let alignment = package.snapshot.alignments[0].id.as_str().to_owned();
    let structure = package.snapshot.structures[0].id.as_str().to_owned();
    let session = GridSession::open(package.snapshot);
    let revision = session.current_revision().revision_id.as_str().to_owned();

    let root = temp_root("dsgrid-correction");
    std::fs::create_dir_all(&root).unwrap();
    let batch = root.join("reviewed.json");
    std::fs::write(
        &batch,
        serde_json::to_vec(&json!({
            "expected_revision": revision,
            "commands": [{
                "command_id": "comment-1",
                "review_ref": "review-1",
                "command": {"command_kind": "describe_structure", "id": structure, "description": "reviewed"},
            }],
        }))
        .unwrap(),
    )
    .unwrap();
    let source = format!("sha256:{:x}", Sha256::digest(&bytes));
    let guard_doc = json!({
        "source_sha256": source,
        "alignment_ids": [alignment],
        "allowed_command_kinds": ["describe_structure"],
    });
    let guard = root.join("scope.json");
    let guard_bytes = serde_json::to_vec(&guard_doc).unwrap();
    std::fs::write(&guard, &guard_bytes).unwrap();
    let (batch_text, guard_text) = (batch.to_str().unwrap(), guard.to_str().unwrap());
    let correction = |extra: &[&str]| {
        let mut args = vec![
            "dsgrid",
            "apply-correction",
            "--model",
            &model,
            "--batch",
            batch_text,
            "--guard",
            guard_text,
            "--select",
            "comment-1",
        ];
        args.extend_from_slice(extra);
        args.extend(["--output", "json"]);
        ds(&args)
    };

    let preview = correction(&["--dry-run"]);
    assert_eq!(preview.code, 0, "{}{}", preview.stdout, preview.stderr);
    let data = &preview.envelope["data"];
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["correction_guard"]["sha256"],
        format!("sha256:{:x}", Sha256::digest(&guard_bytes))
    );
    assert_eq!(data["correction_guard"]["review_state"], "draft");
    assert_eq!(data["correction_guard"]["level"], "alignment");
    assert_eq!(data["selected_reviews"][0]["review_ref"], "review-1");

    // Writing a draft correction is the owner's refusal, not the host's.
    let out = root.join("corrected.dsgrid");
    let refused = correction(&["--out", out.to_str().unwrap()]);
    assert_eq!(
        refused.envelope["error"]["code"], "correction_not_approved",
        "{}",
        refused.stdout
    );
    assert!(!out.exists());

    // A guard that widens its own scope is refused before any transaction.
    let mut widened = guard_doc.clone();
    widened["allow_unscoped"] = json!(true);
    std::fs::write(&guard, serde_json::to_vec(&widened).unwrap()).unwrap();
    let widened = correction(&["--dry-run"]);
    assert_eq!(
        widened.envelope["error"]["code"], "correction_guard_invalid",
        "{}",
        widened.stdout
    );
    let _ = std::fs::remove_dir_all(root);
}
