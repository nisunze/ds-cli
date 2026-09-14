//! Install one complete national reference bundle: a port of the desktop
//! shell's `reference_data.rs::install_reference_bundle`, with the byte
//! transfer injected by the host so this crate never opens a socket.
//!
//! The live cache must never observe a version before its complete expanded
//! digest has been checked: the index is built in a private staging root and
//! promoted atomically by `ds_layer_store::reference_cache::publish_staged`.

use ds_command_kernel::reference_cache::{Identity, MAX_PAGES, MAX_RESOURCE_BYTES, PAGE_ROWS};
use ds_layer_store::reference_cache::{execute_at, publish_staged, BundleIndexWriter};
use flate2::read::GzDecoder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use crate::catalog::BUNDLE_HOST;
use crate::Failure;

/// The verified bundle a catalogue row publishes.
#[derive(Debug, Clone)]
pub struct BundleReceipt {
    pub identity: Identity,
    pub url: String,
    pub bundle_sha256: String,
    pub compressed_bytes: u64,
    pub expanded_sha256: String,
    pub expanded_bytes: u64,
    pub feature_count: u64,
    pub label: String,
}

fn exact_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl BundleReceipt {
    pub fn validate(&self) -> Result<(), Failure> {
        self.identity
            .validate()
            .map_err(Failure::BundleUnavailable)?;
        let valid = self.url.starts_with(BUNDLE_HOST)
            && exact_sha256(&self.bundle_sha256)
            && exact_sha256(&self.expanded_sha256)
            && self.compressed_bytes > 0
            && self.expanded_bytes > 0
            && self.expanded_bytes <= MAX_RESOURCE_BYTES
            && self.feature_count > 0
            && self.feature_count <= (MAX_PAGES * PAGE_ROWS) as u64
            && !self.label.trim().is_empty()
            && self.label.len() <= 160;
        if valid {
            Ok(())
        } else {
            Err(Failure::BundleUnavailable(
                "invalid geographic dataset bundle receipt".into(),
            ))
        }
    }
}

fn unavailable(error: impl std::fmt::Display) -> Failure {
    Failure::BundleUnavailable(error.to_string())
}

/// Digest and size of one file, read once.
fn file_digest(path: &Path) -> Result<(String, u64), Failure> {
    let mut file = File::open(path).map_err(unavailable)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    let mut bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(unavailable)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

/// The staging directory this install owns, under the store's staging root.
/// Anything left there by an interrupted run is removed first: a bundle that
/// failed expansion can never become valid by resuming it.
fn staging_dir(root: &Path, receipt: &BundleReceipt) -> Result<PathBuf, Failure> {
    let parent = root.join("reference-bundle-staging");
    fs::create_dir_all(&parent).map_err(unavailable)?;
    let dir = parent.join(format!(
        "reference-bundle-{}-{}",
        receipt.identity.resource_id, receipt.identity.version
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(unavailable)?;
    }
    fs::create_dir_all(&dir).map_err(unavailable)?;
    Ok(dir)
}

/// Is this exact version already installed and indexed?
pub fn installed(root: &Path, identity: &Identity) -> Result<bool, Failure> {
    let status = execute_at(root, json!({"action": "status", "identity": identity}))
        .map_err(crate::store)?;
    Ok(status["ready"] == Value::Bool(true))
}

/// Download (through `fetch`), verify, expand, index and publish one bundle.
///
/// `fetch(receipt, destination)` transfers the compressed bytes and returns; this
/// crate verifies their size and digest against the receipt before reading a
/// single feature, so a fetch that trusts nothing still installs nothing
/// unverified. Returns the store's status of the installed version.
pub fn install_bundle(
    root: &Path,
    receipt: &BundleReceipt,
    fetch: &mut dyn FnMut(&BundleReceipt, &Path) -> Result<(), String>,
) -> Result<Value, Failure> {
    receipt.validate()?;
    if installed(root, &receipt.identity)? {
        return execute_at(
            root,
            json!({"action": "status", "identity": receipt.identity}),
        )
        .map_err(crate::store);
    }
    let staging = staging_dir(root, receipt)?;
    let result = (|| -> Result<Value, Failure> {
        let bundle_path = staging.join("bundle.geojsonl.gz");
        fetch(receipt, &bundle_path).map_err(Failure::BundleUnavailable)?;
        let (digest, bytes) = file_digest(&bundle_path)?;
        if bytes != receipt.compressed_bytes || digest != receipt.bundle_sha256 {
            return Err(Failure::BundleUnavailable(format!(
                "downloaded bundle for {} is not the published bytes ({bytes} bytes, sha256 {digest})",
                receipt.label
            )));
        }
        let indexed = staging.join("index");
        fs::create_dir_all(&indexed).map_err(unavailable)?;
        let file = File::open(&bundle_path).map_err(unavailable)?;
        let mut reader = BufReader::new(GzDecoder::new(file));
        let mut expanded_sha = Sha256::new();
        let mut expanded_bytes = 0_u64;
        let mut feature_count = 0_u64;
        let mut index =
            BundleIndexWriter::new(&indexed, receipt.identity.clone()).map_err(crate::store)?;
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader
                .read_until(b'\n', &mut line)
                .map_err(|error| Failure::BundleUnavailable(format!("decompress: {error}")))?;
            if read == 0 {
                break;
            }
            expanded_sha.update(&line);
            expanded_bytes = expanded_bytes.saturating_add(read as u64);
            if expanded_bytes > receipt.expanded_bytes {
                return Err(Failure::BundleUnavailable(
                    "geographic dataset exceeds its declared expanded size".into(),
                ));
            }
            let mut start = 0;
            while start < line.len() && line[start].is_ascii_whitespace() {
                start += 1;
            }
            // GeoJSON Text Sequences prefix each record with RS (0x1e).
            if line.get(start) == Some(&0x1e) {
                start += 1;
            }
            while start < line.len() && line[start].is_ascii_whitespace() {
                start += 1;
            }
            let mut end = line.len();
            while end > start && line[end - 1].is_ascii_whitespace() {
                end -= 1;
            }
            if start == end {
                continue;
            }
            let feature: Value = serde_json::from_slice(&line[start..end]).map_err(|_| {
                Failure::BundleUnavailable("geographic dataset contains invalid GeoJSON".into())
            })?;
            if feature["type"] != "Feature"
                || !feature["properties"].is_object()
                || !(feature["geometry"].is_object() || feature["geometry"].is_null())
            {
                return Err(Failure::BundleUnavailable(
                    "geographic dataset contains an invalid feature".into(),
                ));
            }
            index.push(&feature).map_err(crate::store)?;
            feature_count += 1;
        }
        let expanded_digest = format!("{:x}", expanded_sha.finalize());
        if expanded_bytes != receipt.expanded_bytes
            || expanded_digest != receipt.expanded_sha256
            || feature_count != receipt.feature_count
        {
            return Err(Failure::BundleUnavailable(
                "geographic dataset expanded size, digest, or feature count mismatch".into(),
            ));
        }
        index.finish(expanded_bytes).map_err(crate::store)?;
        publish_staged(root, &indexed, &receipt.identity).map_err(crate::store)
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}
