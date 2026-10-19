use super::*;
/// Materialize an owner migration output as the command does after the addon
/// returned it: the strict-current bytes and their receipt, no original.
fn materialize(
    out: &Path,
    output: &ds_grid_exchange::package_migration::MigrationOutput,
) -> Result<Value, Failure> {
    let receipt = serde_json::to_value(&output.receipt).unwrap();
    materialize_preserving_source(
        out,
        &output.bytes,
        &receipt,
        &output.receipt.source_sha256,
        None,
    )
}
fn output() -> ds_grid_exchange::package_migration::MigrationOutput {
    let blank =
        ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest::default())
            .unwrap();
    ds_grid_exchange::package_migration::migrate(&blank.bytes).unwrap()
}
#[test]
fn materialized_current_package_commits_receipt_and_refuses_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("fresh");
    let output = output();
    let receipt = materialize(&out, &output).unwrap();
    let package = std::fs::read(out.join("model.dsgrid")).unwrap();
    assert_eq!(package, output.bytes);
    assert!(receipt["receipt_path"].is_string());
    let saved_receipt = std::fs::read(out.join("migration.receipt.json")).unwrap();
    assert!(materialize(&out, &output).is_err());
    assert_eq!(std::fs::read(out.join("model.dsgrid")).unwrap(), package);
    assert_eq!(
        std::fs::read(out.join("migration.receipt.json")).unwrap(),
        saved_receipt
    );
}
#[test]
fn relative_and_existing_outputs_refuse_without_replacing_files() {
    let output = output();
    assert!(materialize(Path::new("relative-migration-output"), &output).is_err());
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("existing");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("keep"), b"original bytes").unwrap();
    assert!(materialize(&out, &output).is_err());
    assert_eq!(std::fs::read(out.join("keep")).unwrap(), b"original bytes");
    assert!(!out.join("migration.receipt.json").exists());
}
