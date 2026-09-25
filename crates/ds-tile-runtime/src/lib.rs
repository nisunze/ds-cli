//! Shared native tiling for Linux Server, containers and Desktop.
//!
//! Hosts authorize and capture source inputs before calling this crate. It makes
//! no network calls, uses no credentials or ambient active-project selection,
//! and never publishes. A verified local artifact is distinct from publication.
mod archive;
use anyhow::{bail, Context, Result};
pub use archive::TippecanoeOptions;
use fs2::FileExt;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::{io::AsyncReadExt, process::Command};

pub const TOOLCHAIN: &str = include_str!("../toolchain.json");
const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

// Tile archives are built from project data, so the cache is private: its
// directories 0700 and its files 0600. Owner rule: `ds_layer_store::private`,
// applied inline because this crate has no DS dependency. Unix only; a mode
// change to an existing entry is best effort (a volume without unix modes
// refuses chmod, and that never fails a build).
fn private_dir_all(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        let _ = set_private(path, 0o700);
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}

fn private_options() -> OpenOptions {
    #[allow(unused_mut)]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn private_create(path: &Path) -> std::io::Result<File> {
    let file = private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    Ok(file)
}

/// `mode` on one entry this crate made, never through a link.
fn set_private(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.file_type().is_symlink() && metadata.permissions().mode() & 0o777 != mode {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);
impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn check(&self) -> Result<()> {
        if self.0.load(Ordering::SeqCst) {
            bail!("tile_cancelled");
        }
        Ok(())
    }
    async fn wait(&self) {
        loop {
            if self.0.load(Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

/// The host must supply its authenticated cache namespace and captured project.
/// Neither field is interpreted as a filesystem path.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub authority: String,
    pub project: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub scope: Scope,
    pub layers: BTreeMap<String, PathBuf>,
    pub options: TippecanoeOptions,
    #[serde(default)]
    pub force: bool,
}

/// Portable prepared source envelope. Source acquisition remains a host
/// responsibility; all execution and cache semantics belong to this crate.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub schema: String,
    pub project: String,
    pub layers: BTreeMap<String, String>,
    pub options: TippecanoeOptions,
    #[serde(default)]
    pub force: bool,
}
impl Prepared {
    pub fn decode(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_INPUT_BYTES as usize {
            bail!("prepared tile input exceeds byte limit");
        }
        let request: Self = serde_json::from_slice(input).context("invalid prepared tile input")?;
        if request.schema != "ds.tiles.prepared/v1" {
            bail!("unsupported prepared tile schema");
        }
        validate_request(&Request {
            scope: Scope {
                authority: "prepared-input".into(),
                project: request.project.clone(),
            },
            layers: request
                .layers
                .keys()
                .map(|name| (name.clone(), PathBuf::new()))
                .collect(),
            options: request.options.clone(),
            force: request.force,
        })?;
        for text in request.layers.values() {
            for record in text.as_bytes().split(|c| *c == b'\n') {
                if record.len() > MAX_RECORD_BYTES {
                    bail!("tile feature exceeds record limit");
                }
                validate_record(record)?;
            }
        }
        Ok(request)
    }
}

#[derive(Clone)]
pub struct Tools {
    tippecanoe: PathBuf,
    pmtiles: PathBuf,
    identity: String,
}
impl Tools {
    /// Tool installation belongs to the host administrator, not to DS. A
    /// configured shared directory wins, then standard system executables.
    /// Bundled tools are a convenience when no shared toolchain is installed.
    /// Only host configuration can choose tools; request bodies cannot.
    pub async fn resolve(resource_root: &Path, cancel: &Cancel) -> Result<Self> {
        if let Some(directory) = std::env::var_os("DS_TILING_TOOLS_DIR") {
            let directory = PathBuf::from(directory);
            if !directory.is_absolute() {
                bail!("DS_TILING_TOOLS_DIR must be absolute");
            }
            return Self::from_directory(&directory, cancel).await;
        }
        let candidates = |name: &str| {
            std::env::var_os("PATH")
                .map(|path| {
                    let mut seen = BTreeSet::new();
                    std::env::split_paths(&path)
                        .filter(|directory| directory.is_absolute())
                        .map(|directory| directory.join(name))
                        .filter(|candidate| candidate.is_file())
                        .filter(|candidate| seen.insert(candidate.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let tippecanoe = candidates("tippecanoe");
        let pmtiles = candidates("pmtiles");
        if !tippecanoe.is_empty() && !pmtiles.is_empty() {
            return Self::from_candidates(&tippecanoe, &pmtiles, cancel).await;
        }
        Self::bundled(resource_root, cancel).await
    }
    /// Explicit convenience bundle; shared installations use resolve/from_directory.
    pub async fn bundled(resource_root: &Path, cancel: &Cancel) -> Result<Self> {
        Self::from_directory(&resource_root.join("tiling-tools"), cancel).await
    }
    pub async fn from_directory(directory: &Path, cancel: &Cancel) -> Result<Self> {
        Self::from_paths(
            &directory.join("tippecanoe"),
            &directory.join("pmtiles"),
            cancel,
        )
        .await
    }
    async fn from_candidates(
        tippecanoe: &[PathBuf],
        pmtiles: &[PathBuf],
        cancel: &Cancel,
    ) -> Result<Self> {
        let mut last_error = None;
        for tip in tippecanoe {
            for pm in pmtiles {
                match Self::from_paths(tip, pm, cancel).await {
                    Ok(tools) => return Ok(tools),
                    Err(error) => last_error = Some(error),
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            anyhow::anyhow!("no compatible shared tiling toolchain was found on PATH")
        }))
    }
    async fn from_paths(tippecanoe: &Path, pmtiles: &Path, cancel: &Cancel) -> Result<Self> {
        let tippecanoe = tippecanoe.canonicalize().context("Tippecanoe is missing; install the supported system toolchain or set DS_TILING_TOOLS_DIR")?;
        let pmtiles = pmtiles.canonicalize().context(
            "PMTiles is missing; install the supported system toolchain or set DS_TILING_TOOLS_DIR",
        )?;
        let pins: Value = serde_json::from_str(TOOLCHAIN)?;
        let tip = run_child(Command::new(&tippecanoe).arg("--version"), cancel).await?;
        let version = format!(
            "{}{}",
            String::from_utf8_lossy(&tip.stdout),
            String::from_utf8_lossy(&tip.stderr)
        );
        if !tip.status.success()
            || version.trim()
                != format!(
                    "tippecanoe v{}",
                    pins["tippecanoe"]["version"].as_str().unwrap()
                )
        {
            bail!("Tippecanoe version is unsupported; this runtime requires 2.82.0 (configure DS_TILING_TOOLS_DIR for a compatible shared installation)");
        }
        let pm = run_child(Command::new(&pmtiles).arg("version"), cancel).await?;
        if !pm.status.success()
            || !String::from_utf8_lossy(&pm.stdout).starts_with(&format!(
                "pmtiles {},",
                pins["pmtiles"]["version"].as_str().unwrap()
            ))
        {
            bail!("PMTiles version is unsupported; this runtime requires 1.20.0 (configure DS_TILING_TOOLS_DIR for a compatible shared installation)");
        }
        let identity = format!("{}:{}", file_digest(&tippecanoe)?, file_digest(&pmtiles)?);
        Ok(Self {
            tippecanoe,
            pmtiles,
            identity,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub schema: String,
    pub cache_key: String,
    pub sha256: String,
    pub bytes: u64,
    pub features: u64,
    pub zoom_tile_counts: Map<String, Value>,
}
/// A local result. Hosts separately upload and verify the online publication.
pub struct Artifact {
    _cache_lease: File,
    pub path: PathBuf,
    pub receipt: Receipt,
    pub cache_hit: bool,
}

pub struct Runtime {
    root: PathBuf,
    tools: Tools,
}
impl Runtime {
    pub fn new(root: PathBuf, tools: Tools) -> Result<Self> {
        if !root.is_absolute() {
            bail!("tile cache root must be absolute");
        }
        private_dir_all(&root)?;
        Ok(Self { root, tools })
    }
    /// Snapshot first, hash what will actually execute, then serialize identical
    /// builds across processes. Interrupted work never becomes a cache receipt.
    pub async fn build(&self, request: &Request, cancel: &Cancel) -> Result<Option<Artifact>> {
        validate_request(request)?;
        cancel.check()?;
        let work = TempDir::new_in(&self.root)?;
        let _ = set_private(work.path(), 0o700);
        let (layers, digests, features) = snapshot(request, work.path(), cancel)?;
        let key = digest(&serde_json::to_vec(&json!({
            "schema": "ds.tiles.cache/v1", "scope": request.scope,
            "layers": digests, "options": request.options, "tools": self.tools.identity,
            "toolchain": TOOLCHAIN, "runtime": env!("CARGO_PKG_VERSION"),
            "implementation": digest(concat!(include_str!("lib.rs"), include_str!("archive.rs")).as_bytes())
        }))?);
        let entry = self.root.join(&key);
        private_dir_all(&entry)?;
        let lock = private_options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(entry.join("build.lock"))?;
        loop {
            cancel.check()?;
            match lock.try_lock_exclusive() {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(25)).await
                }
                Err(error) => return Err(error.into()),
            }
        }
        let path = entry.join("tiles.pmtiles");
        let receipt_path = entry.join("receipt.json");
        if !request.force {
            if let Ok(receipt) = read_receipt(&receipt_path, &path, &key) {
                if archive::verify_pmtiles_with(&self.tools.pmtiles, &path, cancel)
                    .await
                    .is_ok()
                {
                    cancel.check()?;
                    let _ = File::open(&receipt_path)
                        .and_then(|f| f.set_modified(std::time::SystemTime::now()));
                    return Ok(Some(Artifact {
                        _cache_lease: lock,
                        path,
                        receipt,
                        cache_hit: true,
                    }));
                }
            }
        }
        cancel.check()?;
        if features == 0 {
            return Ok(None);
        }
        let mbtiles = work.path().join("tiles.mbtiles");
        archive::run_tippecanoe_with(
            &self.tools.tippecanoe,
            &layers,
            &mbtiles,
            &request.options,
            cancel,
        )
        .await?;
        let count = archive::mbtiles_tile_count(&mbtiles)
            .context("tiling produced an unreadable MBTiles archive")?;
        // Nonempty input becoming empty is a failure, never authority to delete
        // an existing published map.
        if count == 0 {
            bail!("nonempty tile input produced no renderable tiles");
        }
        let zoom_tile_counts = archive::inspect_mbtiles_zoom_tile_counts(&mbtiles);
        let archive_path = work.path().join("tiles.pmtiles");
        archive::convert_to_pmtiles_with(&self.tools.pmtiles, &mbtiles, &archive_path, cancel)
            .await?;
        cancel.check()?;
        let receipt = Receipt {
            schema: "ds.tiles.artifact/v1".into(),
            cache_key: key,
            sha256: file_digest(&archive_path)?,
            bytes: archive_path.metadata()?.len(),
            features,
            zoom_tile_counts,
        };
        // Both renames are inside one filesystem and under the key lock. A
        // crash between them is rejected by digest validation on the next read.
        let staged_receipt = work.path().join("receipt.json");
        let mut file = private_create(&staged_receipt)?;
        file.write_all(&serde_json::to_vec(&receipt)?)?;
        file.sync_all()?;
        File::open(&archive_path)?.sync_all()?;
        // The external converter wrote the archive with its own mode.
        let _ = set_private(&archive_path, 0o600);
        std::fs::rename(&archive_path, &path)?;
        std::fs::rename(&staged_receipt, &receipt_path)?;
        self.trim_cache(2 * 1024 * 1024 * 1024)?;
        Ok(Some(Artifact {
            _cache_lease: lock,
            path,
            receipt,
            cache_hit: false,
        }))
    }
    /// Bound archive storage. A returned artifact holds its key lock through
    /// publication/readback, so pruning can never unlink an in-use archive.
    /// Lock files stay in place to keep waiters on the same inode.
    fn trim_cache(&self, budget: u64) -> Result<()> {
        let mut entries = Vec::new();
        let mut total = 0u64;
        for item in std::fs::read_dir(&self.root)?.flatten() {
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.len() != 64 || !name.bytes().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let directory = item.path();
            let Ok(meta) = directory.join("tiles.pmtiles").metadata() else {
                continue;
            };
            let age = directory
                .join("receipt.json")
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            total = total.saturating_add(meta.len());
            entries.push((age, meta.len(), directory));
        }
        entries.sort_by_key(|entry| entry.0);
        for (_, bytes, directory) in entries {
            if total <= budget {
                break;
            }
            let Ok(lock) = OpenOptions::new()
                .read(true)
                .write(true)
                .open(directory.join("build.lock"))
            else {
                continue;
            };
            if lock.try_lock_exclusive().is_err() {
                continue;
            }
            let _ = std::fs::remove_file(directory.join("receipt.json"));
            if std::fs::remove_file(directory.join("tiles.pmtiles")).is_ok() {
                total = total.saturating_sub(bytes);
            }
        }
        Ok(())
    }
}

fn validate_request(request: &Request) -> Result<()> {
    for value in [&request.scope.project, &request.scope.authority] {
        if value.trim() != value
            || value.is_empty()
            || value.len() > 2048
            || value.chars().any(char::is_control)
        {
            bail!("tile scope must be explicit and bounded");
        }
    }
    let o = &request.options;
    if o.min_zoom > o.max_zoom
        || o.max_zoom > 24
        || o.base_zoom > 24
        || !(1..=24).contains(&o.full_detail)
    {
        bail!("invalid tile zoom/detail policy");
    }
    if request.layers.len() > 1000 {
        bail!("too many tile layers");
    }
    for name in request.layers.keys() {
        if name.is_empty()
            || name.len() > 200
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            bail!("invalid tile layer name");
        }
    }
    Ok(())
}

fn snapshot(
    request: &Request,
    root: &Path,
    cancel: &Cancel,
) -> Result<(BTreeMap<String, PathBuf>, BTreeMap<String, String>, u64)> {
    let mut layers = BTreeMap::new();
    let mut digests = BTreeMap::new();
    let mut total = 0;
    let mut features = 0;
    for (name, source) in &request.layers {
        cancel.check()?;
        let mut reader = File::open(source)?;
        if !reader.metadata()?.is_file() {
            bail!("tile source must be a regular file");
        }
        let path = root.join(format!("{name}.geojsonl"));
        let mut writer = private_create(&path)?;
        let mut hash = Sha256::new();
        let mut record = Vec::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            cancel.check()?;
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > MAX_INPUT_BYTES {
                bail!("tile input exceeds byte limit");
            }
            hash.update(&buf[..n]);
            writer.write_all(&buf[..n])?;
            for byte in &buf[..n] {
                if *byte == b'\n' {
                    features += validate_record(&record)?;
                    record.clear();
                } else {
                    record.push(*byte);
                }
                if record.len() > MAX_RECORD_BYTES {
                    bail!("tile feature exceeds record limit");
                }
            }
        }
        features += validate_record(&record)?;
        layers.insert(name.clone(), path);
        digests.insert(name.clone(), format!("{:x}", hash.finalize()));
    }
    Ok((layers, digests, features))
}
fn validate_record(record: &[u8]) -> Result<u64> {
    let record = record.strip_prefix(&[0x1e]).unwrap_or(record);
    if record.iter().all(u8::is_ascii_whitespace) {
        return Ok(0);
    }
    let value: Value = serde_json::from_slice(record).context("invalid tile GeoJSON sequence")?;
    if value["type"] != "Feature" || !value["geometry"].is_object() {
        bail!("tile input record must be a Feature with geometry");
    }
    Ok(1)
}
fn read_receipt(receipt: &Path, archive: &Path, key: &str) -> Result<Receipt> {
    let file = File::open(receipt)?;
    if file.metadata()?.len() > 64 * 1024 {
        bail!("oversized tile receipt");
    }
    let value: Receipt = serde_json::from_reader(file)?;
    if value.schema != "ds.tiles.artifact/v1"
        || value.cache_key != key
        || value.bytes == 0
        || archive.metadata()?.len() != value.bytes
        || file_digest(archive)? != value.sha256
    {
        bail!("tile cache verification failed");
    }
    Ok(value)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn file_digest(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
async fn capture(mut reader: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        let keep = n.min(16384usize.saturating_sub(kept.len()));
        kept.extend_from_slice(&buffer[..keep]);
    }
    Ok(kept)
}
async fn run_child(command: &mut Command, cancel: &Cancel) -> Result<std::process::Output> {
    cancel.check()?;
    let mut child = command
        .env("GOMAXPROCS", "2")
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start the configured tile tool")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let execution = async {
        let (status, stdout, stderr) = tokio::try_join!(
            async { Ok::<_, anyhow::Error>(child.wait().await?) },
            capture(stdout),
            capture(stderr)
        )?;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    };
    tokio::select! {
        result = execution => result,
        _ = cancel.wait() => { bail!("tile_cancelled"); },
        _ = tokio::time::sleep(Duration::from_secs(20 * 60)) => { bail!("tile tool timed out"); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(path: PathBuf) -> Request {
        Request {
            scope: Scope {
                authority: "canary/test-owner".into(),
                project: "project-a".into(),
            },
            layers: BTreeMap::from([("poles".into(), path)]),
            options: TippecanoeOptions::project_default(),
            force: false,
        }
    }
    fn feature(lon: f64) -> String {
        format!(
            r#"{{"type":"Feature","id":"pole-01","properties":{{"pole_id":"P01","cable":"3x50"}},"geometry":{{"type":"Point","coordinates":[{lon},-2.4]}}}}"#
        )
    }
    #[test]
    fn snapshots_do_not_follow_a_changed_original_and_keep_engineering_properties() {
        let work = TempDir::new().unwrap();
        let original = work.path().join("input.geojsonl");
        std::fs::write(&original, feature(29.8)).unwrap();
        let staged = work.path().join("staged");
        std::fs::create_dir(&staged).unwrap();
        let (layers, _, count) =
            snapshot(&request(original.clone()), &staged, &Cancel::default()).unwrap();
        std::fs::write(original, feature(30.0)).unwrap();
        assert_eq!(count, 1);
        let value: Value =
            serde_json::from_slice(&std::fs::read(&layers["poles"]).unwrap()).unwrap();
        assert_eq!(value["geometry"]["coordinates"][0], 29.8);
        assert_eq!(value["properties"]["cable"], "3x50");
        assert_eq!(value["id"], "pole-01");
    }
    #[cfg(unix)]
    #[test]
    fn the_tile_cache_and_its_snapshots_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let work = TempDir::new().unwrap();
        let root = work.path().join("state").join("tile-cache");
        let tools = Tools {
            tippecanoe: "tippecanoe".into(),
            pmtiles: "pmtiles".into(),
            identity: String::new(),
        };
        Runtime::new(root.clone(), tools).unwrap();
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(root.parent().unwrap()), 0o700);
        // A cache from before the rule is tightened when it is opened again.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o775)).unwrap();
        let tools = Tools {
            tippecanoe: "tippecanoe".into(),
            pmtiles: "pmtiles".into(),
            identity: String::new(),
        };
        Runtime::new(root.clone(), tools).unwrap();
        assert_eq!(mode(&root), 0o700);

        let original = work.path().join("input.geojsonl");
        std::fs::write(&original, feature(29.8)).unwrap();
        let (layers, _, _) = snapshot(&request(original), &root, &Cancel::default()).unwrap();
        assert_eq!(mode(&layers["poles"]), 0o600);
    }
    #[test]
    fn corrupt_sources_never_become_an_authoritative_empty_result() {
        assert!(validate_record(b"not json").is_err());
        assert!(validate_record(br#"{"type":"Feature","geometry":null}"#).is_err());
        assert_eq!(validate_record(b" \t\r").unwrap(), 0);
        let mut bad = request("input".into());
        bad.layers = BTreeMap::from([("poles:/tmp/other".into(), "input".into())]);
        assert!(validate_request(&bad).is_err());
        bad = request("input".into());
        bad.scope.project.clear();
        assert!(validate_request(&bad).is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn path_search_does_not_let_an_old_tool_hide_a_compatible_shared_installation() {
        use std::os::unix::fs::PermissionsExt;

        let work = TempDir::new().unwrap();
        let old = work.path().join("old-tippecanoe");
        let current = work.path().join("current-tippecanoe");
        let pmtiles = work.path().join("pmtiles");
        for (path, output) in [
            (&old, "tippecanoe v2.80.0"),
            (&current, "tippecanoe v2.82.0"),
            (&pmtiles, "pmtiles 1.20.0, fixture"),
        ] {
            std::fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' '{output}'\n")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let tools = Tools::from_candidates(
            &[old, current.clone()],
            std::slice::from_ref(&pmtiles),
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert_eq!(tools.tippecanoe, current.canonicalize().unwrap());
        assert_eq!(tools.pmtiles, pmtiles.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_terminates_the_tool_and_never_waits_for_its_deadline() {
        let cancel = Cancel::default();
        let trigger = cancel.clone();
        let mut command = Command::new("sleep");
        let execution = run_child(command.arg("30"), &cancel);
        let cancellation = async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            trigger.cancel();
        };
        let (result, _) = tokio::join!(execution, cancellation);
        assert!(result.unwrap_err().to_string().contains("tile_cancelled"));
    }
    /// Run explicitly against the packaged, pinned tools. Exercises actual
    /// Tippecanoe + PMTiles, persistence across hosts, invalidation and repair.
    #[tokio::test]
    #[ignore = "requires DS_TILING_TOOLS_DIR containing the pinned release binaries"]
    async fn real_tools_cache_and_isolation() {
        let tools_dir =
            PathBuf::from(std::env::var_os("DS_TILING_TOOLS_DIR").expect("DS_TILING_TOOLS_DIR"));
        let cancel = Cancel::default();
        let tools = Tools::resolve(Path::new("/no-bundled-ds-product"), &cancel)
            .await
            .unwrap();
        assert_eq!(
            tools.tippecanoe,
            tools_dir.join("tippecanoe").canonicalize().unwrap()
        );
        let work = TempDir::new().unwrap();
        let input = work.path().join("input.geojsonl");
        std::fs::write(&input, feature(29.8)).unwrap();
        let cache = work.path().join("cache");
        let runtime = Runtime::new(cache.clone(), tools.clone()).unwrap();
        let mut req = request(input.clone());
        req.options.max_zoom = 8;
        req.options.base_zoom = 8;
        let first = runtime.build(&req, &cancel).await.unwrap().unwrap();
        assert!(!first.cache_hit);
        assert_eq!(first.receipt.features, 1);
        let first_receipt = first.receipt.clone();
        let first_path = first.path.clone();
        drop(first);
        let restarted = Runtime::new(cache, tools).unwrap();
        let hit = restarted.build(&req, &cancel).await.unwrap().unwrap();
        assert!(hit.cache_hit);
        assert_eq!(hit.receipt.sha256, first_receipt.sha256);
        drop(hit);
        req.scope.project = "project-b".into();
        let other = runtime.build(&req, &cancel).await.unwrap().unwrap();
        assert!(!other.cache_hit);
        assert_ne!(other.receipt.cache_key, first_receipt.cache_key);
        drop(other);
        req.scope.project = "project-a".into();
        std::fs::write(&first_path, b"corrupt archive").unwrap();
        let repaired = runtime.build(&req, &cancel).await.unwrap().unwrap();
        assert!(!repaired.cache_hit);
        assert_eq!(repaired.receipt.sha256, first_receipt.sha256);
        drop(repaired);
        std::fs::write(input, feature(30.0)).unwrap();
        let changed = runtime.build(&req, &cancel).await.unwrap().unwrap();
        assert!(!changed.cache_hit);
        assert_ne!(changed.receipt.cache_key, first_receipt.cache_key);
        req.options.min_zoom = 5;
        let styled = runtime.build(&req, &cancel).await.unwrap().unwrap();
        assert_ne!(styled.receipt.cache_key, changed.receipt.cache_key);
        let styled_receipt = styled.receipt.clone();
        drop(styled);
        req.force = true;
        let forced = runtime.build(&req, &cancel).await.unwrap().unwrap();
        assert!(!forced.cache_hit);
        assert_eq!(forced.receipt.sha256, styled_receipt.sha256);
    }
}
