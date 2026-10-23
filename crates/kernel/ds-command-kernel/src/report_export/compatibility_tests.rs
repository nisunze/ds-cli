//! Independent-release compatibility of the Reporter answer (owner 22 Oct
//! 2026): a Reporter released after this Kernel may add members and warning
//! codes; Core accepts them and behaves the same. A missing required member or
//! a wrong type is still refused with the typed `ResultRefusal::Invalid`.
use super::tests::{engine, layers, result_document, simple_receipt};
use super::*;
use std::path::PathBuf;

fn plan() -> ExportPlan {
    let selection = report_formats::normalize_output_selection(
        None,
        Some(vec!["shp".into(), "xlsx".into(), "voltage_drop".into()]),
        None,
    )
    .unwrap();
    plan_transformer_export(
        "proj-1",
        &simple_receipt("Benin"),
        "tx_a",
        7,
        &layers(),
        Some(&selection),
    )
    .unwrap()
}

fn verify(
    document: &Value,
    plan: &ExportPlan,
    out: &Path,
) -> Result<VerifiedResult, ResultRefusal> {
    verify_result(&serde_json::to_vec(document).unwrap(), plan, out)
}

fn invalid_message(document: &Value, plan: &ExportPlan, out: &Path) -> String {
    match verify(document, plan, out).unwrap_err() {
        ResultRefusal::Invalid { message } => message,
        other => panic!("expected a typed invalid refusal, got {other:?}"),
    }
}

fn receipt(plan: &ExportPlan, verified: &VerifiedResult) -> Value {
    run_receipt(&RunReceiptFacts {
        plan,
        client_run_id: &format!("report-{}", "0".repeat(32)),
        engine: &engine(),
        result: verified,
        artifact_dir: "tx_a",
        now_ms: 1,
        expected_content_digest: Some(&plan.content_digest),
        print_context_sha256: None,
        print_context_layers: &[],
        print_context_omitted: &[],
    })
    .unwrap()
}

fn future_warning() -> Value {
    json!({
        "code": "lv_future_finding", "transformer": "TX_A", "output_id": "xlsx",
        "layout_id": "", "element_id": "LV_Line_07", "total_rows": 0, "omitted_rows": 0,
        "message": "LV_Line_07: a finding only a newer Reporter knows",
        "severity": "advisory"
    })
}

fn span_warning() -> Value {
    json!({
        "code": "lv_span_exceeds_max_length", "transformer": "tx_a", "output_id": "xlsx",
        "layout_id": "", "element_id": "spans", "total_rows": 0, "omitted_rows": 0,
        "message": "span 4 exceeds 50 m"
    })
}

#[test]
fn a_newer_reporter_result_with_additive_members_verifies_the_same() {
    let plan = plan();
    let out = PathBuf::from("/tmp/run/out");
    let mut current = result_document(&plan, &out);
    current["warnings"] = json!([span_warning()]);
    let mut newer = current.clone();
    newer["future_summary"] = json!({"pages": 4, "renderer": "next"});
    newer["admin_bounds"]["future_coverage"] = json!(0.5);
    for artifact in newer["artifacts"].as_array_mut().unwrap() {
        artifact["page_count"] = json!(3);
    }
    newer["warnings"][0]["future_rect"] = json!([0, 0, 1, 1]);
    let accepted = verify(&newer, &plan, &out).unwrap();
    let baseline = verify(&current, &plan, &out).unwrap();
    assert_eq!(accepted, baseline);
    assert_eq!(accepted.artifacts.len(), 3);
    assert_eq!(accepted.warnings.len(), 1);
    assert_eq!(receipt(&plan, &accepted), receipt(&plan, &baseline));
    // A missing required member is still the typed refusal.
    let mut missing = newer;
    missing["artifacts"][0]
        .as_object_mut()
        .unwrap()
        .remove("sha256");
    assert!(invalid_message(&missing, &plan, &out).contains("malformed"));
}

