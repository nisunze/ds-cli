//! Native process qualification; ignored tests must be explicitly executed.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::report_engineering::*;
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
fn required(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{key} is required for real process acceptance"))
}
fn request() -> ReportEngineeringRequestV1 {
    serde_json::from_value(json!({"schema":REPORT_ENGINEERING_REQUEST_SCHEMA_V1,"config_dfs":{},"review_layers":null,"demands":[{"transformer":"second","load_data":{}},{"transformer":"first","load_data":{}}]})).unwrap()
}
fn isolated(test: &str, engine: &Path, case: Option<&str>) {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env("DS_NETWORK_ENGINE_BIN", engine);
    if let Some(case) = case {
        command.env("DS_NETWORK_REPORT_CASE", case);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "requires built immutable Network owner and owning report fixture/golden"]
fn acceptance_report_real_owner_bytes_and_domain_refusal() {
    isolated(
        "isolated_report_real_owner",
        &required("DS_NETWORK_REPORT_REAL_ENGINE"),
        None,
    );
}
#[test]
#[ignore = "isolates selected executable environment"]
fn isolated_report_real_owner() {
    let input = fs::read(required("DS_NETWORK_REPORT_REQUEST")).unwrap();
    let request = decode_report_engineering_request(&input).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let outcome = client
        .report_engineering(&request, scratch.path(), &|| false)
        .unwrap();
    assert_eq!(
        outcome.bytes,
        fs::read(required("DS_NETWORK_REPORT_GOLDEN")).unwrap()
    );
    assert_eq!(
        outcome
            .result
            .findings
            .iter()
            .map(|f| f.code.as_str())
            .collect::<Vec<_>>(),
        [
            "transformer_protection_mismatch",
            "lv_span_exceeds_max_length"
        ]
    );
    assert!(
        matches!(outcome.result.demands[0].demand,TransformerDemandFact::Calculated{kva} if (kva-1.9635).abs()<1e-12)
    );
    let mut missing = request.clone();
    missing
        .config_dfs
        .get_mut("project_params")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("cos_phi");
    let result = client
        .report_engineering(&missing, scratch.path(), &|| false)
        .unwrap();
    assert!(
        matches!(&result.result.demands[0].demand,TransformerDemandFact::Unavailable{reason} if reason=="project_settings.cos_phi is missing; cannot size locally.")
    );
    assert_eq!(
        fs::read(required("DS_NETWORK_REPORT_REQUEST")).unwrap(),
        input
    );
}
#[test]
#[ignore = "requires compiled non-engineering adversarial receipt fixture"]
fn acceptance_report_adversarial_receipts_and_request_counts() {
    let root = tempfile::tempdir().unwrap();
    for case in ["valid", "digest", "input", "partial", "extra"] {
        let dir = root.path().join(case);
        fs::create_dir(&dir).unwrap();
        let binary = dir.join(format!("report-{case}"));
        fs::copy(required("DS_NETWORK_REPORT_ADVERSARIAL"), &binary).unwrap();
        isolated("isolated_report_adversarial", &binary, Some(case));
    }
}
#[test]
#[ignore = "isolates adversarial executable selection"]
fn isolated_report_adversarial() {
    let case = env::var("DS_NETWORK_REPORT_CASE").unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let result = client.report_engineering(&request(), scratch.path(), &|| false);
    let expected = match case.as_str() {
        "valid" => None,
        "digest" => Some("network_result_digest_mismatch"),
        "input" => Some("network_receipt_invalid"),
        "partial" | "extra" => Some("network_result_invalid"),
        _ => panic!("unknown case"),
    };
    if let Some(expected) = expected {
        assert_eq!(result.unwrap_err().code, expected);
    } else {
        assert_eq!(result.unwrap().result.demands.len(), 2);
        client
            .report_engineering(&request(), scratch.path(), &|| false)
            .unwrap();
        let mut sizing = request();
        sizing.sizing = Some(ReportSizingRequest {
            minimum_kva: None,
            transformers: vec![ReportSizingInput {
                transformer: "first".into(),
                plan_kva: None,
                ex_tr_size: None,
                categories: Default::default(),
                outgoing: Default::default(),
            }],
        });
        assert_eq!(
            client
                .report_engineering(&sizing, scratch.path(), &|| false)
                .unwrap_err()
                .code,
            "network_engine_incompatible",
            "the older report capability cannot silently consume sizing requests"
        );
        let mut feeders = request();
        feeders.feeders = Some(ReportFeederRequest {
            policy: ReportFeederPolicy {
                rows: Vec::new(),
                minimum: ReportFeederBound::Missing,
                maximum: ReportFeederBound::Missing,
            },
            minimum_kva: None,
            transformers: Vec::new(),
            feeders: Vec::new(),
        });
        assert_eq!(
            client
                .report_engineering(&feeders, scratch.path(), &|| false)
                .unwrap_err()
                .code,
            "network_engine_incompatible"
        );
        let mut duplicate = request();
        duplicate.demands.push(duplicate.demands[0].clone());
        assert_eq!(
            client
                .report_engineering(&duplicate, scratch.path(), &|| false)
                .unwrap_err()
                .code,
            "report_engineering_invalid"
        );
        assert_eq!(
            fs::read_to_string(client.executable().parent().unwrap().join("calls")).unwrap(),
            "build-info\nreport-engineering\nreport-engineering\n"
        );
    }
}
