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
    when: "model mode has an unreadable, linked, relative or oversized source, a malformed digest or revision, or bytes Network cannot open",
    remedy: "supply an ordinary absolute .dsgrid of at most 512 MiB and its exact sha256: digest from design model list or export",
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
/// The digest that design model list and export report is the one identity a
/// caller supplies. The authored revision is a fact of those exact bytes, so
/// Network derives it when the optional fence is omitted.
pub(super) fn derive_authored_revision(bytes: &[u8]) -> Result<String, Failure> {
    let engine = |error: ds_network_client::NetworkFailure| {
        let refusal = match error.owner_code().unwrap_or(error.code()) {
            "network_engine_missing" => crate::project::mv_context::NETWORK_ENGINE_MISSING,
            code if code.starts_with("network_engine_") => {
                crate::project::mv_context::NETWORK_ENGINE_INCOMPATIBLE
            }
            _ => return invalid(error.to_string()),
        };
        Failure::unavailable(refusal.code, error.to_string()).remedy(refusal.remedy)
    };
    let client = ds_network_client::CapturedNetworkClient::capture(1, &|| false).map_err(engine)?;
    let stage = ds_network_client::private_disk_stage().map_err(engine)?;
    Ok(client
        .grid_inspect(bytes, stage.path(), &|| false)
        .map_err(engine)?
        .result
        .source
        .expected_revision)
}
pub(super) fn admit(
    inputs: &Inputs,
    derive: &dyn Fn(&[u8]) -> Result<String, Failure>,
) -> Result<Option<ModelSource>, Failure> {
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
    if !PINS[..2].iter().all(|name| inputs.value(name).is_some()) {
        return Err(mode_invalid(
            "--model requires --expected-model-sha256; --expected-authored-revision is an optional extra fence"
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
    let revision = inputs.value("expected-authored-revision");
    if revision.is_some_and(|revision| revision.is_empty() || revision.len() > 4096) {
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
    // Only a derived revision needs the verified bytes themselves.
    let mut held = revision.is_none().then(Vec::new);
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
        if let Some(held) = &mut held {
            held.extend_from_slice(&buffer[..read]);
        }
    }
    if format!("{:x}", hash.finalize()) != hex {
        return Err(Failure::conflict(
            SOURCE_CHANGED.code,
            "model bytes differ from --expected-model-sha256",
        )
        .remedy(SOURCE_CHANGED.remedy));
    }
    let revision = match (revision, held) {
        (Some(revision), _) => revision.to_owned(),
        (None, Some(bytes)) => derive(&bytes)?,
        (None, None) => unreachable!("bytes are held whenever the revision is derived"),
    };
    Ok(Some(ModelSource {
        path,
        digest: digest.into(),
        revision,
    }))
}
impl ModelSource {
    /// The model's WGS84 routes, projected by Network from the same verified
    /// bytes Reporter prints; changed bytes refuse before any context read.
    pub(super) fn projection(&self, lane: &str) -> Result<Value, Failure> {
        let bytes = fs::read(&self.path).map_err(|error| invalid(error.to_string()))?;
        if format!("sha256:{:x}", Sha256::digest(&bytes)) != self.digest {
            return Err(Failure::conflict(
                SOURCE_CHANGED.code,
                "model bytes changed after admission",
            )
            .remedy(SOURCE_CHANGED.remedy));
        }
        ds_project_data::mv_projection::project(&bytes, &self.revision, lane).map_err(|error| {
            let refusal = if error.code() == "network_engine_missing" {
                crate::project::mv_context::NETWORK_ENGINE_MISSING
            } else if error.unavailable() {
                crate::project::mv_context::NETWORK_ENGINE_INCOMPATIBLE
            } else {
                return Failure::failed(
                    super::context::CONTEXT_UNAVAILABLE.code,
                    error.to_string(),
                )
                .remedy(super::context::CONTEXT_UNAVAILABLE.remedy);
            };
            Failure::unavailable(refusal.code, error.to_string()).remedy(refusal.remedy)
        })
    }
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
#[path = "model_mode_tests.rs"]
mod tests;
