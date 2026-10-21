use super::asset_read_bound::{READ_BOUND_BYTES, refusal};

#[test]
fn the_client_cores_read_bound_sentence_becomes_asset_too_large() {
    let failure = refusal(
        "the asset is above the 32 MiB read bound; open it from its own surface",
    )
    .expect("the bound sentence is recognised");
    assert_eq!(failure.code(), "asset_too_large");
    assert_eq!(failure.class(), ds_cli_contract::ExitClass::InvalidInput);
    assert_eq!(
        failure.detail_value().unwrap()["read_bound_bytes"],
        READ_BOUND_BYTES
    );
    assert_eq!(READ_BOUND_BYTES, 33_554_432);
}

#[test]
fn any_other_invalid_input_keeps_its_own_mapping() {
    assert!(refusal("the project id is not valid").is_none());
    assert!(refusal("").is_none());
}
