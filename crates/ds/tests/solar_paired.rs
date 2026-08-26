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
use sha2::{Digest, Sha256};

const MEMBERSHIP_REVISION: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BATCH_ID: &str = "solar-batch-123";
const BATCH_DIGEST: &str =
    "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn portfolio_result(run_id: &str) -> Value {
    let digest = |byte: char| format!("sha256:{}", byte.to_string().repeat(64));
    json!({
        "schema_version": "ds-solar.portfolio-result/v2",
        "engine": {
            "name": "ds-solar-engine",
            "version": "0.1.0",
        },
        "project_id": "project-1",
        "root": "solar",
        "portfolio_id": "pf-1",
        "portfolio_name": "Eastern portfolio",
        "membership_revision": MEMBERSHIP_REVISION,
        "run_id": run_id,
        "input_digest": digest('b'),
        "assumptions": {
            "currency": "XAF",
            "project_years": 25,
            "discount_rate": 0.08,
        },
        "representative_city_id": "a",
        "cities": [
            {
                "city_id": "a",
                "display_name": "Alpha",
                "city_run_id": "city-run-a",
                "input_digest": digest('1'),
                "result_digest": digest('2'),
                "contribution_digest": digest('3'),
                "calculation_projection_digest": digest('4'),
                "selected_system": "solar_battery",
                "inverter_count": 1,
                "pv_capacity_mw": 1.0,
                "battery_capacity_mwh": 2.0,
                "generator_count": 0.0,
                "diesel_capacity_mw": 0.0,
                "total_customers": 10,
                "annual_energy_kwh": 1000.0,
            },
            {
                "city_id": "b",
                "display_name": "Beta",
                "city_run_id": "city-run-b",
                "input_digest": digest('5'),
                "result_digest": digest('6'),
                "contribution_digest": digest('7'),
                "calculation_projection_digest": digest('8'),
                "selected_system": "hybrid",
                "inverter_count": 2,
                "pv_capacity_mw": 2.0,
                "battery_capacity_mwh": 3.0,
                "generator_count": 1.0,
                "diesel_capacity_mw": 0.5,
                "total_customers": 20,
                "annual_energy_kwh": 2000.0,
            },
        ],
        "city_count": 2,
        "total_customers": 30,
        "annual_energy_kwh": 3000.0,
        "systems": {
            "solar_battery": {
                "cities": 1,
                "inverter_count": 1,
                "pv_capacity_mw": 1.0,
                "battery_capacity_mwh": 2.0,
                "generator_count": 0.0,
                "diesel_capacity_mw": 0.0,
            },
            "hybrid": {
                "cities": 1,
                "inverter_count": 2,
                "pv_capacity_mw": 2.0,
                "battery_capacity_mwh": 3.0,
                "generator_count": 1.0,
                "diesel_capacity_mw": 0.5,
            },
        },
        "sections": {"investment": {"total": 42}},
        "portfolio_digest": digest('c'),
    })
}

fn portfolio_page(wrapper_run_id: &str, document: &Value) -> Value {
    let content = document.to_string();
    let content_digest = sha256_digest(content.as_bytes());
    portfolio_artifact_page(
        wrapper_run_id,
        "result",
        "portfolio-result.json",
        0,
        &content,
        content.len(),
        &content_digest,
        BATCH_ID,
        BATCH_DIGEST,
    )
}

