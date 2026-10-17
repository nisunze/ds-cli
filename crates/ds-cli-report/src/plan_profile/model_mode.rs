//! Pinned model input admission only; Reporter owns projection and printing.
use super::*;
use std::{
    fs,
    io::Read,
    path::{Component, Path},
};

const MAX_MODEL_BYTES: u64 = 512 * 1024 * 1024;
pub(super) const SOURCE_INVALID: Refusal = Refusal {
    code: "model_source_invalid",
    when: "model mode has an unreadable, linked, relative or oversized source, malformed digest or missing authored revision",
    remedy: "supply an ordinary absolute .dsgrid of at most 512 MiB and its exact sha256: digest and authored revision",
};
pub(super) const SOURCE_CHANGED: Refusal = Refusal {
    code: "model_changed",
    when: "the model bytes differ from the expected SHA-256 before project reads",
    remedy: "retain the source and supply the digest of the exact intended package; Reporter rechecks it before projection",
};

#[derive(Debug)]
pub(super) struct ModelSource {
    path: PathBuf,
    digest: String,
    revision: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RenderTask {
    Projection,
    Model,
}
impl RenderTask {
    pub(super) fn subcommand(self) -> &'static str {
        match self {
            Self::Projection => "render-grid-plan-profile",
            Self::Model => "render-grid-plan-profile-model",
        }
    }
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(SOURCE_INVALID.code, message).remedy(SOURCE_INVALID.remedy)
}
pub(super) fn admit(inputs: &Inputs) -> Result<Option<ModelSource>, Failure> {
    const PINS: &[&str] = &[
        "model",
        "expected-model-sha256",
        "expected-authored-revision",
    ];
    if !PINS.iter().any(|name| inputs.value(name).is_some()) {
        return Ok(None);
    }
    if inputs.value("request").is_some() {
        return Err(mode_invalid(
            "--request excludes all model input flags".into(),
        ));
    }
    if !PINS.iter().all(|name| inputs.value(name).is_some()) {
        return Err(mode_invalid(
            "--model requires --expected-model-sha256 and --expected-authored-revision together"
                .into(),
        ));
    }
    if inputs.value("scene").is_some()
        || inputs.value("plan").is_some()
        || !inputs.repeated("alignment").is_empty()
        || inputs.switch("preview-all")
        || [
            "preview-layout",
            "preview-retention",
            "preview-job",
            "preview-page",
        ]
        .iter()
        .any(|name| inputs.value(name).is_some())
    {
        return Err(mode_invalid("pinned model mode excludes --scene, --plan, --alignment and template/job preview flags".into()));
    }
    let digest = inputs.require("expected-model-sha256")?;
    let hex = digest
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| {
            invalid("expected model digest must be sha256: plus 64 lowercase hexadecimal digits")
        })?;
    let revision = inputs.require("expected-authored-revision")?;
    if revision.is_empty() || revision.len() > 4096 {
        return Err(invalid(
            "expected authored revision must be nonempty and bounded",
        ));
    }
    let path = PathBuf::from(inputs.require("model")?);
    if !path.is_absolute()
        || path
            .extension()
            .is_none_or(|extension| extension != "dsgrid")
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid(
            "model must be an absolute .dsgrid path without parent traversal",
        ));
    }
    for parent in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(parent).map_err(|error| invalid(error.to_string()))?;
        if linked(&metadata) || !metadata.is_dir() {
            return Err(invalid("model ancestors must be ordinary directories"));
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| invalid(error.to_string()))?;
    if linked(&metadata) || !metadata.is_file() || metadata.len() > MAX_MODEL_BYTES {
        return Err(invalid(
            "model must be one ordinary file of at most 512 MiB",
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| invalid(error.to_string()))?;
    let opened = file
        .metadata()
        .map_err(|error| invalid(error.to_string()))?;
    if linked(&opened) || !opened.is_file() || opened.len() > MAX_MODEL_BYTES {
        return Err(invalid("opened model is not an ordinary bounded file"));
    }
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| invalid(error.to_string()))?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > MAX_MODEL_BYTES {
            return Err(invalid("model grew beyond 512 MiB"));
        }
        hash.update(&buffer[..read]);
    }
    if format!("{:x}", hash.finalize()) != hex {
        return Err(Failure::conflict(
            SOURCE_CHANGED.code,
            "model bytes differ from --expected-model-sha256",
        )
        .remedy(SOURCE_CHANGED.remedy));
    }
    Ok(Some(ModelSource {
        path,
        digest: digest.into(),
        revision: revision.into(),
    }))
}
fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
pub(super) fn bind_request(
    mut common: Value,
    model: Option<&ModelSource>,
    scene: Option<&Path>,
    plan: Option<&Path>,
) -> (RenderTask, Value) {
    let task = if let Some(model) = model {
        common["model_path"] = json!(model.path);
        common["expected_model_sha256"] = json!(model.digest);
        common["expected_authored_revision"] = json!(model.revision);
        RenderTask::Model
    } else {
        common["scene_path"] = json!(scene);
        common["plan_path"] = json!(plan);
        RenderTask::Projection
    };
    (task, common)
}

#[cfg(test)]
mod tests {
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
                admit(&inputs(&args)).unwrap_err().code(),
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
                admit(&inputs(&args)).unwrap_err().code(),
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
        let source = admit(&inputs(&args)).unwrap().unwrap();
        assert_eq!(source.digest, digest);
        assert_eq!(source.revision, "exact-held-revision");
        fs::write(&path, b"changed source").unwrap();
        assert_eq!(admit(&inputs(&args)).unwrap_err().code(), "model_changed");
    }
    #[test]
    fn source_admission_refuses_relative_links_oversize_and_bad_digest() {
        let (dir, path, digest) = fixture();
        let check = |path: &str, digest: &str| {
            admit(&inputs(&[
                "--model",
                path,
                "--expected-model-sha256",
                digest,
                "--expected-authored-revision",
                "held",
            ]))
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
        let source = admit(&inputs(&[
            "--model",
            &path,
            "--expected-model-sha256",
            &digest,
            "--expected-authored-revision",
            "revision-native",
        ]))
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
}
