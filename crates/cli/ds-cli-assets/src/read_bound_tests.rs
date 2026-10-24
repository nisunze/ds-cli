use super::*;

fn bound_refusal() -> Failure {
    Failure::invalid("asset_too_large", "the asset is above the bound")
        .detail(json!({ "read_bound_bytes": 33_554_432u64, "asset_bytes": null }))
}

#[test]
fn only_the_native_clients_read_bound_is_recognised() {
    assert!(is_read_bound(&bound_refusal()));
    // The kernel's own preview bound shares the code but carries no read bound.
    assert!(!is_read_bound(&Failure::invalid(
        "asset_too_large",
        "preview bound"
    )));
    assert!(!is_read_bound(&Failure::invalid("auth_input_invalid", "x")));
}

#[test]
fn a_catalogued_asset_names_its_size_and_the_streaming_command() {
    let failure = read_bound_failure("p1", "a_7kq3nr2v0b1c", 33_554_432, Some(50_000_000));
    assert_eq!(failure.code(), "asset_too_large");
    assert!(failure.message().contains("50000000 bytes"));
    let detail = failure.detail_value().unwrap();
    assert_eq!(detail["read_bound_bytes"], 33_554_432u64);
    assert_eq!(detail["asset_bytes"], 50_000_000u64);
    assert_eq!(
        failure.next_commands(),
        ["ds assets download --project p1 --asset a_7kq3nr2v0b1c"]
    );
    assert!(
        failure
            .remedy_text()
            .unwrap()
            .contains("ds assets download")
    );
}

#[test]
fn a_work_attachment_names_its_streaming_download_reference() {
    let failure = read_bound_failure("p1", "sys:pm_attachment:abc", 33_554_432, Some(300_000_000));
    assert_eq!(failure.code(), "asset_too_large");
    assert!(failure.message().contains("300000000 bytes"));
    assert_eq!(
        failure.next_commands(),
        ["ds assets download --project p1 --asset sys:pm_attachment:abc"]
    );
    assert!(
        failure
            .remedy_text()
            .unwrap()
            .contains("ds assets download")
    );
}
