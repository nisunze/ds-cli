//! Protocol-only hostile Grid operation fixture; contains no model algorithm.
//! The executable's file stem selects one deliberate protocol fault.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let exe = env::current_exe().unwrap();
    let mode = exe.file_stem().unwrap().to_str().unwrap().to_owned();
    let args: Vec<_> = env::args_os().skip(1).collect();
    let val = |flag: &str| {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].clone())
    };
    let workers = val("--workers")
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let caps = if mode == "missing-capability" {
        vec!["lv_processing_v1"]
    } else {
        vec!["lv_processing_v1", "grid_file_operations_v1"]
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
        "an older engine must refuse before any task"
    );
    assert_eq!(args[0], "grid-operation");
    let request_bytes = fs::read(val("--request").unwrap()).unwrap();
    let request: Value = serde_json::from_slice(&request_bytes).unwrap();
    // Staged inputs are exactly the declared files.
    if let Some(dir) = val("--inputs") {
        for input in request["inputs"].as_array().unwrap() {
            let bytes = fs::read(Path::new(&dir).join(input["file"].as_str().unwrap())).unwrap();
            assert_eq!(sha(&bytes), input["sha256"].as_str().unwrap());
        }
    }
    if mode == "refusal" {
        println!(
            "{}",
            json!({"schema":"ds.network.task-refusal/v1","code":"structure_unknown","error":"no structure `x` in this revision","remedy":"use a structure id","invalidInput":true,
                "detail":{"schema":"ds.grid.native-operation-refusal/v1","class":"invalid","next":["ds dsgrid report structures"],"detail":{"structures":3}},
                "engineIdentity":identity})
        );
        std::process::exit(1);
    }
    let out = val("--out").unwrap();
    let out = Path::new(&out);
    let model = b"complete opaque operation model".to_vec();
    let mut artifact_bytes = model.clone();
    let mut name = "model.dsgrid".to_owned();
    if mode == "bad-name" {
        name = "..".into();
    }
    if mode != "missing-artifact" && mode != "bad-name" {
        if mode == "artifact-digest" {
            artifact_bytes.push(b'!');
        }
        fs::write(out.join(&name), &artifact_bytes).unwrap();
    }
    if mode == "extra-file" {
        fs::write(out.join("stray.json"), b"{}").unwrap();
    }
    let mut operation = request["call"]["operation"].clone();
    if mode == "wrong-operation" {
        operation = json!("report_staking");
    }
    let mut package = request["package_sha256"].clone();
    if mode == "wrong-package" {
        package = json!("0".repeat(64));
    }
    let result = json!({
        "schema":"ds.grid.native-operation-result/v1",
        "operation": operation,
        "engine_version":"fixture-engine/1",
        "package_sha256": package,
        "result": {"echo": request["call"]},
        "artifacts":[{"file":name,"byteCount":model.len(),"sha256":sha(&model),"schema":"ds.grid/v5"}],
    });
    let bytes = serde_json::to_vec(&result).unwrap();
    fs::write(out.join("result.json"), &bytes).unwrap();
    let mut input_sha = sha(&request_bytes);
    if mode == "receipt-input" {
        input_sha = "0".repeat(64);
    }
    println!(
        "{}",
        json!({"schema":"ds.network.task-result/v1","outcome":"completed","inputSha256":input_sha,"engineIdentity":identity,
            "artifact":{"file":"result.json","byteCount":bytes.len(),"sha256":sha(&bytes),"schema":"ds.grid.native-operation-result/v1"}})
    );
}
