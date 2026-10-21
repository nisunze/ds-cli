//! Wire adversaries only; this executable contains no engineering algorithm.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::PathBuf};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let exe = env::current_exe().unwrap();
    let mode = exe.file_stem().unwrap().to_str().unwrap();
    let args: Vec<_> = env::args_os().skip(1).collect();
    let path = |name: &str| -> PathBuf {
        PathBuf::from(&args[args.iter().position(|arg| arg == name).unwrap() + 1])
    };
    let mut identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":1,"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":1,"internalWorkerThreads":1},"capabilities":["lv_processing_v1","lv_publication_v1","lv_analysis_validation_v1"]});
    if mode == "lv-save-oldcap" {
        identity["capabilities"] = json!(["lv_processing_v1", "lv_publication_v1"]);
    }
    let mut calls = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(exe.parent().unwrap().join("calls"))
        .unwrap();
    writeln!(calls, "{}", args[0].to_str().unwrap()).unwrap();
    if args[0] == "build-info" {
        println!(
            "{}",
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
        );
        return;
    }
    let input_path = path("--request");
    let input = fs::read(&input_path).unwrap();
    let request: Value = serde_json::from_slice(&input).unwrap();
    let out = path("--out");
    let result = if args[0] == "lv-project" {
        Some(fs::read(path("--result")).unwrap())
    } else {
        None
    };
    let (file, schema, mut value) = if let Some(result) = &result {
        let results: Value = serde_json::from_slice(result).unwrap();
        let mut source = request["jobs"][0].clone();
        if source.get("config_dfs").is_none() {
            source["config_dfs"] = json!({});
        }
        let output = &results["jobs"][0]["output"];
        (
            "publication.json",
            if request["schema"] == "ds.fast-lv.request/v1" {
                "ds.fast-lv.publication/v1"
            } else {
                "ds.lv.process.publication/v1"
            },
            json!({"process_input":source,"source_layers":source["gdfs"],"config_dfs":source["config_dfs"],"gdfs":output["gdfs"],"voltage_drop":output["voltage_drop"],"process_metadata":{"settings":output["settings_used"]}}),
        )
    } else {
        (
            "validation.json",
            "ds.lv.analysis-validation.result/v1",
            json!({"schema":"ds.lv.analysis-validation.result/v1","transformer_names":request["jobs"].as_array().unwrap().iter().map(|job|job["source"]["transformer_name"].clone()).collect::<Vec<_>>()}),
        )
    };
    if mode == "lv-save-source" {
        value["process_input"]["transformer_name"] = json!("different");
    }
    if mode == "lv-save-names" {
        value["transformer_names"] = json!(["different"]);
    }
    let bytes = serde_json::to_vec(&value).unwrap();
    let artifact_path = out.join(file);
    fs::write(&artifact_path, &bytes).unwrap();
    if mode == "lv-save-extra" {
        fs::write(out.join("extra.json"), b"{}").unwrap();
    }
    #[cfg(unix)]
    if mode == "lv-save-symlink" {
        fs::remove_file(&artifact_path).unwrap();
        std::os::unix::fs::symlink(&input_path, &artifact_path).unwrap();
    }
    if mode == "lv-save-input-changed" {
        fs::write(&input_path, b"{}").unwrap();
    }
    let mut receipt = json!({"schema":"ds.network.task-result/v1","inputSha256":sha(&input),"engineIdentity":identity,"outcome":"completed","artifact":{"file":file,"schema":schema,"byteCount":bytes.len(),"sha256":sha(&bytes)}});
    if let Some(result) = result {
        receipt["resultInputSha256"] = json!(if mode == "lv-save-retained" {
            "0".repeat(64)
        } else {
            sha(&result)
        });
    }
    if mode == "lv-save-digest" {
        receipt["artifact"]["sha256"] = json!("0".repeat(64));
    }
    if mode == "lv-save-identity" {
        receipt["engineIdentity"]["coreVersion"] = json!("different");
    }
    if mode == "lv-save-cancelled" {
        receipt["outcome"] = json!("cancelled");
    }
    println!("{receipt}");
}
