//! Product-facing Solar bridge smoke tests.
//!
//! These deliberately use a loopback descriptor rather than a real desktop:
//! the assertion is the closed bridge contract. A test that only checks help
//! would not catch a stale operation name or a silently renamed argument.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

struct Bridge {
    descriptor: PathBuf,
    server: JoinHandle<Vec<Value>>,
}

fn ds(args: &[&str]) -> (Value, i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let envelope = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("ds did not return JSON ({error}): {stdout}{stderr}"));
    (envelope, output.status.code().unwrap_or(-1), stdout, stderr)
}

fn bridge(replies: Vec<(&'static str, Value)>) -> Bridge {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback bridge");
    let address = listener.local_addr().expect("bridge address");
    let server = thread::spawn(move || {
        let mut received = Vec::new();
        for (expected_operation, reply) in replies {
            let (mut stream, _) = listener.accept().expect("bridge accepts request");
            let request = read_json_request(&mut stream);
            assert_eq!(
                request["operation"], expected_operation,
                "the CLI must call the closed operation it declares"
            );
            received.push(request);

            let reply = serde_json::to_vec(&reply).expect("serialize bridge reply");
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            );
            stream.write_all(headers.as_bytes()).expect("write headers");
            stream.write_all(&reply).expect("write body");
            stream.flush().expect("flush bridge reply");
        }
        received
    });

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let descriptor = std::env::temp_dir().join(format!(
        "ds-cli-solar-paired-{}-{unique}.json",
        std::process::id()
    ));
    let body = json!({
        "version": 1,
        "url": format!("http://{address}"),
        "token": "test-pairing-secret",
        "pid": 1,
    });
    std::fs::write(
        &descriptor,
        serde_json::to_vec(&body).expect("serialize descriptor"),
    )
    .expect("write descriptor");

    Bridge { descriptor, server }
}

fn finish(bridge: Bridge) -> Vec<Value> {
    let Bridge { descriptor, server } = bridge;
    let _ = std::fs::remove_file(descriptor);
    server.join().expect("bridge server did not panic")
}

fn read_json_request(stream: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).expect("read bridge request");
        assert!(count > 0, "bridge client closed before completing request");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break at;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).expect("request headers UTF-8");
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
        })
        .expect("bridge request has content length");
    let body_start = header_end + 4;
    while bytes.len() < body_start + content_length {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).expect("read bridge request body");
        assert!(
            count > 0,
            "bridge client closed before completing request body"
        );
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[body_start..body_start + content_length])
        .expect("bridge request is JSON")
}

#[test]
fn solar_prepare_calls_the_current_closed_operation() {
    let bridge = bridge(vec![(
        "solar.prepare",
        json!({
            "status": "prepared",
            "contexts": ["rw-kigali", "rw-butare"],
            "root": "solar",
        }),
    )]);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();

    let (envelope, code, stdout, stderr) = ds(&[
        "solar",
        "prepare",
        "--city",
        "rw-kigali",
        "--city",
        "rw-butare",
        "--overwrite",
        "--language",
        "fr",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["command"], "solar.prepare");
    assert_eq!(envelope["data"]["status"], "prepared");

    let requests = finish(bridge);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["arguments"],
        json!({
            "contexts": ["rw-kigali", "rw-butare"],
            "overwrite": true,
            "language": "fr",
        })
    );
    assert!(requests[0]["arguments"].get("reference_url").is_none());
    assert!(requests[0]["arguments"].get("cache_dir").is_none());
}

