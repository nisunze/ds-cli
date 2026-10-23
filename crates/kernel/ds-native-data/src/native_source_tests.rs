use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ds-io-{label}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn linked_source_is_never_copied_and_editable_links_get_a_safe_plan() {
    let root = test_dir("linked");
    let source = root.join("points.csv");
    fs::write(&source, b"x,y\n1,2\n").unwrap();
    let result = prepare_native_source(NativeSourcePrepareRequest {
        source_path: source.clone(),
        disposition: NativeSourceDisposition::Linked,
        link_access: Some(NativeLinkAccess::Editable),
        destination: None,
    })
    .unwrap();
    assert!(!result.copied);
    assert_eq!(result.prepared_path, fs::canonicalize(&source).unwrap());
    assert_eq!(result.fingerprint.size_bytes, 8);
    assert_eq!(
        result.edit.unwrap().strategy,
        "sidecar_then_locked_atomic_replace"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_copy_streams_to_a_content_named_file_without_overwriting() {
    let root = test_dir("managed");
    let source = root.join("points.csv");
    let managed = root.join("managed");
    fs::write(&source, b"x,y\n1,2\n").unwrap();
    let request = || NativeSourcePrepareRequest {
        source_path: source.clone(),
        disposition: NativeSourceDisposition::ManagedCopy,
        link_access: None,
        destination: Some(managed.clone()),
    };
    let result = prepare_native_source(request()).unwrap();
    assert!(result.copied);
    assert_eq!(fs::read(&result.prepared_path).unwrap(), b"x,y\n1,2\n");
    assert!(
        prepare_native_source(request())
            .unwrap_err()
            .contains("refusing to overwrite")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn external_copy_never_overwrites_operator_data() {
    let root = test_dir("external");
    let source = root.join("source.csv");
    let target = root.join("chosen.csv");
    fs::write(&source, b"new").unwrap();
    fs::write(&target, b"old").unwrap();
    let error = prepare_native_source(NativeSourcePrepareRequest {
        source_path: source,
        disposition: NativeSourceDisposition::ExternalCopy,
        link_access: None,
        destination: Some(target.clone()),
    })
    .unwrap_err();
    assert!(error.contains("refusing to overwrite"));
    assert_eq!(fs::read(target).unwrap(), b"old");
    fs::remove_dir_all(root).unwrap();
}
