use super::*;
use ds_network_contracts::grid_unit::{encode_reply, GridRefusal};

const IDENTITY: &str = r#"{"schema":"ds.reporter-engine/v1","protocolMajor":1,"environment":"wasm","coreVersion":"0.1.0","sourceSha":"5d903f1","dependencyClosureSha256":"0","capabilities":["render_held_svg_pdf_v1","export_transformer_report_v1","export_combined_report_v1"]}"#;
// Float text the engines wrote; a value round trip would print `0` and `1.1`.
const REQUEST: &str = r#"{"transformer_document":"/held/transformer.json","network_config":"/held/config.json","transformer":"upgrade_rwatano","out_dir":"/out","offset":-0.0,"factor":1.10,"formats":["shp","xlsx"]}"#;
const ENGINEERING: &str = r#"{"schema":"ds.network.report-engineering.result/v1","findings":[],"demands":[{"transformer":"t","demand":{"status":"calculated","kva":-0.0}}]}"#;

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn export() -> ReporterExport {
    let mut export = ReporterExport::new("transformer", REQUEST.as_bytes(), IDENTITY).unwrap();
    export.hold("/held/transformer.json", b"{}").unwrap();
    export
        .hold("/held/config.json", b"{\"sheets\":{}}")
        .unwrap();
    export
}

