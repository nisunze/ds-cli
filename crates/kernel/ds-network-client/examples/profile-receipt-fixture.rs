//! Adversarial retained transport fixture; contains no Network algorithms.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::Path,
    time::Duration,
};
fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn idle() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
fn main() {
    let exe = env::current_exe().unwrap();
    let mode = exe.file_stem().unwrap().to_str().unwrap();
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args[0] == "fixture-descendant" {
        idle();
    }
    let mut log = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(exe.with_extension("calls"))
        .unwrap();
    writeln!(log, "{}", args[0].to_str().unwrap()).unwrap();
    let identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":if mode=="profile-incompatible"{2}else{1},"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":1,"internalWorkerThreads":1},"capabilities":if mode=="profile-capability"{json!(["lv_processing_v1"])}else{json!(["lv_processing_v1","profile_session_v1"])}});
    if args[0] == "build-info" {
        println!(
            "{}",
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
        );
        return;
    }
    assert_eq!(args[0], "profile-session");
    fs::write(exe.with_extension("pid"), std::process::id().to_string()).unwrap();
    if mode == "profile-open-hung" {
        idle();
    }
    if mode == "profile-open-flood" {
        println!("{}", "x".repeat(64 * 1024));
        idle();
    }
    let input = fs::read(&args[2]).unwrap();
    let open: Value = serde_json::from_slice(&input).unwrap();
    let seed: Value = serde_json::from_str(open["seed_json"].as_str().unwrap()).unwrap();
    let history = seed["history"].clone();
    let model = seed["model_id"].clone();
    let emit = |sequence: u64, input: &[u8], artifact: Value, closed: bool| {
        let mut id = identity.clone();
        if sequence > 0 && mode == "profile-identity" {
            id["coreVersion"] = json!("other");
        }
        println!(
            "{}",
            json!({"schema":"ds.network.profile-reply/v1","sequence":if sequence>0&&mode=="profile-sequence"{sequence+1}else{sequence},"model_id":if sequence>0&&mode=="profile-model"{json!("other")}else{model.clone()},"history":if sequence>0&&mode=="profile-history"{json!({"initial_revision":"other"})}else{history.clone()},"engine_identity":id,"input_sha256":if sequence>0&&mode=="profile-input"{"0".repeat(64)}else{sha(input)},"artifact":artifact,"code":null,"error":null,"detail":null,"closed":closed})
        );
        io::stdout().flush().unwrap();
    };
    emit(0, &input, Value::Null, false);
    if mode == "profile-read-hung" {
        idle();
    }
    if mode == "profile-stderr-flood" {
        io::stderr()
            .write_all(&vec![b'x'; 4 * 1024 * 1024 + 1])
            .unwrap();
        idle();
    }
    if mode == "profile-early-exit" {
        std::process::exit(1);
    }
    for line in io::stdin().lock().lines() {
        let bytes = line.unwrap();
        let request: Value = serde_json::from_str(&bytes).unwrap();
        if request["schema"] == "ds.network.control/v1" {
            writeln!(log, "cancel").unwrap();
            return;
        }
        writeln!(
            log,
            "frame:{}",
            request["operation"]["action"].as_str().unwrap()
        )
        .unwrap();
        if mode == "profile-task-hung" || mode == "profile-task-cancel" {
            let mut child = std::process::Command::new(&exe)
                .arg("fixture-descendant")
                .spawn()
                .unwrap();
            fs::write(exe.with_extension("descendant"), child.id().to_string()).unwrap();
            let _ = child.wait();
            return;
        }
        let seq = request["sequence"].as_u64().unwrap();
        let close = request["operation"]["action"] == "close";
        let checkpoint = close || request["operation"]["action"] == "checkpoint";
        let value = if checkpoint {
            open.clone()
        } else {
            json!({"calculation_requests":0,"analysis_requests":0})
        };
        let output = serde_json::to_vec(&value).unwrap();
        let name = format!("profile-{seq:010}.json");
        let path = Path::new(&args[4]).join(&name);
        if mode == "profile-symlink" {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&args[2], &path).unwrap();
            #[cfg(not(unix))]
            fs::write(&path, &output).unwrap();
        } else {
            fs::write(&path, &output).unwrap();
        }
        if mode == "profile-extra" {
            fs::write(Path::new(&args[4]).join("extra.json"), b"{}").unwrap();
        }
        emit(
            seq,
            bytes.as_bytes(),
            json!({"file":if mode=="profile-name"{"../wrong.json"}else{&name},"schema":if checkpoint{"ds.network.profile-open/v1"}else{"ds.network.profile-result/v1"},"byteCount":output.len(),"sha256":if mode=="profile-digest"{"0".repeat(64)}else{sha(&output)}}),
            close,
        );
        if close {
            if mode == "profile-no-eof" {
                idle();
            }
            return;
        }
    }
}
