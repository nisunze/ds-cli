use super::*;
#[test]
fn optional_catalog_uses_engine_metadata_without_changing_resolved_settings() {
    let context = Context {
        confirmed: false,
        output: ds_cli_contract::Output::resolve(
            ds_cli_contract::output::Format::Json,
            false,
            true,
        ),
    };
    let base = vec!["--preset".into(), "sketch".into()];
    let plain = run(
        &ds_cli_contract::args::parse(&COMMAND, &base).unwrap(),
        &context,
    )
    .unwrap();
    let mut with_catalog = base;
    with_catalog.push("--catalog".into());
    let mut catalogued = run(
        &ds_cli_contract::args::parse(&COMMAND, &with_catalog).unwrap(),
        &context,
    )
    .unwrap();
    let catalog = catalogued
        .as_object_mut()
        .unwrap()
        .remove("catalog")
        .unwrap();
    let owner: Value =
        serde_json::from_str(ds_network_contracts::lv::PROCESSOR_PRESETS_JSON).unwrap();
    assert_eq!(catalog, owner);
    assert_eq!(plain, catalogued);
    assert!(!catalog["groups"].as_array().unwrap().is_empty());
}

#[test]
fn the_catalogue_alone_needs_no_preset_and_a_resolution_still_does() {
    let context = Context {
        confirmed: false,
        output: ds_cli_contract::Output::resolve(
            ds_cli_contract::output::Format::Json,
            false,
            true,
        ),
    };
    let alone = run(
        &ds_cli_contract::args::parse(&COMMAND, &["--catalog".into()]).unwrap(),
        &context,
    )
    .unwrap();
    let owner: Value =
        serde_json::from_str(ds_network_contracts::lv::PROCESSOR_PRESETS_JSON).unwrap();
    assert_eq!(alone, json!({ "catalog": owner }));
    let refused = run(
        &ds_cli_contract::args::parse(&COMMAND, &[]).unwrap(),
        &context,
    )
    .unwrap_err();
    assert_eq!(refused.code(), "process_settings_invalid");
    assert!(refused.to_string().contains("--preset"), "{refused}");
}
