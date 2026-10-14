//! Durable, native Design workspace. SQLite transactions own revisions and the
//! outbox. Network computation remains in ds-network; delivery in ds-report.
use ds_command_kernel::design::{self, Edit, Snapshot};
use fs2::FileExt;
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub mod grid_publication;
mod run;
mod repair;
pub use repair::RepairRequest;
pub use run::RunReceipt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Busy(String),
    #[error("{0}")]
    Io(String),
}
pub type Result<T> = std::result::Result<T, Error>;
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Io(e.to_string())
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Invalid(e.to_string())
    }
}
pub(crate) fn invalid(e: impl ToString) -> Error {
    Error::Invalid(e.to_string())
}
fn preserve_import_approved(retained: &Snapshot, replacement: &Snapshot) -> Result<()> {
    for (layer, collection) in &retained.layers {
        let existing = collection["features"]
            .as_array()
            .ok_or_else(|| invalid("features must be an array"))?;
        let replacement = replacement
            .layers
            .get(layer)
            .and_then(|v| v["features"].as_array())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        ds_network_contracts::import_admission::preserve_approved_rows(
            layer, existing, replacement,
        )
        .map_err(invalid)?;
    }
    Ok(())
}

// A workspace is the user's private offline Design data: its directory is
// 0700 and the files DS keeps in it 0600 (SQLite creates -journal/-wal with
// the database's mode). Owner rule: `ds_layer_store::private`, applied inline
// because this crate does not depend on it. Unix only.
pub(crate) fn private_options() -> OpenOptions {
    #[allow(unused_mut)]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

/// `mode` on one workspace entry, never through a link; absent is fine.
fn private(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if !metadata.file_type().is_symlink() && metadata.permissions().mode() & 0o777 != mode {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

pub struct Workspace {
    root: PathBuf,
    db: Connection,
}
impl Workspace {
    pub fn init(root: &Path, project: &str) -> Result<Value> {
        design::identifier(project).map_err(invalid)?;
        if root.symlink_metadata().is_ok() {
            return Err(Error::Conflict("workspace already exists".into()));
        }
        let parent = root
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let stage = tempfile::tempdir_in(parent)?;
        let _ = private(stage.path(), 0o700);
        let database = stage.path().join("design.sqlite");
        drop(
            private_options()
                .write(true)
                .create_new(true)
                .open(&database)?,
        );
        let db = Connection::open(&database)?;
        db.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE meta(project TEXT NOT NULL, schema_version INTEGER NOT NULL);
            CREATE TABLE revisions(revision TEXT PRIMARY KEY, transformer TEXT NOT NULL, document BLOB NOT NULL);
            CREATE TABLE heads(transformer TEXT PRIMARY KEY, revision TEXT NOT NULL REFERENCES revisions(revision));
            CREATE TABLE operations(id TEXT PRIMARY KEY, request_digest TEXT NOT NULL, receipt TEXT NOT NULL);
            CREATE TABLE outbox(sequence INTEGER PRIMARY KEY AUTOINCREMENT, operation_id TEXT NOT NULL UNIQUE,
                transformer TEXT NOT NULL, base_revision TEXT, revision TEXT NOT NULL, kind TEXT NOT NULL);
            CREATE TABLE runs(id TEXT PRIMARY KEY, selection TEXT NOT NULL, engine TEXT NOT NULL, cancel INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE jobs(run TEXT NOT NULL REFERENCES runs(id), ordinal INTEGER NOT NULL, transformer TEXT NOT NULL,
                revision TEXT NOT NULL REFERENCES revisions(revision), state TEXT NOT NULL, result BLOB, result_sha256 TEXT,
                PRIMARY KEY(run,transformer));
            CREATE TABLE deliveries(digest TEXT PRIMARY KEY, run TEXT NOT NULL, transformer TEXT NOT NULL, manifest BLOB NOT NULL);
            CREATE TABLE artifacts(digest TEXT PRIMARY KEY, bytes BLOB NOT NULL);")?;
        db.execute("INSERT INTO meta VALUES(?1,1)", [project])?;
        drop(db);
        File::open(stage.path().join("design.sqlite"))?.sync_all()?;
        // A fresh private directory is the atomic initialization unit.
        fs::rename(stage.path(), root)?;
        sync_dir(parent)?;
        Ok(
            json!({"workspace":fs::canonicalize(root)?,"project":project,"schema":"ds.design.workspace/v1","publication":"local_only"}),
        )
    }
    pub fn open(root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        let path = root.join("design.sqlite");
        regular(&path)?;
        // A workspace from before the privacy rule is tightened when opened;
        // best effort, so a volume without unix modes still opens.
        let _ = private(&root, 0o700);
        for name in [
            "design.sqlite",
            "design.sqlite-journal",
            "design.sqlite-wal",
            "design.sqlite-shm",
            "compute.lock",
            "worker.log",
        ] {
            let _ = private(&root.join(name), 0o600);
        }
        let db = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
        let version: i64 = db.query_row("SELECT schema_version FROM meta", [], |r| r.get(0))?;
        if version != 1 {
            return Err(invalid("unsupported workspace schema"));
        }
        Ok(Self { root, db })
    }
    pub fn status(&self) -> Result<Value> {
        let project: String = self
            .db
            .query_row("SELECT project FROM meta", [], |r| r.get(0))?;
        let heads: i64 = self
            .db
            .query_row("SELECT count(*) FROM heads", [], |r| r.get(0))?;
        let pending: i64 = self
            .db
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))?;
        let jobs: i64 =
            self.db
                .query_row("SELECT count(*) FROM jobs WHERE state='pending'", [], |r| {
                    r.get(0)
                })?;
        Ok(
            json!({"project":project,"transformers":heads,"pending_publications":pending,"pending_jobs":jobs,
            "publication":"pending_transport","workspace":self.root}),
        )
    }
    pub fn read(&self, transformer: &str, revision: Option<&str>) -> Result<(String, Snapshot)> {
        let revision = match revision {
            Some(r) => r.to_owned(),
            None => self
                .db
                .query_row(
                    "SELECT revision FROM heads WHERE transformer=?1",
                    [transformer],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| invalid("transformer does not exist"))?,
        };
        let bytes: Vec<u8> = self
            .db
            .query_row(
                "SELECT document FROM revisions WHERE revision=?1 AND transformer=?2",
                params![revision, transformer],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| invalid("revision does not exist for this transformer"))?;
        let snapshot = design::decode_retained(&bytes).map_err(invalid)?;
        if design::digest(&snapshot).map_err(invalid)? != revision {
            return Err(invalid("stored revision digest mismatch"));
        }
        Ok((revision, snapshot))
    }
    /// List retained content revisions, not server-assigned version ordinals.
    pub fn revisions(&self, transformer: &str, after: &str, limit: usize) -> Result<Value> {
        if limit == 0 || limit > 200 {
            return Err(invalid("revision limit must be between 1 and 200"));
        }
        let (head, _) = self.read(transformer, None)?;
        let mut query = self.db.prepare("SELECT revision FROM revisions WHERE transformer=?1 AND revision>?2 ORDER BY revision LIMIT ?3")?;
        let mut rows = query
            .query_map(params![transformer, after, limit + 1], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next = more.then(|| rows.last().cloned()).flatten();
        Ok(
            json!({"transformer":transformer,"head":head,"revisions":rows,"more":more,"next":next,
            "scope":"local_workspace","mutated":false}),
        )
    }

    /// Pin head once; immutable reads cannot be redirected by subsequent edits.
    pub fn compare(&self, transformer: &str, from: &str, to: &str, limit: usize) -> Result<Value> {
        let (left_revision, left) = self.read(transformer, Some(from))?;
        let (right_revision, right) = self.read(transformer, (to != "head").then_some(to))?;
        let mut result = design::compare_snapshots(&left, &right, limit).map_err(invalid)?;
        let project: String = self
            .db
            .query_row("SELECT project FROM meta", [], |r| r.get(0))?;
        result["project"] = json!(project);
        result["from_revision"] = json!(left_revision);
        result["to_revision"] = json!(right_revision);
        result["scope"] = json!("local_workspace");
        Ok(result)
    }

    pub fn write(
        &mut self,
        bytes: &[u8],
        expected: Option<&str>,
        operation: &str,
    ) -> Result<Value> {
        // Validate authored input before creation normalization can fill IDs or
        // rewrite owner references. Replacement admission is fenced in commit.
        let mut snapshot = design::decode_retained(bytes).map_err(invalid)?;
        for (layer, collection) in &snapshot.layers {
            ds_network_contracts::import_admission::validate_design_rows(
                layer,
                collection["features"]
                    .as_array()
                    .ok_or_else(|| invalid("features must be an array"))?,
            )
            .map_err(invalid)?;
        }
        if let Some(expected) = expected {
            // The immutable expected revision plus commit's head CAS keeps
            // this raw-input check valid even if another writer wins meanwhile.
            let (_, retained) = self.read(&snapshot.transformer, Some(expected))?;
            preserve_import_approved(&retained, &snapshot)?;
        }
        snapshot.normalize().map_err(invalid)?;
        run::compute_input(&snapshot)?;
        let request =
            design::digest(&json!({"kind":"write","snapshot":snapshot,"expected":expected}))
                .map_err(invalid)?;
        self.commit(snapshot, expected, operation, &request, "write")
    }
    pub fn edit(&mut self, bytes: &[u8]) -> Result<Value> {
        if bytes.len() > design::MAX_BYTES {
            return Err(invalid("edit exceeds 64 MiB"));
        }
        let edit: Edit = serde_json::from_slice(bytes)?;
        let request = design::digest(&edit).map_err(invalid)?;
        if let Some(receipt) = self.replay(&edit.operation_id, &request)? {
            return Ok(receipt);
        }
        let (_, snapshot) = self.read(&edit.transformer, Some(&edit.expected_revision))?;
        let snapshot = design::apply_edit(&snapshot, &edit).map_err(invalid)?;
        self.commit(
            snapshot,
            Some(&edit.expected_revision),
            &edit.operation_id,
            &request,
            "edit",
        )
    }
    pub fn restore(
        &mut self,
        transformer: &str,
        revision: &str,
        expected: &str,
        operation: &str,
    ) -> Result<Value> {
        let (_, snapshot) = self.read(transformer, Some(revision))?;
        let request = design::digest(&json!(["restore", transformer, revision, expected]))
            .map_err(invalid)?;
        self.commit(snapshot, Some(expected), operation, &request, "restore")
    }
    fn replay(&self, id: &str, request: &str) -> Result<Option<Value>> {
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT request_digest,receipt FROM operations WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((digest, receipt)) if digest == request => {
                Ok(Some(serde_json::from_str(&receipt)?))
            }
            Some(_) => Err(Error::Conflict(
                "operation id already names different input".into(),
            )),
        }
    }
    fn commit(
        &mut self,
        snapshot: Snapshot,
        expected: Option<&str>,
        operation: &str,
        request: &str,
        kind: &str,
    ) -> Result<Value> {
        design::identifier(operation).map_err(invalid)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<(String, String)> = tx
            .query_row(
                "SELECT request_digest,receipt FROM operations WHERE id=?1",
                [operation],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((digest, receipt)) = old {
            if digest != request {
                return Err(Error::Conflict(
                    "operation id already names different input".into(),
                ));
            }
            return Ok(serde_json::from_str(&receipt)?);
        }
        let current: Option<String> = tx
            .query_row(
                "SELECT revision FROM heads WHERE transformer=?1",
                [&snapshot.transformer],
                |r| r.get(0),
            )
            .optional()?;
        if current.as_deref() != expected {
            return Err(Error::Conflict(
                "revision conflict; read the current snapshot and reconcile edits".into(),
            ));
        }
        if kind == "write" {
            if let Some(current) = current.as_deref() {
                let bytes: Vec<u8> = tx.query_row(
                    "SELECT document FROM revisions WHERE revision=?1 AND transformer=?2",
                    params![current, snapshot.transformer],
                    |row| row.get(0),
                )?;
                let retained = design::decode_retained(&bytes).map_err(invalid)?;
                preserve_import_approved(&retained, &snapshot)?;
            }
        }
        let revision = design::digest(&snapshot).map_err(invalid)?;
        tx.execute(
            "INSERT OR IGNORE INTO revisions VALUES(?1,?2,?3)",
            params![
                revision,
                snapshot.transformer,
                serde_json::to_vec(&snapshot)?
            ],
        )?;
        tx.execute("INSERT INTO heads VALUES(?1,?2) ON CONFLICT(transformer) DO UPDATE SET revision=excluded.revision",params![snapshot.transformer,revision])?;
        tx.execute("INSERT INTO outbox(operation_id,transformer,base_revision,revision,kind) VALUES(?1,?2,?3,?4,?5)",params![operation,snapshot.transformer,expected,revision,kind])?;
        let receipt = json!({"transformer":snapshot.transformer,"revision":revision,"base_revision":expected,"operation_id":operation,
            "sequence":tx.last_insert_rowid(),"publication":"pending_transport"});
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3)",
            params![operation, request, receipt.to_string()],
        )?;
        tx.commit()?;
        Ok(receipt)
    }
    pub fn outbox(&self, after: i64, limit: usize) -> Result<Value> {
        if after < 0 || !(1..=100).contains(&limit) {
            return Err(invalid("after must be nonnegative and limit 1..100"));
        }
        let mut statement=self.db.prepare("SELECT sequence,operation_id,transformer,base_revision,revision,kind FROM outbox WHERE sequence>?1 ORDER BY sequence LIMIT ?2")?;
        let rows=statement.query_map(params![after,limit as i64+1],|r|Ok(json!({"sequence":r.get::<_,i64>(0)?,"operation_id":r.get::<_,String>(1)?,
            "transformer":r.get::<_,String>(2)?,"base_revision":r.get::<_,Option<String>>(3)?,"revision":r.get::<_,String>(4)?,"kind":r.get::<_,String>(5)?})))?;
        let mut rows = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        Ok(json!({"rows":rows,"more":more,"publication":"pending_transport"}))
    }
    pub fn export_snapshot(
        &self,
        transformer: &str,
        revision: Option<&str>,
        out: &Path,
    ) -> Result<Value> {
        let (revision, snapshot) = self.read(transformer, revision)?;
        let bytes = serde_json::to_vec(&snapshot)?;
        write_new(out, &bytes)?;
        Ok(json!({"transformer":transformer,"revision":revision,"out":out,"bytes":bytes.len()}))
    }
}

pub fn read_file(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    regular(path)?;
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid("input exceeds byte bound"));
    }
    Ok(bytes)
}
pub(crate) fn regular(path: &Path) -> Result<()> {
    let metadata = path.symlink_metadata()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("expected regular non-symlink file"));
    }
    Ok(())
}
pub(crate) fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path)
        .map_err(|e| Error::Io(e.to_string()))?;
    sync_dir(parent)
}
