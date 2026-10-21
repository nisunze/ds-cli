//! Private retained project-head catalogue, shared by native Core and Desktop.
//! Storage is the Kernel-owned SQLite schema
//! (`ds_command_kernel::grid_project_catalog`): this adapter executes its
//! bounded SQL, one transaction per operation, and decides nothing. A head the
//! authorized listing names for the first time is persisted at once; offline
//! reads paint every held head; invalidation writes a marker only; the
//! consuming reader coalesces and may acquire missing/invalidated metadata.
//! The browser runs the same schema and SQL on OPFS.
use ds_command_kernel::{
    grid_project_catalog::{self as kernel, Changes, Held, Operation, Source, Step},
    printing::mv_models,
    printing::retained_reads::{Demand, Failure as ReadFailure, Host, Reads},
};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DATABASE: &str = "catalog.sqlite";
const USER_VERSION: i64 = 1;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub principal: String,
    pub project: String,
    pub lane: String,
    pub audience_sha256: String,
}
impl Scope {
    fn key(&self) -> Result<String, String> {
        if self.principal.is_empty()
            || self.principal.len() > 256
            || self.project.is_empty()
            || self.project.len() > 256
            || self.lane.is_empty()
            || !ds_command_kernel::printing::preview_progress::digest(&self.audience_sha256)
        {
            return Err("invalid catalogue scope".into());
        }
        Ok(hash(&serde_json::to_vec(self).map_err(|e| e.to_string())?))
    }
    fn kernel(&self) -> kernel::Scope {
        kernel::Scope {
            principal: self.principal.clone(),
            lane: self.lane.clone(),
            audience: self.audience_sha256.clone(),
            project: self.project.clone(),
        }
    }
}
#[derive(Debug)]
pub enum Failure<E> {
    Admission(&'static str),
    AcquisitionRequired,
    AccessRevoked,
    Storage(String),
    Remote(E),
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn catalogue_root(root: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err("catalogue root must be absolute".into());
    }
    Ok(root.join("grid-project-catalogue"))
}
/// The scope's private lock directory; it serialises one scope's acquisition
/// across processes and holds no catalogue data.
fn directory(root: &Path, scope: &Scope) -> Result<PathBuf, String> {
    Ok(catalogue_root(root)?.join(scope.key()?))
}
fn owned_directory(directory: &Path) -> Result<(), String> {
    let parent = directory
        .parent()
        .ok_or("catalogue has no private parent")?;
    for path in [parent, directory] {
        if fs::symlink_metadata(path).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
            return Err("catalogue directory is foreign or a link".into());
        }
        crate::private::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    let lock = directory.join("catalogue.lock");
    if fs::symlink_metadata(&lock).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err("catalogue lock is foreign or a link".into());
    }
    Ok(())
}
fn sql(error: rusqlite::Error) -> String {
    format!("project model catalogue store: {error}")
}
fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            crate::snapshot_privacy::file(path)?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
/// Open the one catalogue database. A read never creates it; an unsupported
/// schema refuses rather than being rewritten.
fn open(root: &Path, writable: bool) -> Result<Option<Connection>, String> {
    let path = catalogue_root(root)?.join(DATABASE);
    let present = exists(&path)?;
    if !present && !writable {
        return Ok(None);
    }
    if !present {
        let parent = path.parent().ok_or("catalogue has no private parent")?;
        if fs::symlink_metadata(parent).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
            return Err("catalogue directory is foreign or a link".into());
        }
        crate::private::create_dir_all(parent).map_err(|e| e.to_string())?;
        let file = crate::private::open_options()
            .write(true)
            .create_new(true)
            .open(&path)
            .or_else(|error| {
                // A concurrent first writer created it; admit it like any file.
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    crate::private::open_options().read(true).open(&path)
                } else {
                    Err(error)
                }
            })
            .map_err(|e| e.to_string())?;
        drop(file);
        crate::snapshot_privacy::file(&path)?;
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        exists(&PathBuf::from(format!("{}{suffix}", path.display())))?;
    }
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(crate::sqlite_path(&path)?, flags).map_err(sql)?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(sql)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql)?;
    if writable && version == 0 {
        connection
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
            .map_err(sql)?;
        run(&connection, Operation::Schema, true)?;
        connection
            .pragma_update(None, "user_version", USER_VERSION)
            .map_err(sql)?;
    } else if version == 0 {
        // Created by a writer that has not committed its schema yet.
        return Ok(None);
    } else if version != USER_VERSION {
        return Err("project model catalogue store has an unsupported schema".into());
    }
    Ok(Some(connection))
}
/// One Kernel operation in one transaction; the host only executes its SQL.
fn run(
    connection: &Connection,
    operation: Operation,
    write: bool,
) -> Result<(Option<Held>, Option<Changes>), String> {
    let transaction = rusqlite::Transaction::new_unchecked(
        connection,
        if write {
            TransactionBehavior::Immediate
        } else {
            TransactionBehavior::Deferred
        },
    )
    .map_err(sql)?;
    let mut step = kernel::start(operation)?;
    for _ in 0..16 {
        match step {
            Step::Done { held, changes } => {
                transaction.commit().map_err(sql)?;
                return Ok((held, changes));
            }
            Step::Sql {
                statement,
                continuation,
            } => {
                step = kernel::resume(
                    continuation,
                    ds_command_kernel::sql_effect::execute(&transaction, &statement)?,
                )?;
            }
        }
    }
    Err("project model catalogue exceeded its bounded continuation".into())
}
fn held(root: &Path, scope: &Scope) -> Result<Option<Held>, String> {
    match open(root, false)? {
        None => Ok(None),
        Some(connection) => Ok(run(
            &connection,
            Operation::Read {
                scope: scope.kernel(),
            },
            false,
        )?
        .0),
    }
}
fn write(root: &Path, operation: Operation) -> Result<(Option<Held>, Option<Changes>), String> {
    let connection = open(root, true)?.ok_or("project model catalogue store is unavailable")?;
    run(&connection, operation, true)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}