#[allow(clippy::too_many_arguments)]
fn portfolio_artifact_page(
    run_id: &str,
    artifact: &str,
    name: &str,
    offset: usize,
    content: &str,
    bytes_total: usize,
    content_digest: &str,
    batch_id: &str,
    batch_digest: &str,
) -> Value {
    let next_offset = offset + content.len();
    json!({
        "status": "ok",
        "run_id": run_id,
        "artifact": artifact,
        "name": name,
        "offset": offset,
        "content": content,
        "bytes_returned": content.len(),
        "bytes_total": bytes_total,
        "content_digest": content_digest,
        "batch_id": batch_id,
        "batch_digest": batch_digest,
        "next_offset": next_offset,
        "complete": next_offset == bytes_total,
    })
}

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
fn solar_portfolio_start_descriptor_exposes_the_explicit_contract() {
    let (envelope, code, stdout, stderr) =
        ds(&["capabilities", "solar.run.start", "--output", "json"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let command = &envelope["data"]["command"];
    assert_eq!(command["contract"], 3);
    let inputs = command["inputs"].as_array().expect("start inputs");
    let input = |name: &str| {
        inputs
            .iter()
            .find(|input| input["name"] == name)
            .unwrap_or_else(|| panic!("missing --{name} from the command descriptor"))
    };

    for name in [
        "membership-revision",
        "currency",
        "project-years",
        "discount-rate",
        "representative-city",
        "language",
    ] {
        assert_eq!(input(name)["kind"], "value");
        assert!(
            input(name)["summary"]
                .as_str()
                .expect("input summary")
                .contains("Portfolio-only"),
            "--{name} must disclose its conditional scope"
        );
    }
    assert_eq!(input("language")["choices"], json!(["fr", "en"]));
    assert_eq!(input("report")["kind"], "repeated");
    assert_eq!(
        input("report")["choices"],
        json!(["apd", "network", "plant", "financial"])
    );
}

#[test]
fn solar_portfolio_export_descriptor_exposes_only_native_artifacts() {
    let (envelope, code, stdout, stderr) =
        ds(&["capabilities", "solar.portfolio.export", "--output", "json"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["data"]["command"]["contract"], 3);
    let inputs = envelope["data"]["command"]["inputs"]
        .as_array()
        .expect("portfolio export inputs");
    let artifact = inputs
        .iter()
        .find(|input| input["name"] == "artifact")
        .expect("portfolio export artifact input");
    assert_eq!(artifact["default"], "result");
    assert_eq!(
        artifact["choices"],
        json!(["result", "apd", "network", "plant", "financial"])
    );
}

#[test]
fn solar_city_report_and_portfolio_read_descriptors_expose_current_contracts() {
    let (report, code, stdout, stderr) =
        ds(&["capabilities", "solar.report.export", "--output", "json"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(report["data"]["command"]["contract"], 2);
    let inputs = report["data"]["command"]["inputs"]
        .as_array()
        .expect("report export inputs");
    let variant = inputs
        .iter()
        .find(|input| input["name"] == "variant")
        .expect("report export variant input");
    assert_eq!(
        variant["choices"],
        json!(["apd", "draft", "network", "plant", "financial"])
    );

    let (read, code, stdout, stderr) =
        ds(&["capabilities", "solar.portfolio.read", "--output", "json"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(read["data"]["command"]["contract"], 2);
    let output = read["data"]["command"]["output"]
        .as_str()
        .expect("portfolio read output contract");
    for promised in [
        "v2 schema",
        "engine identity",
        "portfolio name",
        "content/batch digests",
    ] {
        assert!(
            output.contains(promised),
            "missing `{promised}` from {output}"
        );
    }
}

#[test]
fn solar_workflow_reads_import_and_portfolio_batches_use_closed_operations() {
    let bridge = bridge(vec![
        (
            "solar.results.read",
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "context": "rw-kigali",
                "section": "finance",
            }),
        ),
        (
            "solar.sync.status",
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "rows": [],
                "counts": { "synced": 0 },
            }),
        ),
        (
            "solar.portfolio.list",
            json!({ "status": "ok", "portfolios": [{ "id": "pf-1", "city_count": 2 }] }),
        ),
        (
            "solar.run.start",
            json!({ "status": "started", "run_id": "solar-run-portfolio", "contexts": ["a", "b"] }),
        ),
        (
            "solar.final.import",
            json!({ "status": "imported", "run_id": "solar-run-123", "context": "rw-kigali" }),
        ),
        (
            "solar.final.submit",
            json!({ "status": "submitted", "run_id": "solar-run-123", "context": "rw-kigali" }),
        ),
    ]);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();
    let final_path = std::env::temp_dir().join(format!(
        "ds-cli-solar-final-{}-{}.md",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::write(&final_path, "# Interpreted final\n").expect("write final fixture");
    let final_path_text = final_path.to_string_lossy().into_owned();

    let calls: Vec<Vec<&str>> = vec![
        vec![
            "solar",
            "results",
            "read",
            "--run-id",
            "solar-run-123",
            "--city",
            "rw-kigali",
            "--section",
            "finance",
            "--path",
            "financial_summary",
        ],
        vec!["solar", "sync", "status", "--run-id", "solar-run-123"],
        vec!["solar", "portfolio", "list"],
        vec![
            "solar",
            "run",
            "start",
            "--portfolio",
            "pf-1",
            "--membership-revision",
            MEMBERSHIP_REVISION,
            "--currency",
            "XAF",
            "--project-years",
            "25",
            "--discount-rate",
            "0.08",
            "--representative-city",
            "a",
            "--language",
            "fr",
            "--report",
            "apd",
            "--report",
            "financial",
            "--no-charts",
            "--concurrency",
            "4",
            "--serial",
        ],
        vec![
            "solar",
            "final",
            "import",
            "--run-id",
            "solar-run-123",
            "--city",
            "rw-kigali",
            "--file",
            &final_path_text,
            "--yes",
        ],
        vec![
            "solar",
            "final",
            "submit",
            "--run-id",
            "solar-run-123",
            "--city",
            "rw-kigali",
            "--yes",
        ],
    ];
    for base in calls {
        let mut args = base;
        args.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
        let (_envelope, code, stdout, stderr) = ds(&args);
        assert_eq!(code, 0, "{stdout}{stderr}");
    }

    let requests = finish(bridge);
    assert_eq!(
        requests[0]["arguments"],
        json!({
            "run_id": "solar-run-123",
            "context": "rw-kigali",
            "section": "finance",
            "path": ["financial_summary"],
        })
    );
    assert_eq!(
        requests[1]["arguments"],
        json!({ "run_id": "solar-run-123" })
    );
    assert_eq!(requests[2]["arguments"], json!({}));
    assert_eq!(
        requests[3]["arguments"],
        json!({
            "portfolio": "pf-1",
            "membership_revision": MEMBERSHIP_REVISION,
            "currency": "XAF",
            "project_years": 25,
            "discount_rate": 0.08,
            "representative_city": "a",
            "language": "fr",
            "report_intents": ["apd", "financial"],
            "render_charts": false,
            "concurrency": 4,
            "serial": true,
        })
    );
    assert_eq!(requests[4]["arguments"]["run_id"], "solar-run-123");
    assert_eq!(requests[4]["arguments"]["context"], "rw-kigali");
    assert_eq!(requests[4]["arguments"]["source_path"], final_path_text);
    assert_eq!(
        requests[5]["arguments"],
        json!({ "run_id": "solar-run-123", "context": "rw-kigali" })
    );

    let _ = std::fs::remove_file(final_path);
}

#[test]
fn paired_solar_exports_page_sealed_reports_and_portfolios_to_new_files() {
    let report_digest = sha256_digest(b"draft report");
    let portfolio_digest = sha256_digest(br#"{"total":1}"#);
    let bridge = bridge(vec![
        (
            "solar.document.read",
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "context": "rw-kigali",
                "document": "draft",
                "name": "apd-draft-fr.md",
                "offset": 0,
                "content": "draft",
                "bytes_returned": 5,
                "bytes_total": 12,
                "content_digest": report_digest,
                "batch_id": BATCH_ID,
                "batch_digest": BATCH_DIGEST,
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
                "name": "apd-draft-fr.md",
                "offset": 5,
                "content": " report",
                "bytes_returned": 7,
                "bytes_total": 12,
                "content_digest": sha256_digest(b"draft report"),
                "batch_id": BATCH_ID,
                "batch_digest": BATCH_DIGEST,
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
                "name": "portfolio-result.json",
                "offset": 0,
                "content": "{\"total\":1}",
                "bytes_returned": 11,
                "bytes_total": 11,
                "content_digest": portfolio_digest,
                "batch_id": BATCH_ID,
                "batch_digest": BATCH_DIGEST,
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
    assert_eq!(report["data"]["name"], "apd-draft-fr.md");
    assert_eq!(
        report["data"]["content_digest"],
        sha256_digest(b"draft report")
    );
    assert_eq!(report["data"]["batch_id"], BATCH_ID);
    assert_eq!(report["data"]["batch_digest"], BATCH_DIGEST);
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
    assert_eq!(portfolio["data"]["name"], "portfolio-result.json");
    assert_eq!(
        portfolio["data"]["content_digest"],
        sha256_digest(br#"{"total":1}"#)
    );
    assert_eq!(portfolio["data"]["batch_id"], BATCH_ID);
    assert_eq!(portfolio["data"]["batch_digest"], BATCH_DIGEST);
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
fn solar_portfolio_export_sends_each_exact_native_artifact_unchanged() {
    let artifacts = ["result", "apd", "network", "plant", "financial"];
    let replies = artifacts
        .iter()
        .map(|artifact| {
            let content = format!("sealed-{artifact}");
            let content_digest = sha256_digest(content.as_bytes());
            let name = match *artifact {
                "result" => "portfolio-result.json",
                "apd" => "portfolio-draft-fr.md",
                "network" => "portfolio-reseau-fr.md",
                "plant" => "portfolio-centrale-fr.md",
                "financial" => "portfolio-financier-fr.md",
                _ => unreachable!(),
            };
            (
                "solar.portfolio.read",
                json!({
                    "status": "ok",
                    "run_id": "solar-run-123",
                    "artifact": artifact,
                    "name": name,
                    "offset": 0,
                    "content": content,
                    "bytes_returned": content.len(),
                    "bytes_total": content.len(),
                    "content_digest": content_digest,
                    "batch_id": BATCH_ID,
                    "batch_digest": BATCH_DIGEST,
                    "next_offset": content.len(),
                    "complete": true,
                }),
            )
        })
        .collect();
    let bridge = bridge(replies);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let mut outputs = Vec::new();

    for artifact in artifacts {
        let extension = if artifact == "result" { "json" } else { "md" };
        let out = std::env::temp_dir().join(format!(
            "ds-solar-portfolio-{artifact}-{}-{unique}.{extension}",
            std::process::id()
        ));
        let out_string = out.to_string_lossy().into_owned();
        let (envelope, code, stdout, stderr) = ds(&[
            "solar",
            "portfolio",
            "export",
            "--run-id",
            "solar-run-123",
            "--artifact",
            artifact,
            "--out",
            &out_string,
            "--desktop-descriptor",
            &descriptor,
            "--output",
            "json",
        ]);
        assert_eq!(code, 0, "{artifact}: {stdout}{stderr}");
        assert_eq!(envelope["data"]["artifact"], artifact);
        assert_eq!(
            envelope["data"]["content_digest"],
            sha256_digest(format!("sealed-{artifact}").as_bytes())
        );
        assert_eq!(envelope["data"]["batch_id"], BATCH_ID);
        assert_eq!(envelope["data"]["batch_digest"], BATCH_DIGEST);
        assert_eq!(
            std::fs::read_to_string(&out).expect("read exact portfolio export"),
            format!("sealed-{artifact}")
        );
        outputs.push(out);
    }

    let requests = finish(bridge);
    assert_eq!(requests.len(), artifacts.len());
    for (request, artifact) in requests.iter().zip(artifacts) {
        assert_eq!(
            request["arguments"],
            json!({
                "run_id": "solar-run-123",
                "artifact": artifact,
                "offset": 0,
            })
        );
    }
    for output in outputs {
        let _ = std::fs::remove_file(output);
    }
}

#[test]
fn solar_city_report_export_sends_each_native_variant_unchanged() {
    let variants = ["apd", "draft", "network", "plant", "financial"];
    let replies = variants
        .iter()
        .map(|variant| {
            let content = format!("sealed-city-{variant}");
            let content_digest = sha256_digest(content.as_bytes());
            let name = match *variant {
                "apd" => "apd-fr.md",
                "draft" => "apd-draft-fr.md",
                "network" => "reseau-fr.md",
                "plant" => "centrale-fr.md",
                "financial" => "financier-fr.md",
                _ => unreachable!(),
            };
            (
                "solar.document.read",
                json!({
                    "status": "ok",
                    "run_id": "solar-run-123",
                    "context": "rw-kigali",
                    "document": variant,
                    "name": name,
                    "offset": 0,
                    "content": content,
                    "bytes_returned": content.len(),
                    "bytes_total": content.len(),
                    "content_digest": content_digest,
                    "batch_id": BATCH_ID,
                    "batch_digest": BATCH_DIGEST,
                    "next_offset": content.len(),
                    "complete": true,
                }),
            )
        })
        .collect();
    let bridge = bridge(replies);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let mut outputs = Vec::new();

    for variant in variants {
        let out = std::env::temp_dir().join(format!(
            "ds-solar-city-{variant}-{}-{unique}.md",
            std::process::id()
        ));
        let out_string = out.to_string_lossy().into_owned();
        let (envelope, code, stdout, stderr) = ds(&[
            "solar",
            "report",
            "export",
            "--run-id",
            "solar-run-123",
            "--city",
            "rw-kigali",
            "--variant",
            variant,
            "--out",
            &out_string,
            "--desktop-descriptor",
            &descriptor,
            "--output",
            "json",
        ]);
        assert_eq!(code, 0, "{variant}: {stdout}{stderr}");
        assert_eq!(envelope["data"]["variant"], variant);
        assert_eq!(envelope["data"]["batch_id"], BATCH_ID);
        assert_eq!(envelope["data"]["batch_digest"], BATCH_DIGEST);
        assert_eq!(
            std::fs::read_to_string(&out).expect("read exact city report export"),
            format!("sealed-city-{variant}")
        );
        outputs.push(out);
    }

    let requests = finish(bridge);
    assert_eq!(requests.len(), variants.len());
    for (request, variant) in requests.iter().zip(variants) {
        assert_eq!(
            request["arguments"],
            json!({
                "run_id": "solar-run-123",
                "context": "rw-kigali",
                "document": variant,
                "offset": 0,
            })
        );
    }
    for output in outputs {
        let _ = std::fs::remove_file(output);
    }
}

#[test]
fn solar_portfolio_export_rejects_non_native_artifact_names_before_pairing() {
    for artifact in ["draft", "summary"] {
        let (envelope, code, stdout, stderr) = ds(&[
            "solar",
            "portfolio",
            "export",
            "--run-id",
            "solar-run-123",
            "--artifact",
            artifact,
            "--out",
            "/tmp/unreachable-portfolio-export",
            "--desktop-descriptor",
            "/definitely/not/a/bridge.json",
            "--output",
            "json",
        ]);
        assert_eq!(code, 2, "{artifact}: {stdout}{stderr}");
        assert_eq!(envelope["error"]["code"], "invalid_choice");
    }
}

#[test]
fn solar_portfolio_read_returns_one_bounded_sealed_projection() {
    let document = portfolio_result("solar-run-123");
    let expected_content_digest = sha256_digest(document.to_string().as_bytes());
    let bridge = bridge(vec![(
        "solar.portfolio.read",
        portfolio_page("solar-run-123", &document),
    )]);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();

    let (portfolio, code, stdout, stderr) = ds(&[
        "solar",
        "portfolio",
        "read",
        "--run-id",
        "solar-run-123",
        "--path",
        "sections",
        "--path",
        "investment",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(portfolio["command"], "solar.portfolio.read");
    assert_eq!(portfolio["data"]["value"], json!({"total": 42}));
    assert_eq!(portfolio["data"]["complete"], true);
    assert_eq!(portfolio["data"]["path"], json!(["sections", "investment"]));
    assert_eq!(
        portfolio["data"]["schema_version"],
        "ds-solar.portfolio-result/v2"
    );
    assert_eq!(portfolio["data"]["engine"]["name"], "ds-solar-engine");
    assert_eq!(portfolio["data"]["portfolio_id"], "pf-1");
    assert_eq!(portfolio["data"]["portfolio_name"], "Eastern portfolio");
    assert_eq!(
        portfolio["data"]["membership_revision"],
        MEMBERSHIP_REVISION
    );
    assert_eq!(portfolio["data"]["city_ids"], json!(["a", "b"]));
    assert_eq!(portfolio["data"]["currency"], "XAF");
    assert_eq!(portfolio["data"]["project_years"], 25);
    assert_eq!(portfolio["data"]["representative_city"], "a");
    assert_eq!(portfolio["data"]["name"], "portfolio-result.json");
    assert_eq!(portfolio["data"]["content_digest"], expected_content_digest);
    assert_eq!(portfolio["data"]["batch_id"], BATCH_ID);
    assert_eq!(portfolio["data"]["batch_digest"], BATCH_DIGEST);

    let requests = finish(bridge);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["arguments"],
        json!({ "run_id": "solar-run-123", "artifact": "result", "offset": 0 })
    );
}

#[test]
fn solar_portfolio_read_validates_path_before_pairing() {
    let mut args = vec!["solar", "portfolio", "read", "--run-id", "run-123"];
    for _ in 0..9 {
        args.extend(["--path", "nested"]);
    }
    args.extend([
        "--desktop-descriptor",
        "/definitely/not/a/bridge.json",
        "--output",
        "json",
    ]);
    let (envelope, code, _, _) = ds(&args);
    assert_eq!(code, 2);
    assert_eq!(envelope["error"]["code"], "invalid_portfolio_path");
}

#[test]
fn solar_portfolio_read_rejects_a_mismatched_sealed_run() {
    let document = portfolio_result("different-run");
    let bridge = bridge(vec![(
        "solar.portfolio.read",
        portfolio_page("solar-run-123", &document),
    )]);
    let descriptor = bridge.descriptor.to_string_lossy().into_owned();
    let (envelope, code, _, _) = ds(&[
        "solar",
        "portfolio",
        "read",
        "--run-id",
        "solar-run-123",
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
fn paired_solar_rejects_non_start_receipts_for_another_run_or_city() {
    let cases: Vec<(&'static str, Vec<&'static str>, Value)> = vec![
        (
            "solar.run.progress",
            vec!["solar", "run", "progress", "--run-id", "solar-run-123"],
            json!({"status": "running", "run_id": "attacker-run"}),
        ),
        (
            "solar.run.result",
            vec!["solar", "run", "result", "--run-id", "solar-run-123"],
            json!({"status": "succeeded", "run_id": "attacker-run"}),
        ),
        (
            "solar.run.cancel",
            vec!["solar", "run", "cancel", "--run-id", "solar-run-123"],
            json!({"status": "cancel_requested", "run_id": "attacker-run"}),
        ),
        (
            "solar.result.read",
            vec![
                "solar",
                "result",
                "read",
                "--run-id",
                "solar-run-123",
                "--city",
                "rw-kigali",
            ],
            json!({
                "status": "ok",
                "run_id": "solar-run-123",
                "context": "rw-butare",
            }),
        ),
        (
            "solar.results.read",
            vec![
                "solar",
                "results",
                "read",
                "--run-id",
                "solar-run-123",
                "--city",
                "rw-kigali",
                "--section",
                "finance",
            ],
            json!({
                "status": "ok",
                "run_id": "attacker-run",
                "context": "rw-kigali",
            }),
        ),
        (
            "solar.sync.status",
            vec!["solar", "sync", "status", "--run-id", "solar-run-123"],
            json!({"status": "ok", "run_id": "attacker-run", "rows": []}),
        ),
        (
            "solar.final.import",
            vec![
                "solar",
                "final",
                "import",
                "--run-id",
                "solar-run-123",
                "--city",
                "rw-kigali",
                "--file",
                "/tmp/ds-solar-final-not-read-by-cli.md",
                "--yes",
            ],
            json!({
                "status": "imported",
                "run_id": "solar-run-123",
                "context": "rw-butare",
            }),
        ),
        (
            "solar.final.submit",
            vec![
                "solar",
                "final",
                "submit",
                "--run-id",
                "solar-run-123",
                "--city",
                "rw-kigali",
                "--yes",
            ],
            json!({
                "status": "submitted",
                "run_id": "attacker-run",
                "context": "rw-kigali",
            }),
        ),
    ];

    for (operation, mut args, reply) in cases {
        let bridge = bridge(vec![(operation, reply)]);
        let descriptor = bridge.descriptor.to_string_lossy().into_owned();
        args.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
        let (envelope, code, stdout, stderr) = ds(&args);
        assert_eq!(code, 3, "{operation}: {stdout}{stderr}");
        assert_eq!(
            envelope["error"]["code"], "desktop_contract_mismatch",
            "{operation}: {stdout}{stderr}"
        );
        let requests = finish(bridge);
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn solar_portfolio_read_rejects_invalid_v2_trace_and_engine_identity() {
    let mut wrong_schema = portfolio_result("solar-run-123");
    wrong_schema["schema_version"] = json!("ds-solar.portfolio-result/v1");
    let mut missing_name = portfolio_result("solar-run-123");
    missing_name
        .as_object_mut()
        .expect("portfolio object")
        .remove("portfolio_name");
    let mut unknown_engine_field = portfolio_result("solar-run-123");
    unknown_engine_field["engine"]["unbounded"] = json!("not native");
    let mut oversized_engine_list = portfolio_result("solar-run-123");
    oversized_engine_list["engine"]["features"] = json!(
        (0..65)
            .map(|index| format!("feature-{index}"))
            .collect::<Vec<_>>()
    );

    for (case, document) in [
        ("wrong-schema", wrong_schema),
        ("missing-portfolio-name", missing_name),
        ("unknown-engine-field", unknown_engine_field),
        ("oversized-engine-list", oversized_engine_list),
    ] {
        let bridge = bridge(vec![(
            "solar.portfolio.read",
            portfolio_page("solar-run-123", &document),
        )]);
        let descriptor = bridge.descriptor.to_string_lossy().into_owned();
        let (envelope, code, stdout, stderr) = ds(&[
            "solar",
            "portfolio",
            "read",
            "--run-id",
            "solar-run-123",
            "--desktop-descriptor",
            &descriptor,
            "--output",
            "json",
        ]);
        assert_eq!(code, 3, "{case}: {stdout}{stderr}");
        assert_eq!(
            envelope["error"]["code"], "desktop_contract_mismatch",
            "{case}: {stdout}{stderr}"
        );
        let _ = finish(bridge);
    }
}

#[test]
fn solar_portfolio_export_rejects_unpinned_or_inconsistent_page_receipts() {
    let valid_digest = sha256_digest(b"sealed");
    let mut wrong_offset = portfolio_artifact_page(
        "solar-run-123",
        "result",
        "portfolio-result.json",
        0,
        "sealed",
        6,
        &valid_digest,
        BATCH_ID,
        BATCH_DIGEST,
    );
    wrong_offset["offset"] = json!(1);
    let mut false_content_digest = portfolio_artifact_page(
        "solar-run-123",
        "result",
        "portfolio-result.json",
        0,
        "sealed",
        6,
        &valid_digest,
        BATCH_ID,
        BATCH_DIGEST,
    );
    false_content_digest["content_digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");
    let mut missing_batch_id = portfolio_artifact_page(
        "solar-run-123",
        "result",
        "portfolio-result.json",
        0,
        "sealed",
        6,
        &valid_digest,
        BATCH_ID,
        BATCH_DIGEST,
    );
    missing_batch_id
        .as_object_mut()
        .expect("page object")
        .remove("batch_id");
    let wrong_run = portfolio_artifact_page(
        "attacker-run",
        "result",
        "portfolio-result.json",
        0,
        "sealed",
        6,
        &valid_digest,
        BATCH_ID,
        BATCH_DIGEST,
    );
    let overlong_name_text = "n".repeat(1_025);
    let overlong_name = portfolio_artifact_page(
        "solar-run-123",
        "result",
        &overlong_name_text,
        0,
        "sealed",
        6,
        &valid_digest,
        BATCH_ID,
        BATCH_DIGEST,
    );

    for (case, page) in [
        ("wrong-offset", wrong_offset),
        ("false-content-digest", false_content_digest),
        ("missing-batch-id", missing_batch_id),
        ("wrong-wrapper-run", wrong_run),
        ("overlong-native-name", overlong_name),
    ] {
        let bridge = bridge(vec![("solar.portfolio.read", page)]);
        let descriptor = bridge.descriptor.to_string_lossy().into_owned();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let out = std::env::temp_dir().join(format!(
            "ds-solar-invalid-page-{}-{case}-{unique}.json",
            std::process::id()
        ));
        let out_string = out.to_string_lossy().into_owned();
        let (envelope, code, stdout, stderr) = ds(&[
            "solar",
            "portfolio",
            "export",
            "--run-id",
            "solar-run-123",
            "--out",
            &out_string,
            "--desktop-descriptor",
            &descriptor,
            "--output",
            "json",
        ]);
        assert_eq!(code, 3, "{case}: {stdout}{stderr}");
        assert_eq!(envelope["error"]["code"], "desktop_contract_mismatch");
        assert!(!out.exists(), "{case} must not create a partial export");
        let _ = finish(bridge);
    }
}

#[test]
fn solar_portfolio_export_requires_stable_batch_identity_across_pages() {
    let content_digest = sha256_digest(b"sealed");
    for (case, second_batch_id, second_batch_digest) in [
        ("batch-id", "solar-batch-attacker", BATCH_DIGEST),
        (
            "batch-digest",
            BATCH_ID,
            "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        ),
    ] {
        let first = portfolio_artifact_page(
            "solar-run-123",
            "result",
            "portfolio-result.json",
            0,
            "seal",
            6,
            &content_digest,
            BATCH_ID,
            BATCH_DIGEST,
        );
        let second = portfolio_artifact_page(
            "solar-run-123",
            "result",
            "portfolio-result.json",
            4,
            "ed",
            6,
            &content_digest,
            second_batch_id,
            second_batch_digest,
        );
        let bridge = bridge(vec![
            ("solar.portfolio.read", first),
            ("solar.portfolio.read", second),
        ]);
        let descriptor = bridge.descriptor.to_string_lossy().into_owned();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let out = std::env::temp_dir().join(format!(
            "ds-solar-unstable-page-{}-{case}-{unique}.json",
            std::process::id()
        ));
        let out_string = out.to_string_lossy().into_owned();
        let (envelope, code, stdout, stderr) = ds(&[
            "solar",
            "portfolio",
            "export",
            "--run-id",
            "solar-run-123",
            "--out",
            &out_string,
            "--desktop-descriptor",
            &descriptor,
            "--output",
            "json",
        ]);
        assert_eq!(code, 3, "{case}: {stdout}{stderr}");
        assert_eq!(envelope["error"]["code"], "desktop_contract_mismatch");
        assert!(!out.exists(), "{case} must not create a partial export");
        let requests = finish(bridge);
        assert_eq!(requests.len(), 2);
    }
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

#[test]
fn paired_run_refuses_incomplete_or_mixed_portfolio_launches_before_pairing() {
    let cases = vec![
        (vec!["solar", "run", "start"], "invalid_run_selection"),
        (
            vec![
                "solar",
                "run",
                "start",
                "--city",
                "rw-kigali",
                "--portfolio",
                "pf-1",
            ],
            "invalid_run_selection",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--city",
                "rw-kigali",
                "--currency",
                "XAF",
            ],
            "portfolio_only_input",
        ),
        (
            vec!["solar", "run", "start", "--portfolio", "pf-1"],
            "missing_portfolio_input",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--portfolio",
                "pf-1",
                "--membership-revision",
                "sha256:NOT-A-DIGEST",
            ],
            "invalid_membership_revision",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--portfolio",
                "pf-1",
                "--membership-revision",
                MEMBERSHIP_REVISION,
                "--currency",
                "xaf",
            ],
            "invalid_currency",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--portfolio",
                "pf-1",
                "--membership-revision",
                MEMBERSHIP_REVISION,
                "--currency",
                "XAF",
                "--project-years",
                "101",
            ],
            "invalid_project_years",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--portfolio",
                "pf-1",
                "--membership-revision",
                MEMBERSHIP_REVISION,
                "--currency",
                "XAF",
                "--project-years",
                "25",
                "--discount-rate",
                "NaN",
            ],
            "invalid_discount_rate",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--portfolio",
                "pf-1",
                "--membership-revision",
                MEMBERSHIP_REVISION,
                "--currency",
                "XAF",
                "--project-years",
                "25",
                "--discount-rate",
                "0.08",
                "--representative-city=",
            ],
            "invalid_representative_city",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--portfolio",
                "pf-1",
                "--membership-revision",
                MEMBERSHIP_REVISION,
                "--currency",
                "XAF",
                "--project-years",
                "25",
                "--discount-rate",
                "0.08",
                "--representative-city",
                "rw-kigali",
                "--language",
                "fr",
            ],
            "missing_portfolio_input",
        ),
        (
            vec![
                "solar",
                "run",
                "start",
                "--portfolio",
                "pf-1",
                "--membership-revision",
                MEMBERSHIP_REVISION,
                "--currency",
                "XAF",
                "--project-years",
                "25",
                "--discount-rate",
                "0.08",
                "--representative-city",
                "rw-kigali",
                "--language",
                "fr",
                "--report",
                "apd",
                "--report",
                "apd",
            ],
            "duplicate_report_intent",
        ),
    ];

    for (mut args, expected) in cases {
        args.extend([
            "--desktop-descriptor",
            "/definitely/not/a/bridge.json",
            "--output",
            "json",
        ]);
        let (envelope, code, stdout, stderr) = ds(&args);
        assert_eq!(code, 2, "{stdout}{stderr}");
        assert_eq!(
            envelope["error"]["code"],
            expected,
            "unexpected refusal for: {}",
            args.join(" ")
        );
    }
}
