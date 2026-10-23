//! Verified files and atomic publication of an already-produced artifact batch.
use anyhow::{Context, Result, bail, ensure};
use ds_report_artifacts::confined_fs::HeldDirectory;
use ds_solar_contracts::{SolarArtifactBatch, SolarError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path},
};

pub fn ordinary_dir(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink(),
        "Solar directory cannot be a link"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            m.permissions().mode() & 0o077 == 0,
            "Solar workspace must be private (0700)"
        );
    }
    Ok(())
}
pub use ds_solar_host::files::{ordinary_file, read, sync_dir, write};
pub fn private_dir(path: &Path) -> Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
pub fn private_file(path: &Path) -> Result<()> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
pub fn json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write(path, &bytes)
}
pub fn verify(root: &Path, batch: &SolarArtifactBatch) -> Result<()> {
    ordinary_dir(root)?;
    batch.validate()?;
    for artifact in &batch.artifacts {
        let relative = Path::new(&artifact.name);
        ensure!(
            relative
                .components()
                .all(|p| matches!(p, Component::Normal(_))),
            "unsafe artifact name"
        );
        let mut path = root.to_path_buf();
        for part in relative.components() {
            path.push(part);
            ensure!(
                !fs::symlink_metadata(&path)?.file_type().is_symlink(),
                "artifact path contains a link"
            );
        }
        let bytes = read(&path, 128 * 1024 * 1024)?;
        ensure!(
            bytes.len() as u64 == artifact.byte_count
                && format!("sha256:{:x}", Sha256::digest(&bytes)) == artifact.content_digest,
            "artifact size or digest mismatch: {}",
            artifact.name
        );
    }
    Ok(())
}

/// Read one regular file without trusting a pathname metadata check to remain
/// true. Metadata comes from the opened handle, and `max+1` closes the growth
/// race before any caller deserializes the body.
pub fn read_bounded(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("reading {label} {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("inspecting {label} {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{label} {} is not a regular file", path.display());
    }
    if metadata.len() > max_bytes {
        bail!(
            "{label} {} has {} bytes, exceeding the {max_bytes}-byte read bound",
            path.display(),
            metadata.len()
        );
    }
    let read_limit = max_bytes
        .checked_add(1)
        .context("file read bound overflowed")?;
    let initial_capacity = usize::try_from(metadata.len())
        .context("file size does not fit this platform's address space")?;
    let mut bytes = Vec::with_capacity(initial_capacity);
    BufReader::new(file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {label} {}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        bail!(
            "{label} {} grew beyond the {max_bytes}-byte read bound while it was read",
            path.display()
        );
    }
    if bytes.len() as u64 != metadata.len() {
        bail!(
            "{label} {} changed size while it was read ({} to {} bytes)",
            path.display(),
            metadata.len(),
            bytes.len()
        );
    }
    Ok(bytes)
}

/// One failed city, serialized in input order beside the successful artifacts.
#[derive(Debug)]
pub struct RunFailure<'a> {
    pub city_id: &'a str,
    pub error: &'a SolarError,
}

/// Commit an already-closed local batch without selecting or running an engine.
/// The adapter only stages its already-produced artifact bytes. This owner verifies
/// them, writes the unchanged local sidecars, and publishes the directory without
/// replacing another writer's output. Calculation failures do not discard successes.
/// A verified candidate is retained if publication loses a race. If the rename
/// succeeds but its final checks fail, the error also names the destination; both
/// locations must be inspected before retrying a calculation.
pub fn commit_artifact_run(
    out: &Path,
    batch: &SolarArtifactBatch,
    failures: &[RunFailure<'_>],
    progress: Option<&serde_json::Value>,
    write_artifacts: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    batch.validate()?;
    match fs::symlink_metadata(out) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => bail!("Solar run directory already exists"),
        Err(error) => return Err(error.into()),
    }
    let out = std::path::absolute(out)?;
    let parent_path = out.parent().context("run needs a parent")?;
    let name = out.file_name().context("run needs a directory name")?;
    // Preserve local CLI parent aliases and `..` paths, then hold the resolved
    // directory so publication cannot be redirected by a later alias change.
    fs::create_dir_all(parent_path)?;
    let parent_path = fs::canonicalize(parent_path)?;
    let parent = HeldDirectory::open_absolute(&parent_path)
        .map_err(anyhow::Error::msg)?
        .context("Solar run parent disappeared")?;
    let staging = tempfile::Builder::new()
        .prefix(".solar-run-")
        .tempdir_in(parent.path())?;
    let root = staging.path();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    let held = parent
        .open_child(root.file_name().context("staging needs a name")?)
        .map_err(anyhow::Error::msg)?
        .context("Solar staging directory disappeared")?;
    write_artifacts(root)?;
    verify(root, batch)?;
    json(
        &root.join("sync-intent.json"),
        &batch.sync_intent(crate::timestamp()),
    )?;
    json(&root.join("batch.json"), batch)?;
    // Preserve the legacy JSON-value projection, including nested object key order.
    let failures: Vec<_> = failures
        .iter()
        .map(|failure| serde_json::json!({"city_id": failure.city_id, "error": failure.error}))
        .collect();
    json(&root.join("failures.json"), &failures)?;
    if let Some(progress) = progress {
        json(&root.join("progress.json"), progress)?;
    }
    sync_tree(root)?;
    ensure!(
        parent.identity_matches_path().map_err(anyhow::Error::msg)?
            && held.identity_matches_path().map_err(anyhow::Error::msg)?,
        "Solar staging directory changed before publication"
    );
    let candidate = staging.keep();
    parent.rename_child(held, name).map_err(|error| anyhow::anyhow!(
        "Solar run publication did not complete cleanly: {error}; inspect verified candidate {} and destination {} before retrying (the rename may already have completed)",
        candidate.display(), out.display()
    ))?;
    Ok(())
}

fn sync_tree(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        }
    }
    sync_dir(path)
}
