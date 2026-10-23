//! An LV run carries the executor's own provenance and the persisted process
//! verdict; no host stamps a processor it did not observe.
use super::*;
use ds_core_contracts::envelope::success_envelope;
use serde_json::json;

fn captured() -> CapturedLvProcess {
    let input = json!({"jobs":[
        {"ds_project":"project_a","transformer_name":"T2","gdfs":{"tr":{"type":"FeatureCollection","features":[]}},"settings":{}},
        {"ds_project":"project_a","transformer_name":"T1","gdfs":{"tr":{"type":"FeatureCollection","features":[]}},"settings":{}}
    ],"local_tables":{}});
    capture_bytes("project_a", &serde_json::to_vec(&input).unwrap()).unwrap()
}

fn output(warnings: Value) -> Value {
    let native: NativeLvResultV1 = serde_json::from_value(json!({"schema":NATIVE_LV_RESULT_SCHEMA_V1,"engine_core_version":"owner-provenance","jobs":[
        {"transformer_name":"T2","ok":true,"output":{"gdfs":{
            "tr":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":null,"properties":{}}]},
            "lv_lines":{"type":"FeatureCollection","features":[
                {"type":"Feature","geometry":null,"properties":{}},{"type":"Feature","geometry":null,"properties":{}}]}},
            "settings_used":{},"warnings":warnings}},
        {"transformer_name":"T1","ok":false,"error":"the named local table is unavailable","error_code":"source_unavailable"}
    ]})).unwrap();
    serde_json::to_value(captured().response_from_native(native).unwrap()).unwrap()
}

fn envelope(out: Value, data: Value) -> Value {
    let mut value = serde_json::to_value(success_envelope(COMMAND, 4, data)).unwrap();
    value["files"] = json!({"out": out});
    value
}

fn receipt() -> Value {
    json!({"jobs":2,"engine_core_version":"0.9.1","executable_sha256":"a".repeat(64),
        "development_override":false,"engine_identity":{"future":true},"future_receipt":[1]})
}

#[test]
fn provenance_comes_from_the_core_receipt_and_never_names_the_retired_wasm_processor() {
    let run = captured()
        .decode_core_run(envelope(output(json!([])), receipt()))
        .unwrap();
    assert_eq!(run.provenance.processor, CORE_PROCESSOR);
    assert_eq!(run.provenance.engine_core_version.as_deref(), Some("0.9.1"));
    assert_eq!(
        run.provenance.executable_sha256.as_deref(),
        Some("a".repeat(64).as_str())
    );
    let metadata = run.process_metadata[0].as_ref().unwrap();
    assert_eq!(metadata["processor"], "ds-core");
    assert_ne!(metadata["processor"], "ds-network-wasm");
    assert_eq!(metadata["engine_core_version"], "0.9.1");
    assert_eq!(metadata["drafting_source"], "ds-web");
    assert_eq!(metadata["lane"], PROCESSING_LANE);
    assert_eq!(metadata["result_counts"], json!({"tr":1,"lv_lines":2}));
    assert_eq!(metadata["status"], "success");
    assert!(metadata.get("migrated_from").is_none());
    // A failed job has no persisted verdict; job order is the input order.
    assert!(run.process_metadata[1].is_none());
    assert_eq!(run.jobs[1].transformer_name, "T1");
    // The run is additive over the batch shape: every existing member stays.
    let wire = serde_json::to_value(&run).unwrap();
    assert_eq!(wire["jobs"][0]["transformer_name"], "T2");
    assert_eq!(wire["provenance"]["processor"], "ds-core");
}

#[test]
fn a_receipt_without_engine_members_still_names_its_executor_and_nothing_more() {
    let run = captured()
        .decode_core_run(envelope(output(json!([])), json!({"jobs": 2})))
        .unwrap();
    assert_eq!(
        run.provenance,
        LvProcessProvenance {
            processor: CORE_PROCESSOR.into(),
            engine_core_version: None,
            executable_sha256: None,
            development_override: false,
        }
    );
    assert!(run.process_metadata[0]
        .as_ref()
        .unwrap()
        .get("engine_core_version")
        .is_none());
}

#[test]
fn the_verdict_is_error_then_actionable_warning_then_success() {
    let warning = |code: &str, severity: &str| json!({"code":code,"severity":severity,"count":1,"layer":null,"message":format!("{code} message")});
    for (warnings, status) in [
        (json!([warning("lost_nodes", "warning")]), "warning"),
        (json!([warning("lost_nodes", " Warning ")]), "warning"),
        (json!([warning("local_customer_source", "info")]), "success"),
        (
            json!([warning("lost_nodes", "warning"), warning("cycle", "error")]),
            "error",
        ),
    ] {
        let run = captured()
            .decode_core_run(envelope(output(warnings.clone()), receipt()))
            .unwrap();
        let metadata = run.process_metadata[0].as_ref().unwrap();
        assert_eq!(metadata["status"], status, "{warnings}");
        assert_eq!(
            metadata["warnings"].as_array().unwrap().len(),
            warnings.as_array().unwrap().len()
        );
        match status {
            "error" => {
                assert_eq!(metadata["error"]["code"], "cycle");
                assert_eq!(metadata["error_message"], "cycle message");
                assert!(metadata.get("warning").is_none());
            }
            "warning" => assert_eq!(metadata["warning"]["code"], "lost_nodes"),
            _ => assert!(metadata.get("warning").is_none() && metadata.get("error").is_none()),
        }
    }
}

#[test]
fn an_incompatible_result_refuses_the_whole_run_before_any_verdict() {
    let mut out = output(json!([]));
    out["jobs"][0]["transformer_name"] = json!("Other");
    assert_eq!(
        captured()
            .decode_core_run(envelope(out, receipt()))
            .unwrap_err()
            .code,
        "network_result_invalid"
    );
}
