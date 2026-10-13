//! The declared paired captured-page door, against one isolated loopback fixture.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

struct Pair {
    dir: tempfile::TempDir,
    descriptor: PathBuf,
    capture: PathBuf,
    calls: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Pair {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let descriptor = dir.path().join("paired.json");
        let capture = dir.path().join("capture.json");
        std::fs::write(&descriptor, json!({"version":1,"url":format!("http://{}",listener.local_addr().unwrap()),
            "token":"f".repeat(48),"pid":std::process::id(),"instance_id":"a".repeat(32),"lane":"local"}).to_string()).unwrap();
        std::fs::write(
            &capture,
            json!({"schema":"ds.print-layout-export/v1","render":{"layout":{"id":"saved-cover"}}})
                .to_string(),
        )
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, ended) = (calls.clone(), stop.clone());
        let worker = thread::spawn(move || {
            while !ended.load(Ordering::SeqCst) {
                let (stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let invoke = line.starts_with("POST /v1/invoke ");
                assert!(
                    invoke || line.starts_with("GET /v1/session "),
                    "fixed fixture route"
                );
                let mut length = 0;
                loop {
                    line.clear();
                    assert_ne!(reader.read_line(&mut line).unwrap(), 0, "fixture header");
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                let answer = if invoke {
                    let mut bytes = vec![0; length]; reader.read_exact(&mut bytes).unwrap();
                    seen.lock().unwrap().push(serde_json::from_slice(&bytes).unwrap());
                    // A typed file manifest is still mandatory after transport succeeds.
                    json!({"fixture":"missing-native-file-manifest"})
                } else {
                    json!({"uid":"window-owner","lane":"local","credential_audience_sha256":"b".repeat(64),
                        "project":"project","session_revision":3})
                }.to_string();
                write!(reader.get_mut(),"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",answer.len(),answer).unwrap();
            }
        });
        Self {
            dir,
            descriptor,
            capture,
            calls,
            stop,
            worker: Some(worker),
        }
    }
    fn args(&self) -> Vec<String> {
        vec![
            "--request".into(),
            self.capture.display().to_string(),
            "--preview".into(),
            "--project".into(),
            "project".into(),
            "--target".into(),
            "desktop".into(),
            "--desktop-descriptor".into(),
            self.descriptor.display().to_string(),
        ]
    }
}
impl Drop for Pair {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn explicit_desktop_uses_published_authority_without_reading_an_unused_native_profile() {
    let pair = Pair::new();
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["report", "layout", "render"])
        .args(pair.args())
        .args(["--lane", "canary", "--output", "json"])
        .env(
            "DS_NATIVE_CLIENT_CATALOG",
            pair.dir.path().join("unused-native-profile.json"),
        )
        .env("DS_NATIVE_CLIENT_CATALOG_SHA256", "f".repeat(64))
        .env_remove("DS_TARGET")
        .output()
        .unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["error"]["code"], "desktop_unreadable", "{answer}");
    let calls = pair.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["operation"], "printing.layout.render");
    assert_eq!(calls[0]["identity_fence"]["uid"], "window-owner");
    assert_eq!(calls[0]["identity_fence"]["lane"], "local");
    assert_eq!(calls[0]["arguments"]["project"], "project");
    assert_eq!(calls[0]["arguments"].as_object().unwrap().len(), 2);
    assert_eq!(
        calls[0]["arguments"]["request"]["schema"],
        "ds.print-layout-export/v1"
    );
    assert!(!pair.dir.path().join("unused-native-profile.json").exists());
}

#[test]
fn an_existing_caller_requirement_is_preserved_and_mismatches_send_zero_operations() {
    use ds_cli_desktop::ops;
    let pair = Pair::new();
    let inputs =
        ds_cli_contract::args::parse(&ds_cli_report::layout::RENDER, &pair.args()).unwrap();
    for field in ["uid", "lane", "audience", "project"] {
        let mut identity = ops::HeadlessIdentity {
            uid: "window-owner".into(),
            lane: "local".into(),
            credential_audience_sha256: "b".repeat(64),
            project: Some("project".into()),
            command_authority: ds_cli_contract::spec::Authority::Project,
        };
        match field {
            "uid" => identity.uid = "other-owner".into(),
            "lane" => identity.lane = "canary".into(),
            "audience" => identity.credential_audience_sha256 = "c".repeat(64),
            _ => identity.project = Some("other-project".into()),
        }
        let _required = ops::scope_headless_identity(Some(identity));
        let _target = ops::scope_target(Some("desktop".into()));
        let before = ops::scoped_requirement();
        let error = ds_cli_report::layout::render(
            &inputs,
            &ds_cli_contract::Context {
                confirmed: false,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::output::Format::Json,
                    false,
                    true,
                ),
            },
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            if field == "project" {
                "desktop_project_not_open"
            } else {
                "auth_context_mismatch"
            }
        );
        assert_eq!(ops::scoped_requirement(), before);
        assert!(pair.calls.lock().unwrap().is_empty());
    }
}
