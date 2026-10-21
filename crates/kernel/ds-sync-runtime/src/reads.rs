//! A verified read of one head output for hosts without a download manager
//! of their own (the `ds` host, the server).
//!
//! The ticket the record mints is a signed object URL on the DS storage
//! host. Nothing is attached to it — it is its own authority — and nothing is
//! trusted from it: the bytes are streamed to a private temporary file,
//! digested in the same pass, and placed only once the digest and the size
//! both match what the head declared.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use ds_command_kernel::sync;
use sha2::{Digest, Sha256};

use crate::host::Reads;

/// The ONE host a DS-minted download ticket may address (contract §3 of the
/// transport authority: the same pinned host the resumable sessions use).
const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &["storage.googleapis.com"];
const MAX_TICKET_URL_BYTES: usize = 8192;
const READ_SLICE_BYTES: usize = 64 * 1024;
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Progress semantics, not a deadline: the slowest link still served, plus a
/// fixed grace. A download that keeps moving is never killed by a wall clock.
const MIN_PROGRESS_BYTES_PER_SECOND: u64 = 4 * 1024;
const PROGRESS_GRACE: std::time::Duration = std::time::Duration::from_secs(60);

/// A read cache rooted at one host-owned directory.
pub struct VerifiedReads {
    root: PathBuf,
}

impl VerifiedReads {
    pub fn new(root: PathBuf) -> Self {
        VerifiedReads { root }
    }

    /// Inspect an already acquired exact head without creating directories or
    /// contacting the authority. The caller supplies a validated scoped head.
    pub fn retained_head(
        &self,
        project: &str,
        head: &ds_command_kernel::compute_artifact_inventory::Head,
    ) -> Result<Option<PathBuf>, String> {
        if !ds_command_kernel::execution_context::valid_project(project) {
            return Err("Invalid retained project.".into());
        }
        let value = serde_json::to_value(head).map_err(|e| e.to_string())?;
        ds_command_kernel::compute_artifact_inventory::decode_head(
            &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
        )?;
        let row = crate::rows::remote_row(&value);
        validate_output_names(&row)?;
        let mut path = self.root.clone();
        for part in ["", project, &head.engine, &head.operation, &head.variant] {
            if !part.is_empty() {
                path.push(part);
            }
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
                Ok(_) => return Err("Retained output directory is linked or not ordinary.".into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(held_head(&path, &row)?.then_some(path))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Why a ticket URL was refused. Typed, and never echoing the URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TicketRefusal {
    Unparsable,
    TooLong,
    SchemeNotHttps,
    CarriesUserinfo,
    HostNotAllowed,
    NonDefaultPort,
}

pub fn validated_ticket_url(raw: &str) -> Result<String, TicketRefusal> {
    if raw.len() > MAX_TICKET_URL_BYTES {
        return Err(TicketRefusal::TooLong);
    }
    if raw.is_empty() || raw.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(TicketRefusal::Unparsable);
    }
    let url = url::Url::parse(raw).map_err(|_| TicketRefusal::Unparsable)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(TicketRefusal::CarriesUserinfo);
    }
    if url.scheme() != "https" {
        return Err(TicketRefusal::SchemeNotHttps);
    }
    let host = url.host_str().ok_or(TicketRefusal::HostNotAllowed)?;
    if !ALLOWED_DOWNLOAD_HOSTS.contains(&host) {
        return Err(TicketRefusal::HostNotAllowed);
    }
    if url.port_or_known_default() != Some(443) {
        return Err(TicketRefusal::NonDefaultPort);
    }
    Ok(url.to_string())
}

/// `base/<relative>` created component by component, refusing links and
/// anything that is not a plain directory name.
pub fn owned_directory(base: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut directories = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directories.mode(0o700);
    }
    directories
        .recursive(true)
        .create(base)
        .map_err(|error| format!("create owned base {}: {error}", base.display()))?;
    let base_metadata = std::fs::symlink_metadata(base)
        .map_err(|error| format!("inspect owned base {}: {error}", base.display()))?;
    if base_metadata.file_type().is_symlink() || !base_metadata.is_dir() {
        return Err(format!(
            "owned base is linked or not a directory: {}",
            base.display()
        ));
    }
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("owned directory has an unsafe relative path".to_string());
        };
        current.push(name);
        match directories.recursive(false).create(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "create owned directory {}: {error}",
                    current.display()
                ));
            }
        }
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect owned directory {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "owned directory is linked or not a directory: {}",
                current.display()
            ));
        }
    }
    Ok(current)
}

