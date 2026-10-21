use super::*;

fn refusal(args: &[&str]) -> String {
    let tokens: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let inputs = ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
    let outcome = run(
        &inputs,
        &Context {
            confirmed: false,
            output: ds_cli_contract::output::Output {
                format: ds_cli_contract::output::Format::Json,
                pretty: false,
                color: false,
            },
        },
    );
    outcome
        .expect_err("refused before the engine")
        .code()
        .to_owned()
}

#[test]
fn request_mode_is_exclusive_and_refuses_before_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let request = dir.path().join("request.json");
    let path = request.to_str().unwrap();
    let held = dir.path().join("held");
    std::fs::create_dir(&held).unwrap();
    for mixed in [
        &["--request", path, "--project", "fixture"][..],
        &["--request", path, "--alignment", "al-1"],
        &["--request", path, "--preview-only"],
        &["--request", path, "--preview-retention", "7d"],
        &["--scene", path, "--plan", path, "--out-dir", "/x"],
    ] {
        assert_eq!(refusal(mixed), "request_mode_invalid", "{mixed:?}");
    }
    assert_eq!(refusal(&["--request", path]), "print_request_invalid");
    for (bytes, code) in [
        (json!("not an object").to_string(), "print_request_invalid"),
        (
            json!({"out_dir": "relative/out"}).to_string(),
            "print_request_invalid",
        ),
        (json!({"out_dir": held}).to_string(), "output_exists"),
    ] {
        std::fs::write(&request, bytes).unwrap();
        assert_eq!(refusal(&["--request", path]), code);
    }
    std::fs::write(
        &request,
        json!({"out_dir": dir.path().join("fresh")}).to_string(),
    )
    .unwrap();
    assert_eq!(
        refusal(&["--request", path, "--result", held.to_str().unwrap()]),
        "output_exists"
    );
    assert!(!dir.path().join("fresh").exists());
}

#[test]
fn native_disk_preview_contract_admits_only_explicit_retention_choices() {
    assert_eq!(COMMAND.contract, 9);
    let tokens = |retention: &str| {
        [
            "--preview-layout",
            "draft.json",
            "--preview-retention",
            retention,
        ]
        .map(str::to_owned)
    };
    for retention in ["24h", "7d"] {
        let parsed = ds_cli_contract::args::parse(&COMMAND, &tokens(retention)).unwrap();
        assert_eq!(parsed.value("preview-retention"), Some(retention));
    }
    assert!(ds_cli_contract::args::parse(&COMMAND, &tokens("forever")).is_err());
    assert!(COMMAND.output.contains("ds.mv-preview-artifacts/v1"));
}

#[test]
fn progressive_modes_share_native_admission_and_refuse_mixed_reads_before_auth_or_io() {
    use ds_command_kernel::printing::mv_preview::{Action, action};
    let digest = "a".repeat(64);
    assert_eq!(
        action(true, true, None, None, false).unwrap(),
        Action::Start
    );
    assert_eq!(
        action(false, false, Some(&digest), Some(100), false).unwrap(),
        Action::Read {
            input_sha256: digest.clone(),
            page: Some(100)
        }
    );
    for mixed in [
        vec!["--preview-all"],
        vec!["--preview-job", "../foreign", "--project", "fixture"],
        vec![
            "--preview-job",
            &digest,
            "--preview-page",
            "0",
            "--project",
            "fixture",
        ],
        vec![
            "--preview-job",
            &digest,
            "--scene",
            "missing",
            "--project",
            "fixture",
        ],
        vec![
            "--preview-job",
            &digest,
            "--preview-layout",
            "missing",
            "--project",
            "fixture",
        ],
        vec!["--preview-page", "1"],
    ] {
        assert_eq!(refusal(&mixed), "request_mode_invalid", "{mixed:?}");
    }
    assert_eq!(COMMAND.contract, 9);
    let outcome = crate::project::export::host_failure(ds_report_host::HostFailure::new(
        "print_preview_job_capacity",
        "busy",
    ));
    assert_eq!(outcome.code(), "print_preview_job_capacity");
}

#[test]
fn alignment_flag_stages_a_native_fixture_selection_and_cleans_up() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
    let (package, _) = ds_grid_exchange::dsgrid::ingest(&std::fs::read(fixture).unwrap()).unwrap();
    let scene = ds_grid_engine::GridSession::open(package.snapshot)
        .profile_atlas_scene(ds_grid_engine::ProfileAtlasOptions::default())
        .unwrap();
    let id = scene.bands[0].alignment_id.as_str().to_owned();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("scene.json");
    let bytes = serde_json::to_vec(&scene).unwrap();
    std::fs::write(&source, &bytes).unwrap();
    let inputs = ds_cli_contract::args::parse(
        &COMMAND,
        &[
            "--project",
            "fixture",
            "--scene",
            source.to_str().unwrap(),
            "--plan",
            source.to_str().unwrap(),
            "--out-dir",
            dir.path().join("output").to_str().unwrap(),
            "--alignment",
            &id,
            "--alignment",
            &id,
        ]
        .map(str::to_owned),
    )
    .unwrap();
    let selected = stage_selected_scene(&source, inputs.repeated("alignment"))
        .unwrap()
        .unwrap();
    let staged = selected.file.path().to_owned();
    let decoded: ds_network_contracts::ProfileAtlasScene =
        serde_json::from_slice(&std::fs::read(&staged).unwrap()).unwrap();
    assert_eq!(decoded.bands, vec![scene.bands[0].clone()]);
    assert_eq!(decoded.model_revision, scene.model_revision.as_str());
    let engine_selected = ds_grid_engine::publication_scope::select_publication_alignments(
        &scene,
        &[id.clone(), id.clone()],
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&staged).unwrap(),
        serde_json::to_vec(&engine_selected).unwrap()
    );
    assert_eq!(selected.receipt["alignment_ids"], json!([id]));
    assert_eq!(
        selected.receipt["source_scene_sha256"],
        format!("sha256:{:x}", Sha256::digest(&bytes))
    );
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    drop(selected);
    assert!(!staged.exists());
    assert!(stage_selected_scene(&source, &[]).unwrap().is_none());
    assert_eq!(
        stage_selected_scene(&source, &["unknown".into()])
            .err()
            .unwrap()
            .code(),
        "alignment_selection_invalid"
    );
}
