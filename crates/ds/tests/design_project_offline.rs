//! Real CLI -> Rust workspace -> native network -> local report/PDF owners.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, process::Command};

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
fn saved_member(bytes: &[u8]) -> Vec<u8> {
    let result: BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_slice(bytes).unwrap();
    let output: BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_str(result["output"].get()).unwrap();
    output["voltage_drop"].get().as_bytes().to_vec()
}

#[test]
#[ignore = "requires built ds-report and reporter printing fixtures; the engine integration gate runs this explicitly"]
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
    let printing_config = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-network-reporter/examples/printing/network-config.a0-a3.example.json");
    let mut network_config: Value =
        serde_json::from_slice(&std::fs::read(printing_config).unwrap()).unwrap();
    let print_format = "pdf__a3-landscape-aderm-lv";
    // Capture the owner's governed locale, layout, renderer and styles.
    // Legacy pdf_a3 cannot replace a governed print selection.
    let settings = network_config["sheets"]["project_settings"]
        .as_array_mut()
        .unwrap();
    settings
        .iter_mut()
        .find(|row| row["parameter"] == "design_export_formats")
        .unwrap()["value"] = json!(["xlsx", print_format]);
    let setup = network_config["sheets"]["printing_setups"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|setup| setup["id"] == "a3-landscape-aderm-lv")
        .unwrap();
    // The native calculation also exposes spans; bind their captured LV-line
    // style explicitly rather than asking the renderer to invent one.
    setup["layout"]["style_refs"]["spans"] = setup["layout"]["style_refs"]["lv_lines"].clone();
    setup["revision"] = json!(ds_command_kernel::report_export::sha256_hex(
        &serde_json::to_vec(&setup["layout"]).unwrap()
    ));
    let input = root.join("snapshot.json");
    std::fs::write(&input,serde_json::to_vec(&json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
        "layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":"T1","names":"T1"}}]},
        "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]}},
        "settings":{},"network_config":network_config,"sources":[],"include_design_customers":true})).unwrap()).unwrap();
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
    let result_bytes = std::fs::read(result).unwrap();
    let source_bytes = saved_member(&result_bytes);
    let result: Value = serde_json::from_slice(&result_bytes).unwrap();
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
        print_format,
    ]);
    assert_eq!(report["report"]["status"], "completed", "{report:#}");
    assert_eq!(report["delivery"]["artifact_count"], 3);
    assert_eq!(
        report["report"]["formats_requested"],
        json!(["xlsx", "voltage_drop", print_format])
    );
    let artifacts = report["report"]["artifacts"].as_array().unwrap();
    let source = artifacts
        .iter()
        .find(|a| a["format"] == "voltage_drop")
        .unwrap();
    assert_eq!(
        std::fs::read(source["path"].as_str().unwrap()).unwrap(),
        source_bytes
    );
    assert_eq!(
        std::fs::read(report_root.join("voltage-drop.json")).unwrap(),
        source_bytes
    );
    assert_eq!(
        source["sha256"],
        format!("{:x}", Sha256::digest(&source_bytes))
    );
    let captured: Value =
        serde_json::from_slice(&std::fs::read(report_root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(captured["voltage_drop_result"]["sha256"], source["sha256"]);
    assert_eq!(
        captured["layers_sha256"],
        ds_command_kernel::design::digest(&result["output"]["gdfs"]).unwrap()
    );
    let after_report = root.join("result-after-report.json");
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
        path(&after_report),
    ]);
    assert_eq!(std::fs::read(after_report).unwrap(), result_bytes);
    let pdf = artifacts
        .iter()
        .find(|a| a["format"] == print_format)
        .unwrap();
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
    // Historical/damaged saved-result fixtures live only in this test's own
    // workspace. The actual CLI must refuse them before rendering/staging;
    // it must not repair them by running the producer again.
    for (case, code) in [
        ("missing", "voltage_drop_result_missing"),
        ("invalid", "voltage_drop_result_invalid"),
        ("stale", "voltage_drop_result_stale"),
    ] {
        let mut damaged = result.clone();
        match case {
            "missing" => {
                damaged["output"]
                    .as_object_mut()
                    .unwrap()
                    .remove("voltage_drop");
            }
            "invalid" => damaged["output"]["voltage_drop"]["schema"] = json!("unknown"),
            "stale" => {
                damaged["output"]["voltage_drop"]["processed_digest"] = json!("0".repeat(64))
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&damaged).unwrap();
        let db = rusqlite::Connection::open(Path::new(workspace).join("design.sqlite")).unwrap();
        db.execute(
            "UPDATE jobs SET result=?1,result_sha256=?2 WHERE run='r1' AND transformer='T1'",
            rusqlite::params![bytes, format!("{:x}", Sha256::digest(&bytes))],
        )
        .unwrap();
        drop(db);
        let refused_root = root.join(format!("report-{case}"));
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
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
                path(&refused_root),
                "--country",
                "Test",
                "--format",
                "xlsx",
                "--output",
                "json",
            ])
            .output()
            .unwrap();
        let refused: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(!output.status.success(), "{refused}");
        assert_eq!(refused["error"]["code"], code, "{refused}");
        assert!(!refused_root.exists());
        assert_eq!(
            call(&["design", "project", "outbox", "--workspace", workspace]),
            pending
        );
        let retained = root.join(format!("result-{case}.json"));
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
            path(&retained),
        ]);
        assert_eq!(std::fs::read(retained).unwrap(), bytes);
    }
    let db = rusqlite::Connection::open(Path::new(workspace).join("design.sqlite")).unwrap();
    db.execute(
        "UPDATE jobs SET result=?1,result_sha256=?2 WHERE run='r1' AND transformer='T1'",
        rusqlite::params![result_bytes, format!("{:x}", Sha256::digest(&result_bytes))],
    )
    .unwrap();
    drop(db);
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
