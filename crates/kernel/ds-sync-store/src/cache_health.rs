//! Read-only SQLite structural evidence. This does not repair, migrate or discard a store.
use crate::{Error, Result};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheHealth {
    Missing,
    StructurallySound,
    StructurallyCorrupt,
}

fn structural_error(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::SqliteFailure(code, _) if matches!(code.code, ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase))
}
fn sqlite_result<T>(result: rusqlite::Result<T>) -> std::result::Result<T, Option<Error>> {
    result.map_err(|error| {
        if structural_error(&error) {
            None
        } else {
            Some(Error::Storage(error))
        }
    })
}
// SQLite's B-tree integrity checker uses this heading for page/tree/freelist
// corruption. SQL CHECK/NOT NULL failures have separate diagnostics and must
// never be promoted to structural corruption merely because integrity_check failed.
fn structural_report(report: &str) -> bool {
    report.starts_with("*** in database main ***\n")
        && report.lines().skip(1).any(|line| {
            line.starts_with("Tree ")
                || line.starts_with("Page ")
                || line.starts_with("Main freelist:")
                || line.starts_with("Freelist:")
        })
}

/// Assess an existing store without creation, schema migration or row mutation.
/// Only positive SQLite structural evidence permits a Core cache owner to
/// quarantine it. Other errors (including failed constraints) remain errors.
pub fn assess(path: &Path) -> Result<CacheHealth> {
    if !path.try_exists().map_err(|e| Error::Open(e.to_string()))? {
        return Ok(CacheHealth::Missing);
    }
    let inspect = || -> std::result::Result<CacheHealth, Option<Error>> {
        let connection = sqlite_result(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ))?;
        sqlite_result(connection.busy_timeout(std::time::Duration::from_secs(5)))?;
        let mut query = sqlite_result(connection.prepare("PRAGMA integrity_check(100)"))?;
        let mut rows = sqlite_result(query.query([]))?;
        let mut structural = false;
        let mut other = false;
        while let Some(row) = sqlite_result(rows.next())? {
            let report: String = sqlite_result(row.get(0))?;
            structural |= structural_report(&report);
            other |= report != "ok";
        }
        if structural {
            Ok(CacheHealth::StructurallyCorrupt)
        } else if other {
            Err(Some(Error::Open("SQLite integrity failed without confirmed structural corruption; cache left untouched".into())))
        } else {
            Ok(CacheHealth::StructurallySound)
        }
    };
    match inspect() {
        Ok(health) => Ok(health),
        Err(None) => Ok(CacheHealth::StructurallyCorrupt),
        Err(Some(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};
    #[test]
    fn missing_and_healthy_stores_are_not_created_or_modified() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store.sqlite");
        assert_eq!(assess(&path).unwrap(), CacheHealth::Missing);
        assert!(!path.exists());
        drop(crate::Store::open(&path).unwrap());
        let before = std::fs::read(&path).unwrap();
        assert_eq!(assess(&path).unwrap(), CacheHealth::StructurallySound);
        assert_eq!(before, std::fs::read(&path).unwrap());
    }
    #[test]
    fn broken_btree_page_is_positive_structural_evidence_without_repair() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store.sqlite");
        let store = crate::Store::open(&path).unwrap();
        let c = store.connection_for_test();
        let page: i64 = c
            .query_row(
                "SELECT rootpage FROM sqlite_master WHERE name='artifacts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let size: i64 = c.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap();
        drop(store);
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(((page - 1) * size) as u64))
            .unwrap();
        file.write_all(&[255]).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let before = std::fs::read(&path).unwrap();
        assert_eq!(assess(&path).unwrap(), CacheHealth::StructurallyCorrupt);
        assert_eq!(before, std::fs::read(&path).unwrap());
    }
    #[test]
    fn invalid_sqlite_format_is_positive_evidence() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store.sqlite");
        std::fs::write(&path, b"not a SQLite cache").unwrap();
        assert_eq!(assess(&path).unwrap(), CacheHealth::StructurallyCorrupt);
    }
    #[test]
    fn healthy_constraint_failure_is_not_corruption() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store.sqlite");
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE constrained(value INTEGER CHECK(value>0)); PRAGMA ignore_check_constraints=ON; INSERT INTO constrained VALUES(-1);").unwrap();
        c.execute_batch("PRAGMA ignore_check_constraints=OFF;")
            .unwrap();
        let diagnostic: String = c
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_ne!(
            diagnostic,
            "ok",
            "fixture value={:?}",
            c.query_row("SELECT value FROM constrained", [], |r| r.get::<_, i64>(0))
        );
        drop(c);
        let before = std::fs::read(&path).unwrap();
        // This read-only probe is structural evidence, never authority to
        // discard a sound record for a SQL constraint violation.
        assert_eq!(assess(&path).unwrap(), CacheHealth::StructurallySound);
        assert_eq!(before, std::fs::read(&path).unwrap());
        assert!(!structural_report("NULL value in artifacts.account"));
        assert!(!structural_report("CHECK constraint failed in constrained"));
    }
    #[test]
    fn incompatible_schema_and_unknown_healthy_database_remain_sound() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("store.sqlite");
        let store = crate::Store::open(&path).unwrap();
        store
            .connection_for_test()
            .execute("UPDATE meta SET value=999 WHERE key='schema_version'", [])
            .unwrap();
        drop(store);
        assert_eq!(assess(&path).unwrap(), CacheHealth::StructurallySound);
        assert!(matches!(
            crate::Store::open(&path),
            Err(Error::Incompatible { .. })
        ));
        let other = root.path().join("unrelated.sqlite");
        let c = Connection::open(&other).unwrap();
        c.execute_batch(
            "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('retained');",
        )
        .unwrap();
        drop(c);
        let before = std::fs::read(&other).unwrap();
        assert_eq!(assess(&other).unwrap(), CacheHealth::StructurallySound);
        assert_eq!(before, std::fs::read(&other).unwrap());
    }
    #[test]
    fn constraint_permission_busy_and_io_errors_never_classify_as_structural() {
        for code in [
            rusqlite::ffi::SQLITE_CONSTRAINT,
            rusqlite::ffi::SQLITE_PERM,
            rusqlite::ffi::SQLITE_BUSY,
            rusqlite::ffi::SQLITE_IOERR,
            rusqlite::ffi::SQLITE_CANTOPEN,
        ] {
            assert!(!structural_error(&rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(code),
                None
            )));
        }
    }
}
