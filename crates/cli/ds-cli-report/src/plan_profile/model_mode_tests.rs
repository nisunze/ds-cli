use super::*;
fn inputs(args: &[&str]) -> Inputs {
    ds_cli_contract::args::parse(
        &COMMAND,
        &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.dsgrid");
    fs::write(&path, b"exact model bytes").unwrap();
    let digest = format!("sha256:{:x}", Sha256::digest(b"exact model bytes"));
    (dir, path.to_str().unwrap().into(), digest)
}
#[test]
fn requires_all_pins_and_rejects_every_unsupported_model_combination() {
    for args in [
        vec!["--model", "/missing.dsgrid"],
        vec!["--expected-model-sha256", "sha256:missing"],
        vec!["--expected-authored-revision", "held"],
    ] {
        assert_eq!(
            admit(&inputs(&args), &never).unwrap_err().code(),
            "request_mode_invalid"
        );
    }
    let (_, path, digest) = fixture();
    for extra in [
        vec!["--scene", "missing"],
        vec!["--plan", "missing"],
        vec!["--alignment", "al-1"],
        vec!["--preview-layout", "missing"],
        vec!["--preview-all"],
        vec!["--preview-retention", "7d"],
        vec!["--preview-job", "held"],
        vec!["--preview-page", "1"],
        vec!["--request", "missing"],
    ] {
        let mut args = vec![
            "--model",
            &path,
            "--expected-model-sha256",
            &digest,
            "--expected-authored-revision",
            "held-revision",
        ];
        args.extend(extra);
        assert_eq!(
            admit(&inputs(&args), &never).unwrap_err().code(),
            "request_mode_invalid",
            "{args:?}"
        );
    }
}
#[test]
fn hashes_before_project_effects_and_preserves_both_exact_pin_spellings() {
    let (_dir, path, digest) = fixture();
    let args = [
        "--model",
        &path,
        "--expected-model-sha256",
        &digest,
        "--expected-authored-revision",
        "exact-held-revision",
        "--preview-only",
        "--sample-pages",
        "2",
        "--notes",
        "held-notes.json",
    ];
    let source = admit(&inputs(&args), &never).unwrap().unwrap();
    assert_eq!(source.digest, digest);
    assert_eq!(source.revision, "exact-held-revision");
    fs::write(&path, b"changed source").unwrap();
    assert_eq!(
        admit(&inputs(&args), &never).unwrap_err().code(),
        "model_changed"
    );
}
#[test]
fn source_admission_refuses_relative_links_oversize_and_bad_digest() {
    let (dir, path, digest) = fixture();
    let check = |path: &str, digest: &str| {
        admit(
            &inputs(&[
                "--model",
                path,
                "--expected-model-sha256",
                digest,
                "--expected-authored-revision",
                "held",
            ]),
            &never,
        )
        .unwrap_err()
        .code()
        .to_string()
    };
    assert_eq!(check("relative.dsgrid", &digest), "model_source_invalid");
    assert_eq!(check(&path, "aabb"), "model_source_invalid");
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_MODEL_BYTES + 1)
        .unwrap();
    assert_eq!(check(&path, &digest), "model_source_invalid");
    #[cfg(unix)]
    {
        let alias = dir.path().join("alias.dsgrid");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert_eq!(
            check(alias.to_str().unwrap(), &digest),
            "model_source_invalid"
        );
    }
}
#[test]
fn model_and_projection_requests_share_governed_setup_and_allowed_assets_exactly() {
    let (_dir, path, digest) = fixture();
    let source = admit(
        &inputs(&[
            "--model",
            &path,
            "--expected-model-sha256",
            &digest,
            "--expected-authored-revision",
            "revision-native",
        ]),
        &never,
    )
    .unwrap()
    .unwrap();
    let common = json!({"project_id":"project","project_crs":{"held":"exact"},"report_config":{"paper":"held"},"renderer_defaults":{"document":"renderer"},"style_resolution":{"layers":["style"]},"mv_setup":{"revision":"adopted"},"settings":{"title":"governed"},"publication_assets":{"logo":"held.pdf"},"context_page_files":["context.pdf"],"notes_path":"notes.json","side_profiles_path":"side.json","structure_descriptions_path":"descriptions.json","model_crs":"EPSG:32736","out_dir":"/output","sample_pages":2,"preview_only":true});
    let (projection_task, mut projection) = bind_request(
        common.clone(),
        None,
        Some(Path::new("/scene.json")),
        Some(Path::new("/plan.json")),
    );
    let (model_task, mut model) = bind_request(common.clone(), Some(&source), None, None);
    assert_eq!(projection_task.subcommand(), "render-grid-plan-profile");
    assert_eq!(model_task.subcommand(), "render-grid-plan-profile-model");
    assert_eq!(model["expected_model_sha256"], digest);
    assert_eq!(model["expected_authored_revision"], "revision-native");
    assert!(model.get("scene_path").is_none());
    assert!(model.get("plan_path").is_none());
    for key in ["scene_path", "plan_path"] {
        projection.as_object_mut().unwrap().remove(key);
    }
    for key in [
        "model_path",
        "expected_model_sha256",
        "expected_authored_revision",
    ] {
        model.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(
        serde_json::to_vec(&projection).unwrap(),
        serde_json::to_vec(&common).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&model).unwrap(),
        serde_json::to_vec(&common).unwrap()
    );
}
#[cfg(unix)]
#[test]
fn staged_dispatch_uses_only_fixed_model_or_projection_task_and_identical_request_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("reporter");
    let task_path = dir.path().join("task");
    let request_path = dir.path().join("request-copy");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    let script = format!(
        "#!/bin/sh\ntask=\"$1\"; shift\nwhile [ $# -gt 0 ]; do case \"$1\" in --request) request=\"$2\"; shift 2;; --result) result=\"$2\"; shift 2;; *) exit 9;; esac; done\nprintf '%s' \"$task\" > '{}'\ncp \"$request\" '{}'\nprintf '%s' '{{\"status\":\"ok\"}}' > \"$result\"\n",
        quote(&task_path),
        quote(&request_path)
    );
    fs::write(&executable, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let engine = DS_REPORT.capture_pinned(&executable).unwrap();
    for task in [RenderTask::Projection, RenderTask::Model] {
        let staged = Staged::new(None).unwrap();
        let bytes =
            serde_json::to_vec(&json!({"all":"exact staged bytes","pin":"sha256:unchanged"}))
                .unwrap();
        assert_eq!(
            staged.render(&engine, bytes.clone(), task).unwrap()["status"],
            "ok"
        );
        assert_eq!(fs::read_to_string(&task_path).unwrap(), task.subcommand());
        assert_eq!(fs::read(&request_path).unwrap(), bytes);
    }
}
#[test]
fn plain_output_reads_nested_model_print_or_preview_receipts() {
    assert!(render(&json!({"source_model_sha256":"sha256:held","source_authored_revision":"rev-model","preview":{"page_count":2,"alignments":1}})).contains("rev-model"));
    assert!(render(&json!({"source_model_sha256":"sha256:held","print":{"page_count":3,"alignments":2,"pdf":"held.pdf","model_revision":"print-revision"}})).contains("PDF held.pdf"));
}

