//! Buildings and contours are seeded once per project through shared
//! revisions. Through the real `ds` binary: the seed contract an agent
//! discovers names the shared-revision behaviour and its refusals, and an
//! unconfirmed or retired-door seed acquires nothing and opens no socket.
mod common;

use common::json;
use std::net::TcpListener;
use std::process::Command;

fn seed_descriptor() -> serde_json::Value {
    let (descriptor, code) = json(&[
        "capabilities",
        "data.project-cache.seed",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{descriptor}");
    descriptor["data"]["command"].clone()
}

#[test]
fn the_seed_contract_names_shared_revisions_and_their_refusals() {
    let command = seed_descriptor();
    let purpose = command["purpose"].as_str().unwrap_or_default();
    // The three rules an agent needs before it spends provider cost: what
    // area is acquired, how far around it, and that shared revisions come
    // before any fetch.
    for rule in [
        "once per project",
        "union of its active LV designs and MV routes",
        "project_context_tolerance_m setting (default 200 m)",
        "never one project-wide box",
        "Published revisions are replayed first",
        "fetches only uncovered",
        "published as the next shared revision",
        "Seeds declared datasets plus what it holds",
    ] {
        assert!(
            purpose.contains(rule),
            "seed purpose omits {rule:?}: {purpose}"
        );
    }
    let refusals: Vec<&str> = command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .filter_map(|refusal| refusal["code"].as_str())
        .collect();
    for code in [
        "project_dataset_revision_invalid",
        "project_dataset_revision_stale",
        "project_dataset_publication_refused",
        "reporter_engine_missing",
        "reporter_engine_incompatible",
        "contour_surface_uncovered",
        "engine_refused",
        "engine_response_invalid",
        "native_compute_required",
        "project_context_tolerance_invalid",
    ] {
        assert!(refusals.contains(&code), "seed does not declare {code}");
    }
    let examples: Vec<&str> = command["examples"]
        .as_array()
        .expect("examples")
        .iter()
        .filter_map(|example| example["command"].as_str())
        .collect();
    assert!(
        examples
            .iter()
            .any(|example| example.contains("--dataset google_open_buildings")),
        "{examples:?}"
    );
}

/// A proxy that counts every connection the binary attempts.
fn counted_proxy() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    (listener, address)
}

#[test]
fn an_unconfirmed_or_retired_door_seed_reaches_nothing() {
    let home = tempfile::tempdir().unwrap();
    let (listener, proxy) = counted_proxy();
    for (args, code) in [
        (
            vec![
                "data",
                "project-cache",
                "seed",
                "--project",
                "demo",
                "--dataset",
                "google_open_buildings",
                "--output",
                "json",
            ],
            "confirmation_required",
        ),
        (
            vec![
                "data",
                "project-cache",
                "seed",
                "--project",
                "demo",
                "--remote-context",
                "--yes",
                "--output",
                "json",
            ],
            "native_compute_required",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(&args)
            .env("NO_COLOR", "1")
            .env("DS_SRE_REPORTING", "off")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("HTTP_PROXY", &proxy)
            .env("HTTPS_PROXY", &proxy)
            .env("ALL_PROXY", &proxy)
            .env("NO_PROXY", "")
            .output()
            .expect("ds runs");
        let envelope: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("a JSON envelope");
        assert_eq!(envelope["error"]["code"], code, "{envelope}");
        assert!(!output.status.success());
    }
    assert!(
        listener.accept().is_err(),
        "a refused seed opened a connection"
    );
}
