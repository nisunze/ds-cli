//! Native adversarial transport fixture; contains no DS engineering algorithm.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let exe = env::current_exe().unwrap();
    let mode = exe.file_stem().unwrap().to_str().unwrap();
    let args: Vec<_> = env::args_os().skip(1).collect();
    let command = args.first().and_then(|v| v.to_str()).unwrap_or("");
    let workers = args
        .windows(2)
        .find(|v| v[0] == "--workers")
        .and_then(|v| v[1].to_str())
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    assert_eq!(workers, 1);
    let identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":if mode=="incompatible"{2}else{1},"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":1,"internalWorkerThreads":1},"capabilities":["lv_processing_v1","lv_processor_catalog_v1"]});
    match command {
        "build-info" => {
            assert_eq!(args.len(), 3);
            let path = exe.parent().unwrap().join("probes");
            if path.is_file() {
                let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
                file.write_all(b"probe\n").unwrap();
            }
            println!(
                "{}",
                json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
            );
        }
        "lv-process" => {
            assert_eq!(args.len(), 7);
            assert_eq!(args[1], "--request");
            assert_eq!(args[3], "--out");
            assert_eq!(args[5], "--workers");
            let request = fs::read(&args[2]).unwrap();
            let value: Value = serde_json::from_slice(&request).unwrap();
            let mut jobs:Vec<Value>=value["jobs"].as_array().unwrap().iter().map(|j|json!({"transformer_name":j["transformer_name"],"ok":false,"error":"fixture refused","error_code":"fixture_refused"})).collect();
            if mode == "partial" {
                jobs.pop();
            }
            if mode == "wrong-order" {
                assert!(jobs.len() >= 2);
                jobs.swap(0, 1);
            }
            let result=serde_json::to_vec(&json!({"schema":"ds.fast-lv.result/v1","engine_core_version":"fixture-core","jobs":jobs})).unwrap();
            let path = Path::new(&args[4]).join("result.json");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            file.write_all(&result).unwrap();
            drop(file);
            // Ignore stdin cancel and exit inside the supervisor grace period.
            if mode == "ignored-cancel" {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            match mode {
                "no-terminal" => {}
                "malformed" => println!(
                    "{}",
                    json!({"schema":"ds.network.task-result/v1","unexpected":true})
                ),
                _ => println!(
                    "{}",
                    json!({"schema":"ds.network.task-result/v1","inputSha256":sha(&request),"engineIdentity":identity,"artifact":{"file":"result.json","byteCount":result.len(),"sha256":if mode=="wrong-digest"{"0".repeat(64)}else{sha(&result)},"schema":"ds.fast-lv.result/v1"},"outcome":"completed"})
                ),
            }
        }
        // The LV processor catalogue Core admits every request against: the
        // catalogue the Network engine ships, answered as its grid operation.
        "grid-operation" => {
            assert_eq!(args.len(), 7);
            let request = fs::read(&args[2]).unwrap();
            let call: Value = serde_json::from_slice(&request).unwrap();
            assert_eq!(call["call"]["operation"], "lv_processor_catalog");
            // Fixture snapshot of ds-network-contracts/src/processor_presets.json;
            // this test addon must build without the producer's implementation tree.
            let catalog: Value =
                serde_json::from_str(include_str!("fixtures/processor_presets.json")).unwrap();
            let result = serde_json::to_vec(&json!({"schema":"ds.grid.native-operation-result/v1","operation":"lv_processor_catalog","engine_version":"fixture-core","package_sha256":null,"result":{"schema":"ds.lv.processor-catalog/v1","catalog":catalog},"artifacts":[]})).unwrap();
            fs::write(Path::new(&args[4]).join("result.json"), &result).unwrap();
            println!(
                "{}",
                json!({"schema":"ds.network.task-result/v1","inputSha256":sha(&request),"engineIdentity":identity,"artifact":{"file":"result.json","byteCount":result.len(),"sha256":sha(&result),"schema":"ds.grid.native-operation-result/v1"},"outcome":"completed"})
            );
        }
        _ => panic!("fixture accepts three fixed tasks"),
    }
}
