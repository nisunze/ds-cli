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
    let owner = shipped_catalogue();
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
    let owner = shipped_catalogue();
    assert_eq!(alone, json!({ "catalog": owner }));
    let refused = run(
        &ds_cli_contract::args::parse(&COMMAND, &[]).unwrap(),
        &context,
    )
    .unwrap_err();
    assert_eq!(refused.code(), "process_settings_invalid");
    assert!(refused.to_string().contains("--preset"), "{refused}");
}

/// The catalogue the Network engine ships, read from its checkout as the test
/// oracle: Core compiles no copy and answers what the installed engine says.
fn shipped_catalogue() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../ds-network/crates/ds-network-contracts/src/processor_presets.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn a_preset_the_installed_engine_does_not_offer_is_refused_by_name() {
    let context = Context {
        confirmed: false,
        output: ds_cli_contract::Output::resolve(
            ds_cli_contract::output::Format::Json,
            false,
            true,
        ),
    };
    let mut older = shipped_catalogue();
    older["presets"]
        .as_array_mut()
        .unwrap()
        .retain(|preset| preset["name"] != "sketch");
    let offer =
        ds_network_contracts::lv_processor_catalog::LvProcessorOffer::from_catalog(&older).unwrap();
    let refused = crate::lv::catalogue::admit_preset(&offer, "sketch").unwrap_err();
    assert_eq!(refused.code(), "lv_process_preset_not_offered");
    assert!(refused.to_string().contains("drafting"), "{refused}");
    assert!(COMMAND.refusals.iter().any(|r| r.code == refused.code()));
    // The installed engine offers both declared presets.
    for preset in ["drafting", "sketch"] {
        run(
            &ds_cli_contract::args::parse(&COMMAND, &["--preset".into(), preset.into()]).unwrap(),
            &context,
        )
        .unwrap();
    }
}
