use super::*;
#[test]
fn global_confirmation_selects_write_and_refuses_missing_or_conflicting_modes() {
    let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../out/nonexistent-confirmation-source.dsgrid");
    assert!(!source.exists());
    for (dry, confirmed, expected) in [
        (false, false, "package_migration_confirmation"),
        (true, true, "package_migration_confirmation"),
        (true, false, "model_not_found"),
        (false, true, "model_not_found"),
    ] {
        let mut tokens = vec![
            "--path".into(),
            source.to_str().unwrap().into(),
            "--expected-source-sha256".into(),
            "a".repeat(64),
        ];
        if dry {
            tokens.push("--dry-run".into());
        }
        let inputs = ds_cli_contract::parse(&COMMAND, &tokens).unwrap();
        let context = Context {
            confirmed,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        // Reaching the source guard proves both admitted modes; no files
        // are written. Actual DS materialization proves the end-to-end path.
        assert_eq!(run(&inputs, &context).unwrap_err().code(), expected);
    }
}
