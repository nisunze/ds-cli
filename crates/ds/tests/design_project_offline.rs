//! Real CLI -> Rust workspace -> native network -> local report/PDF owners.
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn call(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .args(["--output", "json"])
        .output()
        .unwrap();
    let envelope: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    assert!(output.status.success(), "{envelope}");
    envelope["data"].clone()
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
#[ignore = "requires built ds-report; the engine integration gate runs this explicitly"]
fn full_offline_flow_keeps_pinned_results_and_print_bytes_without_desktop() {
    let root = std::env::temp_dir().join(format!("ds-design-offline-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let workspace = root.join("project");
    let workspace = path(&workspace);
    let init = call(&[
        "design",
        "project",
        "init",
        "--workspace",
        workspace,
        "--project",
        "offline-test",
    ]);
    assert_eq!(init["publication"], "local_only");
    let input = root.join("snapshot.json");
    std::fs::write(&input,serde_json::to_vec(&json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
        "layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":"T1","names":"T1"}}]},
        "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]}},
        "settings":{},"network_config":{"sheets":{"project_settings":[{"parameter":"design_export_format","value":["xlsx","pdf_a3"]}]}},"sources":[],"include_design_customers":true})).unwrap()).unwrap();
    let written = call(&[
        "design",
        "project",
        "write",
        "--workspace",
        workspace,
        "--input",
        path(&input),
        "--operation-id",
        "create",
    ]);
    assert_eq!(written["publication"], "pending_transport");
    let revision = written["revision"].as_str().unwrap();
    let exported = root.join("read.json");
    assert_eq!(
        call(&[
            "design",
            "project",
            "read",
            "--workspace",
            workspace,
            "--transformer",
            "T1",
            "--out",
            path(&exported)
        ])["revision"],
        revision
    );
    let edit = root.join("edit.json");
    std::fs::write(&edit,serde_json::to_vec(&json!({"schema":"ds.design.edit/v1","transformer":"T1","expected_revision":revision,
        "operation_id":"edit","mutations":[{"kind":"set_properties","layer":"lv_lines","ids":["line"],"values":{"note":"offline"}}]})).unwrap()).unwrap();
    let changed = call(&[
        "design",
        "project",
        "edit",
        "--workspace",
        workspace,
        "--input",
        path(&edit),
    ]);
    let restored = call(&[
        "design",
        "project",
        "restore",
        "--workspace",
        workspace,
        "--transformer",
        "T1",
        "--revision",
        revision,
        "--expected",
        changed["revision"].as_str().unwrap(),
        "--operation-id",
        "undo",
    ]);
    assert_eq!(restored["revision"], revision);
    let processed = call(&[
        "design",
        "project",
        "process",
        "--workspace",
        workspace,
        "--run-id",
        "r1",
        "--transformer",
        "T1",
        "--workers",
        "1",
    ]);
    assert_eq!(processed["completed"], 1);
    let result = root.join("result.json");
    call(&[
        "design",
        "project",
        "result",
        "--workspace",
        workspace,
        "--run-id",
        "r1",
        "--transformer",
        "T1",
        "--out",
        path(&result),
    ]);
    let result: Value = serde_json::from_slice(&std::fs::read(result).unwrap()).unwrap();
    assert_eq!(result["input_revision"], revision);
    assert!(
        result["output"]["gdfs"]["lv_poles"]["features"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
    );
    let report_root = root.join("report");
    let report = call(&[
        "design",
        "project",
        "report",
        "--workspace",
        workspace,
        "--run-id",
        "r1",
        "--transformer",
        "T1",
        "--out-dir",
        path(&report_root),
        "--country",
        "Test",
        "--format",
        "xlsx",
        "--format",
        "pdf_a3",
    ]);
    assert_eq!(report["report"]["status"], "completed", "{report:#}");
    assert_eq!(report["delivery"]["artifact_count"], 2);
    let artifacts = report["report"]["artifacts"].as_array().unwrap();
    let pdf = artifacts.iter().find(|a| a["format"] == "pdf_a3").unwrap();
    let bytes = std::fs::read(pdf["path"].as_str().unwrap()).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(bytes.len() > 1000);
    let pending = call(&["design", "project", "outbox", "--workspace", workspace]);
    assert_eq!(
        pending["rows"].as_array().unwrap().last().unwrap()["kind"],
        "report_artifacts"
    );
    assert_eq!(
        call(&["design", "project", "status", "--workspace", workspace])["pending_publications"],
        5
    );
    let cancelled = call(&[
        "design",
        "project",
        "cancel",
        "--workspace",
        workspace,
        "--run-id",
        "r1",
    ]);
    assert_eq!(cancelled["cancellation"], "requested_at_job_boundaries");
    let sources = root.join("sources.json");
    let selected = root.join("selected.json");
    std::fs::write(&sources,br#"{"schema":"ds.design.source-resolution/v1","kind":"poles","project":{"addresses":[],"labels":[]},"user":null}"#).unwrap();
    assert_eq!(
        call(&[
            "design",
            "project",
            "resolve-sources",
            "--input",
            path(&sources),
            "--out",
            path(&selected)
        ])["scope"],
        "project"
    );
    // The OS worker starts with captured inputs and survives the CLI caller.
    let launched = call(&[
        "design",
        "project",
        "process",
        "--workspace",
        workspace,
        "--run-id",
        "r2",
        "--transformer",
        "T1",
        "--background",
        "--workers",
        "1",
    ]);
    assert!(launched["worker_pid"].as_u64().unwrap() > 0);
    let start = std::time::Instant::now();
    loop {
        let status = call(&[
            "design",
            "project",
            "status",
            "--workspace",
            workspace,
            "--run-id",
            "r2",
        ]);
        if status["jobs"][0]["state"] == "completed" {
            break;
        }
        assert!(start.elapsed().as_secs() < 30, "{status}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::fs::remove_dir_all(root).unwrap();
}
