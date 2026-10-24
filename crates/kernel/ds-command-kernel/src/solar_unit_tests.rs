use super::*;

fn output(name: &str, media_type: &str, bytes: &[u8]) -> Value {
    json!({
        "entry": {
            "name": name, "media_type": media_type, "byte_count": bytes.len(),
            "content_digest": format!("sha256:{:x}", Sha256::digest(bytes)),
        },
        "body_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

fn reply(outputs: Vec<Value>) -> String {
    json!({
        "schema_version": "ds-solar.result/v3",
        "engine": { "name": "ds-solar-engine", "version": "0.1.0" },
        "identity": { "city_id": "aderm_kyabe" },
        "result_digest": format!("sha256:{}", "b".repeat(64)),
        "timings": { "pipeline_ms": 1 },
        "plots_generated": true,
        "outputs": outputs,
    })
    .to_string()
}

const REPORT: &[u8] =
    br#"{"05_financial":{"financial_summary":{"irr":0.12}},"_root":{"city":"Kyabe"}}"#;

#[test]
fn verified_reply_projects_the_result_and_every_output() {
    let decoded = decode_reply(
        &reply(vec![
            output("report_input.json", "application/json", REPORT),
            output("plots/site/a.png", "image/png", b"\x89PNG"),
        ]),
        false,
    )
    .unwrap();
    assert_eq!(decoded.summary["schema"], REPLY_SCHEMA);
    assert_eq!(
        decoded.summary["result_digest"],
        format!("sha256:{}", "b".repeat(64))
    );
    assert_eq!(
        decoded.summary["result"]["identity"]["city_id"],
        "aderm_kyabe"
    );
    assert!(decoded.summary["result"].get("outputs").is_none());
    assert!(decoded.summary["result"].get("timings").is_none());
    assert_eq!(decoded.summary["files"].as_array().unwrap().len(), 2);
    assert_eq!(decoded.files[0].bytes, REPORT);
    let section = read_section(
        &decoded.files[0].bytes,
        "finance",
        &["financial_summary".into()],
    )
    .unwrap();
    assert_eq!(section["value"]["irr"], 0.12);
    assert_eq!(
        read_section(REPORT, "unknown", &[]).unwrap_err().code,
        "solar_dashboard_path_invalid"
    );
}

#[test]
fn corrupt_incomplete_or_unnamed_replies_refuse() {
    let mut corrupt = output("report_input.json", "application/json", REPORT);
    corrupt["body_base64"] = json!(base64::engine::general_purpose::STANDARD.encode(b"{}"));
    let unsafe_name = output("../report_input.json", "application/json", REPORT);
    let report = output("report_input.json", "application/json", REPORT);
    for (case, text) in [
        ("corrupt", reply(vec![corrupt])),
        ("unsafe name", reply(vec![unsafe_name])),
        (
            "no report input",
            reply(vec![output("a.png", "image/png", b"x")]),
        ),
        ("duplicate", reply(vec![report.clone(), report])),
        ("not a result", r#"{"schema_version":"other/v1"}"#.into()),
        ("not JSON", "{".into()),
    ] {
        assert_eq!(
            decode_reply(&text, false).unwrap_err().code,
            RESPONSE_INVALID,
            "{case}"
        );
    }
    let mut undigested: Value = serde_json::from_str(&reply(vec![])).unwrap();
    undigested["result_digest"] = json!("");
    assert_eq!(
        decode_reply(&undigested.to_string(), false)
            .unwrap_err()
            .code,
        RESPONSE_INVALID
    );
}

#[test]
fn unit_refusals_keep_their_typed_solar_code() {
    let refusal = decode_reply(
        r#"{"code":"city_input_not_prepared","message":"digest differs"}"#,
        true,
    )
    .unwrap_err();
    assert_eq!(refusal.code, "solar_city_input_not_prepared");
    assert_eq!(refusal.message, "digest differs");
    for text in [
        "panic",
        r#"{"code":"Bad Code","message":"x"}"#,
        r#"{"code":"","message":"x"}"#,
    ] {
        assert_eq!(
            decode_reply(text, true).unwrap_err().code,
            RESPONSE_INVALID,
            "{text}"
        );
    }
    assert_eq!(unavailable("no unit").code, ENGINE_UNAVAILABLE);
}

#[test]
fn requests_refuse_before_any_effect() {
    for prepared in [
        &b"\xff"[..],
        b"{",
        br#"{"schema_version":"ds-solar.prepared-city-input/v1"}"#,
    ] {
        assert_eq!(
            calculate_effect(prepared, b"").unwrap_err().code,
            REQUEST_INVALID
        );
    }
}

#[test]
fn run_settings_take_core_defaults_and_stay_closed() {
    assert_eq!(
        run_options(b"").unwrap(),
        json!({"run_id": null, "render_charts": true, "generate_documents": true, "report_drafts": ["apd"]})
    );
    assert_eq!(
        run_options(
            br#"{"run_id":"web-1","render_charts":false,"report_drafts":["plant","plant","apd"]}"#
        )
        .unwrap(),
        json!({"run_id": "web-1", "render_charts": false, "generate_documents": true, "report_drafts": ["plant", "apd"]})
    );
    for settings in [
        &br#"{"concurrency":2}"#[..],
        br#"{"report_drafts":["summary"]}"#,
        br#"{"run_id":"has space"}"#,
        br#"{"run_id":""}"#,
        b"[",
    ] {
        // Settings refuse before the prepared input is read.
        assert_eq!(
            calculate_effect(b"\xff", settings).unwrap_err().code,
            REQUEST_INVALID,
            "{}",
            String::from_utf8_lossy(settings)
        );
        assert!(run_options(settings).is_err());
    }
}