struct Store<'a, E, L, A> {
    root: PathBuf,
    directory: PathBuf,
    scope: &'a Scope,
    gate: Option<fs::File>,
    lock_error: Option<String>,
    load: Option<L>,
    unauthorized: A,
    /// The fence the next commit must find: the revision this read observed.
    expected: Option<u64>,
    held: Option<Held>,
    changes: Option<Changes>,
    _error: std::marker::PhantomData<E>,
}
impl<E, L, A> Store<'_, E, L, A> {
    fn ensure_lock(&mut self) -> Result<(), String> {
        if self.gate.is_none() {
            owned_directory(&self.directory)?;
            self.gate = Some(crate::exclusive_lock(
                &self.directory.join("catalogue.lock"),
            )?);
        }
        Ok(())
    }
    fn settle(&mut self, operation: Operation) -> Result<(), String> {
        let (held, changes) = write(&self.root, operation)?;
        self.expected = held.as_ref().map(|v| v.observation.revision);
        self.held = held;
        if changes.is_some() {
            self.changes = changes;
        }
        Ok(())
    }
}
impl<E, L: FnOnce() -> Result<Value, E>, A: Fn(&E) -> bool> Host<Value> for Store<'_, E, L, A> {
    type Error = Failure<E>;
    fn retained(&mut self, _key: &str) -> Option<(Value, bool)> {
        if let Err(e) = self.ensure_lock() {
            self.lock_error = Some(e);
            return None;
        }
        let held = match held(&self.root, self.scope) {
            Ok(held) => held,
            Err(e) => {
                self.lock_error = Some(e);
                return None;
            }
        };
        self.expected = held.as_ref().map(|v| v.observation.revision);
        self.held = held.clone();
        let held = held.filter(Held::servable)?;
        let listing = held.listing(&self.scope.project);
        mv_models::project(&self.scope.project, &self.scope.principal, &listing, None).ok()?;
        Some((listing, held.observation.needs_refresh))
    }
    fn invalidate(&mut self) -> Result<(), Self::Error> {
        self.ensure_lock().map_err(Failure::Storage)?;
        self.settle(Operation::Invalidate {
            scope: self.scope.kernel(),
        })
        .map_err(Failure::Storage)
    }
    fn acquire(&mut self, _key: &str) -> Result<Value, Self::Error> {
        if let Some(error) = self.lock_error.take() {
            return Err(Failure::Storage(error));
        }
        self.ensure_lock().map_err(Failure::Storage)?;
        match self.load.take().expect("one consuming acquisition")() {
            Ok(value) => Ok(value),
            Err(error) => {
                let operation = if (self.unauthorized)(&error) {
                    Operation::Revoke {
                        scope: self.scope.kernel(),
                    }
                } else {
                    Operation::MarkFailed {
                        scope: self.scope.kernel(),
                        expected: self.expected,
                    }
                };
                self.settle(operation).map_err(Failure::Storage)?;
                Err(Failure::Remote(error))
            }
        }
    }
    fn retain(&mut self, _key: &str, data: &Value) -> Result<(), Self::Error> {
        mv_models::project(&self.scope.project, &self.scope.principal, data, None)
            .map_err(|e| Failure::Storage(e.into()))?;
        self.settle(Operation::Commit {
            scope: self.scope.kernel(),
            listing: data.clone(),
            now: now(),
            expected: self.expected,
        })
        .map_err(Failure::Storage)
    }
}
static READS: OnceLock<Reads> = OnceLock::new();
fn coordinated<'a, E, L: FnOnce() -> Result<Value, E>, A: Fn(&E) -> bool>(
    root: &Path,
    scope: &'a Scope,
    demand: Demand,
    load: L,
    unauthorized: A,
) -> Result<(Value, Store<'a, E, L, A>), (Failure<E>, Option<Held>)> {
    let directory = directory(root, scope).map_err(|e| (Failure::Storage(e), None))?;
    let mut store = Store {
        root: root.to_path_buf(),
        directory,
        scope,
        gate: None,
        lock_error: None,
        load: Some(load),
        unauthorized,
        expected: None,
        held: None,
        changes: None,
        _error: std::marker::PhantomData,
    };
    let revoked = held(root, scope)
        .map_err(|e| (Failure::Storage(e), None))?
        .is_some_and(|held| held.observation.revoked);
    if revoked && !(demand.refresh && demand.online && demand.acquire) {
        return Err((Failure::AccessRevoked, None));
    }
    let room = hash(store.directory.as_os_str().as_encoded_bytes());
    match READS
        .get_or_init(Reads::default)
        .read(&room, "heads", demand, &mut store)
    {
        Ok(value) => Ok((value, store)),
        Err(e) => {
            let held = store.held.take();
            Err((
                match e {
                    ReadFailure::AcquisitionRequired => Failure::AcquisitionRequired,
                    ReadFailure::GateUnavailable => {
                        Failure::Storage("catalogue coordinator unavailable".into())
                    }
                    ReadFailure::Unavailable(e) => e,
                },
                held,
            ))
        }
    }
}
pub fn read<E>(
    root: &Path,
    scope: &Scope,
    demand: Demand,
    load: impl FnOnce() -> Result<Value, E>,
    unauthorized: impl Fn(&E) -> bool,
) -> Result<Value, Failure<E>> {
    coordinated(root, scope, demand, load, unauthorized)
        .map(|(value, _)| value)
        .map_err(|(failure, _)| failure)
}
/// One answer of the catalogue projection and, when a deliberate refresh
/// failed while rows are held, the failure that left them labelled.
pub struct CatalogRead<E> {
    pub catalog: Value,
    pub refresh_failure: Option<Failure<E>>,
}
/// The Kernel's catalogue projection for Design, Project Control and printing.
/// An offline demand reads only retained rows (never-observed is `unread`); a
/// refresh is one authorized listing compared with the held source basis.
pub fn catalog<E>(
    root: &Path,
    scope: &Scope,
    demand: Demand,
    load: impl FnOnce() -> Result<Value, E>,
    unauthorized: impl Fn(&E) -> bool,
) -> Result<CatalogRead<E>, Failure<E>> {
    let project = scope.project.as_str();
    if !(demand.online && demand.acquire) {
        let held = held(root, scope).map_err(Failure::Storage)?;
        return Ok(CatalogRead {
            catalog: kernel::view(project, held.as_ref(), Source::Held, None, None)
                .map_err(Failure::Storage)?,
            refresh_failure: None,
        });
    }
    match coordinated(root, scope, demand, load, unauthorized) {
        Ok((_, store)) => {
            let held = match store.held {
                Some(held) => Some(held),
                None => held(root, scope).map_err(Failure::Storage)?,
            };
            let refreshed = store.changes.as_ref().is_some_and(|c| !c.superseded);
            Ok(CatalogRead {
                catalog: kernel::view(
                    project,
                    held.as_ref(),
                    if refreshed {
                        Source::Refresh
                    } else {
                        Source::Held
                    },
                    None,
                    store.changes.as_ref(),
                )
                .map_err(Failure::Storage)?,
                refresh_failure: None,
            })
        }
        Err((failure @ Failure::Remote(_), held)) => match held.filter(Held::servable) {
            Some(held) => Ok(CatalogRead {
                catalog: kernel::view(
                    project,
                    Some(&held),
                    Source::Held,
                    Some(kernel::REFRESH_FAILED),
                    None,
                )
                .map_err(Failure::Storage)?,
                refresh_failure: Some(failure),
            }),
            None => Err(failure),
        },
        Err((failure, _)) => Err(failure),
    }
}
/// Record the exact version whose verified bytes this host's bytes owner
/// holds for one held head. No acquisition; a moved or retired head refuses.
pub fn retain_version(
    root: &Path,
    scope: &Scope,
    model_id: &str,
    version: Value,
) -> Result<Value, Failure<()>> {
    let directory = directory(root, scope).map_err(Failure::Storage)?;
    owned_directory(&directory).map_err(Failure::Storage)?;
    let _gate =
        crate::exclusive_lock(&directory.join("catalogue.lock")).map_err(Failure::Storage)?;
    let (held, _) = write(
        root,
        Operation::RetainVersion {
            scope: scope.kernel(),
            model_id: model_id.into(),
            version,
        },
    )
    .map_err(|e| match e.as_str() {
        kernel::VERSION_STALE => Failure::Admission(kernel::VERSION_STALE),
        kernel::REVOKED => Failure::AccessRevoked,
        kernel::INVALID => Failure::Admission(kernel::INVALID),
        _ => Failure::Storage(e),
    })?;
    kernel::view(&scope.project, held.as_ref(), Source::Held, None, None).map_err(Failure::Storage)
}
pub fn invalidate(root: &Path, scope: &Scope) -> Result<(), String> {
    let directory = directory(root, scope)?;
    owned_directory(&directory)?;
    let _gate = crate::exclusive_lock(&directory.join("catalogue.lock"))?;
    write(
        root,
        Operation::Invalidate {
            scope: scope.kernel(),
        },
    )
    .map(|_| ())
}
/// One consuming catalogue/control path for both native hosts. Context and
/// selected-source admission precede cache or transport access.
pub fn consume<E>(
    root: &Path,
    scope: &Scope,
    demand: Demand,
    inventory: Option<ds_command_kernel::grid_held_models::Inventory>,
    source: Option<mv_models::SourceBinding>,
    load: impl FnOnce() -> Result<Value, E>,
    unauthorized: impl Fn(&E) -> bool,
) -> Result<Value, Failure<E>> {
    if source.is_some() && inventory.is_none() {
        return Err(Failure::Admission(mv_models::STALE));
    }
    if let Some(source) = &source {
        mv_models::admit_context(&scope.project, source).map_err(Failure::Admission)?;
    }
    let inventory = inventory
        .map(|inventory| {
            ds_command_kernel::grid_held_models::admit(
                serde_json::to_value(inventory)
                    .map_err(|_| Failure::Admission(mv_models::INVALID))?,
            )
            .map_err(|_| Failure::Admission(mv_models::STALE))
        })
        .transpose()?;
    if inventory.as_ref().is_some_and(|v| {
        v.context.project.as_deref() != Some(&scope.project)
            || v.context.account.as_deref() != Some(&scope.principal)
    }) {
        return Err(Failure::Admission(mv_models::STALE));
    }
    let listing = read(root, scope, demand, load, unauthorized)?;
    let mut catalogue = mv_models::project(&scope.project, &scope.principal, &listing, inventory)
        .map_err(Failure::Admission)?;
    if let Some(source) = source {
        mv_models::admit_source(&scope.project, &catalogue, &source, &source.worker_revision)
            .map_err(Failure::Admission)?;
        catalogue["selected_source"] =
            serde_json::to_value(source).map_err(|_| Failure::Admission(mv_models::INVALID))?;
    }
    Ok(catalogue)
}
#[cfg(test)]
#[path = "grid_project_catalogue_tests.rs"]
mod tests;
