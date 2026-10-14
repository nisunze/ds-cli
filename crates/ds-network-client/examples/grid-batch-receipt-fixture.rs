//! Protocol-only hostile Grid batch fixture; contains no model algorithm.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let exe = env::current_exe().unwrap();
    let mode = exe.file_stem().unwrap().to_str().unwrap();
    let args: Vec<_> = env::args_os().skip(1).collect();
    let val = |flag: &str| args.windows(2).find(|p| p[0] == flag).unwrap()[1].clone();
    let workers = val("--workers").to_str().unwrap().parse::<usize>().unwrap();
    let caps = if mode == "missing-capability" {
        vec!["lv_processing_v1"]
    } else {
        vec!["lv_processing_v1", "grid_apply_batch_v1"]
    };
    let identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":1,"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":workers,"internalWorkerThreads":workers},"capabilities":caps});
    if args[0] == "build-info" {
        println!(
            "{}",
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
        );
        return;
    }
    assert_ne!(
        mode, "missing-capability",
        "old capability must refuse before task"
    );
    assert_eq!(args[0], "grid-apply-batch");
    let bytes = fs::read(val("--request")).unwrap();
    let request: Value = serde_json::from_slice(&bytes).unwrap();
    let out = val("--out");
    let out = Path::new(&out);
    let payload = b"complete opaque model";
    let mut source = request["source"].clone();
    if mode == "wrong-source" {
        source["model_id"] = json!("other");
    }
    let dry = request["dry_run"].as_bool().unwrap();
    let mut operations = std::collections::BTreeMap::<String, usize>::new();
    for row in request["commands"].as_array().unwrap() {
        *operations
            .entry(row["command"]["command_kind"].as_str().unwrap().into())
            .or_default() += 1;
    }
    let mut ids: Vec<_> = request["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["command_id"].clone())
        .collect();
    if mode == "reversed-ids" {
        ids.reverse();
    }
    let mut projection = json!({"source":{"model_id":source["model_id"],"package_revision":0,"authored_revision":source["expected_revision"]},"selected_command_ids":ids,"commands":request["commands"].as_array().unwrap().len(),"operations":operations,
        "batch_sha256":request["batch_sha256"],"source_sha256":format!("sha256:{}",source["package_sha256"].as_str().unwrap()),
        "parent_revision":source["expected_revision"],"would_apply":true,"idempotent_replay":false,"package_revision_increment":1,
        "dry_run":dry,"persisted":false,"resulting_revision":format!("rev:{}","c".repeat(64))});
    if mode == "wrong-digest" {
        projection["batch_sha256"] = json!(format!("sha256:{}", "0".repeat(64)));
    }
    if let Some(guard) = request["guard"].as_object() {
        let mut projected = guard["document"].as_object().unwrap().clone();
        projected.remove("source_sha256");
        projected.insert("sha256".into(), guard["sha256"].clone());
        projection["correction_guard"] = json!(projected);
        projection["selected_reviews"]=json!(request["commands"].as_array().unwrap().iter().map(|row|json!({"command_id":row["command_id"],"review_ref":row["review_ref"],"command_kind":row["command"]["command_kind"]})).collect::<Vec<_>>());
        projection["changes"] = json!({"tables":[],"added":0,"removed":0,"changed":0});
        if mode == "wrong-guard" {
            projection["correction_guard"]["sha256"] = json!(format!("sha256:{}", "0".repeat(64)));
        }
    }
    let mut artifacts = Vec::new();
    if !dry || mode == "preview-model" {
        let target = out.join("model.dsgrid");
        if mode != "missing-model" {
            #[cfg(unix)]
            if mode == "symlink-model" {
                let other = out.parent().unwrap().join("untrusted.dsgrid");
                fs::write(&other, payload).unwrap();
                std::os::unix::fs::symlink(other, target).unwrap();
            } else {
                fs::write(target, payload).unwrap();
            }
            #[cfg(not(unix))]
            fs::write(target, payload).unwrap();
        }
        artifacts.push(json!({"file":"model.dsgrid","schema":"ds.grid/v5","byteCount":payload.len(),"sha256":if mode=="model-digest"{"0".repeat(64)}else{sha(payload)}}));
        projection["artifact"] = json!({"package_revision":1,"byte_len":payload.len(),"sha256":format!("sha256:{}",sha(payload))});
    }
    if mode == "extra-file" {
        fs::write(out.join("unexpected.bin"), b"extra").unwrap();
    }
    let result = json!({"schema":"ds.grid.native-apply-batch-result/v1","source":source,"engine_version":"fixture-grid/1","result_revision":format!("rev:{}","c".repeat(64)),"result_engineering_input_root":format!("fnv1a64:{}","b".repeat(16)),"model_schema":5,"projection":projection,"journal":{"history":{},"entries":[]},"artifacts":artifacts});
    let encoded = serde_json::to_vec(&result).unwrap();
    fs::write(out.join("result.json"), &encoded).unwrap();
    println!(
        "{}",
        json!({"schema":"ds.network.task-result/v1","outcome":"completed","inputSha256":sha(&bytes),"artifact":{"file":"result.json","schema":"ds.grid.native-apply-batch-result/v1","byteCount":encoded.len(),"sha256":sha(&encoded)},"engineIdentity":identity})
    );
}