/// Stream `reader` into `destination` through a private temporary file,
/// proving the digest and the size before the file is placed.
pub fn place_verified(
    mut reader: impl Read,
    destination: &Path,
    sha256: &str,
    size_bytes: u64,
) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "the download destination has no directory".to_string())?;
    let temporary = parent.join(format!(
        ".{}.part-{}",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("output"),
        uuid::Uuid::new_v4().simple()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("could not create the download file: {error}"))?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; READ_SLICE_BYTES];
    let outcome = loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(read) => read,
            Err(error) => break Err(format!("the download stopped: {}", error.kind())),
        };
        total = total.saturating_add(read as u64);
        if total > size_bytes {
            break Err("the download exceeds the size the head declares".to_string());
        }
        hasher.update(&buffer[..read]);
        if let Err(error) = file.write_all(&buffer[..read]) {
            break Err(format!("could not write the download file: {error}"));
        }
    };
    let outcome = outcome.and_then(|()| {
        if total != size_bytes {
            return Err("the download ended before the size the head declares".to_string());
        }
        if format!("{:x}", hasher.finalize()) != sha256 {
            return Err(
                "the downloaded bytes do not match the digest the head declares".to_string(),
            );
        }
        file.sync_all()
            .map_err(|error| format!("could not sync the download file: {error}"))?;
        std::fs::rename(&temporary, destination)
            .map_err(|error| format!("could not place the download file: {error}"))?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("could not sync the download directory: {error}"))?;
        Ok(())
    });
    if outcome.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    outcome
}