#[test]
fn solar_run_lifecycle_uses_closed_operations_and_preserves_arguments() {
    let bridge = bridge(vec![
        (
            "solar.run.start",
            json!({
                "status": "started",
                "run_id": "solar-run-123",
                "contexts": ["rw-kigali", "rw-butare"],
                "placement": "paired",
            }),
        ),
        (
            "solar.run.progress",
            json!({ "status": "running", "run_id": "solar-run-123" }),
        ),
        (
            "solar.run.result",
            json!({ "status": "succeeded", "run_id": "solar-run-123" }),
        ),
        (
            "solar.run.cancel",
            json!({ "status": "cancel_requested", "run_id": "solar-run-123" }),
        ),
        (
            "solar.result.read",
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "context": "rw-kigali",
                "result_digest": "sha256:abc",
                "value": { "annual": 42 },
            }),
        ),
    ]);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();

    let calls: &[(&[&str], &str)] = &[
        (
            &[
                "solar",
                "run",
                "start",
                "--city",
                "rw-kigali",
                "--city",
                "rw-butare",
                "--no-charts",
                "--concurrency",
                "2",
                "--serial",
            ],
            "solar.run.start",
        ),
        (
            &["solar", "run", "progress", "--run-id", "solar-run-123"],
            "solar.run.progress",
        ),
        (
            &["solar", "run", "result", "--run-id", "solar-run-123"],
            "solar.run.result",
        ),
        (
            &["solar", "run", "cancel", "--run-id", "solar-run-123"],
            "solar.run.cancel",
        ),
        (
            &[
                "solar",
                "result",
                "read",
                "--run-id",
                "solar-run-123",
                "--city",
                "rw-kigali",
                "--path",
                "annual",
                "--path",
                "losses",
            ],
            "solar.result.read",
        ),
    ];

    for (base, command) in calls {
        let mut args = base.to_vec();
        args.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
        let (envelope, code, stdout, stderr) = ds(&args);
        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(envelope["command"], *command);
    }

    let requests = finish(bridge);
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests[0]["arguments"],
        json!({
            "contexts": ["rw-kigali", "rw-butare"],
            "render_charts": false,
            "concurrency": 2,
            "serial": true,
        })
    );
    for request in &requests[1..4] {
        assert_eq!(request["arguments"], json!({ "run_id": "solar-run-123" }));
    }
    assert_eq!(
        requests[4]["arguments"],
        json!({
            "run_id": "solar-run-123",
            "context": "rw-kigali",
            "path": ["annual", "losses"],
        })
    );
}

#[test]
fn paired_solar_exports_page_sealed_reports_and_portfolios_to_new_files() {
    let bridge = bridge(vec![
        (
            "solar.document.read",
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "context": "rw-kigali",
                "document": "draft",
                "content": "draft",
                "next_offset": 5,
                "complete": false,
            }),
        ),
        (
            "solar.document.read",
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "context": "rw-kigali",
                "document": "draft",
                "content": " report",
                "next_offset": 12,
                "complete": true,
            }),
        ),
        (
            "solar.portfolio.read",
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "artifact": "result",
                "content": "{\"total\":1}",
                "next_offset": 11,
                "complete": true,
            }),
        ),
    ]);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let report_out = std::env::temp_dir().join(format!("ds-solar-report-{unique}.md"));
    let portfolio_out = std::env::temp_dir().join(format!("ds-solar-portfolio-{unique}.json"));
    let report_out_string = report_out.to_string_lossy().into_owned();
    let portfolio_out_string = portfolio_out.to_string_lossy().into_owned();

    let (report, code, stdout, stderr) = ds(&[
        "solar",
        "report",
        "export",
        "--run-id",
        "solar-run-123",
        "--city",
        "rw-kigali",
        "--variant",
        "draft",
        "--out",
        &report_out_string,
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(report["command"], "solar.report.export");
    assert_eq!(
        std::fs::read_to_string(&report_out).unwrap(),
        "draft report"
    );

    let (portfolio, code, stdout, stderr) = ds(&[
        "solar",
        "portfolio",
        "export",
        "--run-id",
        "solar-run-123",
        "--artifact",
        "result",
        "--out",
        &portfolio_out_string,
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(portfolio["command"], "solar.portfolio.export");
    assert_eq!(
        std::fs::read_to_string(&portfolio_out).unwrap(),
        "{\"total\":1}"
    );

    let requests = finish(bridge);
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0]["arguments"],
        json!({
            "run_id": "solar-run-123",
            "context": "rw-kigali",
            "document": "draft",
            "offset": 0,
        })
    );
    assert_eq!(requests[1]["arguments"]["offset"], 5);
    assert_eq!(
        requests[2]["arguments"],
        json!({ "run_id": "solar-run-123", "artifact": "result", "offset": 0 })
    );
    let _ = std::fs::remove_file(report_out);
    let _ = std::fs::remove_file(portfolio_out);
}

#[test]
fn paired_solar_rejects_a_reply_without_a_receipt_status() {
    let bridge = bridge(vec![("solar.prepare", json!({}))]);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();

    let (envelope, code, _, _) = ds(&[
        "solar",
        "prepare",
        "--city",
        "rw-kigali",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 3);
    assert_eq!(envelope["error"]["code"], "desktop_contract_mismatch");
    let _ = finish(bridge);
}

#[test]
fn paired_run_rejects_an_out_of_contract_concurrency_before_pairing() {
    let (envelope, code, _, _) = ds(&[
        "solar",
        "run",
        "start",
        "--city",
        "rw-kigali",
        "--concurrency",
        "33",
        "--desktop-descriptor",
        "/definitely/not/a/bridge.json",
        "--output",
        "json",
    ]);
    assert_eq!(code, 2);
    assert_eq!(envelope["error"]["code"], "invalid_concurrency");
}
