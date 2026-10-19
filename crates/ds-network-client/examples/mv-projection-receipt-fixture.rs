//! Adversarial process fixture; it never decodes or projects a Grid model.
use ds_network_contracts::{engine::*, mv_projection::*};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let executable = std::env::current_exe().unwrap();
    let mode = executable.file_stem().unwrap().to_str().unwrap();
    let identity = EngineIdentity {
        schema: "ds.network-engine/v1".into(),
        protocol_major: 1,
        protocol_minor: 0,
        core_version: "fixture".into(),
        source_sha: None,
        dependency_closure_sha256: None,
        environment: ExecutionEnvironment::Native,
        runtime: ExecutionRuntime {
            environment: ExecutionEnvironment::Native,
            logical_cpus: std::thread::available_parallelism().map_or(1, usize::from),
            internal_worker_threads: 1,
        },
        json_object_order: json_object_order(),
        capabilities: if mode == "missing-capability" {
            vec![NetworkCapability::LvProcessingV1]
        } else {
            vec![
                NetworkCapability::LvProcessingV1,
                NetworkCapability::MvProjectionV1,
            ]
        },
    };
    if args[0] == "build-info" {
        println!(
            "{}",
            serde_json::to_string(&NetworkBuildInfo {
                schema: "ds.engine-build/v1".into(),
                module: "network".into(),
                engine_identity: identity,
                build: None
            })
            .unwrap()
        );
        return;
    }
    assert_eq!(args[0], "mv-projection");
    let path = |flag: &str| -> PathBuf {
        args[args.iter().position(|s| s == flag).unwrap() + 1]
            .clone()
            .into()
    };
    let input = fs::read(path("--request")).unwrap();
    let request: MvProjectionRequestV1 = serde_json::from_slice(&input).unwrap();
    let model = fs::read(path("--model")).unwrap();
    let mut source: ds_network_contracts::grid::NativeGridSourcePin = serde_json::from_value(serde_json::json!({
        "package_sha256": digest(&model), "model_id": "fixture-model",
        "expected_revision": format!("rev:{}", "1".repeat(64)), "engineering_input_root": "fnv1a64:1111111111111111",
    })).unwrap();
    if mode == "wrong-source" {
        source.package_sha256 = "0".repeat(64);
    }
    let mut result = MvProjectionResultV1 {
        schema: MV_PROJECTION_RESULT_SCHEMA_V1.into(),
        source,
        json_object_order: request.json_object_order,
        model_revision: 7,
        source_crs: "EPSG:32735".into(),
        target_crs: "OGC:CRS84".into(),
        revision_id: request.revision_id,
        layers: request.layers,
        projection: serde_json::json!({"type":"FeatureCollection", "crs":"EPSG:32735", "features":[{"type":"Feature", "id":"s-fixture", "properties":{"_layer":"structures"}, "geometry":{"type":"Point", "coordinates":[30.,-2.,1200.]}}]}),
    };
    if mode == "wrong-filter" {
        result.layers = vec![MvProjectionLayer::Alignments];
    }
    if mode == "wrong-feature" {
        result.projection["features"][0]["properties"]["_layer"] = serde_json::json!("conductors");
    }
    if mode == "wrong-revision" {
        result.revision_id = "other-revision".into();
    }
    let bytes = serde_json::to_vec(&result).unwrap();
    fs::write(path("--out").join(MV_PROJECTION_ARTIFACT), &bytes).unwrap();
    if mode == "extra-artifact" {
        fs::write(path("--out").join("unexpected.json"), b"{}").unwrap();
    }
    let receipt = NetworkReceiptV1 {
        outcome: NetworkTaskOutcome::Completed,
        result_input_sha256: None,
        schema: "ds.network.task-result/v1".into(),
        input_sha256: digest(&input),
        engine_identity: identity,
        artifact: ArtifactReceipt {
            file: if mode == "escaped-artifact" {
                "../projection.json".into()
            } else {
                MV_PROJECTION_ARTIFACT.into()
            },
            byte_count: bytes.len(),
            sha256: if mode == "bad-digest" {
                "0".repeat(64)
            } else {
                digest(&bytes)
            },
            schema: MV_PROJECTION_RESULT_SCHEMA_V1.into(),
        },
    };
    println!("{}", serde_json::to_string(&receipt).unwrap());
}
