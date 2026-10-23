//! Governed Rwanda 10 m DEM resource shared by native application callers.
//!
//! The ordinary mode keeps only the immutable block index, coverage and
//! provenance locally. Native compute range-reads the public COG directly and
//! may keep compressed blocks in the bounded cache declared here. Heavy users
//! can explicitly install the whole COG for offline work. No DS API or Cloud
//! Run service participates in either path.

use crc32c::crc32c_append;
use ds_update_runtime::transfer::{
    download_verified, ensure_owned_directory, metadata_is_link_or_reparse,
    remove_owned_directory_if_present, TransferAdmission, TransferProgress,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const VERSION: &str = "2026-08-28";
const CONTRACT: &str = "ds-rwanda-dem-resource/v1";
const BASE_URL: &str = "https://storage.googleapis.com/edcl/reference/rwanda_dem/2026-08-28";
const TERRARIUM_URL_TEMPLATE: &str =
    "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png";
const RESOURCE_ROOT: &str = "reference-assets/rwanda-dem-10m/2026-08-28";
const ACTIVE_DIR: &str = "active";
const STAGING_DIR: &str = "staging";
const CACHE_DIR: &str = "blocks";
const RECEIPT_FILE: &str = "receipt.json";
/// Why the last installation failed, kept beside (never inside) the staging
/// and active directories, so status names it after the job has ended.
const LAST_FAILURE_FILE: &str = "last-install-failure.txt";
const LAST_FAILURE_MAX_BYTES: usize = 2048;
const INDEX_FILE: &str = "rwanda_dem10m_nix.blockindex.json";
const COVERAGE_FILE: &str = "rwanda_dem10m_coverage.geojson";
const PROVENANCE_FILE: &str = "README.md";
const COG_FILE: &str = "rwanda_dem10m_nix.tif";
const INDEX_SIZE: u64 = 184_412;
const COVERAGE_SIZE: u64 = 71_282;
const PROVENANCE_SIZE: u64 = 9_419;
const COG_SIZE: u64 = 882_872_744;
const INDEX_SHA256: &str = "c0bdd9c9058a999cebb5b34e1f4f39b3056f24dcf28892227833983f923f10e7";
const COVERAGE_SHA256: &str = "972ae047474b1ad553854438622e5156c76c08d0e0090ce4e99e5ee19e37d1e5";
const PROVENANCE_SHA256: &str = "c3b5ac0cea71817203ff256abe7534b1fd19120680f9e6d3e930be95db7185c3";
// Authoritative whole-object CRC32C published by GCS as `MdrRgA==`.
// The composite COG has no published MD5 or SHA-256; do not invent one.
const COG_CRC32C: u32 = 0x31da_d180;
const BLOCK_CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const BLOCK_CACHE_MAX_ENTRIES: u32 = 1_536;
pub(crate) const FULL_LOCAL_REQUIRED_ABOVE_POINTS: u32 = 4_000;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RwandaDemInstallRequest {
    mode: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwandaDemStatus {
    version: &'static str,
    state: String,
    mode: Option<String>,
    metadata_bytes: u64,
    offline_bytes: u64,
    cache_bytes: u64,
    cache_entries: u32,
    cache_max_bytes: u64,
    cache_max_entries: u32,
    partial_bytes: u64,
    required_offline_bytes: u64,
    message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwandaDemResourceConfig {
    version: &'static str,
    mode: String,
    index_path: String,
    coverage_path: String,
    provenance_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cog_path: Option<String>,
    cog_url: &'static str,
    terrarium_url_template: &'static str,
    cache_path: String,
    cache_max_bytes: u64,
    cache_max_entries: u32,
    cog_size_bytes: u64,
    cog_crc32c: String,
    full_local_required_above_points: u32,
}

/// Verified paths handed to native elevation compute. This is deliberately
/// crate-private: the public command returns strings for the webview, while
/// native callers retain `PathBuf` authority and cannot invent a resource
/// location or bypass receipt verification.
pub(crate) struct VerifiedRwandaDemRuntime {
    pub(crate) mode: String,
    pub(crate) index_path: PathBuf,
    pub(crate) cog_path: Option<PathBuf>,
    pub(crate) cog_url: &'static str,
    pub(crate) terrarium_url_template: &'static str,
    pub(crate) cache_path: PathBuf,
    pub(crate) cache_max_bytes: u64,
    pub(crate) cache_max_entries: u32,
    pub(crate) cog_size_bytes: u64,
    pub(crate) full_local_required_above_points: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwandaDemInstallProgress {
    running: bool,
    status: String,
    progress: u8,
    message: String,
    bytes_downloaded: u64,
    bytes_total: u64,
}

impl Default for RwandaDemInstallProgress {
    fn default() -> Self {
        Self {
            running: false,
            status: "idle".to_string(),
            progress: 0,
            message: "No Rwanda DEM installation is running".to_string(),
            bytes_downloaded: 0,
            bytes_total: 0,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InstalledReceipt {
    schema: u32,
    contract: String,
    version: String,
    mode: String,
    index_sha256: String,
    coverage_sha256: String,
    provenance_sha256: String,
    cog_size_bytes: u64,
    cog_crc32c: String,
    cog_observed_sha256: Option<String>,
    installed_at: String,
}

struct ActiveJob {
    progress: Arc<Mutex<RwandaDemInstallProgress>>,
}

#[derive(Default)]
pub struct RwandaDemResourceState(Mutex<Option<ActiveJob>>);

fn resource_root() -> Result<PathBuf, String> {
    ensure_owned_directory(
        &ds_layer_store::reference_cache::configured_root()?,
        Path::new(RESOURCE_ROOT),
    )
}

fn regular_file(path: &Path, expected_size: Option<u64>) -> Result<u64, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("inspect {}: {error}", path.display()))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(format!("{} is not an owned regular file", path.display()));
    }
    if expected_size.is_some_and(|expected| metadata.len() != expected) {
        return Err(format!("{} has an unexpected byte size", path.display()));
    }
    Ok(metadata.len())
}

fn sha256_file(path: &Path, expected_size: u64) -> Result<String, String> {
    regular_file(path, Some(expected_size))?;
    let mut file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_metadata(active: &Path) -> Result<InstalledReceipt, String> {
    let receipt_path = active.join(RECEIPT_FILE);
    regular_file(&receipt_path, None)?;
    let bytes = std::fs::read(&receipt_path)
        .map_err(|error| format!("read Rwanda DEM receipt: {error}"))?;
    if bytes.len() > 16 * 1024 {
        return Err("Rwanda DEM receipt is unexpectedly large".to_string());
    }
    let receipt: InstalledReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse Rwanda DEM receipt: {error}"))?;
    if receipt.schema != 1
        || receipt.contract != CONTRACT
        || receipt.version != VERSION
        || !matches!(receipt.mode.as_str(), "range" | "offline")
        || receipt.index_sha256 != INDEX_SHA256
        || receipt.coverage_sha256 != COVERAGE_SHA256
        || receipt.provenance_sha256 != PROVENANCE_SHA256
        || receipt.cog_size_bytes != COG_SIZE
        || receipt.cog_crc32c != format!("{COG_CRC32C:08x}")
    {
        return Err("Rwanda DEM receipt does not match the immutable catalog".to_string());
    }
    if sha256_file(&active.join(INDEX_FILE), INDEX_SIZE)? != INDEX_SHA256
        || sha256_file(&active.join(COVERAGE_FILE), COVERAGE_SIZE)? != COVERAGE_SHA256
        || sha256_file(&active.join(PROVENANCE_FILE), PROVENANCE_SIZE)? != PROVENANCE_SHA256
    {
        return Err("Rwanda DEM metadata failed exact SHA-256 verification".to_string());
    }
    if receipt.mode == "offline" {
        regular_file(&active.join(COG_FILE), Some(COG_SIZE))?;
    }
    Ok(receipt)
}

fn directory_usage(path: &Path) -> Result<(u64, u32), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(format!("inspect Rwanda DEM cache: {error}")),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err("Rwanda DEM cache is not an owned directory".to_string());
    }
    let mut bytes = 0_u64;
    let mut entries = 0_u32;
    for item in
        std::fs::read_dir(path).map_err(|error| format!("read Rwanda DEM cache: {error}"))?
    {
        let item = item.map_err(|error| format!("read Rwanda DEM cache entry: {error}"))?;
        let metadata = item
            .file_type()
            .and_then(|kind| {
                if kind.is_file() {
                    item.metadata()
                } else {
                    Err(std::io::Error::other("not a regular file"))
                }
            })
            .map_err(|_| "Rwanda DEM cache contains a linked or nested entry".to_string())?;
        bytes = bytes.saturating_add(metadata.len());
        entries = entries.saturating_add(1);
    }
    Ok((bytes, entries))
}

fn partial_bytes(root: &Path) -> u64 {
    let staging = root.join(STAGING_DIR);
    [INDEX_FILE, COVERAGE_FILE, PROVENANCE_FILE, COG_FILE]
        .iter()
        .flat_map(|name| {
            [
                staging.join(name),
                staging.join(name).with_extension("part"),
            ]
        })
        .filter_map(|path| std::fs::metadata(path).ok().map(|metadata| metadata.len()))
        .sum()
}

fn status_at(root: &Path) -> RwandaDemStatus {
    let (cache_bytes, cache_entries) = directory_usage(&root.join(CACHE_DIR)).unwrap_or((0, 0));
    let active = root.join(ACTIVE_DIR);
    match verify_metadata(&active) {
        Ok(receipt) => RwandaDemStatus {
            required_offline_bytes: COG_SIZE + INDEX_SIZE + COVERAGE_SIZE + PROVENANCE_SIZE,
            version: VERSION,
            state: "ready".to_string(),
            mode: Some(receipt.mode.clone()),
            metadata_bytes: INDEX_SIZE + COVERAGE_SIZE + PROVENANCE_SIZE,
            offline_bytes: if receipt.mode == "offline" {
                COG_SIZE
            } else {
                0
            },
            cache_bytes,
            cache_entries,
            cache_max_bytes: BLOCK_CACHE_MAX_BYTES,
            cache_max_entries: BLOCK_CACHE_MAX_ENTRIES,
            partial_bytes: partial_bytes(root),
            message: None,
        },
        Err(_error) if !active.exists() => RwandaDemStatus {
            required_offline_bytes: COG_SIZE + INDEX_SIZE + COVERAGE_SIZE + PROVENANCE_SIZE,
            version: VERSION,
            state: "missing".to_string(),
            mode: None,
            metadata_bytes: 0,
            offline_bytes: 0,
            cache_bytes,
            cache_entries,
            cache_max_bytes: BLOCK_CACHE_MAX_BYTES,
            cache_max_entries: BLOCK_CACHE_MAX_ENTRIES,
            partial_bytes: partial_bytes(root),
            message: match last_install_failure(root) {
                Some(failure) => Some(format!(
                    "The last Rwanda DEM installation failed: {failure}"
                )),
                None if partial_bytes(root) > 0 => {
                    Some("An interrupted download can be resumed".to_string())
                }
                None => None,
            },
        },
        Err(error) => RwandaDemStatus {
            required_offline_bytes: COG_SIZE + INDEX_SIZE + COVERAGE_SIZE + PROVENANCE_SIZE,
            version: VERSION,
            state: "broken".to_string(),
            mode: None,
            metadata_bytes: 0,
            offline_bytes: 0,
            cache_bytes,
            cache_entries,
            cache_max_bytes: BLOCK_CACHE_MAX_BYTES,
            cache_max_entries: BLOCK_CACHE_MAX_ENTRIES,
            partial_bytes: partial_bytes(root),
            message: Some(error),
        },
    }
}

/// Feedback dbe4d17e: a complete download whose verification or activation
/// then failed left no `.part` and no active directory, so status answered a
/// bare "missing" and the cause lived only in the ended job's progress.
fn record_install_failure(root: &Path, error: &str) {
    let mut text = error.trim().to_string();
    if text.len() > LAST_FAILURE_MAX_BYTES {
        let mut end = LAST_FAILURE_MAX_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    if !text.is_empty() {
        let _ = atomic_write(&root.join(LAST_FAILURE_FILE), text.as_bytes());
    }
}

fn clear_install_failure(root: &Path) {
    let _ = std::fs::remove_file(root.join(LAST_FAILURE_FILE));
}

fn last_install_failure(root: &Path) -> Option<String> {
    let path = root.join(LAST_FAILURE_FILE);
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > LAST_FAILURE_MAX_BYTES as u64 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn set_progress(
    progress: &Arc<Mutex<RwandaDemInstallProgress>>,
    status: &str,
    percent: u8,
    message: impl Into<String>,
    downloaded: u64,
    total: u64,
) {
    if let Ok(mut current) = progress.lock() {
        *current = RwandaDemInstallProgress {
            running: !matches!(status, "complete" | "failed"),
            status: status.to_string(),
            progress: percent,
            message: message.into(),
            bytes_downloaded: downloaded,
            bytes_total: total,
        };
    }
}

fn ensure_exact_file(
    url: &str,
    destination: &Path,
    expected_sha256: &str,
    expected_size: u64,
    label: &str,
    progress: &Arc<Mutex<RwandaDemInstallProgress>>,
    check_online: crate::OnlineCheck,
) -> Result<(), String> {
    if destination.exists() {
        if sha256_file(destination, expected_size).is_ok_and(|value| value == expected_sha256) {
            return Ok(());
        }
        std::fs::remove_file(destination)
            .map_err(|error| format!("remove invalid staged {label}: {error}"))?;
    }
    let verified = download_verified(
        url,
        destination,
        expected_sha256,
        expected_size,
        label,
        |event: TransferProgress| {
            set_progress(
                progress,
                event.status,
                event.progress.min(94),
                event.message,
                event.bytes_downloaded,
                event.bytes_total,
            );
        },
        TransferAdmission {
            check_cancelled: || Ok(()),
            admit_transport: check_online,
        },
    )?;
    drop(verified);
    Ok(())
}

// Full terrain uses the same retry/resume and verification boundary as software
// and geographic bundles. The previous one-shot transfer failed after 45 seconds.
fn download_cog(
    staging: &Path,
    progress: &Arc<Mutex<RwandaDemInstallProgress>>,
    check_online: crate::OnlineCheck,
) -> Result<String, String> {
    let destination = staging.join(COG_FILE);
    if !destination.exists() {
        let held = ds_update_runtime::transfer::download_crc32c_verified(
            &format!("{BASE_URL}/{COG_FILE}"),
            &destination,
            COG_CRC32C,
            COG_SIZE,
            "Rwanda elevation",
            |event| {
                set_progress(
                    progress,
                    event.status,
                    event.progress,
                    &event.message,
                    event.bytes_downloaded,
                    event.bytes_total,
                );
            },
            TransferAdmission {
                check_cancelled: || Ok(()),
                admit_transport: check_online,
            },
        )?;
        drop(held);
    }
    verify_cog(&destination, progress)
}

fn verify_cog(
    path: &Path,
    progress: &Arc<Mutex<RwandaDemInstallProgress>>,
) -> Result<String, String> {
    regular_file(path, Some(COG_SIZE))?;
    let mut file = File::open(path).map_err(|error| format!("open Rwanda DEM COG: {error}"))?;
    let mut sha = Sha256::new();
    let mut crc = 0_u32;
    let mut read = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read Rwanda DEM COG: {error}"))?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
        crc = crc32c_append(crc, &buffer[..count]);
        read += count as u64;
        set_progress(
            progress,
            "verifying",
            94 + ((read as f64 / COG_SIZE as f64) * 5.0).floor() as u8,
            "Verifying the offline COG against its published CRC32C",
            read,
            COG_SIZE,
        );
    }
    if read != COG_SIZE || crc != COG_CRC32C {
        return Err("Rwanda DEM COG failed its published size/CRC32C proof".to_string());
    }
    Ok(format!("{:x}", sha.finalize()))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Rwanda DEM receipt has no parent".to_string())?;
    let temp = parent.join(format!(".receipt-{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut file = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| format!("create Rwanda DEM receipt staging file: {error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("persist Rwanda DEM receipt: {error}"))?;
    drop(file);
    std::fs::rename(&temp, path).map_err(|error| format!("publish Rwanda DEM receipt: {error}"))
}

fn remove_staged_file_if_present(path: &Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("inspect staged Rwanda DEM file: {error}")),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err("staged Rwanda DEM path is not an owned regular file".to_string());
    }
    std::fs::remove_file(path).map_err(|error| format!("remove staged Rwanda DEM file: {error}"))
}

fn activate_staging(root: &Path) -> Result<(), String> {
    let active = root.join(ACTIVE_DIR);
    let staging = root.join(STAGING_DIR);
    let backup = root.join(format!("previous-{}", uuid::Uuid::new_v4().simple()));
    let had_active = active.exists();
    if had_active {
        std::fs::rename(&active, &backup)
            .map_err(|error| format!("hold prior Rwanda DEM resource: {error}"))?;
    }
    if let Err(error) = std::fs::rename(&staging, &active) {
        if had_active {
            let _ = std::fs::rename(&backup, &active);
        }
        return Err(format!("publish Rwanda DEM resource: {error}"));
    }
    if had_active {
        remove_owned_directory_if_present(&backup, "prior Rwanda DEM resource")?;
    }
    Ok(())
}

fn install_at(
    root: &Path,
    mode: &str,
    progress: Arc<Mutex<RwandaDemInstallProgress>>,
    check_online: crate::OnlineCheck,
) -> Result<(), String> {
    if !matches!(mode, "range" | "offline") {
        return Err("Rwanda DEM install mode must be range or offline".to_string());
    }
    let _installation = installation_lock(root)?;
    let staging = ensure_owned_directory(root, Path::new(STAGING_DIR))?;
    let total = if mode == "offline" {
        COG_SIZE + INDEX_SIZE + COVERAGE_SIZE + PROVENANCE_SIZE
    } else {
        INDEX_SIZE + COVERAGE_SIZE + PROVENANCE_SIZE
    };
    set_progress(
        &progress,
        "downloading",
        1,
        "Installing Rwanda DEM metadata",
        0,
        total,
    );
    ensure_exact_file(
        &format!("{BASE_URL}/{INDEX_FILE}"),
        &staging.join(INDEX_FILE),
        INDEX_SHA256,
        INDEX_SIZE,
        "Rwanda DEM block index",
        &progress,
        check_online,
    )?;
    ensure_exact_file(
        &format!("{BASE_URL}/{COVERAGE_FILE}"),
        &staging.join(COVERAGE_FILE),
        COVERAGE_SHA256,
        COVERAGE_SIZE,
        "Rwanda DEM coverage",
        &progress,
        check_online,
    )?;
    ensure_exact_file(
        &format!("{BASE_URL}/{PROVENANCE_FILE}"),
        &staging.join(PROVENANCE_FILE),
        PROVENANCE_SHA256,
        PROVENANCE_SIZE,
        "Rwanda DEM provenance",
        &progress,
        check_online,
    )?;
    let cog_sha = if mode == "offline" {
        Some(download_cog(&staging, &progress, check_online)?)
    } else {
        // An interrupted offline upgrade may have left resumable COG bytes in
        // staging. A later explicit range-only install must not publish or
        // retain that undeclared 842 MiB payload.
        remove_staged_file_if_present(&staging.join(COG_FILE))?;
        remove_staged_file_if_present(&staging.join(COG_FILE).with_extension("part"))?;
        None
    };
    let receipt = InstalledReceipt {
        schema: 1,
        contract: CONTRACT.to_string(),
        version: VERSION.to_string(),
        mode: mode.to_string(),
        index_sha256: INDEX_SHA256.to_string(),
        coverage_sha256: COVERAGE_SHA256.to_string(),
        provenance_sha256: PROVENANCE_SHA256.to_string(),
        cog_size_bytes: COG_SIZE,
        cog_crc32c: format!("{COG_CRC32C:08x}"),
        cog_observed_sha256: cog_sha,
        installed_at: chrono::Utc::now().to_rfc3339(),
    };
    let receipt_bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| format!("encode Rwanda DEM receipt: {error}"))?;
    atomic_write(&staging.join(RECEIPT_FILE), &receipt_bytes)?;
    verify_metadata(&staging)?;
    ensure_owned_directory(root, Path::new(CACHE_DIR))?;
    activate_staging(root)?;
    set_progress(
        &progress,
        "complete",
        100,
        if mode == "offline" {
            "Rwanda DEM is verified and ready offline"
        } else {
            "Rwanda DEM metadata is verified; native compute will use direct range reads"
        },
        total,
        total,
    );
    Ok(())
}

/// CLI processes and Desktop share staging and activation, so process-local
/// state alone cannot own installation. The held regular-file lock covers the
/// complete staging/verification/activation sequence and releases on failure.
fn installation_lock(root: &Path) -> Result<File, String> {
    let mut options = ds_layer_store::private::open_options();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options
        .open(root.join("install.lock"))
        .map_err(|e| format!("open Rwanda DEM installation lock: {e}"))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("inspect Rwanda DEM installation lock: {e}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err("Rwanda DEM installation lock must be a regular file".into());
    }
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
        "Rwanda DEM installation is already running; wait for it to finish, then retry".to_string()
    })?;
    Ok(file)
}

/// Admission-only inspection: no cache directory is created and no acquisition
/// occurs while deciding a point-cloud plan.
pub(crate) fn installed_full_available() -> bool {
    ds_layer_store::reference_cache::configured_root()
        .ok()
        .and_then(|root| verify_metadata(&root.join(RESOURCE_ROOT).join(ACTIVE_DIR)).ok())
        .is_some_and(|receipt| receipt.mode == "offline")
}

pub(crate) fn retry_full_local<T>(
    mut run: impl FnMut() -> Result<T, String>,
    requires_full: impl Fn(&T) -> bool,
    mut install_full: impl FnMut() -> Result<(), String>,
) -> Result<T, String> {
    let result = run()?;
    if requires_full(&result) {
        install_full()?;
        return run();
    }
    Ok(result)
}

fn start_install(
    state: &RwandaDemResourceState,
    mode: String,
    check_online: crate::OnlineCheck,
) -> Result<RwandaDemInstallProgress, String> {
    if !matches!(mode.as_str(), "range" | "offline") {
        return Err("Rwanda DEM install mode must be range or offline".to_string());
    }
    let root = resource_root()?;
    let mut slot = state
        .0
        .lock()
        .map_err(|_| "Rwanda DEM installation state is unavailable".to_string())?;
    if let Some(job) = slot.as_ref() {
        let current = job
            .progress
            .lock()
            .map_err(|_| "Rwanda DEM progress is unavailable".to_string())?
            .clone();
        if current.running {
            return Ok(current);
        }
    }
    let progress = Arc::new(Mutex::new(RwandaDemInstallProgress {
        running: true,
        status: "starting".to_string(),
        progress: 0,
        message: "Preparing Rwanda DEM resource".to_string(),
        bytes_downloaded: 0,
        bytes_total: if mode == "offline" {
            COG_SIZE
        } else {
            INDEX_SIZE + COVERAGE_SIZE + PROVENANCE_SIZE
        },
    }));
    *slot = Some(ActiveJob {
        progress: Arc::clone(&progress),
    });
    drop(slot);
    let initial = progress
        .lock()
        .map_err(|_| "Rwanda DEM progress is unavailable".to_string())?
        .clone();
    std::thread::spawn(
        move || match install_at(&root, &mode, Arc::clone(&progress), check_online) {
            Ok(()) => clear_install_failure(&root),
            Err(error) => {
                record_install_failure(&root, &error);
                set_progress(&progress, "failed", 0, error, 0, 0);
            }
        },
    );
    Ok(initial)
}

pub fn rwanda_dem_resource_status() -> Result<RwandaDemStatus, String> {
    Ok(status_at(&resource_root()?))
}

pub fn rwanda_dem_resource_config() -> Result<RwandaDemResourceConfig, String> {
    let root = resource_root()?;
    let active = root.join(ACTIVE_DIR);
    let receipt = verify_metadata(&active)?;
    let cache = ensure_owned_directory(&root, Path::new(CACHE_DIR))?;
    let cog_url: &'static str = concat!(
        "https://storage.googleapis.com/edcl/reference/rwanda_dem/2026-08-28/",
        "rwanda_dem10m_nix.tif"
    );
    Ok(RwandaDemResourceConfig {
        version: VERSION,
        mode: receipt.mode.clone(),
        index_path: active.join(INDEX_FILE).to_string_lossy().into_owned(),
        coverage_path: active.join(COVERAGE_FILE).to_string_lossy().into_owned(),
        provenance_path: active.join(PROVENANCE_FILE).to_string_lossy().into_owned(),
        cog_path: (receipt.mode == "offline")
            .then(|| active.join(COG_FILE).to_string_lossy().into_owned()),
        cog_url,
        terrarium_url_template: TERRARIUM_URL_TEMPLATE,
        cache_path: cache.to_string_lossy().into_owned(),
        cache_max_bytes: BLOCK_CACHE_MAX_BYTES,
        cache_max_entries: BLOCK_CACHE_MAX_ENTRIES,
        cog_size_bytes: COG_SIZE,
        cog_crc32c: format!("{COG_CRC32C:08x}"),
        full_local_required_above_points: FULL_LOCAL_REQUIRED_ABOVE_POINTS,
    })
}

