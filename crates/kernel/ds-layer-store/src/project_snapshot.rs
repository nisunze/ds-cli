//! Verified immutable columnar snapshots. This store has no network transport:
//! local reads and work-area changes cannot acquire data or check remote freshness.
mod delta_acquisition;
mod local_view;
use crate::{private, snapshot_privacy};
pub use delta_acquisition::{DeltaAcquisition, DeltaAcquisitionState};
use ds_columnar::digest_file;
use ds_command_kernel::project_snapshot::{
    self as policy, Context, Manifest, Refusal, RefusalCode, SourceKind,
};
use ds_network_contracts::columnar_io::GeoparquetSummaryV1;
use fs2::FileExt;
pub use local_view::{LocalReadReceipt, LocalView, ViewPredicate, WorkArea, WorkAreaUpdate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Refusal>;
const MAX_CHUNK_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_SNAPSHOT_ROWS: u64 = 5_000_000;
const BATCH_ROWS: usize = 128;
const MAX_GEOMETRY_NODES: usize = 100_000;
/// How a chunk is named to the engine in a refusal; never a host path.
const CHUNK_NAME: &str = "chunk.parquet";

fn refusal(code: RefusalCode, reason: impl std::fmt::Display) -> Refusal {
    Refusal {
        code,
        reason: reason.to_string(),
    }
}
fn storage(error: impl std::fmt::Display) -> Refusal {
    refusal(RefusalCode::SnapshotCorrupt, error)
}
fn canonical_hash(domain: &[u8], value: &impl Serialize) -> Result<String> {
    let mut value = serde_json::to_value(value).map_err(storage)?;
    value.sort_all_objects();
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(serde_json::to_vec(&value).map_err(storage)?);
    Ok(format!("{:x}", digest.finalize()))
}

pub struct Store {
    root: PathBuf,
    context: Context,
    source: SourceKind,
}

#[derive(Debug, Serialize)]
pub struct Admission {
    pub manifest_sha256: String,
    pub changed: bool,
    pub imported_chunks: u64,
    pub reused_chunks: u64,
    pub imported_bytes: u64,
    pub verified_reused_bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Retention {
    pub removed_manifests: u64,
    pub removed_chunk_bytes: u64,
    pub pinned_manifests: u64,
}

/// Pins an immutable manifest across refresh. The shared pin excludes retention
/// while permitting another job to promote a different head.
pub struct Pin {
    manifest: Manifest,
    root: PathBuf,
    _lock: File,
}

impl Store {
    pub fn open(base: &Path, context: Context, source: SourceKind) -> Result<Self> {
        context.validate()?;
        if !base.is_absolute() {
            return Err(storage("snapshot base must be absolute"));
        }
        let parent = base.join("project-snapshots-v1");
        snapshot_privacy::directory(&parent).map_err(storage)?;
        let root = parent.join(canonical_hash(
            b"ds.snapshot-room/v1\0",
            &(&context, source),
        )?);
        snapshot_privacy::directory(&root).map_err(storage)?;
        for name in ["chunks", "pins"] {
            snapshot_privacy::directory(&root.join(name)).map_err(storage)?;
        }
        let store = Self {
            root,
            context,
            source,
        };
        let _lock = store.lock(true)?;
        let connection = store.connection()?;
        let claimed = serde_json::to_string(&(&store.context, store.source)).map_err(storage)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO authority(id,context) VALUES(1,?1)",
                [&claimed],
            )
            .map_err(storage)?;
        let saved: String = connection
            .query_row("SELECT context FROM authority WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(storage)?;
        if saved != claimed {
            return Err(refusal(
                RefusalCode::SnapshotContextMismatch,
                "snapshot room authority differs",
            ));
        }
        Ok(store)
    }

    fn lock(&self, exclusive: bool) -> Result<File> {
        let path = self.root.join("catalog.lock");
        guard_file_if_present(&path)?;
        let lock = private::open_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(storage)?;
        snapshot_privacy::file(&path).map_err(storage)?;
        if exclusive {
            lock.lock_exclusive()
        } else {
            FileExt::lock_shared(&lock)
        }
        .map_err(storage)?;
        Ok(lock)
    }

    fn connection(&self) -> Result<Connection> {
        let path = self.root.join("catalog.sqlite");
        guard_file_if_present(&path)?;
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            guard_sqlite_sidecar_if_present(Path::new(&name))?;
        }
        private::prepare_file(&path).map_err(storage)?;
        snapshot_privacy::file(&path).map_err(storage)?;
        let connection =
            Connection::open(crate::sqlite_path(&path).map_err(storage)?).map_err(storage)?;
        persist_sqlite_sidecars(&connection)?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(storage)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA journal_size_limit=4194304;
            CREATE TABLE IF NOT EXISTS authority(id INTEGER PRIMARY KEY CHECK(id=1),context TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS chunks(sha TEXT PRIMARY KEY,layer TEXT NOT NULL,schema_sha TEXT NOT NULL,bytes INTEGER NOT NULL,rows INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS objects(chunk TEXT NOT NULL,id TEXT NOT NULL,revision TEXT NOT NULL,row_sha TEXT NOT NULL,ordinal INTEGER NOT NULL,PRIMARY KEY(chunk,id));
            CREATE INDEX IF NOT EXISTS objects_by_id ON objects(id,chunk);
            CREATE TABLE IF NOT EXISTS manifests(sha TEXT PRIMARY KEY,body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS members(manifest TEXT NOT NULL,layer TEXT NOT NULL,chunk TEXT NOT NULL,PRIMARY KEY(manifest,layer,chunk));
            CREATE TABLE IF NOT EXISTS head(id INTEGER PRIMARY KEY CHECK(id=1),sha TEXT NOT NULL);").map_err(storage)?;
        delta_acquisition::schema(&connection)?;
        local_view::schema(&connection, &self.namespace_sha256()?)?;
        Ok(connection)
    }

    fn head_digest(connection: &Connection) -> Result<Option<String>> {
        connection
            .query_row("SELECT sha FROM head WHERE id=1", [], |row| row.get(0))
            .optional()
            .map_err(storage)
    }

    pub fn head(&self) -> Result<Option<Manifest>> {
        let _lock = self.lock(false)?;
        let connection = self.connection()?;
        Self::head_digest(&connection)?
            .map(|digest| self.manifest(&connection, &digest))
            .transpose()
    }

    fn manifest(&self, connection: &Connection, digest: &str) -> Result<Manifest> {
        if !policy::sha256(digest) {
            return Err(storage("invalid manifest identity"));
        }
        let body: String = connection
            .query_row("SELECT body FROM manifests WHERE sha=?1", [digest], |row| {
                row.get(0)
            })
            .map_err(storage)?;
        let manifest: Manifest = serde_json::from_str(&body).map_err(storage)?;
        manifest.validate(&self.context)?;
        manifest.verify_sha256()?;
        if manifest.source_kind != self.source || manifest.manifest_sha256 != digest {
            return Err(storage("manifest namespace differs"));
        }
        Ok(manifest)
    }

    pub fn pin(&self, digest: &str) -> Result<Pin> {
        let _catalog = self.lock(false)?;
        let manifest = self.manifest(&self.connection()?, digest)?;
        let path = self.root.join("pins").join(digest);
        guard_file_if_present(&path)?;
        let lock = private::open_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(storage)?;
        snapshot_privacy::file(&path).map_err(storage)?;
        FileExt::lock_shared(&lock).map_err(storage)?;
        Ok(Pin {
            manifest,
            root: self.root.clone(),
            _lock: lock,
        })
    }

    /// Inputs are already acquired local files keyed by declared chunk SHA.
    /// All validation and bytes land before the one transactional head CAS.
    /// Cancellation or an error leaves the old head/cursor untouched.
    pub fn admit(
        &self,
        manifest: &Manifest,
        inputs: &BTreeMap<String, PathBuf>,
        expected_head: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Admission> {
        self.admit_checked(manifest, inputs, expected_head, cancelled, None)
    }

    /// Ordered source deltas replace chunks against an exact immutable base.
    /// All removed object IDs require final tombstones before the head changes.
    pub fn apply_delta(
        &self,
        pages: &[policy::DeltaPage],
        inputs: &BTreeMap<String, PathBuf>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Admission> {
        let started = Instant::now();
        let mut receipt = self.apply_delta_checked(pages, inputs, cancelled)?;
        receipt.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        Ok(receipt)
    }

    fn apply_delta_checked(
        &self,
        pages: &[policy::DeltaPage],
        inputs: &BTreeMap<String, PathBuf>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Admission> {
        check_cancelled(cancelled)?;
        let completed = delta_acquisition::Completed::from_pages(self, pages)?;
        if let Some(receipt) = self.replay_delta(&completed)? {
            return Ok(receipt);
        }
        let first = &pages[0];
        let base = match self.pin(&first.base_manifest_sha256) {
            Ok(base) => base,
            Err(error) => {
                // A concurrent exact batch may have committed and its base
                // been retained away between early replay and pin acquisition.
                if let Some(receipt) = self.replay_delta(&completed)? {
                    return Ok(receipt);
                }
                return Err(error);
            }
        };
        let plan = policy::assemble_delta(base.manifest(), pages)?;
        self.admit_checked(
            plan.manifest,
            inputs,
            Some(&first.base_manifest_sha256),
            cancelled,
            Some((&plan.tombstones, &completed)),
        )
    }

    fn admit_checked(
        &self,
        manifest: &Manifest,
        inputs: &BTreeMap<String, PathBuf>,
        expected_head: Option<&str>,
        cancelled: &dyn Fn() -> bool,
        delta: Option<(&[policy::Tombstone], &delta_acquisition::Completed)>,
    ) -> Result<Admission> {
        let tombstones = delta.map(|(tombstones, _)| tombstones);
        let completed = delta.map(|(_, completed)| completed);
        let started = Instant::now();
        manifest.validate(&self.context)?;
        manifest.verify_sha256()?;
        if manifest.source_kind != self.source {
            return Err(refusal(
                RefusalCode::SnapshotContextMismatch,
                "snapshot source namespace differs",
            ));
        }
        let rows = manifest
            .layers
            .iter()
            .try_fold(0u64, |sum, layer| sum.checked_add(layer.row_count))
            .ok_or_else(|| storage("snapshot row count overflow"))?;
        let bytes = manifest
            .layers
            .iter()
            .try_fold(0u64, |sum, layer| sum.checked_add(layer.byte_count))
            .ok_or_else(|| storage("snapshot byte count overflow"))?;
        if rows > MAX_SNAPSHOT_ROWS
            || bytes > MAX_SNAPSHOT_BYTES
            || manifest
                .layers
                .iter()
                .flat_map(|layer| &layer.chunks)
                .any(|chunk| chunk.bytes > MAX_CHUNK_BYTES)
        {
            return Err(refusal(
                RefusalCode::SnapshotBudgetExceeded,
                "snapshot exceeds local admission budget",
            ));
        }
        let _lock = self.lock(true)?;
        let mut connection = self.connection()?;
        let head = Self::head_digest(&connection)?;
        if let Some(completed) = completed {
            if let Some(receipt) = delta_acquisition::replay(&connection, completed)? {
                return Ok(receipt);
            }
            delta_acquisition::check_pending(&connection, completed)?;
        }
        let mut receipt = Admission {
            manifest_sha256: manifest.manifest_sha256.clone(),
            changed: false,
            imported_chunks: 0,
            reused_chunks: 0,
            imported_bytes: 0,
            verified_reused_bytes: 0,
            elapsed_ms: 0,
        };
        if head.as_deref() == Some(&manifest.manifest_sha256) {
            if completed.is_some() {
                return Err(refusal(
                    RefusalCode::SnapshotStaleDelta,
                    "target head exists without this exact completed delta receipt",
                ));
            }
            receipt.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            return Ok(receipt);
        }
        if head.as_deref() != expected_head {
            return Err(refusal(
                RefusalCode::SnapshotStaleDelta,
                "snapshot head changed before admission",
            ));
        }
        let mut decoder = Decoder::default();
        let transaction = connection.transaction().map_err(storage)?;
        transaction
            .execute(
                "DELETE FROM members WHERE manifest=?1",
                [&manifest.manifest_sha256],
            )
            .map_err(storage)?;
        for layer in &manifest.layers {
            for chunk in &layer.chunks {
                check_cancelled(cancelled)?;
                let existing: Option<(String, String, u64, u64)> = transaction
                    .query_row(
                        "SELECT layer,schema_sha,bytes,rows FROM chunks WHERE sha=?1",
                        [&chunk.sha256],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()
                    .map_err(storage)?;
                let path = self.root.join("chunks").join(&chunk.sha256);
                if let Some(metadata) = existing {
                    snapshot_privacy::file(&path).map_err(storage)?;
                    if metadata
                        != (
                            layer.name.clone(),
                            layer.schema_sha256.clone(),
                            chunk.bytes,
                            chunk.rows,
                        )
                        || fs::metadata(&path).map_err(storage)?.len() != chunk.bytes
                    {
                        return Err(storage("reusable chunk metadata differs"));
                    }
                    if digest_file(&path).map_err(storage)? != chunk.sha256 {
                        return Err(storage("reusable chunk checksum differs"));
                    }
                    receipt.reused_chunks += 1;
                    receipt.verified_reused_bytes += chunk.bytes;
                } else {
                    let source = inputs
                        .get(&chunk.sha256)
                        .or_else(|| path.exists().then_some(&path))
                        .ok_or_else(|| storage("required chunk was not acquired"))?;
                    let mut input = File::open(source).map_err(storage)?;
                    let mut staged = tempfile::Builder::new()
                        .prefix("snapshot-stage-")
                        .tempfile_in(self.root.join("chunks"))
                        .map_err(storage)?;
                    snapshot_privacy::file(staged.path()).map_err(storage)?;
                    let mut hash = Sha256::new();
                    let mut transferred = 0u64;
                    let mut buffer = [0u8; 65536];
                    loop {
                        check_cancelled(cancelled)?;
                        let read = input.read(&mut buffer).map_err(storage)?;
                        if read == 0 {
                            break;
                        }
                        transferred += read as u64;
                        if transferred > chunk.bytes {
                            return Err(storage("chunk exceeds declared size"));
                        }
                        hash.update(&buffer[..read]);
                        staged.write_all(&buffer[..read]).map_err(storage)?;
                    }
                    if transferred != chunk.bytes
                        || format!("{:x}", hash.finalize()) != chunk.sha256
                    {
                        return Err(storage("chunk bytes or SHA-256 differs"));
                    }
                    staged.flush().map_err(storage)?;
                    staged.as_file().sync_all().map_err(storage)?;
                    let held = fs::read(staged.path()).map_err(storage)?;
                    let summary = decoder.summary(&held)?;
                    if summary.feature_count as u64 != chunk.rows
                        || summary.skipped_coordinate_rows != 0
                        || summary.layer_name.as_deref() != Some(&layer.name)
                        || summary.schema_sha256 != layer.schema_sha256
                    {
                        return Err(storage("chunk columnar schema, layer or coverage differs"));
                    }
                    transaction
                        .execute(
                            "INSERT INTO chunks VALUES(?1,?2,?3,?4,?5)",
                            params![
                                chunk.sha256,
                                layer.name,
                                layer.schema_sha256,
                                chunk.bytes,
                                chunk.rows
                            ],
                        )
                        .map_err(storage)?;
                    let mut ordinal = 0u64;
                    decoder.features(&held, &mut |feature| {
                        check_cancelled(cancelled)?;
                        {
                            let id = object_value(&feature, &layer.stable_id_column)?;
                            let revision = object_value(&feature, &layer.revision_column)?;
                            ds_command_kernel::geometry_admission::admit_geometry(
                                &feature["geometry"],
                            )
                            .map_err(storage)?;
                            let kind = feature["geometry"]["type"]
                                .as_str()
                                .ok_or_else(|| storage("snapshot feature geometry is absent"))?;
                            let bounds =
                                ds_command_kernel::reference_cache::feature_bounds(&feature)
                                    .ok_or_else(|| {
                                        storage("snapshot geometry has no coordinates")
                                    })?;
                            if bounds[0] < -180.0
                                || bounds[2] > 180.0
                                || bounds[1] < -90.0
                                || bounds[3] > 90.0
                            {
                                return Err(storage(
                                    "snapshot geometry is outside its admitted CRS",
                                ));
                            }
                            let admitted = format!("{:?}", layer.geometry);
                            if admitted != "Mixed" && admitted != kind {
                                return Err(storage("snapshot geometry kind differs"));
                            }
                            let row_sha = canonical_hash(b"ds.snapshot-feature/v1\0", &feature)?;
                            transaction
                                .execute(
                                    "INSERT INTO objects VALUES(?1,?2,?3,?4,?5)",
                                    params![chunk.sha256, id, revision, row_sha, ordinal],
                                )
                                .map_err(storage)?;
                            ordinal += 1;
                        }
                        Ok(())
                    })?;
                    drop(held);
                    if ordinal != chunk.rows {
                        return Err(storage("decoded chunk row count differs"));
                    }
                    // An orphan from interrupted admission may exist. It is not
                    // a reusable indexed chunk until its exact bytes verify.
                    if path.exists() {
                        snapshot_privacy::file(&path).map_err(storage)?;
                        if digest_file(&path).map_err(storage)? != chunk.sha256 {
                            return Err(storage("orphan chunk is corrupt"));
                        }
                    } else {
                        staged.persist_noclobber(&path).map_err(storage)?;
                    }
                    sync_directory(&self.root.join("chunks"))?;
                    receipt.imported_chunks += 1;
                    receipt.imported_bytes += transferred;
                }
                transaction
                    .execute(
                        "INSERT INTO members VALUES(?1,?2,?3)",
                        params![manifest.manifest_sha256, layer.name, chunk.sha256],
                    )
                    .map_err(storage)?;
            }
        }
        if let Some(tombstones) = tombstones {
            validate_deletions(
                &transaction,
                expected_head.ok_or_else(|| storage("delta base is absent"))?,
                &manifest.manifest_sha256,
                tombstones,
            )?;
        }
        if logical_digest(&transaction, &manifest.manifest_sha256)? != manifest.logical_data_sha256
        {
            return Err(storage("snapshot logical data digest differs"));
        }
        check_cancelled(cancelled)?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO manifests VALUES(?1,?2)",
                params![
                    manifest.manifest_sha256,
                    serde_json::to_string(manifest).map_err(storage)?
                ],
            )
            .map_err(storage)?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO head VALUES(1,?1)",
                [&manifest.manifest_sha256],
            )
            .map_err(storage)?;
        if let Some(completed) = completed {
            delta_acquisition::commit(&transaction, completed)?;
        }
        transaction.commit().map_err(storage)?;
        receipt.changed = true;
        receipt.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        Ok(receipt)
    }

    /// Explicit retention never evicts the current head, an admitted keep-set
    /// or an immutable snapshot another process has pinned for a running job.
    pub fn retain(&self, keep: &BTreeSet<String>) -> Result<Retention> {
        if keep.iter().any(|digest| !policy::sha256(digest)) {
            return Err(storage("invalid retention manifest identity"));
        }
        let _catalog = self.lock(true)?;
        let mut connection = self.connection()?;
        let head = Self::head_digest(&connection)?;
        let pending_bases = delta_acquisition::pending_bases(&connection)?;
        let candidates = connection
            .prepare("SELECT sha FROM manifests ORDER BY sha")
            .map_err(storage)?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)?;
        let transaction = connection.transaction().map_err(storage)?;
        let mut receipt = Retention::default();
        let mut pins = Vec::new();
        for digest in candidates {
            if head.as_deref() == Some(&digest) || keep.contains(&digest) {
                continue;
            }
            if pending_bases.contains(&digest) {
                receipt.pinned_manifests += 1;
                continue;
            }
            let path = self.root.join("pins").join(&digest);
            guard_file_if_present(&path)?;
            let pin = private::open_options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(storage)?;
            snapshot_privacy::file(&path).map_err(storage)?;
            match pin.try_lock_exclusive() {
                Ok(()) => pins.push(pin),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
                {
                    receipt.pinned_manifests += 1;
                    continue;
                }
                Err(error) => return Err(storage(error)),
            }
            transaction
                .execute("DELETE FROM members WHERE manifest=?1", [&digest])
                .map_err(storage)?;
            transaction
                .execute("DELETE FROM manifests WHERE sha=?1", [&digest])
                .map_err(storage)?;
            receipt.removed_manifests += 1;
        }
        transaction
            .execute(
                "DELETE FROM objects WHERE chunk NOT IN (SELECT chunk FROM members)",
                [],
            )
            .map_err(storage)?;
        transaction
            .execute(
                "DELETE FROM chunks WHERE sha NOT IN (SELECT chunk FROM members)",
                [],
            )
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;
        let held = connection
            .prepare("SELECT sha FROM chunks")
            .map_err(storage)?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?
            .collect::<std::result::Result<BTreeSet<_>, _>>()
            .map_err(storage)?;
        // Catalog exclusivity also excludes every active local staging writer.
        for entry in fs::read_dir(self.root.join("chunks")).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if (policy::sha256(&name) && !held.contains(&name))
                || name.starts_with("snapshot-stage-")
            {
                snapshot_privacy::file(&entry.path()).map_err(storage)?;
                receipt.removed_chunk_bytes += entry.metadata().map_err(storage)?.len();
                fs::remove_file(entry.path()).map_err(storage)?;
            }
        }
        sync_directory(&self.root.join("chunks"))?;
        Ok(receipt)
    }
}

fn check_cancelled(cancelled: &dyn Fn() -> bool) -> Result<()> {
    if cancelled() {
        Err(refusal(
            RefusalCode::SnapshotInvalid,
            "snapshot admission cancelled",
        ))
    } else {
        Ok(())
    }
}

fn guard_file_if_present(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => snapshot_privacy::file(path).map_err(storage),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage(error)),
    }
}

fn persist_sqlite_sidecars(connection: &Connection) -> Result<()> {
    // Bundled SQLite's Unix and Windows VFS implement this control. Set it
    // before any WAL/schema access, then verify it: the last Store connection
    // must retain WAL/SHM rather than racing another reader's privacy checks.
    for value in [1, -1] {
        let mut enabled = value;
        // SAFETY: the live Connection owns the handle for this synchronous
        // control call; SQLite reads/writes exactly one native C integer.
        let code = unsafe {
            rusqlite::ffi::sqlite3_file_control(
                connection.handle(),
                c"main".as_ptr(),
                rusqlite::ffi::SQLITE_FCNTL_PERSIST_WAL,
                (&mut enabled as *mut std::os::raw::c_int).cast(),
            )
        };
        if code != rusqlite::ffi::SQLITE_OK || enabled != 1 {
            return Err(storage(format!(
                "SQLite persistent WAL control failed: {code}/{enabled}"
            )));
        }
    }
    Ok(())
}

fn guard_sqlite_sidecar_if_present(path: &Path) -> Result<()> {
    guard_sqlite_sidecar_if_present_checked(path, snapshot_privacy::file)
}

fn guard_sqlite_sidecar_if_present_checked(
    path: &Path,
    mut verify: impl FnMut(&Path) -> std::result::Result<(), String>,
) -> Result<()> {
    let mut last_error = String::new();
    for _ in 0..16 {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                // An initially observed dangling link refuses even if it is
                // subsequently removed. Never follow it to test absence.
                if metadata.file_type().is_symlink() {
                    return Err(storage(
                        "snapshot privacy: symbolic link is not a private file",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(storage(error)),
        }
        match verify(path) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        // A concurrent SQLite close can unlink a sidecar before open or after
        // open (the verifier then sees nlink=0). A new sidecar may already have
        // replaced it. Only strict no-follow absence passes here; an existing
        // replacement gets the full privacy checks again, with bounded retries.
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(storage(last_error)),
            Ok(_) => {}
            Err(error) => return Err(storage(error)),
        }
    }
    Err(storage(last_error))
}

fn object_value<'a>(feature: &'a Value, column: &str) -> Result<&'a str> {
    let value = if column == ds_columnar::FEATURE_ID_COLUMN {
        &feature["id"]
    } else {
        &feature["properties"][column]
    };
    value
        .as_str()
        .filter(|value| policy::identifier(value))
        .ok_or_else(|| storage("snapshot stable ID or object revision is absent/invalid"))
}

fn logical_digest(connection: &Connection, manifest: &str) -> Result<String> {
    let mut statement = connection.prepare("SELECT m.layer,o.id,o.revision,o.row_sha FROM members m JOIN objects o ON o.chunk=m.chunk WHERE m.manifest=?1 ORDER BY m.layer COLLATE BINARY,o.id COLLATE BINARY").map_err(storage)?;
    let mut rows = statement.query([manifest]).map_err(storage)?;
    let mut hash = Sha256::new();
    hash.update(b"ds.snapshot-logical-rows/v1\0");
    let mut previous: Option<(String, String)> = None;
    while let Some(row) = rows.next().map_err(storage)? {
        let layer: String = row.get(0).map_err(storage)?;
        let id: String = row.get(1).map_err(storage)?;
        if previous.as_ref() == Some(&(layer.clone(), id.clone())) {
            return Err(storage("duplicate live object ID across chunks"));
        }
        previous = Some((layer.clone(), id.clone()));
        for value in [
            layer,
            id,
            row.get::<_, String>(2).map_err(storage)?,
            row.get::<_, String>(3).map_err(storage)?,
        ] {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn validate_deletions(
    connection: &Connection,
    base: &str,
    target: &str,
    tombstones: &[policy::Tombstone],
) -> Result<()> {
    let declared: BTreeSet<(&str, &str)> = tombstones
        .iter()
        .map(|t| (t.layer.as_str(), t.object_id.as_str()))
        .collect();
    for (layer, id) in &declared {
        let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM members m JOIN objects o ON o.chunk=m.chunk WHERE m.manifest=?1 AND m.layer=?2 AND o.id=?3)",params![target,layer,id],|row|row.get(0)).map_err(storage)?;
        if exists {
            return Err(storage("tombstoned object remains in terminal snapshot"));
        }
    }
    let mut statement = connection.prepare("SELECT m.layer,o.id FROM members m JOIN objects o ON o.chunk=m.chunk WHERE m.manifest=?1 AND NOT EXISTS(SELECT 1 FROM members n JOIN objects p ON p.chunk=n.chunk WHERE n.manifest=?2 AND n.layer=m.layer AND p.id=o.id)").map_err(storage)?;
    let mut removed = statement.query(params![base, target]).map_err(storage)?;
    while let Some(row) = removed.next().map_err(storage)? {
        let layer: String = row.get(0).map_err(storage)?;
        let id: String = row.get(1).map_err(storage)?;
        if !declared.contains(&(layer.as_str(), id.as_str())) {
            return Err(storage("removed live object has no delta tombstone"));
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(storage)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

impl Pin {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    /// Local exact masks are applied to a retained immutable snapshot. This
    /// bounded streaming door precedes analytical projection/pushdown controls.
    pub fn visit_layer(
        &self,
        name: &str,
        area: Option<&Value>,
        cancelled: &dyn Fn() -> bool,
        visitor: &mut dyn FnMut(Value) -> std::result::Result<(), String>,
    ) -> Result<u64> {
        let predicate = if let Some(area) = area {
            ds_command_kernel::reference_cache::validate_area(area).map_err(storage)?;
            Some(
                ds_command_kernel::reference_cache::PreparedArea::new(
                    area,
                    ds_command_kernel::reference_cache::AreaSelection::Intersects,
                )
                .map_err(storage)?,
            )
        } else {
            None
        };
        let layer = self
            .manifest
            .layers
            .iter()
            .find(|layer| layer.name == name)
            .ok_or_else(|| storage("snapshot layer is absent"))?;
        let mut count = 0;
        let mut decoder = Decoder::default();
        for chunk in &layer.chunks {
            check_cancelled(cancelled)?;
            let path = self.root.join("chunks").join(&chunk.sha256);
            snapshot_privacy::file(&path).map_err(storage)?;
            // The digest is of the very bytes the engine decodes.
            let held = fs::read(&path).map_err(storage)?;
            if format!("{:x}", Sha256::digest(&held)) != chunk.sha256 {
                return Err(storage("retained snapshot chunk is corrupt"));
            }
            decoder.features(&held, &mut |feature| {
                check_cancelled(cancelled)?;
                if let Some(predicate) = &predicate {
                    if !predicate.selects(&feature).map_err(storage)? {
                        return Ok(());
                    }
                }
                visitor(feature).map_err(storage)?;
                count += 1;
                Ok(())
            })?;
        }
        Ok(count)
    }
}

/// The Network engine decodes columnar chunks (`columnar_io_v1`); this store
/// verifies their bytes and admits what the engine returns. The engine is
/// captured once, on the first chunk that needs it. One call decodes one
/// bounded chunk; cancellation is checked between chunks and features.
#[derive(Default)]
struct Decoder(Option<ds_network_client::CapturedNetworkClient>);

/// A chunk the converter refuses is corrupt; an engine that is absent, too
/// old or failed is unavailable, with its own code in the reason.
fn engine_refusal(failure: ds_network_client::NetworkFailure) -> Refusal {
    if failure.is_columnar_refusal() {
        storage(failure.message)
    } else {
        refusal(
            RefusalCode::SnapshotEngineUnavailable,
            format!("{}: {}", failure.code(), failure.message),
        )
    }
}

impl Decoder {
    fn client(&mut self) -> Result<&ds_network_client::CapturedNetworkClient> {
        if self.0.is_none() {
            self.0 = Some(
                ds_network_client::CapturedNetworkClient::capture(1, &|| false)
                    .map_err(engine_refusal)?,
            );
        }
        self.0
            .as_ref()
            .ok_or_else(|| storage("the Network engine was not captured"))
    }

    fn summary(&mut self, held: &[u8]) -> Result<GeoparquetSummaryV1> {
        self.client()?
            .geoparquet_summary(CHUNK_NAME, held, &|| false)
            .map_err(engine_refusal)
    }

    /// Every feature of one chunk, in chunk order; the first refusal stops.
    fn features(&mut self, held: &[u8], visit: &mut dyn FnMut(Value) -> Result<()>) -> Result<()> {
        let mut stopped = Ok(());
        self.client()?
            .geoparquet_features(
                CHUNK_NAME,
                held,
                BATCH_ROWS,
                MAX_GEOMETRY_NODES,
                &|| false,
                &mut |feature| match visit(feature) {
                    Ok(()) => std::ops::ControlFlow::Continue(()),
                    Err(refusal) => {
                        stopped = Err(refusal);
                        std::ops::ControlFlow::Break(())
                    }
                },
            )
            .map_err(engine_refusal)?;
        stopped
    }
}

#[cfg(test)]
#[path = "project_snapshot_tests.rs"]
mod tests;