/// Inspect a downloaded replica against the current authoritative head. These
/// facts are never producer rows: a stale or removed remote head cannot turn a
/// cached download into a local publication.
pub(crate) fn held_head(root: &Path, head: &crate::rows::RemoteRow) -> Result<bool, String> {
    let receipt = root.join("head.json");
    let metadata = match std::fs::symlink_metadata(&receipt) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("could not inspect the downloaded head: {error}")),
    };
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Ok(false);
    }
    let file = std::fs::File::open(receipt).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let Ok(receipt) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(false);
    };
    if receipt["sha256"].as_str() != Some(head.sha256.as_str())
        || receipt["revision"].as_str() != Some(head.revision.as_str())
    {
        return Ok(false);
    }
    validate_output_names(head)?;
    for output in &head.outputs {
        let path = root.join(&output.output_id);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("could not inspect the cached output: {error}")),
        };
        if !metadata.is_file() || metadata.len() != output.size_bytes {
            return Ok(false);
        }
        let mut file = std::fs::File::open(path)
            .map_err(|error| error.to_string())?
            .take(output.size_bytes.saturating_add(1));
        let mut hash = Sha256::new();
        let mut count = 0_u64;
        let mut buffer = [0_u8; READ_SLICE_BYTES];
        loop {
            let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            count += read as u64;
            hash.update(&buffer[..read]);
        }
        if count != output.size_bytes || format!("{:x}", hash.finalize()) != output.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn validate_output_names(head: &crate::rows::RemoteRow) -> Result<(), String> {
    if head.outputs.is_empty() {
        return Err("head declares no outputs".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for output in &head.outputs {
        let name = &output.output_id;
        if name.is_empty()
            || name.len() > 128
            || name.eq_ignore_ascii_case("head.json")
            || name == "."
            || name == ".."
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            || !seen.insert(name.to_ascii_lowercase())
        {
            return Err("head output names are unsafe or repeated".into());
        }
    }
    Ok(())
}

impl Reads for VerifiedReads {
    fn head_root(&self, project: &str, identity: &sync::Identity) -> Result<PathBuf, String> {
        let relative = Path::new(project)
            .join(&identity.engine)
            .join(&identity.operation)
            .join(&identity.variant);
        owned_directory(&self.root, &relative)
    }

    fn download_verified(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<(), String> {
        let url = validated_ticket_url(url)
            .map_err(|refusal| format!("the download ticket was refused: {refusal:?}"))?;
        let body_budget =
            std::time::Duration::from_secs(size_bytes / MIN_PROGRESS_BYTES_PER_SECOND)
                .saturating_add(PROGRESS_GRACE);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .https_only(true)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(CONNECT_TIMEOUT))
            .timeout_recv_body(Some(body_budget))
            .build()
            .into();
        let response = agent.get(&url).call().map_err(|error| match error {
            ureq::Error::StatusCode(status) => format!("the storage host answered {status}"),
            _ => "the storage host could not be reached".to_string(),
        })?;
        let mut body = response.into_body();
        let reader = body
            .with_config()
            .limit(size_bytes.saturating_add(1))
            .reader();
        place_verified(reader, destination, sha256, size_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_full_head_is_read_only_and_requires_exact_revision_and_physical_bytes() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let reads = VerifiedReads::new(dir.path().join("absent"));
        let bytes = b"report";
        let sha = format!("{:x}", Sha256::digest(bytes));
        let raw = json!({"work_id":"work1","engine":"network_reporter","operation":"export-test","variant":"default","input_base_fingerprint":"a".repeat(64),"head_revision":1,"updated_at":"2026-10-18T00:00:00Z","outputs":[{"output_id":"xlsx","format":"xlsx","content_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","sha256":sha,"size_bytes":bytes.len()}]});
        let mut head = ds_command_kernel::compute_artifact_inventory::decode_head(
            &serde_json::to_vec(&raw).unwrap(),
        )
        .unwrap();
        assert!(reads.retained_head("project", &head).unwrap().is_none());
        assert!(!reads.root().exists());
        let row = crate::rows::remote_row(&raw);
        let root = reads.head_root("project", &row.identity).unwrap();
        std::fs::write(root.join("xlsx"), bytes).unwrap();
        std::fs::write(
            root.join("head.json"),
            serde_json::to_vec(&json!({"sha256":row.sha256,"revision":row.revision})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            reads.retained_head("project", &head).unwrap(),
            Some(root.clone())
        );
        head.head_revision = 2;
        assert!(reads.retained_head("project", &head).unwrap().is_none());
        head.head_revision = 1;
        std::fs::write(root.join("xlsx"), b"broken").unwrap();
        assert!(reads.retained_head("project", &head).unwrap().is_none());
    }

    #[test]
    fn only_a_ds_storage_ticket_is_accepted() {
        assert!(
            validated_ticket_url("https://storage.googleapis.com/b/o?X-Goog-Signature=1").is_ok()
        );
        for bad in [
            "http://storage.googleapis.com/b/o",
            "https://user:pw@storage.googleapis.com/b/o",
            "https://storage.example/signed",
            "https://storage.googleapis.com:8443/b/o",
            "",
        ] {
            assert!(validated_ticket_url(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn bytes_are_placed_only_when_digest_and_size_match() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("xlsx");
        let bytes = b"hello world".to_vec();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert!(place_verified(&bytes[..], &destination, &digest, 11).is_ok());
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);

        let wrong = dir.path().join("wrong");
        assert!(place_verified(&bytes[..], &wrong, &"0".repeat(64), 11).is_err());
        assert!(!wrong.exists());
        assert!(place_verified(&bytes[..], &wrong, &digest, 10).is_err());
        assert!(place_verified(&bytes[..], &wrong, &digest, 12).is_err());
        assert!(!wrong.exists());
        assert!(
            std::fs::read_dir(dir.path()).unwrap().count() == 1,
            "no partial file is left behind"
        );
    }

    #[test]
    fn a_verified_cache_survives_reader_restart_without_trusting_new_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sync-downloads");
        let identity = sync::Identity {
            engine: "network_reporter".into(),
            operation: "export-tx_1".into(),
            variant: "default".into(),
        };
        let first = VerifiedReads::new(root.clone());
        let destination = first.head_root("p1", &identity).unwrap().join("xlsx");
        let bytes = b"restart-safe bytes";
        let digest = format!("{:x}", Sha256::digest(bytes));
        place_verified(&bytes[..], &destination, &digest, bytes.len() as u64).unwrap();
        drop(first);

        let restarted = VerifiedReads::new(root);
        let reopened = restarted.head_root("p1", &identity).unwrap().join("xlsx");
        assert_eq!(std::fs::read(reopened).unwrap(), bytes);
        assert!(
            place_verified(&b"corrupt"[..], &destination, &digest, bytes.len() as u64).is_err()
        );
        assert_eq!(std::fs::read(destination).unwrap(), bytes);
    }

    #[test]
    fn a_head_root_is_keyed_by_project_and_identity_only() {
        let dir = tempfile::tempdir().unwrap();
        let reads = VerifiedReads::new(dir.path().join("sync-downloads"));
        let identity = sync::Identity {
            engine: "network_reporter".into(),
            operation: "export-tx_1".into(),
            variant: "default".into(),
        };
        let root = reads.head_root("p1", &identity).unwrap();
        assert_eq!(
            root,
            dir.path()
                .join("sync-downloads")
                .join("p1")
                .join("network_reporter")
                .join("export-tx_1")
                .join("default")
        );
        assert!(owned_directory(dir.path(), Path::new("../escape")).is_err());
    }
}
