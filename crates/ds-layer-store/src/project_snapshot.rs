//! Verified immutable columnar snapshots. This store has no network transport:
//! local reads and work-area changes cannot acquire data or check remote freshness.
use crate::{private, snapshot_privacy};
use ds_columnar::{batch_to_features, digest_file, schema_digest, ArtifactReader};
use ds_command_kernel::project_snapshot::{
    self as policy, Context, Manifest, Refusal, RefusalCode, SourceKind,
};
use fs2::FileExt;
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
            guard_file_if_present(Path::new(&name))?;
        }
        private::prepare_file(&path).map_err(storage)?;
        snapshot_privacy::file(&path).map_err(storage)?;
        let connection =
            Connection::open(crate::sqlite_path(&path).map_err(storage)?).map_err(storage)?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(storage)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS authority(id INTEGER PRIMARY KEY CHECK(id=1),context TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS chunks(sha TEXT PRIMARY KEY,layer TEXT NOT NULL,schema_sha TEXT NOT NULL,bytes INTEGER NOT NULL,rows INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS objects(chunk TEXT NOT NULL,id TEXT NOT NULL,revision TEXT NOT NULL,row_sha TEXT NOT NULL,ordinal INTEGER NOT NULL,PRIMARY KEY(chunk,id));
            CREATE INDEX IF NOT EXISTS objects_by_id ON objects(id,chunk);
            CREATE TABLE IF NOT EXISTS manifests(sha TEXT PRIMARY KEY,body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS members(manifest TEXT NOT NULL,layer TEXT NOT NULL,chunk TEXT NOT NULL,PRIMARY KEY(manifest,layer,chunk));
            CREATE TABLE IF NOT EXISTS head(id INTEGER PRIMARY KEY CHECK(id=1),sha TEXT NOT NULL);").map_err(storage)?;
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
        let first = pages
            .first()
            .ok_or_else(|| storage("delta pages are empty"))?;
        let base = self.pin(&first.base_manifest_sha256)?;
        let plan = policy::assemble_delta(base.manifest(), pages)?;
        self.admit_checked(
            plan.manifest,
            inputs,
            Some(&first.base_manifest_sha256),
            cancelled,
            Some(&plan.tombstones),
        )
    }

    fn admit_checked(
        &self,
        manifest: &Manifest,
        inputs: &BTreeMap<String, PathBuf>,
        expected_head: Option<&str>,
        cancelled: &dyn Fn() -> bool,
        tombstones: Option<&[policy::Tombstone]>,
    ) -> Result<Admission> {
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
        if let Some(area) = area {
            ds_command_kernel::reference_cache::validate_area(area).map_err(storage)?;
        }
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
                    if let Some(area) = area {
                        if !ds_command_kernel::reference_cache::intersects_area(&feature, area)
                            .map_err(storage)?
                        {
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
