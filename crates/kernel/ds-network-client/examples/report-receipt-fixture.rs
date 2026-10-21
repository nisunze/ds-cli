//! Adversarial report transport fixture, deliberately without engineering.
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
    let width = args
        .windows(2)
        .find(|a| a[0] == "--workers")
        .and_then(|a| a[1].to_str())
        .unwrap();
    assert_eq!(width, "1");
    let identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":1,"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":1,"internalWorkerThreads":1},"capabilities":["lv_processing_v1","report_engineering_v1"]});
    let counter = exe.parent().unwrap().join("calls");
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(counter)
        .unwrap();
    writeln!(log, "{}", args[0].to_str().unwrap()).unwrap();
    if args[0] == "build-info" {
        println!(
            "{}",
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
        );
        return;
    }
    assert_eq!(args[0], "report-engineering");
    let input = fs::read(&args[2]).unwrap();
    let request: Value = serde_json::from_slice(&input).unwrap();
    let mut demands: Vec<_> = request["demands"]
        .as_array()
        .unwrap()
        .iter()
        .map(
            |d| json!({"transformer":d["transformer"],"demand":{"status":"calculated","kva":12.5}}),
        )
        .collect();
    if mode == "report-partial" {
        demands.pop();
    }
    let output = serde_json::to_vec(&json!({"schema":"ds.network.report-engineering.result/v1","findings":[],"demands":demands})).unwrap();
    let out = Path::new(&args[4]);
    fs::write(out.join("engineering.json"), &output).unwrap();
    if mode == "report-extra" {
        fs::write(out.join("unrequested.json"), b"{}").unwrap();
    }
    let artifact = json!({"file":"engineering.json","byteCount":output.len(),"sha256":if mode=="report-digest"{"0".repeat(64)}else{sha(&output)},"schema":"ds.network.report-engineering.result/v1"});
    println!(
        "{}",
        json!({"schema":"ds.network.task-result/v1","outcome":"completed","inputSha256":if mode=="report-input"{"0".repeat(64)}else{sha(&input)},"engineIdentity":identity,"artifact":artifact})
    );
}
