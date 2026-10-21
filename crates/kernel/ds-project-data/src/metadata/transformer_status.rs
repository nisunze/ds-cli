//! Whole-project transformer observations. No legacy JSON reader or network IO.
use super::{Error, Kind, Scope};
use ds_command_kernel::sql_effect::execute;
use ds_command_kernel::transformer_status_store as shared;
use ds_layer_store::{private, snapshot_privacy};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
pub use shared::Observation;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn path(root: &Path, scope: &Scope) -> Result<PathBuf, String> {
    Ok(super::path(root, scope, Kind::TransformerStatus)?.with_extension("sqlite"))
}
fn sql(error: rusqlite::Error) -> String {
    format!("transformer status store: {error}")
}
fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            snapshot_privacy::file(path)?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
fn open(path: &Path, writable: bool) -> Result<Connection, String> {
    let present = exists(path)?;
    if writable && !present {
        let file = private::open_options()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        drop(file);
        snapshot_privacy::file(path)?;
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        exists(&PathBuf::from(format!("{}{suffix}", path.display())))?;
    }
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection =
        Connection::open_with_flags(ds_layer_store::sqlite_path(path)?, flags).map_err(sql)?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(sql)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql)?;
    if writable && (!present || version == 1) {
        connection
            .execute_batch("PRAGMA journal_mode=WAL; BEGIN IMMEDIATE;")
            .map_err(sql)?;
        // Declared derived-cache v2 transition: no legacy import or dual reader.
        // This database belongs only to status; Styles uses its separate JSON owner.
        let upgraded = (|| {
            if version == 1 {
                connection
                    .execute_batch("DROP TABLE observation")
                    .map_err(sql)?;
            }
            execute(&connection, &shared::schema())?;
            connection
                .execute_batch("PRAGMA user_version=2; COMMIT;")
                .map_err(sql)
        })();
        if upgraded.is_err() {
            let _ = connection.execute_batch("ROLLBACK");
        }
        upgraded?;
    } else if version != 2 && !(version == 1 && !writable) {
        return Err("transformer status store has an unsupported schema".into());
    }
    Ok(connection)
}
fn run(
    connection: &Connection,
    operation: shared::Operation,
) -> Result<(Option<Observation>, Option<u64>), String> {
    let transaction = connection.unchecked_transaction().map_err(sql)?;
    let mut step = shared::start(operation)?;
    for _ in 0..8 {
        match step {
            shared::Step::Done {
                observation,
                revision,
            } => {
                transaction.commit().map_err(sql)?;
                return Ok((observation, revision));
            }
            shared::Step::Sql {
                statement,
                continuation,
            } => {
                step = shared::resume(continuation, execute(&transaction, &statement)?)?;
            }
        }
    }
    Err("status owner exceeded its bounded continuation".into())
}
fn read(connection: &Connection, scope: &Scope) -> Result<Option<Observation>, String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql)?;
    if version == 1 {
        return Ok(None);
    }
    run(
        connection,
        shared::Operation::Read {
            scope: scope.clone(),
        },
    )
    .map(|(observation, _)| observation)
}
/// Never creates a missing database, invokes a fetch, or reads the retired JSON.
pub fn held(root: &Path, scope: &Scope) -> Result<Option<Observation>, String> {
    let path = path(root, scope)?;
    if !exists(&path)? {
        return Ok(None);
    }
    read(&open(&path, false)?, scope)
}
fn write(connection: &Connection, scope: &Scope, document: &Value) -> Result<(), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    execute(connection, &shared::write(scope, document, now)?)?;
    Ok(())
}
fn locked(root: &Path, scope: &Scope) -> Result<(fs::File, Connection), String> {
    let path = path(root, scope)?;
    snapshot_privacy::directory(path.parent().ok_or("missing status parent")?)?;
    let lock = super::lock(&path)?;
    Ok((lock, open(&path, true)?))
}
pub(super) fn acquire<E>(
    root: &Path,
    scope: &Scope,
    refresh: bool,
    fetch: impl FnOnce() -> Result<Value, E>,
) -> Result<(Value, bool), Error<E>> {
    let (_lock, connection) = locked(root, scope).map_err(Error::Store)?;
    let mode = if refresh {
        shared::ReadMode::Refresh
    } else {
        shared::ReadMode::Acquire
    };
    let mut step = shared::flow_start(scope.clone(), mode).map_err(Error::Store)?;
    let mut fetch = Some(fetch);
    let mut fetch_error = None;
    for _ in 0..8 {
        step = match step {
            shared::FlowStep::Storage {
                operation,
                continuation,
            } => {
                let (observation, revision) = run(&connection, operation).map_err(Error::Store)?;
                shared::flow_resume(
                    continuation,
                    shared::FlowResult::Storage {
                        observation,
                        revision,
                    },
                )
            }
            shared::FlowStep::Fetch { continuation, .. } => {
                let result = match fetch.take().ok_or_else(|| {
                    Error::Store("status owner requested a duplicate fetch".into())
                })?() {
                    Ok(document) => shared::FlowResult::Fetch {
                        document,
                        now: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_err(|e| Error::Store(e.to_string()))?
                            .as_millis() as u64,
                    },
                    Err(error) => {
                        fetch_error = Some(error);
                        shared::FlowResult::Failure
                    }
                };
                shared::flow_resume(continuation, result)
            }
            shared::FlowStep::Done {
                observation,
                refreshed,
                failed,
            } => {
                if let Some(error) = fetch_error {
                    return Err(Error::Read(error));
                }
                if failed {
                    return Err(Error::Store("status_refresh_failed".into()));
                }
                return Ok((
                    observation
                        .ok_or_else(|| Error::Store("status observation is absent".into()))?
                        .document,
                    !refreshed,
                ));
            }
        }
        .map_err(Error::Store)?;
    }
    Err(Error::Store(
        "status acquisition exceeded its continuation bound".into(),
    ))
}

