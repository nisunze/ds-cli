//! Verified immutable columnar snapshots. This store has no network transport:
//! local reads and work-area changes cannot acquire data or check remote freshness.
mod delta_acquisition;
mod local_view;
use crate::{private, snapshot_privacy};
pub use delta_acquisition::{DeltaAcquisition, DeltaAcquisitionState};
use ds_columnar::{batch_to_features, digest_file, schema_digest, ArtifactReader};
use ds_command_kernel::project_snapshot::{
    self as policy, Context, Manifest, Refusal, RefusalCode, SourceKind,
};
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
                    let mut reader =
                        ArtifactReader::open(staged.path(), BATCH_ROWS).map_err(storage)?;
                    if reader.summary().feature_count as u64 != chunk.rows
                        || reader.summary().skipped_coordinate_rows != 0
                        || reader.summary().layer_name.as_deref() != Some(&layer.name)
                        || schema_digest(reader.schema()).map_err(storage)? != layer.schema_sha256
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
                    while let Some(batch) = reader.next_batch().map_err(storage)? {
                        check_cancelled(cancelled)?;
                        for feature in
                            batch_to_features(&batch, MAX_GEOMETRY_NODES).map_err(storage)?
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
                    }
                    if ordinal != chunk.rows {
                        return Err(storage("decoded chunk row count differs"));
                    }
                    drop(reader);
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
        for chunk in &layer.chunks {
            check_cancelled(cancelled)?;
            let path = self.root.join("chunks").join(&chunk.sha256);
            snapshot_privacy::file(&path).map_err(storage)?;
            if digest_file(&path).map_err(storage)? != chunk.sha256 {
                return Err(storage("retained snapshot chunk is corrupt"));
            }
            let mut reader = ArtifactReader::open(&path, BATCH_ROWS).map_err(storage)?;
            while let Some(batch) = reader.next_batch().map_err(storage)? {
                check_cancelled(cancelled)?;
                for feature in batch_to_features(&batch, MAX_GEOMETRY_NODES).map_err(storage)? {
                    if let Some(predicate) = &predicate {
                        if !predicate.selects(&feature).map_err(storage)? {
                            continue;
                        }
                    }
                    visitor(feature).map_err(storage)?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_columnar::{ConversionStream, Provenance};
    use serde_json::json;

    fn fixture() -> tempfile::TempDir {
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        builder
            .prefix("snapshot-store-")
            .tempdir_in(dirs::home_dir().unwrap())
            .unwrap()
    }
    fn point(id: &str, x: f64, revision: &str) -> Value {
        json!({"type":"Feature","id":id,"geometry":{"type":"Point","coordinates":[x,x]},"properties":{"revision":revision}})
    }
    fn context() -> Context {
        serde_json::from_value(json!({"principal":"reader","lane":"canary","audience":"native-dev","owner":{"kind":"project","project_id":"project-a"},"visibility":"visible-1"})).unwrap()
    }
    // Independent fixture construction from logical source features: the
    // admission path must derive the same digest from actual Parquet bytes.
    fn snapshot(
        root: &Path,
        parts: Vec<Vec<Value>>,
        revision: &str,
    ) -> (Manifest, BTreeMap<String, PathBuf>) {
        let mut chunks = Vec::new();
        let mut inputs = BTreeMap::new();
        let mut features: Vec<Value> = Vec::new();
        let mut schema_sha = "a".repeat(64);
        for part in parts {
            let path = root.join(format!("{}.parquet", uuid::Uuid::new_v4()));
            let mut writer = ConversionStream::create(&path, Provenance::default()).unwrap();
            writer.append(&part, &[]).unwrap();
            writer.finish("poles").unwrap();
            let reader = ArtifactReader::open(&path, 128).unwrap();
            schema_sha = schema_digest(reader.schema()).unwrap();
            let hash = digest_file(&path).unwrap();
            chunks.push(json!({"sha256":hash,"bytes":fs::metadata(&path).unwrap().len(),"rows":part.len(),"artifact":format!("chunk-{hash}")}));
            inputs.insert(hash, path);
            features.extend(part);
        }
        features.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        let mut digest = Sha256::new();
        digest.update(b"ds.snapshot-logical-rows/v1\0");
        for feature in &features {
            for value in [
                "poles".to_string(),
                feature["id"].as_str().unwrap().to_string(),
                feature["properties"]["revision"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                canonical_hash(b"ds.snapshot-feature/v1\0", feature).unwrap(),
            ] {
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            }
        }
        let source_revision = json!({"epoch":"epoch-1","vector":{"survey":revision},"coverage":"coverage-1","change_cursor":revision,"schema_sha256":schema_sha,"vocabulary_revision":"tags-1"});
        let byte_count: u64 = chunks
            .iter()
            .map(|chunk| chunk["bytes"].as_u64().unwrap())
            .sum();
        let mut manifest: Manifest = serde_json::from_value(json!({
            "schema":policy::SCHEMA,"context":context(),"source_kind":"survey","read_source":"bigquery_survey_mirror","selection":"project_complete","acquisition_filter":null,
            "revision":source_revision,
            "layers":[{"name":"poles","geometry":"point","crs":"OGC:CRS84","schema_sha256":schema_sha,"stable_id_column":ds_columnar::FEATURE_ID_COLUMN,"revision_column":"revision","row_count":features.len(),"byte_count":byte_count,"chunks":chunks}],
            "acquisition":{"attestation_id":format!("receipt-{revision}"),"revision":source_revision,"expected_layers":["poles"],"observed_layers":["poles"],"expected_pages":1,"completed_pages":1,"terminal":true,"whole_authorized_scope":true,"watermark_kind":"bigquery_mirror","watermark":revision,"mirror_behind":false,"deletion_history_complete":true},
            "saved_design":null,"original_asset":null,"logical_data_sha256":format!("{:x}",digest.finalize()),"manifest_sha256":"a".repeat(64),"created_at_ms":42
        })).unwrap();
        manifest.manifest_sha256 = manifest.computed_sha256().unwrap();
        (manifest, inputs)
    }

    #[test]
    fn retained_real_artifacts_reuse_unchanged_chunks_and_mask_without_acquisition() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (first, inputs) = snapshot(
            root,
            vec![vec![point("p1", 0.0, "r1")], vec![point("p2", 10.0, "r1")]],
            "head-1",
        );
        let receipt = store.admit(&first, &inputs, None, &|| false).unwrap();
        assert_eq!(receipt.imported_chunks, 2);
        let pin = store.pin(&first.manifest_sha256).unwrap();
        let (second, mut changed) = snapshot(
            root,
            vec![vec![point("p1", 4.0, "r2")], vec![point("p2", 10.0, "r1")]],
            "head-2",
        );
        assert_eq!(
            first.layers[0].chunks[1].sha256,
            second.layers[0].chunks[1].sha256
        );
        changed.remove(&second.layers[0].chunks[1].sha256);
        let receipt = store
            .admit(&second, &changed, Some(&first.manifest_sha256), &|| false)
            .unwrap();
        assert_eq!((receipt.imported_chunks, receipt.reused_chunks), (1, 1));
        let before = store.head().unwrap().unwrap();
        // No input remains available: local visits must consume retained bytes.
        for path in inputs.values().chain(changed.values()) {
            fs::remove_file(path).unwrap();
        }
        let mask = json!({"type":"Polygon","coordinates":[[[-1.0,-1.0],[1.0,-1.0],[1.0,1.0],[-1.0,1.0],[-1.0,-1.0]]]});
        let mut selected = Vec::new();
        assert_eq!(
            pin.visit_layer("poles", Some(&mask), &|| false, &mut |feature| {
                selected.push(feature);
                Ok(())
            })
            .unwrap(),
            1
        );
        assert_eq!(selected[0]["id"], "p1");
        let current = store.pin(&second.manifest_sha256).unwrap();
        assert_eq!(
            current
                .visit_layer("poles", Some(&mask), &|| false, &mut |_| Ok(()))
                .unwrap(),
            0
        );
        assert_eq!(store.head().unwrap().unwrap(), before);
        assert!(
            !store
                .admit(&second, &BTreeMap::new(), Some("stale"), &|| false)
                .unwrap()
                .changed
        );
        assert_eq!(
            pin.visit_layer("poles", None, &|| false, &mut |_| Ok(()))
                .unwrap(),
            2
        );
        let retained = store.retain(&BTreeSet::new()).unwrap();
        assert_eq!(
            (retained.removed_manifests, retained.pinned_manifests),
            (0, 1)
        );
        drop(pin);
        let retired = store.retain(&BTreeSet::new()).unwrap();
        assert_eq!(retired.removed_manifests, 1);
        assert!(retired.removed_chunk_bytes > 0);
        assert!(store.pin(&first.manifest_sha256).is_err());
        assert_eq!(
            current
                .visit_layer("poles", None, &|| false, &mut |_| Ok(()))
                .unwrap(),
            2
        );
    }

    fn delta(
        base: &Manifest,
        target: &Manifest,
        tombstones: Vec<policy::Tombstone>,
    ) -> Vec<policy::DeltaPage> {
        let old: BTreeSet<String> = base.layers[0]
            .chunks
            .iter()
            .map(|chunk| chunk.sha256.clone())
            .collect();
        let new: BTreeSet<String> = target.layers[0]
            .chunks
            .iter()
            .map(|chunk| chunk.sha256.clone())
            .collect();
        let first = policy::DeltaPage {
            context: base.context.clone(),
            batch_id: format!("batch-{}", target.revision.change_cursor),
            base_manifest_sha256: base.manifest_sha256.clone(),
            from: base.revision.clone(),
            target: target.revision.clone(),
            page: 0,
            next_page: Some(1),
            changed_chunks: BTreeMap::new(),
            retired_chunks: BTreeMap::from([(
                "poles".into(),
                old.difference(&new).cloned().collect(),
            )]),
            tombstones,
            terminal_manifest: None,
        };
        let last = policy::DeltaPage {
            page: 1,
            next_page: None,
            retired_chunks: BTreeMap::new(),
            tombstones: Vec::new(),
            changed_chunks: BTreeMap::from([(
                "poles".into(),
                target.layers[0]
                    .chunks
                    .iter()
                    .filter(|chunk| !old.contains(&chunk.sha256))
                    .cloned()
                    .collect(),
            )]),
            terminal_manifest: Some(target.clone()),
            ..first.clone()
        };
        vec![first, last]
    }

    fn local_area(limit: f64) -> Value {
        json!({"type":"Polygon","coordinates":[[[-limit,-limit],[limit,-limit],[limit,limit],[-limit,limit],[-limit,-limit]]]})
    }

    #[cfg(unix)]
    #[test]
    fn optional_sidecar_guard_handles_unlink_without_accepting_links_or_public_files() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let workspace = fixture();
        let path = workspace.path().join("catalog.sqlite-wal");
        let create = || {
            private::open_options()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
        };
        create();
        guard_sqlite_sidecar_if_present_checked(&path, |path| {
            fs::remove_file(path).unwrap();
            snapshot_privacy::file(path)
        })
        .unwrap();
        assert!(fs::symlink_metadata(&path).is_err());
        create();
        let mut calls = 0;
        guard_sqlite_sidecar_if_present_checked(&path, |path| {
            calls += 1;
            if calls == 1 {
                fs::remove_file(path).unwrap();
                private::open_options()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .unwrap();
                Err("snapshot privacy: prior opened sidecar was unlinked".into())
            } else {
                snapshot_privacy::file(path)
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        fs::remove_file(&path).unwrap();
        create();
        assert!(guard_sqlite_sidecar_if_present_checked(&path, |path| {
            fs::remove_file(path).unwrap();
            symlink("missing-target", path).unwrap();
            snapshot_privacy::file(path)
        })
        .is_err());
        // The initial dangling link must refuse even if a checker would remove
        // it. The verification callback must never be reached for that entry.
        assert!(
            guard_sqlite_sidecar_if_present_checked(&path, |_| panic!("link was admitted"))
                .is_err()
        );
        fs::remove_file(&path).unwrap();
        create();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(guard_sqlite_sidecar_if_present(&path).is_err());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::hard_link(&path, workspace.path().join("alias")).unwrap();
        assert!(guard_sqlite_sidecar_if_present(&path).is_err());
    }

    #[test]
    fn concurrent_sqlite_sidecar_lifecycles_keep_shared_snapshot_reads_valid() {
        use std::sync::{Arc, Barrier};
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
        store.admit(&base, &files, None, &|| false).unwrap();
        let barrier = Arc::new(Barrier::new(4));
        let mut threads = Vec::new();
        for _ in 0..4 {
            let root = root.to_owned();
            let barrier = barrier.clone();
            let expected = base.manifest_sha256.clone();
            threads.push(std::thread::spawn(move || {
                let store = Store::open(&root, context(), SourceKind::Survey).unwrap();
                barrier.wait();
                for _ in 0..64 {
                    assert_eq!(store.head().unwrap().unwrap().manifest_sha256, expected);
                    assert!(store.pending_delta("absent").unwrap().is_none());
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(store.head().unwrap().unwrap(), base);
    }
    #[cfg(unix)]
    #[test]
    fn sqlite_sidecars_persist_with_private_identity_after_last_connection_closes() {
        use std::os::unix::fs::MetadataExt;
        let workspace = fixture();
        let store = Store::open(workspace.path(), context(), SourceKind::Survey).unwrap();
        let paths = [
            store.root.join("catalog.sqlite-wal"),
            store.root.join("catalog.sqlite-shm"),
        ];
        let identities: Vec<_> = paths
            .iter()
            .map(|path| {
                snapshot_privacy::file(path).unwrap();
                let m = fs::symlink_metadata(path).unwrap();
                (m.dev(), m.ino())
            })
            .collect();
        for _ in 0..32 {
            assert!(store.head().unwrap().is_none());
            for (path, identity) in paths.iter().zip(&identities) {
                snapshot_privacy::file(path).unwrap();
                let m = fs::symlink_metadata(path).unwrap();
                assert_eq!((m.dev(), m.ino()), *identity);
            }
        }
        assert!(fs::metadata(&paths[0]).unwrap().len() <= 4 * 1024 * 1024);
    }

    fn read_local(view: &LocalView) -> (Vec<Value>, LocalReadReceipt) {
        let mut features = Vec::new();
        let receipt = view
            .visit_layer("poles", &|| false, &mut |f| {
                features.push(f);
                Ok(())
            })
            .unwrap();
        (features, receipt)
    }

    #[test]
    fn saved_local_masks_reopen_shrink_expand_clear_and_revisit_without_source_changes() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, files) = snapshot(
            root,
            vec![vec![point("p1", 1.0, "r1"), point("p2", 10.0, "r1")]],
            "base",
        );
        store.admit(&base, &files, None, &|| false).unwrap();
        let chunk = store
            .root
            .join("chunks")
            .join(&base.layers[0].chunks[0].sha256);
        let before = fs::read(&chunk).unwrap();
        let mut mask = store
            .set_work_area("desk", Some(&local_area(12.0)), None)
            .unwrap()
            .mask;
        assert_eq!(
            read_local(
                &store
                    .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
                    .unwrap()
            )
            .0
            .len(),
            2
        );
        drop(store);
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        assert_eq!(store.work_area("desk").unwrap().unwrap(), mask);
        for (area, count) in [
            (Some(local_area(2.0)), 1),
            (Some(local_area(12.0)), 2),
            (None, 2),
            (Some(local_area(2.0)), 1),
        ] {
            mask = store
                .set_work_area("desk", area.as_ref(), Some(&mask.revision))
                .unwrap()
                .mask;
            let view = store
                .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
                .unwrap();
            let (_, receipt) = read_local(&view);
            assert_eq!(receipt.rows, count);
            assert_eq!(receipt.work_area, mask);
            assert_eq!(receipt.source_revision, base.revision);
            assert_eq!(receipt.acquisition, base.acquisition);
            assert_eq!(receipt.selection, base.selection);
            assert_eq!(
                receipt.predicate,
                if area.is_some() {
                    ViewPredicate::Intersects
                } else {
                    ViewPredicate::AllRetainedRows
                }
            );
        }
        assert_eq!(store.head().unwrap().unwrap(), base);
        assert_eq!(fs::read(chunk).unwrap(), before);
        assert_eq!(
            base.layers[0].chunks[0].sha256,
            digest_file(files.values().next().unwrap()).unwrap()
        );
        assert!(store.remove_work_area("desk", &mask.revision).unwrap());
        assert!(store.work_area("desk").unwrap().is_none());
        assert!(store
            .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
            .is_err());
    }

    #[test]
    fn captured_local_view_survives_mask_changes_source_moves_deletions_and_retention() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, files) = snapshot(
            root,
            vec![vec![point("p1", 1.0, "r1"), point("p2", 10.0, "r1")]],
            "base",
        );
        store.admit(&base, &files, None, &|| false).unwrap();
        let first = store
            .set_work_area("desk", Some(&local_area(2.0)), None)
            .unwrap()
            .mask;
        let captured = store
            .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
            .unwrap();
        let expanded = store
            .set_work_area("desk", Some(&local_area(25.0)), Some(&first.revision))
            .unwrap()
            .mask;
        let (target, files) = snapshot(root, vec![vec![point("p1", 20.0, "r2")]], "target");
        let pages = delta(
            &base,
            &target,
            vec![policy::Tombstone {
                layer: "poles".into(),
                object_id: "p2".into(),
                object_revision: "r2".into(),
            }],
        );
        store.apply_delta(&pages, &files, &|| false).unwrap();
        assert_eq!(store.retain(&BTreeSet::new()).unwrap().pinned_manifests, 1);
        let (old, receipt) = read_local(&captured);
        assert_eq!(old, vec![point("p1", 1.0, "r1")]);
        assert_eq!(receipt.work_area, first);
        assert_eq!(receipt.manifest_sha256, base.manifest_sha256);
        let new = store
            .capture_view("desk", store.pin(&target.manifest_sha256).unwrap())
            .unwrap();
        let (moved, receipt) = read_local(&new);
        assert_eq!(moved, vec![point("p1", 20.0, "r2")]);
        assert_eq!(receipt.work_area, expanded);
        assert_eq!(receipt.source_revision, target.revision);
        let shrinking = store
            .set_work_area("desk", Some(&local_area(2.0)), Some(&expanded.revision))
            .unwrap()
            .mask;
        let empty = store
            .capture_view("desk", store.pin(&target.manifest_sha256).unwrap())
            .unwrap();
        assert_eq!(read_local(&empty).1.rows, 0);
        assert_eq!(read_local(&new).0, moved);
        assert_eq!(empty.work_area(), &shrinking);
        assert!(new.visit_layer("poles", &|| true, &mut |_| Ok(())).is_err());
        assert_eq!(store.head().unwrap().unwrap(), target);
    }

    #[test]
    fn local_view_receipt_preserves_filtered_scope_and_refuses_cross_namespace_pins() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (mut partial, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "partial");
        partial.selection = policy::SelectionScope::Filtered;
        partial.acquisition_filter = Some("opaque-filter".into());
        partial.acquisition.whole_authorized_scope = false;
        partial.acquisition.mirror_behind = true;
        partial.manifest_sha256 = partial.computed_sha256().unwrap();
        store.admit(&partial, &files, None, &|| false).unwrap();
        store
            .set_work_area("desk", Some(&local_area(0.5)), None)
            .unwrap();
        let (_, receipt) = read_local(
            &store
                .capture_view("desk", store.pin(&partial.manifest_sha256).unwrap())
                .unwrap(),
        );
        assert_eq!(receipt.rows, 0);
        assert_eq!(receipt.selection, policy::SelectionScope::Filtered);
        assert_eq!(receipt.acquisition_filter, partial.acquisition_filter);
        assert_eq!(receipt.acquisition, partial.acquisition);
        assert_eq!(receipt.source_revision.coverage, partial.revision.coverage);
        let mut variants = Vec::new();
        let mut other = context();
        other.principal = "other".into();
        variants.push((other, SourceKind::Survey));
        let mut other = context();
        other.lane = policy::Lane::Stable;
        variants.push((other, SourceKind::Survey));
        let mut other = context();
        other.owner = policy::Owner::Project {
            project_id: "other".into(),
        };
        variants.push((other, SourceKind::Survey));
        let mut other = context();
        other.visibility = "other".into();
        variants.push((other, SourceKind::Survey));
        variants.push((context(), SourceKind::SavedDesign));
        let mut other = context();
        other.audience = "other".into();
        variants.push((other, SourceKind::Survey));
        for (ctx, source) in variants {
            let other = Store::open(root, ctx, source).unwrap();
            assert!(other.work_area("desk").unwrap().is_none());
            assert_eq!(
                other
                    .capture_view("desk", store.pin(&partial.manifest_sha256).unwrap())
                    .err()
                    .unwrap()
                    .code,
                RefusalCode::SnapshotContextMismatch
            );
        }
    }

    #[test]
    fn local_mask_cas_rejects_aba_recreation_and_two_writer_races() {
        use std::sync::{Arc, Barrier};
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let a = store
            .set_work_area("desk", Some(&local_area(2.0)), None)
            .unwrap()
            .mask;
        assert!(
            !store
                .set_work_area("desk", Some(&local_area(2.0)), Some(&a.revision))
                .unwrap()
                .changed
        );
        assert_eq!(
            store
                .set_work_area("desk", Some(&local_area(2.0)), None)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotStaleView
        );
        let b = store
            .set_work_area("desk", None, Some(&a.revision))
            .unwrap()
            .mask;
        let again = store
            .set_work_area("desk", Some(&local_area(2.0)), Some(&b.revision))
            .unwrap()
            .mask;
        assert_eq!(a.content_sha256, again.content_sha256);
        assert_ne!(a.revision, again.revision);
        assert_eq!(
            store
                .set_work_area("desk", Some(&local_area(2.0)), Some(&a.revision))
                .unwrap_err()
                .code,
            RefusalCode::SnapshotStaleView
        );
        assert!(store.remove_work_area("desk", &a.revision).is_err());
        store.remove_work_area("desk", &again.revision).unwrap();
        drop(store);
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let recreated = store
            .set_work_area("desk", Some(&local_area(2.0)), None)
            .unwrap()
            .mask;
        assert_ne!(again.revision, recreated.revision);
        assert!(recreated.generation > again.generation);
        let barrier = Arc::new(Barrier::new(2));
        let mut threads = Vec::new();
        for limit in [3.0, 4.0] {
            let root = root.to_owned();
            let barrier = barrier.clone();
            let expected = recreated.revision.clone();
            threads.push(std::thread::spawn(move || {
                let store = Store::open(&root, context(), SourceKind::Survey).unwrap();
                barrier.wait();
                store.set_work_area("desk", Some(&local_area(limit)), Some(&expected))
            }));
        }
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r,Err(e) if e.code==RefusalCode::SnapshotStaleView))
                .count(),
            1
        );
    }

    #[test]
    fn local_mask_budgets_malformed_inputs_and_clock_failure_are_atomic() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let first = store.set_work_area("desk", None, None).unwrap().mask;
        for bad in [
            json!({"type":"LineString","coordinates":[[0,0],[1,1]]}),
            json!({"type":"Polygon","coordinates":[]}),
        ] {
            assert!(store
                .set_work_area("desk", Some(&bad), Some(&first.revision))
                .is_err());
        }
        assert!(store.set_work_area(&"x".repeat(257), None, None).is_err());
        let mut deep = Value::Null;
        for _ in 0..34 {
            deep = json!([deep]);
        }
        assert_eq!(
            store
                .set_work_area("deep", Some(&deep), None)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotBudgetExceeded
        );
        let too_large = json!({"type":"Feature","geometry":local_area(2.0),"properties":{"padding":"x".repeat(300_000)}});
        assert_eq!(
            store
                .set_work_area("large", Some(&too_large), None)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotBudgetExceeded
        );
        let large = json!({"type":"Feature","geometry":local_area(2.0),"properties":{"padding":"x".repeat(250_000)}});
        for i in 0..16 {
            store
                .set_work_area(&format!("mask-{i}"), Some(&large), None)
                .unwrap();
        }
        assert_eq!(
            store
                .set_work_area("overflow", Some(&large), None)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotBudgetExceeded
        );
        for i in 0..16 {
            let key = format!("mask-{i}");
            let mask = store.work_area(&key).unwrap().unwrap();
            store.remove_work_area(&key, &mask.revision).unwrap();
        }
        for i in 0..127 {
            store
                .set_work_area(&format!("clear-{i}"), None, None)
                .unwrap();
        }
        assert_eq!(
            store
                .set_work_area("overflow", None, None)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotBudgetExceeded
        );
        assert_eq!(store.work_area("desk").unwrap().unwrap(), first);
        let _lock = store.lock(true).unwrap();
        let c = store.connection().unwrap();
        c.execute(
            "UPDATE local_work_area_clock SET generation=?1,sha=?2 WHERE id=1",
            params![
                i64::MAX,
                local_view::clock_digest(&store.namespace_sha256().unwrap(), i64::MAX as u64)
                    .unwrap()
            ],
        )
        .unwrap();
        drop(c);
        drop(_lock);
        assert_eq!(
            store
                .set_work_area("desk", Some(&local_area(2.0)), Some(&first.revision))
                .unwrap_err()
                .code,
            RefusalCode::SnapshotBudgetExceeded
        );
        assert_eq!(store.work_area("desk").unwrap().unwrap(), first);
    }

    #[test]
    fn local_mask_corrupt_clock_refuses_without_repair_or_mask_mutation() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let first = store.set_work_area("desk", None, None).unwrap().mask;
        let _lock = store.lock(true).unwrap();
        let c = store.connection().unwrap();
        c.execute(
            "UPDATE local_work_area_clock SET generation=0 WHERE id=1",
            [],
        )
        .unwrap();
        drop(_lock);
        assert_eq!(
            store
                .set_work_area("desk", Some(&local_area(2.0)), Some(&first.revision))
                .unwrap_err()
                .code,
            RefusalCode::SnapshotCorrupt
        );
        assert_eq!(
            c.query_row(
                "SELECT revision FROM local_work_areas WHERE name='desk'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            first.revision
        );
        assert_eq!(
            c.query_row(
                "SELECT generation FROM local_work_area_clock WHERE id=1",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            0
        );
        c.execute("DELETE FROM local_work_area_clock WHERE id=1", [])
            .unwrap();
        assert!(Store::open(root, context(), SourceKind::Survey).is_err());
        assert_eq!(
            c.query_row("SELECT count(*) FROM local_work_area_clock", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn delta_metadata_reopens_revalidates_files_and_replays_after_base_retention() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
        store.admit(&base, &files, None, &|| false).unwrap();
        let (target, files) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
        let pages = delta(&base, &target, vec![]);
        let batch = &pages[0].batch_id;
        assert!(store.stage_delta_page(&pages[1]).is_err());
        let first = store.stage_delta_page(&pages[0]).unwrap();
        assert_eq!(first.state, DeltaAcquisitionState::Acquiring);
        assert_eq!(store.stage_delta_page(&pages[0]).unwrap(), first);
        let mut conflict = pages[0].clone();
        conflict.retired_chunks.clear();
        assert!(store.stage_delta_page(&conflict).is_err());
        assert_eq!(store.pending_delta(batch).unwrap().unwrap(), first);
        assert!(store.finish_delta(batch, &files, &|| false).is_err());
        drop(store);
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        assert_eq!(
            store.pending_delta_pages(batch).unwrap(),
            vec![pages[0].clone()]
        );
        let ready = store.stage_delta_page(&pages[1]).unwrap();
        assert_eq!(ready.state, DeltaAcquisitionState::Ready);
        assert!(store
            .finish_delta(batch, &BTreeMap::new(), &|| false)
            .is_err());
        assert!(store.finish_delta(batch, &files, &|| true).is_err());
        let cancellation_checks = std::cell::Cell::new(0usize);
        assert!(store
            .finish_delta(batch, &files, &|| {
                let n = cancellation_checks.get() + 1;
                cancellation_checks.set(n);
                n >= 5
            })
            .is_err());
        assert!(cancellation_checks.get() >= 5);
        assert_eq!(store.pending_delta(batch).unwrap().unwrap(), ready);
        assert_eq!(store.head().unwrap().unwrap(), base);
        assert!(
            store
                .finish_delta(batch, &files, &|| false)
                .unwrap()
                .changed
        );
        assert!(store.pending_delta(batch).unwrap().is_none());
        assert!(store.stage_delta_page(&pages[0]).is_err());
        store.retain(&BTreeSet::new()).unwrap();
        assert!(store.pin(&base.manifest_sha256).is_err());
        // A receipt is sufficient after the base is retired; no file reads.
        assert!(
            !store
                .apply_delta(&pages, &BTreeMap::new(), &|| false)
                .unwrap()
                .changed
        );
        let mut mismatch = pages.clone();
        mismatch[0].retired_chunks.clear();
        assert!(store.apply_delta(&mismatch, &files, &|| false).is_err());
        assert_eq!(store.head().unwrap().unwrap(), target);
        let (advanced, files) = snapshot(root, vec![vec![point("p1", 3.0, "r3")]], "advanced");
        store
            .admit(&advanced, &files, Some(&target.manifest_sha256), &|| false)
            .unwrap();
        assert!(store
            .apply_delta(&pages, &BTreeMap::new(), &|| false)
            .is_err());
        assert_eq!(store.head().unwrap().unwrap(), advanced);
    }

    #[test]
    fn pending_delta_limits_discard_and_retention_are_metadata_only() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
        store.admit(&base, &files, None, &|| false).unwrap();
        let (target, files) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
        let pages = delta(&base, &target, vec![]);
        for i in 0..8 {
            let mut page = pages[0].clone();
            page.batch_id = format!("pending-{i}");
            store.stage_delta_page(&page).unwrap();
        }
        let mut extra = pages[0].clone();
        extra.batch_id = "overflow".into();
        assert!(store.stage_delta_page(&extra).is_err());
        let mut oversized = pages[1].clone();
        oversized.batch_id = "pending-0".into();
        oversized.changed_chunks.get_mut("poles").unwrap()[0].artifact = "x".repeat(1024 * 1024);
        assert!(store.stage_delta_page(&oversized).is_err());
        let mut high_index = pages[0].clone();
        high_index.page = u64::MAX;
        assert!(store.stage_delta_page(&high_index).is_err());
        assert!(store.discard_delta("pending-7").unwrap());
        assert!(!store.discard_delta("pending-7").unwrap());
        store
            .admit(&target, &files, Some(&base.manifest_sha256), &|| false)
            .unwrap();
        let retained = store.retain(&BTreeSet::new()).unwrap();
        assert_eq!(retained.pinned_manifests, 1);
        assert!(store.pin(&base.manifest_sha256).is_ok());
        // An ordinary admission cannot create an unearned delta receipt.
        assert!(store.apply_delta(&pages, &files, &|| false).is_err());
        for i in 0..7 {
            assert!(store.discard_delta(&format!("pending-{i}")).unwrap());
        }
        store.retain(&BTreeSet::new()).unwrap();
        assert!(store.pin(&base.manifest_sha256).is_err());
        assert!(files.values().all(|path| path.exists()));
        assert_eq!(store.head().unwrap().unwrap(), target);
    }

    #[test]
    fn simultaneous_delta_callers_commit_one_receipt_without_rollback() {
        use std::sync::{Arc, Barrier};
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
        store.admit(&base, &files, None, &|| false).unwrap();
        let (target, files) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
        let pages = delta(&base, &target, vec![]);
        let barrier = Arc::new(Barrier::new(2));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let root = root.to_owned();
            let pages = pages.clone();
            let files = files.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                let store = Store::open(&root, context(), SourceKind::Survey).unwrap();
                barrier.wait();
                store
                    .apply_delta(&pages, &files, &|| false)
                    .unwrap()
                    .changed
            }));
        }
        let changed: usize = threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum();
        assert_eq!(changed, 1);
        assert_eq!(store.head().unwrap().unwrap(), target);
        let _lock = store.lock(false).unwrap();
        let c = store.connection().unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM delta_receipts", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM delta_receipt_pages", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn completed_delta_receipt_history_is_bounded() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (mut base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
        store.admit(&base, &files, None, &|| false).unwrap();
        let mut oldest = Vec::new();
        for i in 0..65 {
            // Only the acquisition revision changes; no repeated file import.
            let (target, files) = snapshot(
                root,
                vec![vec![point("p1", 1.0, "r1")]],
                &format!("head-{i}"),
            );
            let pages = delta(&base, &target, vec![]);
            if i == 0 {
                oldest = pages.clone();
            }
            assert!(
                store
                    .apply_delta(&pages, &files, &|| false)
                    .unwrap()
                    .changed
            );
            base = target;
        }
        assert!(store
            .apply_delta(&oldest, &BTreeMap::new(), &|| false)
            .is_err());
        let _lock = store.lock(false).unwrap();
        let c = store.connection().unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM delta_receipts", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            64
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM delta_receipt_pages", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            128
        );
    }

    #[test]
    fn staged_page_count_is_bounded_and_duplicate_tombstones_do_not_poison_cursor() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
        store.admit(&base, &files, None, &|| false).unwrap();
        let (target, _) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
        let mut page = delta(&base, &target, vec![])[0].clone();
        page.retired_chunks.clear();
        let tombstone = policy::Tombstone {
            layer: "poles".into(),
            object_id: "missing".into(),
            object_revision: "r2".into(),
        };
        page.tombstones = vec![tombstone.clone()];
        store.stage_delta_page(&page).unwrap();
        page.page = 1;
        page.next_page = Some(2);
        assert!(store.stage_delta_page(&page).is_err());
        assert_eq!(
            store
                .pending_delta(&page.batch_id)
                .unwrap()
                .unwrap()
                .next_page,
            Some(1)
        );
        page.tombstones.clear();
        for i in 1..255 {
            page.page = i;
            page.next_page = Some(i + 1);
            store.stage_delta_page(&page).unwrap();
        }
        let before = store.pending_delta(&page.batch_id).unwrap().unwrap();
        page.page = 255;
        page.next_page = Some(256);
        assert_eq!(
            store.stage_delta_page(&page).unwrap_err().code,
            RefusalCode::SnapshotBudgetExceeded
        );
        assert_eq!(
            store.pending_delta(&page.batch_id).unwrap().unwrap(),
            before
        );
        page.page = 256;
        page.next_page = Some(257);
        assert_eq!(
            store.stage_delta_page(&page).unwrap_err().code,
            RefusalCode::SnapshotBudgetExceeded
        );
        assert_eq!(
            store.pending_delta(&page.batch_id).unwrap().unwrap(),
            before
        );
        assert_eq!(store.head().unwrap().unwrap(), base);
    }

    #[test]
    fn ordered_delta_deletion_restore_and_move_match_complete_artifacts() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (first, inputs) = snapshot(
            root,
            vec![vec![point("p1", 1.0, "r1")], vec![point("p2", 10.0, "r1")]],
            "head-1",
        );
        store.admit(&first, &inputs, None, &|| false).unwrap();
        let (deleted, _) = snapshot(root, vec![vec![point("p2", 10.0, "r1")]], "head-2");
        let pages = delta(&first, &deleted, Vec::new());
        assert!(store
            .apply_delta(&pages, &BTreeMap::new(), &|| false)
            .unwrap_err()
            .reason
            .contains("no delta tombstone"));
        assert_eq!(store.head().unwrap().unwrap(), first);
        let pages = delta(
            &first,
            &deleted,
            vec![policy::Tombstone {
                layer: "poles".into(),
                object_id: "p1".into(),
                object_revision: "r2".into(),
            }],
        );
        let receipt = store
            .apply_delta(&pages, &BTreeMap::new(), &|| false)
            .unwrap();
        assert_eq!((receipt.imported_chunks, receipt.reused_chunks), (0, 1));
        assert!(
            !store
                .apply_delta(&pages, &BTreeMap::new(), &|| false)
                .unwrap()
                .changed
        );
        let mut base = deleted;
        for (head, x, revision) in [("head-3", 4.0, "r3"), ("head-4", 5.0, "r4")] {
            let expected = vec![point("p1", x, revision), point("p2", 10.0, "r1")];
            let (target, mut input) = snapshot(
                root,
                expected
                    .iter()
                    .cloned()
                    .map(|feature| vec![feature])
                    .collect(),
                head,
            );
            for chunk in &base.layers[0].chunks {
                input.remove(&chunk.sha256);
            }
            let receipt = store
                .apply_delta(&delta(&base, &target, Vec::new()), &input, &|| false)
                .unwrap();
            assert_eq!((receipt.imported_chunks, receipt.reused_chunks), (1, 1));
            let mut observed = Vec::new();
            store
                .pin(&target.manifest_sha256)
                .unwrap()
                .visit_layer("poles", None, &|| false, &mut |feature| {
                    observed.push(feature);
                    Ok(())
                })
                .unwrap();
            assert_eq!(observed, expected);
            assert_eq!(store.head().unwrap().unwrap(), target);
            base = target;
        }
    }

    #[test]
    fn malformed_delta_assembly_never_changes_a_native_head() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (base, input) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "head-1");
        store.admit(&base, &input, None, &|| false).unwrap();
        let (target, input) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "head-2");
        let valid = delta(&base, &target, Vec::new());
        for kind in 0..5 {
            let mut bad = valid.clone();
            match kind {
                0 => bad.reverse(),
                1 => {
                    bad[1].batch_id = "other-batch".into();
                }
                2 => {
                    bad[0].retired_chunks.clear();
                }
                3 => {
                    bad[1].changed_chunks.clear();
                }
                _ => {
                    bad[1].tombstones.push(policy::Tombstone {
                        layer: "poles".into(),
                        object_id: "p1".into(),
                        object_revision: "r3".into(),
                    });
                }
            }
            assert!(
                store.apply_delta(&bad, &input, &|| false).is_err(),
                "case {kind}"
            );
            assert_eq!(store.head().unwrap().unwrap(), base);
        }
        assert!(
            store
                .apply_delta(&valid, &input, &|| false)
                .unwrap()
                .changed
        );
    }

    #[test]
    fn cancellation_recovers_verified_orphans_without_a_source_refetch() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let workspace = fixture();
        let store = Store::open(workspace.path(), context(), SourceKind::Survey).unwrap();
        let (manifest, inputs) = snapshot(
            workspace.path(),
            vec![vec![point("p1", 1.0, "r1")]],
            "head-1",
        );
        let calls = AtomicUsize::new(0);
        assert!(store
            .admit(&manifest, &inputs, None, &|| calls
                .fetch_add(1, Ordering::Relaxed)
                >= 4)
            .is_err());
        assert!(store.head().unwrap().is_none());
        let chunk = &manifest.layers[0].chunks[0];
        assert!(store.root.join("chunks").join(&chunk.sha256).exists());
        for path in inputs.values() {
            fs::remove_file(path).unwrap();
        }
        assert!(
            store
                .admit(&manifest, &BTreeMap::new(), None, &|| false)
                .unwrap()
                .changed
        );
        assert_eq!(store.head().unwrap().unwrap(), manifest);
    }

    #[test]
    fn corruption_stale_heads_and_duplicate_objects_never_advance_the_cursor() {
        let workspace = fixture();
        let root = workspace.path();
        let store = Store::open(root, context(), SourceKind::Survey).unwrap();
        let (manifest, inputs) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "head-1");
        let path = inputs.values().next().unwrap();
        let bytes = fs::read(path).unwrap();
        fs::write(path, b"corrupt").unwrap();
        assert_eq!(
            store
                .admit(&manifest, &inputs, None, &|| false)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotCorrupt
        );
        assert!(store.head().unwrap().is_none());
        fs::write(path, bytes).unwrap();
        store.admit(&manifest, &inputs, None, &|| false).unwrap();
        let (duplicate, inputs) = snapshot(
            root,
            vec![vec![point("p1", 1.0, "r1")], vec![point("p1", 2.0, "r2")]],
            "head-2",
        );
        assert!(store
            .admit(
                &duplicate,
                &inputs,
                Some(&manifest.manifest_sha256),
                &|| false
            )
            .unwrap_err()
            .reason
            .contains("duplicate"));
        assert_eq!(store.head().unwrap().unwrap(), manifest);
        assert_eq!(
            store
                .admit(&duplicate, &inputs, Some("stale"), &|| false)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotStaleDelta
        );
        let mut other = context();
        other.visibility = "restricted".into();
        let other_store = Store::open(root, other, SourceKind::Survey).unwrap();
        assert!(other_store.head().unwrap().is_none());
        assert_eq!(
            other_store
                .admit(&manifest, &BTreeMap::new(), None, &|| false)
                .unwrap_err()
                .code,
            RefusalCode::SnapshotContextMismatch
        );
        let retained = store
            .root
            .join("chunks")
            .join(&manifest.layers[0].chunks[0].sha256);
        let mut corrupt = fs::read(&retained).unwrap();
        corrupt[0] ^= 1;
        fs::write(&retained, corrupt).unwrap();
        assert_eq!(
            store
                .pin(&manifest.manifest_sha256)
                .unwrap()
                .visit_layer("poles", None, &|| false, &mut |_| Ok(()))
                .unwrap_err()
                .code,
            RefusalCode::SnapshotCorrupt
        );
    }
}
