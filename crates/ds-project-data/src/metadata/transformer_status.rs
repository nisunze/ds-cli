//! Whole-project transformer observations. No legacy JSON reader or network IO.
use super::{digest, Error, Kind, Scope, MAX_BYTES};
use ds_layer_store::{private, snapshot_privacy};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize)]
pub struct Observation {
    pub document: Value,
    pub sha256: String,
    pub observed_at_ms: u64,
    pub needs_refresh: bool,
    pub last_error: Option<String>,
}
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
    if writable && !present {
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; BEGIN IMMEDIATE;
            CREATE TABLE observation (id INTEGER PRIMARY KEY CHECK(id=1), scope TEXT NOT NULL,
            document TEXT NOT NULL, sha256 TEXT NOT NULL, observed_at_ms INTEGER NOT NULL,
            needs_refresh INTEGER NOT NULL CHECK(needs_refresh IN (0,1)), last_error TEXT);
            PRAGMA user_version=1; COMMIT;",
            )
            .map_err(sql)?;
    } else if version != 1 {
        return Err("transformer status store has an unsupported schema".into());
    }
    Ok(connection)
}
fn read(connection: &Connection, scope: &Scope) -> Result<Option<Observation>, String> {
    let size: Option<u64> = connection
        .query_row(
            "SELECT length(CAST(document AS BLOB)) FROM observation WHERE id=1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    if size.is_some_and(|size| size > MAX_BYTES) {
        return Err("transformer status observation exceeds its byte bound".into());
    }

    let row: Option<(String,String,String,u64,bool,Option<String>)> = connection.query_row(
        "SELECT scope,document,sha256,observed_at_ms,needs_refresh,last_error FROM observation WHERE id=1 AND length(document)<=?1",
        [MAX_BYTES], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(sql)?;
    let Some((stored_scope, document, sha256, observed_at_ms, needs_refresh, last_error)) = row
    else {
        return Ok(None);
    };
    if stored_scope != serde_json::to_string(scope).map_err(|e| e.to_string())?
        || document.len() as u64 > MAX_BYTES
    {
        return Err("transformer status observation scope or byte bound mismatch".into());
    }
    let document: Value =
        serde_json::from_str(&document).map_err(|_| "transformer status observation is corrupt")?;
    if digest(&document)? != sha256 {
        return Err("transformer status observation digest mismatch".into());
    }
    Ok(Some(Observation {
        document,
        sha256,
        observed_at_ms,
        needs_refresh,
        last_error,
    }))
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
    let bytes = serde_json::to_string(document).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("transformer status exceeds its byte bound".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    connection.execute("INSERT INTO observation VALUES(1,?1,?2,?3,?4,0,NULL)
        ON CONFLICT(id) DO UPDATE SET scope=excluded.scope,document=excluded.document,sha256=excluded.sha256,
        observed_at_ms=excluded.observed_at_ms,needs_refresh=0,last_error=NULL",
        params![serde_json::to_string(scope).map_err(|e|e.to_string())?,bytes,digest(document)?,now]).map_err(sql)?;
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
    let previous = read(&connection, scope).map_err(Error::Store)?;
    if !refresh {
        if let Some(value) = previous.filter(|value| !value.needs_refresh) {
            return Ok((value.document, true));
        }
    }
    match fetch() {
        Ok(document) => {
            write(&connection, scope, &document).map_err(Error::Store)?;
            Ok((document, false))
        }
        Err(error) => {
            connection
                .execute(
                    "UPDATE observation SET needs_refresh=1,last_error='refresh_failed' WHERE id=1",
                    [],
                )
                .map_err(sql)
                .map_err(Error::Store)?;
            Err(Error::Read(error))
        }
    }
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
    connection
        .execute(
            "UPDATE observation SET needs_refresh=1,last_error=NULL WHERE id=1",
            [],
        )
        .map_err(sql)?;
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
            .execute("UPDATE observation SET sha256='wrong'", [])
            .unwrap();
        assert!(held(dir.path(), &scope).is_err());
    }
}