pub(crate) fn verified_rwanda_dem_runtime() -> Result<VerifiedRwandaDemRuntime, String> {
    let root = resource_root()?;
    let active = root.join(ACTIVE_DIR);
    let receipt = verify_metadata(&active)?;
    let cache = ensure_owned_directory(&root, Path::new(CACHE_DIR))?;
    let cog_url: &'static str = concat!(
        "https://storage.googleapis.com/edcl/reference/rwanda_dem/2026-08-28/",
        "rwanda_dem10m_nix.tif"
    );
    Ok(VerifiedRwandaDemRuntime {
        mode: receipt.mode.clone(),
        index_path: active.join(INDEX_FILE),
        cog_path: (receipt.mode == "offline").then(|| active.join(COG_FILE)),
        cog_url,
        terrarium_url_template: TERRARIUM_URL_TEMPLATE,
        cache_path: cache,
        cache_max_bytes: BLOCK_CACHE_MAX_BYTES,
        cache_max_entries: BLOCK_CACHE_MAX_ENTRIES,
        cog_size_bytes: COG_SIZE,
        full_local_required_above_points: FULL_LOCAL_REQUIRED_ABOVE_POINTS,
    })
}

pub fn rwanda_dem_resource_install(
    context: &crate::RequestContext,
    state: &RwandaDemResourceState,
    request: RwandaDemInstallRequest,
) -> Result<RwandaDemInstallProgress, String> {
    context.validate()?;
    (context.check_online)()?;
    start_install(state, request.mode, context.check_online)
}

pub fn rwanda_dem_resource_progress(
    state: &RwandaDemResourceState,
) -> Result<RwandaDemInstallProgress, String> {
    let slot = state
        .0
        .lock()
        .map_err(|_| "Rwanda DEM installation state is unavailable".to_string())?;
    let Some(job) = slot.as_ref() else {
        return Ok(RwandaDemInstallProgress::default());
    };
    let current = job
        .progress
        .lock()
        .map_err(|_| "Rwanda DEM progress is unavailable".to_string())?
        .clone();
    Ok(current)
}

/// Resolve the governed local resource; acquisition is an intentional effect of
/// the same application operation on CLI and Desktop.
pub(crate) fn ensure_available(context: &crate::RequestContext, full: bool) -> Result<(), String> {
    context.validate()?;
    if let Ok(runtime) = verified_rwanda_dem_runtime() {
        if !full || runtime.mode == "offline" {
            return Ok(());
        }
    }
    (context.check_online)()?;
    let root = resource_root()?;
    install_at(
        &root,
        if full { "offline" } else { "range" },
        Arc::new(Mutex::new(RwandaDemInstallProgress::default())),
        context.check_online,
    )
}

#[cfg(test)]
#[path = "rwanda_dem_tests.rs"]
mod tests;
