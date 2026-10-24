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
use std::sync::atomic::{AtomicBool, Ordering};

use ds_command_kernel::sync;
use sha2::{Digest, Sha256};

use crate::gateway::GatewayError;
use crate::host::Reads;
use crate::read_cause;

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
    reader: impl Read,
    destination: &Path,
    sha256: &str,
    size_bytes: u64,
) -> Result<(), String> {
    place_verified_cause(reader, destination, sha256, size_bytes).map_err(String::from)
}

/// [`place_verified`] with the closed cause of the failure when the network or
/// the verification named one (`read_cause`): the sentence is the same.
pub fn place_verified_cause(
    mut reader: impl Read,
    destination: &Path,
    sha256: &str,
    size_bytes: u64,
) -> Result<(), GatewayError> {
    let parent = destination
        .parent()
        .ok_or_else(|| GatewayError::from("the download destination has no directory"))?;
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
    let mut file = options.open(&temporary).map_err(|error| {
        GatewayError::from(format!("could not create the download file: {error}"))
    })?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; READ_SLICE_BYTES];
    let outcome = loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(read) => read,
            Err(error) => break Err(read_cause::body_failure(error.kind())),
        };
        total = total.saturating_add(read as u64);
        if total > size_bytes {
            break Err(read_cause::size_exceeded());
        }
        hasher.update(&buffer[..read]);
        if let Err(error) = file.write_all(&buffer[..read]) {
            break Err(GatewayError::from(format!(
                "could not write the download file: {error}"
            )));
        }
    };
    let outcome = outcome.and_then(|()| {
        if total != size_bytes {
            return Err(read_cause::size_short());
        }
        if format!("{:x}", hasher.finalize()) != sha256 {
            return Err(read_cause::digest_mismatch());
        }
        file.sync_all().map_err(|error| {
            GatewayError::from(format!("could not sync the download file: {error}"))
        })?;
        std::fs::rename(&temporary, destination).map_err(|error| {
            GatewayError::from(format!("could not place the download file: {error}"))
        })?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                GatewayError::from(format!("could not sync the download directory: {error}"))
            })?;
        Ok(())
    });
    if outcome.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    outcome
}

/// The most directory entries one sweep inspects: a bounded walk, whatever a
/// cache has grown to. What it does not reach waits for the next pass.
const SWEEP_MAX_ENTRIES: usize = 100_000;

/// Remove the temporaries `place_verified` left under one project's download
/// root (`<root>/<project>/<engine>/<operation>/<variant>/.<output>.part-<uuid>`)
/// when its process was killed or crashed mid-object: only regular files of
/// exactly that name, last written before `before`. A live writer keeps its
/// file's mtime moving, and one opened after `before` is never touched; links
/// are neither followed nor removed. Answers the files and bytes removed.
/// The caller holds the project's lease (`SyncHost::sweep_temporaries`).
pub fn sweep_temporaries(
    root: &Path,
    project: &str,
    before: std::time::SystemTime,
) -> Result<(usize, u64), String> {
    if !ds_command_kernel::execution_context::valid_project(project) {
        return Err("download temporaries are swept for one valid project".into());
    }
    let mut removed = (0_usize, 0_u64);
    let mut visited = 0_usize;
    // (directory, depth below the project root); heads sit at depth 3.
    let mut pending = vec![(root.join(project), 0_u8)];
    while let Some((directory, depth)) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("could not read the download cache: {error}")),
        };
        for entry in entries {
            visited += 1;
            if visited > SWEEP_MAX_ENTRIES {
                return Ok(removed);
            }
            let entry =
                entry.map_err(|error| format!("could not read the download cache: {error}"))?;
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.is_dir() && depth < 3 {
                pending.push((entry.path(), depth + 1));
                continue;
            }
            let orphan = depth == 3
                && metadata.is_file()
                && entry.file_name().to_str().is_some_and(is_temporary_name)
                && metadata.modified().is_ok_and(|written| written < before);
            if orphan {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => {
                        removed.0 += 1;
                        removed.1 = removed.1.saturating_add(metadata.len());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(format!("could not remove a download temporary: {error}"))
                    }
                }
            }
        }
    }
    Ok(removed)
}

/// `.<output>.part-<32 lowercase hex>`, the one name `place_verified` writes.
fn is_temporary_name(name: &str) -> bool {
    let Some(stem) = name.strip_prefix('.') else {
        return false;
    };
    let Some((output, id)) = stem.rsplit_once(".part-") else {
        return false;
    };
    !output.is_empty()
        && id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
        if !holds_output(
            &root.join(&output.output_id),
            &output.sha256,
            output.size_bytes,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `path` already holds exactly these declared bytes: a regular file
/// of `size_bytes` whose digest is `sha256`, read again, never assumed.
pub(crate) fn holds_output(path: &Path, sha256: &str, size_bytes: u64) -> Result<bool, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("could not inspect the cached output: {error}")),
    };
    if !metadata.is_file() || metadata.len() != size_bytes {
        return Ok(false);
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(size_bytes.saturating_add(1));
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
    Ok(count == size_bytes && format!("{:x}", hash.finalize()) == sha256)
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
        static NEVER: AtomicBool = AtomicBool::new(false);
        self.download_verified_until(url, destination, sha256, size_bytes, &NEVER)
    }

    fn download_verified_until(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        self.download_verified_cause_until(url, destination, sha256, size_bytes, stop)
            .map_err(String::from)
    }

    fn download_verified_cause_until(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
        stop: &AtomicBool,
    ) -> Result<(), GatewayError> {
        let url = validated_ticket_url(url).map_err(|refusal| {
            GatewayError::from(format!("the download ticket was refused: {refusal:?}"))
        })?;
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
        let response = agent
            .get(&url)
            .call()
            .map_err(|error| read_cause::request_failure(&error))?;
        let mut body = response.into_body();
        let reader = body
            .with_config()
            .limit(size_bytes.saturating_add(1))
            .reader();
        place_verified_cause(Stoppable { reader, stop }, destination, sha256, size_bytes)
    }

    fn sweep_temporaries(
        &self,
        project: &str,
        before: std::time::SystemTime,
    ) -> Result<(usize, u64), String> {
        sweep_temporaries(&self.root, project, before)
    }
}

/// A body that ends with an error at the next read once the host is
/// stopping. `place_verified` then removes its partial file, so nothing
/// unverified is ever placed and a later pass reads the object again.
struct Stoppable<'a, R> {
    reader: R,
    stop: &'a AtomicBool,
}

impl<R: Read> Read for Stoppable<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.stop.load(Ordering::Acquire) {
            return Err(std::io::Error::other("the host is stopping"));
        }
        self.reader.read(buffer)
    }
}

#[cfg(test)]
#[path = "reads_stop_tests.rs"]
mod stop_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_full_head_is_read_only_and_requires_exact_revision_and_physical_bytes() {
        use serde_json::json;
        let dir = crate::test_directory::tempdir().unwrap();
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
        let dir = crate::test_directory::tempdir().unwrap();
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
        let dir = crate::test_directory::tempdir().unwrap();
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
        let dir = crate::test_directory::tempdir().unwrap();
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