fn required(digests: &[&str]) -> String {
    json!({
        "code": ENGINEERING_REQUIRED,
        "message": "Network report engineering is required",
        "requests": digests.iter().map(|digest| json!({
            "request_digest": digest,
            "request_json": format!(r#"{{"schema":"ds.network.report-engineering.request/v1","n":{}}}"#, &digest[..2]),
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

fn network_answer(text: &str) -> Vec<u8> {
    let operation = GridOperation::Model(GridModelMethod::ReportEngineering);
    let receipt = json!({"schema":"ds.network.task-result/v1","outcome":"completed",
        "artifact":{"file":"engineering.json","byteCount":text.len(),"sha256":digest(text),
        "schema":"ds.network.report-engineering.result/v1"}});
    encode_reply(
        operation,
        &Ok(GridResult::Artifact {
            bytes: text.as_bytes().to_vec(),
            receipt_json: receipt.to_string(),
        }),
    )
}

fn completed(files: &[(&str, &[u8])]) -> String {
    json!({
        "schema": "ds.reporter-held-transformer-export-result/v1",
        "result": {
            "task": "export_transformer_report",
            "status": "partial",
            "blockers": [{"format": "voltage_drop", "code": "voltage_drop_missing"}],
            "artifacts": files.iter().map(|(name, bytes)| json!({
                "format": name.rsplit('.').next(), "filename": name, "path": format!("/out/{name}"),
                "bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(bytes)),
            })).collect::<Vec<_>>(),
        },
        "outputs": files.iter().map(|(name, bytes)| json!({
            "path": format!("/out/{name}"),
            "body_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

#[test]
fn an_older_unit_refuses_by_the_capability_the_export_calls() {
    let older = r#"{"schema":"ds.reporter-engine/v1","capabilities":["render_held_svg_pdf_v1"]}"#;
    for kind in ["transformer", "combined"] {
        let refusal = ReporterExport::new(kind, REQUEST.as_bytes(), older)
            .err()
            .unwrap();
        assert_eq!(refusal.code, ENGINE_UPDATE_REQUIRED, "{refusal}");
    }
    // A newer unit that declares more is admitted.
    let newer = r#"{"schema":"ds.reporter-engine/v1","capabilities":["export_combined_report_v1","a_future_capability_v9"],"addedMember":true}"#;
    assert!(ReporterExport::new("combined", REQUEST.as_bytes(), newer).is_ok());
    let solar =
        r#"{"schema":"ds.solar-engine/v1","capabilities":["export_transformer_report_v1"]}"#;
    assert_eq!(
        ReporterExport::new("transformer", REQUEST.as_bytes(), solar)
            .err()
            .unwrap()
            .code,
        ENGINE_UNAVAILABLE
    );
}

#[test]
fn requests_are_admitted_closed() {
    for (kind, request) in [
        ("print", REQUEST),
        ("transformer", "[]"),
        ("transformer", "not json"),
        ("transformer", "{\"a\":1} trailing"),
    ] {
        let refusal = ReporterExport::new(kind, request.as_bytes(), IDENTITY)
            .err()
            .unwrap();
        assert_eq!(refusal.code, REQUEST_INVALID, "{kind} {request}");
    }
    let mut export = export();
    assert_eq!(
        export.hold("/held/config.json", b"again").unwrap_err().code,
        REQUEST_INVALID
    );
    assert_eq!(export.hold("", b"").unwrap_err().code, REQUEST_INVALID);
}

#[test]
fn the_effect_is_the_closed_envelope_with_the_request_text_verbatim() {
    let effect = export().effect().unwrap();
    assert_eq!(effect.unit, UNIT);
    assert_eq!(effect.entry, "reporter.export.transformer");
    assert!(
        effect.payload.contains(REQUEST),
        "the request crosses as written"
    );
    let envelope: Value = serde_json::from_str(&effect.payload).unwrap();
    let keys: Vec<_> = envelope.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["schema", "request", "inputs", "network_results"]);
    assert_eq!(envelope["schema"], "ds.reporter-held-transformer-export/v1");
    assert_eq!(envelope["inputs"][1]["name"], "/held/config.json");
    assert_eq!(
        envelope["inputs"][1]["body_base64"],
        base64::engine::general_purpose::STANDARD.encode(b"{\"sheets\":{}}")
    );
    assert_eq!(envelope["network_results"], json!([]));
}

#[test]
fn network_facts_are_asked_answered_and_held_as_the_engine_wrote_them() {
    let mut export = export();
    let (a, b) = ("a".repeat(64), "b".repeat(64));
    let ReportStep::Network(asks) = export.reporter_reply(&required(&[&a, &b]), true).unwrap()
    else {
        panic!("the unit named Network facts");
    };
    assert_eq!(asks.len(), 2);
    assert_eq!(asks[0].entry, GridEntry::Model);
    let request = GridRequest::decode(&asks[0].payload).unwrap();
    assert_eq!(
        request.operation,
        GridOperation::Model(GridModelMethod::ReportEngineering)
    );
    assert_eq!(
        request.args,
        vec![GridArg::Bytes(
            br#"{"schema":"ds.network.report-engineering.request/v1","n":aa}"#.to_vec()
        )]
    );
    // The unit is not called again until every fact is answered.
    export
        .network_reply(&a, &network_answer(ENGINEERING))
        .unwrap();
    assert_eq!(export.effect().unwrap_err().code, REQUEST_INVALID);
    export
        .network_reply(&b, &network_answer(ENGINEERING))
        .unwrap();
    let envelope: Value = serde_json::from_str(&export.effect().unwrap().payload).unwrap();
    assert_eq!(envelope["network_results"][0]["request_digest"], a);
    assert_eq!(envelope["network_results"][0]["result_json"], ENGINEERING);
    assert_eq!(
        envelope["network_results"][1]["receipt"]["artifact"]["byteCount"],
        ENGINEERING.len()
    );
    // A fact the unit names again after its answer is a loop, not a retry.
    assert_eq!(
        export
            .reporter_reply(&required(&[&a]), true)
            .unwrap_err()
            .code,
        ENGINEERING_UNANSWERED
    );
}

#[test]
fn network_answers_are_admitted_against_their_receipt() {
    let mut export = export();
    let a = "a".repeat(64);
    export.reporter_reply(&required(&[&a]), true).unwrap();
    let operation = GridOperation::Model(GridModelMethod::ReportEngineering);
    // Bytes that are not the artifact the receipt names.
    let mut tampered = json!({"artifact":{"byteCount":ENGINEERING.len(),"sha256":digest("other")}});
    let forged = encode_reply(
        operation,
        &Ok(GridResult::Artifact {
            bytes: ENGINEERING.as_bytes().to_vec(),
            receipt_json: tampered.to_string(),
        }),
    );
    assert_eq!(
        export.network_reply(&a, &forged).unwrap_err().code,
        NETWORK_RESPONSE_INVALID
    );
    tampered["artifact"]["sha256"] = json!(digest(ENGINEERING));
    tampered["artifact"]["byteCount"] = json!(1);
    let short = encode_reply(
        operation,
        &Ok(GridResult::Artifact {
            bytes: ENGINEERING.as_bytes().to_vec(),
            receipt_json: tampered.to_string(),
        }),
    );
    assert_eq!(
        export.network_reply(&a, &short).unwrap_err().code,
        NETWORK_RESPONSE_INVALID
    );
    // An older Network unit has no report engineering.
    let older = encode_reply(
        operation,
        &Err(ds_network_contracts::grid_unit::method_unavailable(
            "reportEngineering",
        )),
    );
    assert_eq!(
        export.network_reply(&a, &older).unwrap_err().code,
        NETWORK_UPDATE_REQUIRED
    );
    // The engine's own refusal keeps its code.
    let refused = encode_reply(
        operation,
        &Err(GridRefusal::new(
            "report_engineering_invalid",
            "bad request",
        )),
    );
    assert_eq!(
        export.network_reply(&a, &refused).unwrap_err().code,
        "report_engineering_invalid"
    );
    assert_eq!(
        export.network_reply(&a, b"not a frame").unwrap_err().code,
        NETWORK_RESPONSE_INVALID
    );
    // An answer to a fact nobody asked.
    assert_eq!(
        export
            .network_reply(&"c".repeat(64), &network_answer(ENGINEERING))
            .unwrap_err()
            .code,
        REQUEST_INVALID
    );
    // A missing Network answer keeps the export from running.
    assert_eq!(export.effect().unwrap_err().code, REQUEST_INVALID);
}

#[test]
fn unit_refusals_keep_their_own_code() {
    let mut export = export();
    let print =
        json!({"code":"report_print_native_only","message":"prints run on DS Core"}).to_string();
    assert_eq!(
        export.reporter_reply(&print, true).unwrap_err().code,
        "report_print_native_only"
    );
    for malformed in [
        "boom".to_string(),
        json!({"code":"Not A Code","message":"x"}).to_string(),
        json!({"code":ENGINEERING_REQUIRED,"message":"x","requests":[]}).to_string(),
        json!({"code":ENGINEERING_REQUIRED,"message":"x","requests":[{"request_digest":"short","request_json":"{}"}]}).to_string(),
    ] {
        assert_eq!(
            export.reporter_reply(&malformed, true).unwrap_err().code,
            RESPONSE_INVALID,
            "{malformed}"
        );
    }
}

#[test]
fn every_output_is_the_artifact_the_result_names() {
    let files: [(&str, &[u8]); 2] = [("t.xlsx", b"PK\x03\x04xlsx"), ("t.kmz", b"PK\x03\x04kmz")];
    let mut export = export();
    let ReportStep::Completed(reply) = export.reporter_reply(&completed(&files), false).unwrap()
    else {
        panic!("the export completed");
    };
    assert_eq!(reply.summary["schema"], REPLY_SCHEMA);
    assert_eq!(reply.summary["status"], "partial");
    assert_eq!(reply.summary["blockers"][0]["code"], "voltage_drop_missing");
    assert_eq!(reply.files.len(), 2);
    assert_eq!(reply.files[0].name, "t.xlsx");
    assert_eq!(reply.files[0].bytes, files[0].1);
    assert_eq!(reply.summary["files"][1]["byte_count"], files[1].1.len());

    // Bytes that differ from their artifact refuse the whole export.
    let mut tampered: Value = serde_json::from_str(&completed(&files)).unwrap();
    tampered["outputs"][0]["body_base64"] =
        json!(base64::engine::general_purpose::STANDARD.encode(b"other"));
    assert_eq!(
        export
            .reporter_reply(&tampered.to_string(), false)
            .unwrap_err()
            .code,
        RESPONSE_INVALID
    );
    let mut unsafe_name: Value = serde_json::from_str(&completed(&files)).unwrap();
    unsafe_name["result"]["artifacts"][0]["filename"] = json!("../t.xlsx");
    assert_eq!(
        export
            .reporter_reply(&unsafe_name.to_string(), false)
            .unwrap_err()
            .code,
        RESPONSE_INVALID
    );
    let mut combined_schema: Value = serde_json::from_str(&completed(&files)).unwrap();
    combined_schema["schema"] = json!("ds.reporter-held-combined-export-result/v1");
    assert_eq!(
        export
            .reporter_reply(&combined_schema.to_string(), false)
            .unwrap_err()
            .code,
        RESPONSE_INVALID
    );
}