/// A supplied fence never needs Network; a test that reaches it has failed.
fn never(_: &[u8]) -> Result<String, Failure> {
    panic!("the supplied authored revision must not be derived")
}

#[test]
fn omitted_revision_is_derived_from_exactly_the_verified_bytes() {
    let (_dir, path, digest) = fixture();
    let args = ["--model", &path, "--expected-model-sha256", &digest];
    let seen = std::cell::RefCell::new(Vec::new());
    let derive = |bytes: &[u8]| {
        seen.borrow_mut().push(bytes.to_vec());
        Ok("rev:derived-from-bytes".to_owned())
    };
    let source = admit(&inputs(&args), &derive).unwrap().unwrap();
    assert_eq!(source.revision, "rev:derived-from-bytes");
    assert_eq!(seen.borrow().as_slice(), [b"exact model bytes".to_vec()]);
    // The digest still fences first: changed bytes never reach Network.
    fs::write(&path, b"changed source").unwrap();
    assert_eq!(
        admit(&inputs(&args), &derive).unwrap_err().code(),
        "model_changed"
    );
    assert_eq!(seen.borrow().len(), 1);
    // Network's refusal is the command's refusal, never a guessed revision.
    fs::write(&path, b"exact model bytes").unwrap();
    let refused = |_: &[u8]| -> Result<String, Failure> {
        Err(Failure::unavailable("network_engine_missing", "absent"))
    };
    assert_eq!(
        admit(&inputs(&args), &refused).unwrap_err().code(),
        "network_engine_missing"
    );
}
