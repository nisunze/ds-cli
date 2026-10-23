//! Native filesystem source preparation for desktop hosts.
//!
//! A browser `File` is already a copy-oriented abstraction. Native desktop
//! imports need an explicit decision before a large source is touched:
//! managed copy, copy to an operator-chosen destination, or link in place.
//! This module owns that filesystem policy so the Tauri shell stays a thin
//! transport and Windows/macOS/Linux use the same implementation.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const FINGERPRINT_SAMPLE_BYTES: usize = 64 * 1024;

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSourceDisposition {
    ManagedCopy,
    ExternalCopy,
    Linked,
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeLinkAccess {
    ReadOnly,
    Editable,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSourcePrepareRequest {
    pub source_path: PathBuf,
    pub disposition: NativeSourceDisposition,
    #[serde(default)]
    pub link_access: Option<NativeLinkAccess>,
    /// Directory for `managed_copy`, exact file path for `external_copy`, and
    /// absent for `linked`. The desktop shell supplies its app-data import
    /// directory for a managed copy; the frontend never chooses that path.
    #[serde(default)]
    pub destination: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSourceFingerprint {
    pub size_bytes: u64,
    pub modified_unix_ms: Option<u64>,
    /// SHA-256 over size, mtime, and bounded first/last samples. This detects
    /// ordinary in-place changes without reading a 1 GB source twice merely
    /// to open it. A full content digest belongs in the streamed import pass.
    pub quick_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSourceEditContract {
    /// Changes are staged beside the source, never written through an open
    /// source handle. A later commit must lock, re-fingerprint, fsync, and
    /// atomically replace the source.
    pub strategy: &'static str,
    pub sidecar_path: PathBuf,
    pub lock_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSourcePrepareResult {
    pub source_path: PathBuf,
    pub prepared_path: PathBuf,
    pub disposition: NativeSourceDisposition,
    pub link_access: NativeLinkAccess,
    pub copied: bool,
    pub fingerprint: NativeSourceFingerprint,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<NativeSourceEditContract>,
}

pub fn prepare_native_source(
    request: NativeSourcePrepareRequest,
) -> Result<NativeSourcePrepareResult, String> {
    let source = canonical_regular_file(&request.source_path)?;
    let fingerprint = quick_fingerprint(&source)?;
    let access = request.link_access.unwrap_or(NativeLinkAccess::ReadOnly);

    let (prepared_path, copied) = match request.disposition {
        NativeSourceDisposition::Linked => {
            if request.destination.is_some() {
                return Err("linked sources do not accept a copy destination".to_string());
            }
            (source.clone(), false)
        }
        NativeSourceDisposition::ManagedCopy => {
            let root = request.destination.as_deref().ok_or_else(|| {
                "managed copy requires an app-data destination directory".to_string()
            })?;
            fs::create_dir_all(root).map_err(|error| {
                format!(
                    "could not create managed import directory {}: {error}",
                    root.display()
                )
            })?;
            let target = managed_copy_path(root, &source, &fingerprint)?;
            copy_new(&source, &target)?;
            (target, true)
        }
        NativeSourceDisposition::ExternalCopy => {
            let target = request
                .destination
                .as_deref()
                .ok_or_else(|| "external copy requires an exact destination file".to_string())?;
            if target.file_name().is_none() {
                return Err("external copy destination must name a file".to_string());
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!("could not create destination {}: {error}", parent.display())
                })?;
            }
            copy_new(&source, target)?;
            (target.to_path_buf(), true)
        }
    };

    let edit = if request.disposition == NativeSourceDisposition::Linked
        && access == NativeLinkAccess::Editable
    {
        Some(edit_contract(&prepared_path))
    } else {
        None
    };

    Ok(NativeSourcePrepareResult {
        source_path: source,
        prepared_path,
        disposition: request.disposition,
        link_access: access,
        copied,
        fingerprint,
        edit,
    })
}

fn canonical_regular_file(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("could not open source {}: {error}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("could not inspect source {}: {error}", canonical.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "source is not a regular file: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn quick_fingerprint(path: &Path) -> Result<NativeSourceFingerprint, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("could not read source {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not inspect source {}: {error}", path.display()))?;
    let size_bytes = metadata.len();
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok());

    let mut digest = Sha256::new();
    digest.update(size_bytes.to_le_bytes());
    digest.update(modified_unix_ms.unwrap_or(0).to_le_bytes());
    let mut sample = vec![0_u8; FINGERPRINT_SAMPLE_BYTES];
    let first = file
        .read(&mut sample)
        .map_err(|error| format!("could not sample source {}: {error}", path.display()))?;
    digest.update(&sample[..first]);
    if size_bytes > FINGERPRINT_SAMPLE_BYTES as u64 {
        let tail = size_bytes.saturating_sub(FINGERPRINT_SAMPLE_BYTES as u64);
        file.seek(SeekFrom::Start(tail))
            .map_err(|error| format!("could not seek source {}: {error}", path.display()))?;
        let last = file
            .read(&mut sample)
            .map_err(|error| format!("could not sample source {}: {error}", path.display()))?;
        digest.update(&sample[..last]);
    }

    Ok(NativeSourceFingerprint {
        size_bytes,
        modified_unix_ms,
        quick_sha256: format!("{:x}", digest.finalize()),
    })
}

fn managed_copy_path(
    root: &Path,
    source: &Path,
    fingerprint: &NativeSourceFingerprint,
) -> Result<PathBuf, String> {
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "source file name is not valid UTF-8".to_string())?;
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    Ok(root.join(format!(
        "{stem}-{}{}",
        &fingerprint.quick_sha256[..12],
        extension
    )))
}

fn copy_new(source: &Path, target: &Path) -> Result<(), String> {
    if source == target {
        return Err("copy destination resolves to the source file".to_string());
    }
    let mut input = File::open(source)
        .map_err(|error| format!("could not read source {}: {error}", source.display()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| {
            format!(
                "refusing to overwrite destination {}: {error}",
                target.display()
            )
        })?;
    if let Err(error) = std::io::copy(&mut input, &mut output) {
        let _ = fs::remove_file(target);
        return Err(format!(
            "could not copy source to {}: {error}",
            target.display()
        ));
    }
    output
        .flush()
        .and_then(|_| output.sync_all())
        .map_err(|error| format!("could not finish copy {}: {error}", target.display()))
}

fn edit_contract(path: &Path) -> NativeSourceEditContract {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("source");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    NativeSourceEditContract {
        strategy: "sidecar_then_locked_atomic_replace",
        sidecar_path: parent.join(format!(".{file_name}.ds-delta")),
        lock_path: parent.join(format!(".{file_name}.ds-lock")),
    }
}

#[cfg(test)]
#[path = "native_source_tests.rs"]
mod tests;