pub(super) fn observe(root: &Path, scope: &Scope, document: &Value) -> Result<(), String> {
    let (_lock, connection) = locked(root, scope)?;
    read(&connection, scope)?;
    write(&connection, scope, document)
}
pub(super) fn invalidate(root: &Path, scope: &Scope) -> Result<(), String> {
    if !exists(&path(root, scope)?)? {
        return Ok(());
    }
    let (_lock, connection) = locked(root, scope)?;
    read(&connection, scope)?;
    execute(&connection, &shared::invalidate(scope, false)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn scope() -> Scope {
        Scope {
            principal: "user".into(),
            lane: "canary".into(),
            audience: "a".into(),
            project: "p".into(),
        }
    }
    #[test]
    fn declared_v1_reset_discards_only_derived_status_and_preserves_styles() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        super::super::observe(dir.path(), &scope, Kind::Styles, &json!({"style":"kept"})).unwrap();
        let path = path(dir.path(), &scope).unwrap();
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE observation(id INTEGER PRIMARY KEY,document TEXT); INSERT INTO observation VALUES(1,'legacy derived status'); PRAGMA user_version=1;").unwrap();
        drop(legacy);
        assert!(held(dir.path(), &scope).unwrap().is_none());
        acquire(dir.path(), &scope, false, || {
            Ok::<_, ()>(json!({"transformers":[]}))
        })
        .unwrap();
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='observation'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            0
        );
        let (style, cached) =
            super::super::acquire(dir.path(), &scope, Kind::Styles, false, || {
                Err::<Value, _>(())
            })
            .unwrap();
        assert!(cached);
        assert_eq!(style, json!({"style":"kept"}));
    }
    #[test]
    fn missing_held_does_not_create_and_legacy_json_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        assert!(held(dir.path(), &scope).unwrap().is_none());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        super::super::observe(dir.path(), &scope, Kind::Styles, &json!({})).unwrap();
        super::super::write(
            &super::super::path(dir.path(), &scope, Kind::TransformerStatus).unwrap(),
            &scope,
            &json!({"old":true}),
        )
        .unwrap();
        assert!(held(dir.path(), &scope).unwrap().is_none());
    }
    #[test]
    fn refresh_failure_and_invalidation_keep_exact_observation_and_cold_acquire_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        acquire(dir.path(), &scope, false, || {
            Ok::<_, ()>(json!({"transformers":[{"name":"TX-1"}]}))
        })
        .unwrap();
        let original = held(dir.path(), &scope).unwrap().unwrap();
        assert!(acquire(dir.path(), &scope, true, || Err::<Value, _>("offline")).is_err());
        let failed = held(dir.path(), &scope).unwrap().unwrap();
        assert_eq!(failed.sha256, original.sha256);
        assert_eq!(failed.observed_at_ms, original.observed_at_ms);
        assert!(failed.needs_refresh);
        assert_eq!(failed.last_error.as_deref(), Some("refresh_failed"));
        let (_, cached) = acquire(dir.path(), &scope, false, || {
            Ok::<_, ()>(json!({"transformers":[]}))
        })
        .unwrap();
        assert!(!cached);
        assert!(!held(dir.path(), &scope).unwrap().unwrap().needs_refresh);
        invalidate(dir.path(), &scope).unwrap();
        assert!(held(dir.path(), &scope).unwrap().unwrap().needs_refresh);
    }
    #[test]
    fn concurrent_cold_reads_fetch_once_and_empty_snapshot_is_held() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|threads| {
            for _ in 0..6 {
                let calls = calls.clone();
                let root = dir.path();
                threads.spawn(move || {
                    acquire(root, &scope(), false, || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok::<_, ()>(json!({"transformers":[]}))
                    })
                    .unwrap();
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let held = held(dir.path(), &scope()).unwrap().unwrap();
        assert_eq!(held.document, json!({"transformers":[]}));
    }
    #[cfg(unix)]
    #[test]
    fn linked_database_and_wrong_permissions_refuse_without_repair() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        observe(dir.path(), &scope, &json!([])).unwrap();
        let path = path(dir.path(), &scope).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(held(dir.path(), &scope).is_err());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let renamed = path.with_extension("saved");
        fs::rename(&path, &renamed).unwrap();
        symlink(&renamed, &path).unwrap();
        assert!(held(dir.path(), &scope).is_err());
    }
    #[test]
    fn scope_and_digest_corruption_refuse_and_scopes_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        observe(dir.path(), &scope, &json!({"transformers":[]})).unwrap();
        for other in [
            Scope {
                principal: "other".into(),
                ..scope.clone()
            },
            Scope {
                lane: "stable".into(),
                ..scope.clone()
            },
            Scope {
                audience: "b".into(),
                ..scope.clone()
            },
            Scope {
                project: "q".into(),
                ..scope.clone()
            },
        ] {
            assert!(held(dir.path(), &other).unwrap().is_none())
        }
        let connection = open(&path(dir.path(), &scope).unwrap(), true).unwrap();
        connection
            .execute("UPDATE project_status_observations SET sha256='wrong'", [])
            .unwrap();
        assert!(held(dir.path(), &scope).is_err());
    }
}
