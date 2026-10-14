//! Adversarial artifact transport only: this fixture performs no Grid computation.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let executable = env::current_exe().unwrap();
    let mode = executable.file_stem().unwrap().to_str().unwrap();
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    let command = arguments[0].to_str().unwrap();
    let value = |flag: &str| arguments.windows(2).find(|pair| pair[0] == flag).unwrap()[1].clone();
    let workers = value("--workers")
        .to_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(workers > 0);
    let mut capabilities = vec!["lv_processing_v1"];
    if mode != "missing-capability" {
        capabilities.extend(["grid_query_v1", "grid_manifest_v1", "grid_publication_v1"]);
        if mode != "catalog-missing-capability" {
            capabilities.push("grid_catalog_v1");
        }
        if mode != "inspection-missing-capability" {
            capabilities.push("grid_inspect_v1");
        }
    }
    let identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":1,"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":workers,"internalWorkerThreads":workers},"capabilities":capabilities});
    if command == "build-info" {
        println!(
            "{}",
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
        );
        return;
    }
    if mode == "missing-capability"
        || mode == "inspection-missing-capability"
        || mode == "catalog-missing-capability"
    {
        panic!("missing capability must refuse without starting an operation");
    }
    if mode == "structured-refusal" || mode == "malformed-refusal" {
        println!(
            "{}",
            if mode == "malformed-refusal" {
                json!({"schema":"invalid","code":"bad"})
            } else {
                json!({"schema":"ds.network.task-refusal/v1","code":"precise_grid_refusal","error":"owner declined","detail":{"constraint":"retained"},"remedy":"use the exact owner parameters","invalidInput":false,"engineIdentity":identity})
            }
        );
        std::process::exit(1);
    }
    let model_path = if command == "grid-catalog" {
        None
    } else {
        Some(value("--model"))
    };
    let model = model_path
        .as_ref()
        .map(|path| fs::read(path).unwrap())
        .unwrap_or_default();
    let request_bytes = if command == "grid-manifest" || command == "grid-inspect" {
        None
    } else {
        Some(fs::read(value("--request")).unwrap())
    };
    let request: Value = request_bytes
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes).unwrap())
        .unwrap_or(Value::Null);
    if mode == "live-progress" || mode == "wrong-progress-input" {
        eprintln!(
            "{}",
            json!({"schema":"ds.network.grid-progress/v1",
            "input_sha256":if mode == "wrong-progress-input" {"0".repeat(64)} else {sha(request_bytes.as_ref().unwrap())},
            "progress":{"fixture":"before-terminal"},"elapsed_ms":0,"eta":null})
        );
        std::io::stderr().flush().unwrap();
        if mode == "live-progress" {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    let source = json!({"package_sha256":if mode=="wrong-package" {"0".repeat(64)} else {sha(&model)}, "model_id":"fixture-model", "expected_revision":format!("rev:{}", if mode=="wrong-revision" {"0"} else {"a"}.repeat(64)), "engineering_input_root":format!("fnv1a64:{}", "b".repeat(16))});
    let (name, schema, result) = match command {
        "grid-catalog" => {
            let kind = if mode == "catalog-wrong-kind" {
                json!("types")
            } else {
                request["kind"].clone()
            };
            let version = "fixture-grid/1";
            let data = if let Some(id) = request["id"].as_str() {
                json!({"engine":version,"kind":kind,"descriptor":{"operation_id":if mode=="catalog-wrong-descriptor" {"other"} else {id}}})
            } else {
                json!({"engine":if mode=="catalog-wrong-version" {"other"} else {version},"kind":kind,"entries":[]})
            };
            (
                "catalog.json",
                "ds.grid.native-catalog-result/v1",
                json!({"schema":"ds.grid.native-catalog-result/v1","kind":kind,"engine_version":version,"data":data}),
            )
        }
        "grid-inspect" => (
            "inspection.json",
            "ds.grid.native-inspection/v1",
            json!({"schema":"ds.grid.native-inspection/v1","source":source,"model_schema":5,"model_revision":0,
                "coordinate_system":"EPSG:32736","source_snapshot_fingerprint":"retained","opened_content_root":"retained",
                "table_counts":{},"content_summary":{"structure_type_names":[],"cable_names":[],"resource_leaves":[],
                "route_node_extent":null,"structure_extent":null,"terrain_extent":null}}),
        ),
        "grid-query" => (
            "query.json",
            "ds.grid.native-query-result/v1",
            json!({"schema":"ds.grid.native-query-result/v1","source":source,"model_revision":0,"engine_version":"fixture-grid/1","operation":{"operation_id":if mode=="wrong-operation" {json!("other")} else {request["operation_id"].clone()},"journaled":false},"result":{"complete":true}}),
        ),
        "grid-manifest" => (
            "manifest.json",
            "ds.grid.native-manifest/v1",
            json!({"schema":"ds.grid.native-manifest/v1","package_sha256":if mode=="manifest-wrong-source" {"0".repeat(64)} else {sha(&model)},"decoded":mode=="manifest-decoded","manifest":{"model":{"schema_version":5}},"members":["manifest.json"]}),
        ),
        "grid-publication" => (
            "publication.json",
            "ds.grid.native-publication-result/v1",
            json!({"schema":"ds.grid.native-publication-result/v1","source":source,"source_revision_id":format!("rev:{}",sha(&model)),"manifest_model_revision":0,"model_schema":5,"engine_version":"fixture-grid/1"}),
        ),
        _ => panic!("unknown fixed Grid fixture task"),
    };
    let bytes = serde_json::to_vec(&result).unwrap();
    let out = value("--out");
    let artifact = Path::new(&out).join(name);
    #[cfg(unix)]
    if mode == "symlink-result" {
        let target = Path::new(&out).join("untrusted.json");
        fs::write(&target, &bytes).unwrap();
        std::os::unix::fs::symlink(target, &artifact).unwrap();
    } else {
        fs::write(&artifact, &bytes).unwrap();
    }
    #[cfg(not(unix))]
    fs::write(&artifact, &bytes).unwrap();
    if mode == "changed-input" {
        if let Some(model_path) = &model_path {
            fs::write(model_path, b"changed package").unwrap();
        } else {
            fs::write(value("--request"), b"changed request").unwrap();
        }
    }
    println!(
        "{}",
        json!({
            "schema":"ds.network.task-result/v1", "inputSha256":if mode=="wrong-input" {"0".repeat(64)} else {sha(request_bytes.as_deref().unwrap_or(&model))},
            "engineIdentity":identity, "outcome":if mode=="cancelled" {"cancelled"} else {"completed"},
            "artifact":{"file":if mode=="wrong-file" {"other.json"} else {name},"schema":if mode=="wrong-receipt-schema" {"other/v1"} else {schema},"byteCount":bytes.len(),"sha256":if mode=="wrong-digest" {"0".repeat(64)} else {sha(&bytes)}}
        })
    );
}
