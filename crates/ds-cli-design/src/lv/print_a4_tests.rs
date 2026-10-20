use super::*;
use ds_command_kernel::report_export::{INPUT_RECEIPT_SCHEMA, sha256_hex};
use ds_network_contracts::lv_processing::decode_native_lv_request;

fn binding() -> Value {
    let defaults: Value = serde_json::from_str(include_str!(
        "../../../../../ds-command-kernel/tests/fixtures/governed-renderer-defaults.json"
    ))
    .unwrap();
    let document = json!({"type":"line","paint":{},"layout":{},
        "metadata":{"print_renderer_defaults":defaults}});
    json!({"key": ds_command_kernel::printing::renderer_defaults::key(),
        "style_ref": "print_context/renderer_defaults_print", "revision_id": "a".repeat(64),
        "content_sha256": ds_command_kernel::style_plan::style_digest(&document),
        "scope": "global", "document": document})
}

/// A one-transformer export with its sealed receipt beside it, as
/// `project-export --project-config` writes them.
fn exported(directory: &Path, jobs: usize, seal: bool) -> (PathBuf, Vec<NativeLvJobV1>) {
    let config = json!({
        "printing_context": {"project_id": "project"},
        "printing_a4": {"schema": "ds.print-a4-document/v1", "project_id": "project"},
        "printing_renderer_defaults": binding(),
        "project_settings": [{"parameter": "report_locale", "value": "en"}],
    });
    let layer = json!({"type": "FeatureCollection", "features": []});
    let jobs: Vec<Value> = ["agahehe", "agasharu"][..jobs]
        .iter()
        .map(|name| json!({"transformer_name": name, "gdfs": {"tr": layer}, "config_dfs": config}))
        .collect();
    let request = json!({"schema": "ds.fast-lv.request/v1", "jobs": jobs});
    let path = directory.join("agahehe.request.json");
    std::fs::write(&path, request.to_string()).unwrap();
    if seal {
        let mut sheets = config.clone();
        sheets["printing_context"]["project"] = json!("Rwanda Template");
        let sheets_json = sheets.to_string();
        let receipt = InputReceipt {
            local_print_recipe: None,
            schema: INPUT_RECEIPT_SCHEMA,
            country: "Rwanda".into(),
            sheets_sha256: sha256_hex(sheets_json.as_bytes()),
            sheets_json,
            reference_semantic_sha256: "d".repeat(64),
        };
        std::fs::write(
            printing_receipt_path(&path),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
    }
    let jobs = decode_native_lv_request(&std::fs::read(&path).unwrap())
        .unwrap()
        .jobs;
    (path, jobs)
}

fn inputs(directory: &Path, extra: &[&str]) -> Inputs {
    let mut tokens = vec![
        "--input".to_owned(),
        directory.join("agahehe.request.json").display().to_string(),
        "--out".to_owned(),
        directory.join("agahehe.vd.json").display().to_string(),
    ];
    tokens.extend(extra.iter().map(|token| token.to_string()));
    parse(&super::super::voltage_drop::COMMAND, &tokens).unwrap()
}

/// Stands in for the Reporter: reaching it proves every local check passed.
fn reached(lane: &str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure> {
    Err(Failure::unavailable("preflight_reached", lane.to_owned()))
}

fn refused(directory: &Path, jobs: &[NativeLvJobV1], extra: &[&str]) -> String {
    let input = directory.join("agahehe.request.json");
    match admit_with(&inputs(directory, extra), &input, jobs, reached) {
        Err(error) => error.code().to_owned(),
        Ok(_) => panic!("admitted"),
    }
}

#[test]
fn without_the_flag_nothing_is_admitted_or_preflighted() {
    let directory = tempfile::tempdir().unwrap();
    let (input, jobs) = exported(directory.path(), 1, false);
    let admitted = admit_with(&inputs(directory.path(), &[]), &input, &jobs, reached).unwrap();
    assert!(admitted.is_none());
}

#[test]
fn a_sealed_single_export_reaches_the_lane_reporter_before_any_calculation() {
    let directory = tempfile::tempdir().unwrap();
    let (_, jobs) = exported(directory.path(), 1, true);
    let pdf = directory.path().join("agahehe.pdf").display().to_string();
    let input = directory.path().join("agahehe.request.json");
    let error = admit_with(
        &inputs(directory.path(), &["--print-a4", &pdf, "--lane", "canary"]),
        &input,
        &jobs,
        reached,
    )
    .err()
    .unwrap();
    assert_eq!(
        (error.code(), error.message()),
        ("preflight_reached", "canary")
    );
}

#[test]
fn scenarios_unsealed_or_multiple_exports_and_existing_pdfs_refuse_locally() {
    let directory = tempfile::tempdir().unwrap();
    let (_, jobs) = exported(directory.path(), 1, true);
    let pdf = directory.path().join("agahehe.pdf").display().to_string();
    for scenario in [
        &["--year", "3"][..],
        &["--outlook", "0,5"],
        &["--no-outlook"],
        &["--load", "Residential=100"],
        &["--set", "vd_limit_pct=8"],
    ] {
        let mut extra = vec!["--print-a4", pdf.as_str()];
        extra.extend_from_slice(scenario);
        assert_eq!(
            refused(directory.path(), &jobs, &extra),
            "vd_print_input_invalid"
        );
    }
    let (_, two) = exported(directory.path(), 2, true);
    assert_eq!(
        refused(directory.path(), &two, &["--print-a4", &pdf]),
        "vd_print_input_invalid"
    );
    std::fs::write(directory.path().join("agahehe.pdf"), b"%PDF").unwrap();
    assert_eq!(
        refused(directory.path(), &jobs, &["--print-a4", &pdf]),
        "vd_print_output_exists"
    );
    let unsealed = tempfile::tempdir().unwrap();
    let (_, jobs) = exported(unsealed.path(), 1, false);
    let pdf = unsealed.path().join("agahehe.pdf").display().to_string();
    assert_eq!(
        refused(unsealed.path(), &jobs, &["--print-a4", &pdf]),
        "vd_print_input_invalid"
    );
}

#[test]
fn a_failed_print_keeps_the_written_result_and_the_reporter_code() {
    let error = Failure::failed("engine_refused", "voltage_drop_pdf_not_calculated: refused")
        .remedy("read detail.engine");
    let wrapped = failed(error, json!({"out": "agahehe.vd.json"}));
    assert_eq!(wrapped.code(), "vd_print_failed");
    let detail = wrapped.detail_value().unwrap();
    assert_eq!(detail["result"]["out"], "agahehe.vd.json");
    assert_eq!(detail["printed"], false);
    assert_eq!(detail["print_error"]["code"], "engine_refused");
}