#[test]
fn an_unknown_warning_code_is_carried_with_its_code_and_message() {
    let plan = plan();
    let out = PathBuf::from("/tmp/run/out");
    let mut document = result_document(&plan, &out);
    document["warnings"] = json!([future_warning()]);
    let verified = verify(&document, &plan, &out).unwrap();
    assert_eq!(verified.warnings.len(), 1);
    let warning = &verified.warnings[0];
    assert_eq!(warning.code, "lv_future_finding");
    assert_eq!(
        warning.message,
        "LV_Line_07: a finding only a newer Reporter knows"
    );
    assert_eq!(warning.element_id, "LV_Line_07");
    // This Kernel does not know the code; it is carried, not refused, and it
    // is cartographic: only the named engineering codes hold approval.
    assert!(known_print_warning(warning, &plan.transformer, &plan.formats).is_none());
    assert!(valid_print_warning(
        warning,
        &plan.transformer,
        &plan.formats
    ));
    assert!(!ENGINEERING_REVIEW_CODES.contains(&warning.code.as_str()));
    // Every artifact of the run is still verified and receipted.
    assert_eq!(verified.artifacts.len(), 3);
    assert!(verified.failed.is_empty());
    let receipt = receipt(&plan, &verified);
    assert_eq!(receipt["warnings"][0]["code"], "lv_future_finding");
    assert_eq!(
        receipt["warnings"][0]["message"],
        "LV_Line_07: a finding only a newer Reporter knows"
    );
    // A run-level warning that names no transformer or output is carried too.
    let mut run_level = future_warning();
    run_level["transformer"] = json!("");
    run_level["output_id"] = json!("");
    document["warnings"] = json!([run_level]);
    assert_eq!(verify(&document, &plan, &out).unwrap().warnings.len(), 1);
}

#[test]
fn an_unknown_warning_outside_the_shared_bounds_or_missing_a_member_still_refuses() {
    let plan = plan();
    let out = PathBuf::from("/tmp/run/out");
    let document = result_document(&plan, &out);
    for (key, invalid) in [
        ("code", json!("Lv Future Finding")),
        ("code", json!("")),
        ("code", json!(format!("lv_{}", "x".repeat(62)))),
        ("code", json!("9_starts_with_a_digit")),
        ("transformer", json!("tx_b")),
        ("output_id", json!("pdf__a3")),
        ("element_id", json!("line\u{7}07")),
        ("layout_id", json!("l".repeat(241))),
        ("omitted_rows", json!(1)),
        ("message", json!("m".repeat(2049))),
    ] {
        let mut warning = future_warning();
        warning[key] = invalid.clone();
        let mut changed = document.clone();
        changed["warnings"] = json!([warning]);
        assert!(
            invalid_message(&changed, &plan, &out).contains("print warnings"),
            "{key}: {invalid}"
        );
    }
    for (key, wrong) in [("message", Value::Null), ("total_rows", json!("0"))] {
        let mut warning = future_warning();
        match wrong {
            Value::Null => {
                warning.as_object_mut().unwrap().remove(key);
            }
            other => warning[key] = other,
        }
        let mut changed = document.clone();
        changed["warnings"] = json!([warning]);
        assert!(
            invalid_message(&changed, &plan, &out).contains("malformed"),
            "{key}"
        );
    }
    let mut excessive = document.clone();
    excessive["warnings"] = json!(vec![future_warning(); MAX_PRINT_WARNINGS + 1]);
    assert!(invalid_message(&excessive, &plan, &out).contains("print warnings"));
}

#[test]
fn a_known_warning_code_keeps_its_exact_shape() {
    let plan = plan();
    let out = PathBuf::from("/tmp/run/out");
    let mut document = result_document(&plan, &out);
    // A known engineering code names the audited layer exactly.
    document["warnings"] = json!([span_warning()]);
    assert_eq!(verify(&document, &plan, &out).unwrap().warnings.len(), 1);
    document["warnings"][0]["element_id"] = json!("tr");
    assert!(invalid_message(&document, &plan, &out).contains("print warnings"));
}

#[test]
fn a_newer_reporter_build_identity_with_additive_facts_and_revision_is_the_same_engine() {
    let source = "a".repeat(40);
    let info = json!({
        "schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
        "package_version": "0.1.0", "source_sha": source,
        "engine_version": format!("ds-network-reporter@0.1.0+{source}"),
        "cargo_lock_sha256": "d".repeat(64), "features": [], "rustc": "rustc 1.95.0",
        "target": "x86_64-unknown-linux-gnu", "profile": "release",
        "schemas": ["ds-network-reporter.task-result/v1", "ds.engine-build/v1"],
        "build_manifest_sha256": "f".repeat(64)
    });
    let current = engine_identity(&info).unwrap();
    let mut newer = info.clone();
    newer["schema"] = json!("ds.engine-build/v2");
    newer["sbom_sha256"] = json!("e".repeat(64));
    newer["schemas"]
        .as_array_mut()
        .unwrap()
        .push(json!("ds-network-reporter.task-result/v2"));
    assert_eq!(engine_identity(&newer).unwrap(), current);
    let mut other = info.clone();
    other["schema"] = json!("ds.module-build/v1");
    assert!(engine_identity(&other)
        .unwrap_err()
        .contains("outside the closed contract"));
    let mut wrong = info;
    wrong["features"] = json!("none");
    assert!(engine_identity(&wrong).unwrap_err().contains("malformed"));
}
