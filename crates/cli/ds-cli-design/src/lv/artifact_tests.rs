use super::*;

#[cfg(unix)]
#[test]
fn result_writer_produces_a_private_single_link_core_handoff() {
    use std::os::unix::fs::MetadataExt;
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("result.json");
    let bytes = br#"{"project":"project-a","jobs":[]}"#;
    write_new(&output, bytes, &RESULT).unwrap();
    let metadata = std::fs::metadata(&output).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.uid(), std::fs::metadata(directory.path()).unwrap().uid());
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn existing_result_is_never_replaced_and_failed_stage_is_removed() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("result.json");
    let original = b"original result";
    std::fs::write(&output, original).unwrap();
    let failure = write_new(&output, b"replacement result", &RESULT).unwrap_err();
    assert_eq!(failure.code(), RESULT.exists_code);
    assert_eq!(std::fs::read(&output).unwrap(), original);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn symlink_result_is_refused_without_changing_its_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.json");
    let output = directory.path().join("result.json");
    std::fs::write(&target, b"retained result").unwrap();
    std::os::unix::fs::symlink(&target, &output).unwrap();
    let failure = write_new(&output, b"replacement result", &RESULT).unwrap_err();
    assert_eq!(failure.code(), RESULT.exists_code);
    assert_eq!(std::fs::read(&target).unwrap(), b"retained result");
    assert!(std::fs::symlink_metadata(&output).unwrap().is_symlink());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}
