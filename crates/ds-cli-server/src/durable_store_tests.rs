use super::*;
use std::cell::Cell;

#[cfg(windows)]
struct Directory(PathBuf);
#[cfg(windows)]
impl Directory {
    fn path(&self) -> &Path {
        &self.0
    }
}
#[cfg(windows)]
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[cfg(windows)]
fn directory() -> Directory {
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let path = ds_cli_auth::windows_host::local_application_data()
        .unwrap()
        .join(format!("ds-core-store-test-{}-{nonce}", std::process::id()));
    super::super::host::prepare_directory(&path).unwrap();
    Directory(path)
}
#[cfg(not(windows))]
fn directory() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    super::super::host::prepare_directory(dir.path()).unwrap();
    dir
}
fn identity(account: &str) -> Identity {
    Identity {
        principal: Principal::Account(account.into()),
        lane: "stable".into(),
        audience: "a".repeat(64),
    }
}
fn call_of(value: Value) -> Call {
    serde_json::from_value(value).unwrap()
}
fn result(path: &Path) -> Value {
    let payload: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(payload["schema"], "ds.core-store/v1");
    payload["result"].clone()
}

#[test]
fn a_preference_round_trips_through_the_protected_store_under_the_bound_account() {
    let dir = directory();
    let database = dir.path().join("store.sqlite");
    let set = call_of(json!({"request":{"store":"ui_preferences","operation":{"action":"set","key":"active_project","value":"kigali_north","now":42}}}));
    let out = dir.path().join("set.json");
    let receipt = capture(&database, "stable", set, None, &out, None, || Ok(identity("owner-a"))).unwrap();
    assert_eq!(receipt["schema"], "ds.core-store-file/v1");
    assert_eq!(receipt["store"], "ui_preferences");
    assert_eq!(receipt["owner"]["account"], "owner-a");
    assert_eq!(receipt["bytes"], Value::Null);
    let written = result(&out);
    assert_eq!(written["values"]["active_project"], "kigali_north");
    assert_eq!(written["generation"], 1);
    assert_eq!(written["owner"], identity("owner-a").owner());

    let get = json!({"request":{"store":"ui_preferences","operation":{"action":"get","keys":["active_project"]}}});
    let mine = dir.path().join("get-a.json");
    capture(&database, "stable", call_of(get.clone()), None, &mine, None, || Ok(identity("owner-a"))).unwrap();
    assert_eq!(result(&mine)["values"]["active_project"], "kigali_north");
    let theirs = dir.path().join("get-b.json");
    capture(&database, "stable", call_of(get), None, &theirs, None, || Ok(identity("owner-b"))).unwrap();
    assert_eq!(result(&theirs)["values"]["active_project"], Value::Null, "another account never reads it");
}

#[test]
fn a_store_with_its_own_command_is_refused_by_name() {
    let dir = directory();
    let request = dir.path().join("request.json");
    {
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&request).unwrap();
        file.write_all(br#"{"request":{"store":"status","operation":{"action":"schema"}}}"#)
            .unwrap();
    }
    let error = call(&request).unwrap_err();
    assert_eq!(error.code(), "core_store_not_served");
}

#[test]
fn an_identity_change_refuses_before_the_call_and_leaves_no_output() {
    let dir = directory();
    let database = dir.path().join("store.sqlite");
    let out = dir.path().join("answer.json");
    let calls = Cell::new(0);
    let error = capture(
        &database,
        "stable",
        call_of(json!({"request":{"store":"ui_preferences","operation":{"action":"set","key":"active_project","value":"kigali_north","now":1}}})),
        None,
        &out,
        None,
        || {
            calls.set(calls.get() + 1);
            Ok(identity(if calls.get() == 1 { "owner-a" } else { "owner-b" }))
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "core_store_identity_changed");
    assert!(!out.exists(), "an unacknowledged output is removed");
    assert!(!database.exists(), "nothing ran");
}

#[test]
fn a_kernel_refusal_writes_nothing_and_keeps_its_code() {
    let dir = directory();
    let database = dir.path().join("store.sqlite");
    let out = dir.path().join("answer.json");
    let error = capture(
        &database,
        "stable",
        call_of(json!({"request":{"store":"ui_preferences","operation":{"action":"set","key":"active_project","value":"a/b","now":1}}})),
        None,
        &out,
        None,
        || Ok(identity("owner-a")),
    )
    .unwrap_err();
    assert_eq!(error.code(), "core_store_refused");
    assert!(error.message().contains("ui_preference_invalid"), "{}", error.message());
    assert!(!out.exists());
}
