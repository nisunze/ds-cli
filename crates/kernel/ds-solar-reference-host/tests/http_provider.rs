//! Hermetic HTTP-provider refusal tests; no live service is contacted.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;

use ds_solar_contracts::SolarErrorCode;
use ds_solar_reference_host::HttpReferenceProvider;
use ds_solar_reference_host::{ReferenceBundleProvider, ReferencePreparer};

fn serve_once(body: String) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("one local request");
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        let mut content_length = None;
        loop {
            let mut line = String::new();
            stream.read_line(&mut line).expect("request header");
            assert!(!line.is_empty(), "request ended before its headers");
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = Some(value.trim().parse::<usize>().expect("body length"));
            }
            request.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut request_body = vec![0; content_length.expect("JSON request has a content length")];
        stream
            .read_exact(&mut request_body)
            .expect("complete request body");
        request.push_str(std::str::from_utf8(&request_body).expect("JSON request is UTF-8"));
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .get_mut()
            .write_all(response.as_bytes())
            .expect("local response");
        request
    });
    (format!("http://{address}"), handle)
}

#[test]
fn http_provider_refuses_a_schema_skew_before_downloading_any_artifact() {
    let (base_url, server) = serve_once(
        serde_json::json!({
            "schema_version": "ds-solar.reference-bundle/v0",
            "source_key": "test",
            "bundle_digest": "sha256:deadbeef",
            "artifacts": []
        })
        .to_string(),
    );
    let request = ReferencePreparer::new()
        .request_for(&common::city_input("aderm_bere"))
        .expect("fixture request");
    let provider = HttpReferenceProvider::new(base_url, Some("fixture-bearer".to_string()))
        .expect("local provider")
        .with_api_key(Some("fixture-key".to_string()));
    let error = provider
        .fetch(&request)
        .expect_err("schema skew is a typed refusal");
    assert_eq!(error.code, SolarErrorCode::SchemaIncompatible);

    let request = server.join().expect("server thread");
    assert!(request.starts_with("POST /api/v1/solar/reference-unit HTTP/1.1"));
    assert!(request.contains("authorization: Bearer fixture-bearer"));
    assert!(request.contains("x-api-key: fixture-key"));
}

#[test]
fn inventory_bound_is_enforced_before_json_decoding() {
    let (base_url, server) = serve_once(" ".repeat(256 * 1024 + 1));
    let request = ReferencePreparer::new()
        .request_for(&common::city_input("aderm_bere"))
        .unwrap();
    let error = HttpReferenceProvider::new(base_url, None)
        .unwrap()
        .fetch(&request)
        .unwrap_err();
    assert!(error.message.contains("bound"));
    // The client may close early after reading the declared size.
    let _ = server.join();
}

#[test]
fn duplicate_inventory_entries_are_refused_before_artifact_download() {
    let artifact = serde_json::json!({"name":"weather.json", "bytes":1, "sha256":"a".repeat(64), "url":"http://127.0.0.1:1/must-not-be-called"});
    let (base_url, server) = serve_once(serde_json::json!({
        "schema_version": ds_solar_contracts::REFERENCE_BUNDLE_SCHEMA_VERSION,
        "source_key":"test", "bundle_digest":"test", "artifacts":[artifact.clone(),artifact.clone(),artifact.clone(),artifact]
    }).to_string());
    let request = ReferencePreparer::new()
        .request_for(&common::city_input("aderm_bere"))
        .unwrap();
    let error = HttpReferenceProvider::new(base_url, None)
        .unwrap()
        .fetch(&request)
        .unwrap_err();
    assert!(error.message.contains("exactly four"));
    server.join().unwrap();
}
