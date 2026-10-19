use super::*;
#[cfg(windows)]
struct FixtureDirectory(PathBuf);
#[cfg(windows)]
impl FixtureDirectory {
    fn path(&self) -> &Path {
        &self.0
    }
}
#[cfg(windows)]
impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[cfg(windows)]
fn directory() -> FixtureDirectory {
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
    // Sandbox TEMP may be foreign-writable. Retain the existing native
    // SID/DACL owner and use its current-user known-folder fixture pattern.
    let path = ds_cli_auth::windows_host::local_application_data()
        .unwrap()
        .join(format!("ds-core-cache-test-{}-{nonce}", std::process::id()));
    crate::host::prepare_directory(&path).unwrap();
    FixtureDirectory(path)
}
#[cfg(not(windows))]
fn directory() -> tempfile::TempDir {
    let d = ds_cli_contract::testing::private_tempdir("core-cache-");
    crate::host::prepare_directory(d.path()).unwrap();
    d
}
fn write(path: &Path, bytes: &[u8]) {
    create_private(path, bytes).unwrap();
}
#[test]
fn corrupt_cache_and_all_sidecars_retained_while_every_other_file_stays_identical() {
    let root = directory();
    let d = root.path();
    for name in [
        "connection.json",
        "credential.fixture",
        "identity.fixture",
        "component-selection.fixture",
        "input.fixture",
        "output.fixture",
    ] {
        write(&d.join(name), name.as_bytes());
    }
    for suffix in SUFFIXES {
        write(
            &d.join(format!("{DATABASE}{suffix}")),
            if suffix.is_empty() {
                b"broken SQLite"
            } else {
                b""
            },
        );
    }
    let receipt = prepare_cache(d).unwrap().unwrap();
    assert!(!d.join(DATABASE).exists());
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(document["files"].as_array().unwrap().len(), 4);
    let prefix = receipt
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".receipt.json")
        .unwrap();
    for suffix in SUFFIXES {
        assert_eq!(
            fs::read(d.join(format!("{prefix}{suffix}"))).unwrap(),
            if suffix.is_empty() {
                b"broken SQLite".as_slice()
            } else {
                b"".as_slice()
            }
        );
    }
    for name in [
        "connection.json",
        "credential.fixture",
        "identity.fixture",
        "component-selection.fixture",
        "input.fixture",
        "output.fixture",
    ] {
        assert_eq!(fs::read(d.join(name)).unwrap(), name.as_bytes());
    }
    drop(ds_sync_store::Store::open(&d.join(DATABASE)).unwrap());
    assert_eq!(
        cache_health::assess(&d.join(DATABASE)).unwrap(),
        CacheHealth::StructurallySound
    );
}
#[test]
fn sound_incompatible_schema_is_never_quarantined_or_reset() {
    let root = directory();
    let path = root.path().join(DATABASE);
    let store = ds_sync_store::Store::open(&path).unwrap();
    store
        .connection_for_test()
        .execute("UPDATE meta SET value=999 WHERE key='schema_version'", [])
        .unwrap();
    drop(store);
    let before = held(&path).unwrap();
    assert!(prepare_cache(root.path()).unwrap().is_none());
    assert!(matches!(
        ds_sync_store::Store::open(&path),
        Err(ds_sync_store::Error::Incompatible { .. })
    ));
    assert_eq!(before.sha256, held(&path).unwrap().sha256);
}
#[test]
fn healthy_not_null_migration_failure_does_not_authorize_quarantine() {
    let root = directory();
    let path = root.path().join(DATABASE);
    drop(ds_sync_store::Store::open(&path).unwrap());
    let raw = rusqlite::Connection::open(&path).unwrap();
    // Generated logical-constraint violation, with sound B-tree pages.
    raw.execute_batch("BEGIN; ALTER TABLE artifacts RENAME TO artifacts_v2;
        CREATE TABLE artifacts(account TEXT, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
        scope TEXT NOT NULL, engine TEXT NOT NULL, operation TEXT NOT NULL, variant TEXT NOT NULL,
        replay_key TEXT NOT NULL, state TEXT NOT NULL, updated_at_ms INTEGER NOT NULL, row TEXT NOT NULL,
        PRIMARY KEY(account,deployment,install_id,scope,engine,operation,variant),
        UNIQUE(account,deployment,install_id,replay_key));
        INSERT INTO artifacts VALUES(NULL,'fixture','fixture','personal','fixture','fixture','fixture','fixture','queued',1,'fixture');
        DROP TABLE artifacts_v2; UPDATE meta SET value=1 WHERE key='schema_version'; COMMIT;
        PRAGMA writable_schema=ON;
        UPDATE sqlite_master SET sql=replace(sql,'account TEXT,','account TEXT NOT NULL,') WHERE name='artifacts';
        PRAGMA writable_schema=OFF;").unwrap();
    drop(raw);
    let before = held(&path).unwrap();
    let prepared = prepare_cache(root.path());
    assert!(!matches!(prepared, Ok(Some(_))));
    assert_eq!(before.sha256, held(&path).unwrap().sha256);
    assert!(
        matches!(ds_sync_store::Store::open(&path),Err(ds_sync_store::Error::Storage(rusqlite::Error::SqliteFailure(error,_))) if error.code==rusqlite::ErrorCode::ConstraintViolation)
    );
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        raw.query_row(
            "SELECT typeof(account) FROM artifacts NOT INDEXED",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "null"
    );
}
#[test]
fn unknown_healthy_migration_layout_refuses_without_quarantine() {
    let root = directory();
    let path = root.path().join(DATABASE);
    let store = ds_sync_store::Store::open(&path).unwrap();
    store
        .connection_for_test()
        .execute("UPDATE meta SET value=1 WHERE key='schema_version'", [])
        .unwrap();
    drop(store);
    let before = held(&path).unwrap();
    assert!(prepare_cache(root.path()).unwrap().is_none());
    assert!(ds_sync_store::Store::open(&path).is_err());
    assert_eq!(before.sha256, held(&path).unwrap().sha256);
}
#[cfg(unix)]
#[test]
fn symlinks_hardlinks_and_foreign_permissions_refuse_without_cache_mutation() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for kind in 0..3 {
        let root = directory();
        let target = root.path().join("other");
        write(&target, b"retained");
        let path = root.path().join(DATABASE);
        if kind == 0 {
            symlink(&target, &path).unwrap();
        } else if kind == 1 {
            fs::hard_link(&target, &path).unwrap();
        } else {
            write(&path, b"retained");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }
        assert!(prepare_cache(root.path()).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"retained");
        assert!(path.exists());
    }
}
#[test]
fn orphan_sidecar_refuses_instead_of_attaching_it_to_a_new_database() {
    let root = directory();
    write(&root.path().join("store.sqlite-wal"), b"retained");
    assert!(prepare_cache(root.path()).is_err());
    assert!(!root.path().join(DATABASE).exists());
}
#[test]
fn nonempty_quarantine_members_remain_exact_diagnostic_bytes() {
    let root = directory();
    let d = root.path();
    let mut files = Vec::new();
    for suffix in SUFFIXES {
        let path = d.join(format!("{DATABASE}{suffix}"));
        write(&path, format!("diagnostic fixture{suffix}").as_bytes());
        files.push((path.clone(), held(&path).unwrap()));
    }
    let receipt = quarantine(d, &files, &move_private).unwrap();
    let prefix = receipt
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".receipt.json")
        .unwrap();
    for suffix in SUFFIXES {
        assert_eq!(
            fs::read(d.join(format!("{prefix}{suffix}"))).unwrap(),
            format!("diagnostic fixture{suffix}").as_bytes()
        );
    }
}
#[test]
fn readonly_hot_journal_refusal_is_not_promoted_to_corruption() {
    let root = directory();
    let d = root.path();
    write(&d.join(DATABASE), b"broken SQLite");
    write(&d.join("store.sqlite-journal"), b"hot journal fixture");
    assert!(prepare_cache(d).is_err());
    assert_eq!(fs::read(d.join(DATABASE)).unwrap(), b"broken SQLite");
    assert_eq!(
        fs::read(d.join("store.sqlite-journal")).unwrap(),
        b"hot journal fixture"
    );
}
#[test]
fn post_rename_durability_failure_restores_original_names() {
    let root = directory();
    let d = root.path();
    let path = d.join(DATABASE);
    write(&path, b"retained");
    let files = vec![(path.clone(), held(&path).unwrap())];
    let fail = |from: &Path, to: &Path| {
        fs::rename(from, to).unwrap();
        Err("fixture durability refusal".into())
    };
    assert!(quarantine(d, &files, &fail).is_err());
    assert_eq!(fs::read(path).unwrap(), b"retained");
}
#[test]
fn failed_member_move_rolls_back_and_never_creates_a_fresh_database() {
    let root = directory();
    let d = root.path();
    let mut files = Vec::new();
    for suffix in ["-wal", ""] {
        let path = d.join(format!("{DATABASE}{suffix}"));
        write(&path, b"retained");
        files.push((path.clone(), held(&path).unwrap()));
    }
    let count = std::cell::Cell::new(0);
    let fail = |from: &Path, to: &Path| {
        count.set(count.get() + 1);
        if count.get() == 2 {
            Err("fixture refusal".into())
        } else {
            move_private(from, to)
        }
    };
    assert!(quarantine(d, &files, &fail).is_err());
    for suffix in ["-wal", ""] {
        assert_eq!(
            fs::read(d.join(format!("{DATABASE}{suffix}"))).unwrap(),
            b"retained"
        );
    }
}
