//! Explicit execution against an independently selected Network executable.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::grid_catalog::*;
use std::{env, fs, path::PathBuf, process::Command};

#[test]
#[ignore = "requires exact native owner and installed CLI catalog baselines"]
fn acceptance_catalog_owner_matches_installed_projections() {
    let engine = env::var_os("DS_NETWORK_CATALOG_ENGINE").expect("native owner executable");
    let result = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "isolated_catalog_owner",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
#[ignore = "isolated executable selection"]
fn isolated_catalog_owner() {
    let baseline =
        PathBuf::from(env::var_os("DS_NETWORK_CATALOG_BASELINE").expect("installed baseline"));
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    for kind in [
        GridCatalogKind::Commands,
        GridCatalogKind::Operations,
        GridCatalogKind::Projections,
        GridCatalogKind::Types,
    ] {
        let name = kind.as_str();
        for id in [
            None,
            Some(fs::read_to_string(baseline.join(format!("{name}-id.txt"))).unwrap()),
        ] {
            let request = NativeGridCatalogRequestV1 {
                schema: GRID_CATALOG_REQUEST_SCHEMA_V1.into(),
                kind,
                id,
            };
            let suffix = if request.id.is_some() {
                "-descriptor"
            } else {
                ""
            };
            let expected: serde_json::Value = serde_json::from_slice(
                &fs::read(baseline.join(format!("{name}{suffix}.json"))).unwrap(),
            )
            .unwrap();
            let outcome = captured
                .grid_catalog(&request, scratch.path(), &|| false)
                .unwrap();
            assert_eq!(outcome.result.data, expected["data"]);
            assert_eq!(outcome.result.kind, kind);
        }
    }
    let request = NativeGridCatalogRequestV1 {
        schema: GRID_CATALOG_REQUEST_SCHEMA_V1.into(),
        kind: GridCatalogKind::Operations,
        id: Some("unknown".into()),
    };
    let error = captured
        .grid_catalog(&request, scratch.path(), &|| false)
        .unwrap_err();
    assert_eq!(error.owner_code(), Some("unknown_descriptor"));
    assert_eq!(error.owner_invalid_input, Some(true));
    assert_eq!(
        error.owner_detail.as_ref().unwrap()["ids"]
            .as_array()
            .unwrap()
            .len(),
        40
    );
    assert!(error
        .owner_remedy
        .as_ref()
        .unwrap()
        .contains("dsgrid describe"));
    assert!(captured
        .grid_catalog(&request, scratch.path(), &|| true)
        .is_err());
    assert_eq!(
        fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "catalog reads retain no model or task outputs"
    );
}
