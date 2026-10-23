use super::*;

#[test]
fn reads_refuse_oversized_and_nonregular_sources() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("input.json");
    fs::write(&file, b"abc").unwrap();
    assert_eq!(read(&file, 3).unwrap(), b"abc");
    assert!(read(&file, 2).is_err());
    assert!(read(root.path(), 3).is_err());
    #[cfg(unix)]
    {
        let link = root.path().join("linked.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(read(&link, 3).is_err());
    }
    assert_eq!(fs::read(&file).unwrap(), b"abc");
}

#[test]
fn durable_writes_never_replace_existing_bytes() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("artifacts/result.json");
    write(&file, b"complete").unwrap();
    sync_dir(file.parent().unwrap()).unwrap();
    assert!(write(&file, b"replacement").is_err());
    assert_eq!(fs::read(&file).unwrap(), b"complete");
}
