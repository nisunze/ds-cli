//! Real captured executable; baselines come from the previous public owner.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::grid_create::*;
use std::{env, fs, path::PathBuf};

#[test]
#[ignore = "requires exact native engine and captured installed create baselines"]
fn acceptance_blank_create_preserves_bytes_context_and_staging() {
    let baseline = PathBuf::from(env::var_os("DS_NETWORK_CREATE_BASELINE").expect("baseline"));
    let scratch = baseline.join("client-scratch");
    fs::create_dir_all(&scratch).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    for (name, id, crs) in [
        ("default", None, None),
        ("named", Some("native-mv"), Some("EPSG:32735")),
    ] {
        let request = NativeGridCreateRequestV1 {
            schema: GRID_CREATE_REQUEST_SCHEMA_V1.into(),
            model_id: id.map(str::to_owned),
            coordinate_system: crs.map(str::to_owned),
            standards_sha256: None,
        };
        let expected: serde_json::Value = serde_json::from_slice(
            &fs::read(baseline.join(format!("baseline-{name}.json"))).unwrap(),
        )
        .unwrap();
        let outcome = captured
            .grid_create(&request, None, &scratch, &|| false)
            .unwrap();
        assert_eq!(
            outcome.model,
            fs::read(baseline.join(format!("baseline-{name}.dsgrid"))).unwrap()
        );
        let result = &outcome.metadata.result;
        assert_eq!(result.model_id, expected["data"]["model_id"]);
        assert_eq!(
            result.coordinate_system,
            expected["data"]["coordinate_system"]
        );
        assert_eq!(
            result.authored_revision,
            expected["data"]["authored_revision"]
        );
        assert_eq!(
            result.snapshot_fingerprint,
            expected["data"]["snapshot_fingerprint"]
        );
        assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    }
    let mut request = NativeGridCreateRequestV1 {
        schema: GRID_CREATE_REQUEST_SCHEMA_V1.into(),
        model_id: None,
        coordinate_system: Some("EPSG:4326".into()),
        standards_sha256: None,
    };
    let error = captured
        .grid_create(&request, None, &scratch, &|| false)
        .unwrap_err();
    assert_eq!(error.owner_code(), Some("unsupported_coordinate_system"));
    assert_eq!(error.owner_invalid_input, Some(true));
    request.coordinate_system = None;
    request.standards_sha256 = Some("0".repeat(64));
    assert_eq!(
        captured
            .grid_create(&request, Some(b"template"), &scratch, &|| false)
            .unwrap_err()
            .code(),
        "grid_create_request_invalid"
    );
    request.standards_sha256 = None;
    assert!(captured
        .grid_create(&request, None, &scratch, &|| true)
        .is_err());
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}
