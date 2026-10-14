//! Adversarial artifact transport only: this fixture performs no Grid computation.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let executable = env::current_exe().unwrap();
    let mode = executable.file_stem().unwrap().to_str().unwrap();
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    let command = arguments[0].to_str().unwrap();
    let value = |flag: &str| arguments.windows(2).find(|pair| pair[0] == flag).unwrap()[1].clone();
    assert_eq!(value("--workers"), "1");
    let mut capabilities = vec!["lv_processing_v1"];
    if mode != "missing-capability" {
        capabilities.extend(["grid_query_v1", "grid_manifest_v1", "grid_publication_v1"]);
    }
    let identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":1,"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":1,"internalWorkerThreads":1},"capabilities":capabilities});
    if command == "build-info" {
        println!(
            "{}",
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
        );
        return;
    }
    if mode == "missing-capability" {
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
    let model_path = value("--model");
    let model = fs::read(&model_path).unwrap();
    let request_bytes = if command == "grid-manifest" {
        None
    } else {
        Some(fs::read(value("--request")).unwrap())
    };
    let request: Value = request_bytes
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes).unwrap())
        .unwrap_or(Value::Null);
    let source = json!({"package_sha256":if mode=="wrong-package" {"0".repeat(64)} else {sha(&model)}, "model_id":"fixture-model", "expected_revision":format!("rev:{}", if mode=="wrong-revision" {"0"} else {"a"}.repeat(64)), "engineering_input_root":format!("fnv1a64:{}", "b".repeat(16))});
    let (name, schema, result) = match command {
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
        fs::write(&model_path, b"changed package").unwrap();
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
