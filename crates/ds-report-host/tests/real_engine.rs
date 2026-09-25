//! The host over the real engine.
//!
//! Every other test scripts the engine. This one runs the installed
//! `ds-report` over the reporter's own checked-in synthetic room and
//! configuration — the same inputs `examples/printing/README.md` documents —
//! and proves the whole lifecycle with real bytes: geospatial, tabular AND
//! print artifacts placed under the output root, each one's digest verified,
//! the receipts written, staging gone.
//!
//! It needs the engine: set `DS_REPORT_BIN` to a built `ds-report` (the
//! reporter's `target/debug/ds-report` will do) and the sibling
//! `ds-network-reporter` checkout for its examples. Run with `--ignored`;
//! missing prerequisites fail rather than report unexecuted evidence as passing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use ds_command_kernel::report_export::InputReceipt;
use ds_report_host::{
    answer, execute, run_batch, run_transformer_report, transformer_page, BatchSettings,
    EngineExit, HeldContext, HeldRoom, HeldState, HostFailure, PageRef, PreviewRequest,
    PreviewSettings, ReportEngine, RunSettings, TransformerReportInputs,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

struct ProcessEngine {
    binary: PathBuf,
}

impl ReportEngine for ProcessEngine {
    fn build_info(&self) -> Result<Value, HostFailure> {
        let output = Command::new(&self.binary)
            .arg("build-info")
            .output()
            .map_err(|error| HostFailure::new("reporter_engine_missing", error.to_string()))?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| HostFailure::new("callee_contract_mismatch", error.to_string()))
    }

    fn export_transformer_report(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        let output = Command::new(&self.binary)
            .arg("export-transformer-report")
            .arg("--request")
            .arg(request)
            .arg("--result")
            .arg(result)
            .output()
            .map_err(|error| HostFailure::new("reporter_engine_missing", error.to_string()))?;
        Ok(EngineExit {
            succeeded: output.status.success(),
            summary: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn examples() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-network-reporter/examples/printing");
    root.is_dir().then_some(root)
}

fn receipt_from_example(config: &Path) -> InputReceipt {
    let document: Value = serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
    // The receipt carries the sheets exactly as the service serialized them;
    // here the example file's own `sheets` member, re-serialized once, plays
    // that role.
    let sheets_json = serde_json::to_string(&document["sheets"]).unwrap();
    InputReceipt {
        local_print_recipe: None,
        schema: 1,
        country: "Exampleland".to_string(),
        sheets_sha256: format!("{:x}", Sha256::digest(sheets_json.as_bytes())),
        sheets_json,
        reference_semantic_sha256: "7".repeat(64),
    }
}

fn room(path: &Path, name: &str) -> TransformerReportInputs {
    let document: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let layers: BTreeMap<String, Value> = document["layers"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    TransformerReportInputs {
        transformer: name.to_string(),
        server_version: 3,
        layers,
        content_digest: None,
        selection: None,
        print_context: None,
        sheet: None,
        survey: None,
    }
}

#[test]
#[ignore = "requires DS_REPORT_BIN and the reporter printing fixtures"]
fn the_installed_engine_produces_prints_and_data_the_host_proves_and_places() {
    let binary = std::env::var_os("DS_REPORT_BIN")
        .map(PathBuf::from)
        .expect("DS_REPORT_BIN must name the locally built reporter");
    let examples = examples().expect("the reporter printing examples must be present");
    let engine = ProcessEngine { binary };
    let receipt = receipt_from_example(&examples.join("network-config.a0-a3.example.json"));
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("reports");
    let staging = temp.path().join("staging");
    let settings = RunSettings {
        project_id: "example-project",
        receipt: &receipt,
        admin_bounds: None,
        staging_root: &staging,
        out_root: &out,
    };

    let outcome = run_transformer_report(
        &engine,
        &settings,
        &room(&examples.join("rooms/tr_example_a.json"), "tr_example_a"),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
    // The example policy names every individual format, prints included.
    let formats: Vec<&str> = outcome
        .artifacts
        .iter()
        .map(|artifact| artifact.format.as_str())
        .collect();
    assert_eq!(formats, ["shp", "kmz", "xlsx", "gpkg", "pdf_a0", "pdf_a3"]);
    for artifact in &outcome.artifacts {
        let path = outcome.artifact_dir.join(&artifact.filename);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            bytes.len() as u64,
            artifact.size_bytes,
            "{}",
            artifact.filename
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            artifact.sha256,
            "{}",
            artifact.filename
        );
        if artifact.format.starts_with("pdf") {
            assert!(
                bytes.starts_with(b"%PDF-"),
                "{} is not a PDF",
                artifact.filename
            );
            assert_eq!(artifact.content_type, "application/pdf");
        }
    }
    assert!(outcome.receipt_path.is_file());
    assert_eq!(outcome.receipt["publication"]["state"], "local_only");
    assert_eq!(outcome.receipt["country"], "Exampleland");
    assert_eq!(outcome.receipt["admin_bounds"]["configured"], false);
    assert!(std::fs::read_dir(&staging).unwrap().next().is_none());

    // A batch over both example rooms, two engines at once, ends complete.
    let batch_out = temp.path().join("batch");
    let batch = run_batch(
        &engine,
        &BatchSettings {
            run: RunSettings {
                out_root: &batch_out,
                ..settings.clone()
            },
            lane: "stable",
            concurrency: 2,
            media_grant: None,
        },
        &["tr_example_b".to_string(), "tr_example_a".to_string()],
        |name| Ok(room(&examples.join(format!("rooms/{name}.json")), name)),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
    assert_eq!(batch.receipt["status"], "completed");
    assert_eq!(batch.receipt["completed"], 2);
    assert_eq!(batch.receipt["results"][0]["transformer"], "tr_example_a");
    assert!(batch_out
        .join("tr_example_b")
        .join("tr_example_b.a3.pdf")
        .is_file());
    assert!(batch_out.join("report-batch.json").is_file());
}

#[test]
#[ignore = "requires DS_REPORT_BIN and the reporter printing fixtures"]
fn a_survey_policy_delivers_the_print_and_names_every_omitted_form() {
    let binary = std::env::var_os("DS_REPORT_BIN")
        .map(PathBuf::from)
        .expect("DS_REPORT_BIN must name the locally built reporter");
    let examples = examples().expect("the reporter printing examples must be present");
    let engine = ProcessEngine { binary };
    let mut receipt = receipt_from_example(&examples.join("network-config.a0-a3.example.json"));
    let mut sheets: Value = serde_json::from_str(&receipt.sheets_json).unwrap();
    let settings = sheets["project_settings"].as_array_mut().unwrap();
    settings.push(
        serde_json::json!({"parameter":"include_survey_data_in_transformer_exports","value":true}),
    );
    settings.push(serde_json::json!({"parameter":"survey_forms_in_transformer_exports","value":["field_notes","transformer_survey"]}));
    receipt.sheets_json = serde_json::to_string(&sheets).unwrap();
    receipt.sheets_sha256 = format!("{:x}", Sha256::digest(receipt.sheets_json.as_bytes()));
    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("staging");
    let reports = temp.path().join("reports");
    let outcome = run_batch(
        &engine,
        &BatchSettings {
            run: RunSettings {
                project_id: "example-project",
                receipt: &receipt,
                admin_bounds: None,
                staging_root: &staging,
                out_root: &reports,
            },
            lane: "stable",
            concurrency: 1,
            media_grant: None,
        },
        &["tr_example_a".to_string()],
        |name| Ok(room(&examples.join(format!("rooms/{name}.json")), name)),
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
    assert_eq!(outcome.receipt["status"], "completed");
    let omitted = &outcome.receipt["survey_layers_omitted"];
    assert_eq!(omitted["code"], "print_survey_layers_omitted");
    assert_eq!(
        omitted["forms"],
        serde_json::json!(["field_notes", "transformer_survey"])
    );
    assert_eq!(omitted["context_omitted"][0]["layer"], "field_notes");
    assert_eq!(
        outcome.receipt["results"][0]["survey_layers_omitted"],
        *omitted
    );
    assert_eq!(outcome.runs[0].receipt["survey_layers_omitted"], *omitted);
    assert!(outcome.runs[0]
        .artifacts
        .iter()
        .any(|artifact| artifact.format == "pdf_a3"));
}

#[test]
#[ignore = "requires DS_REPORT_BIN and the reporter printing fixtures"]
fn a_preview_page_is_the_engines_svg_over_the_synthetic_room() {
    let binary = std::env::var_os("DS_REPORT_BIN")
        .map(PathBuf::from)
        .expect("DS_REPORT_BIN must name the locally built reporter");
    let examples = examples().expect("the reporter printing examples must be present");
    let engine = ProcessEngine { binary };
    let receipt = receipt_from_example(&examples.join("network-config.a0-a3.example.json"));
    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("staging");
    let inputs = room(&examples.join("rooms/tr_example_a.json"), "tr_example_a");
    let held = HeldRoom {
        transformer: inputs.transformer.clone(),
        server_version: inputs.server_version,
        layers: inputs.layers.clone(),
        content_digest: None,
        media_grant: None,
        sheet: None,
    };
    let mut draft = ds_command_kernel::printing::default_layout();
    draft.id = "preview-draft".into();
    draft.name = "Preview draft".into();
    let page = transformer_page(
        &engine,
        &PreviewSettings {
            project_id: "example-project",
            receipt: &receipt,
            admin_bounds: None,
            staging_root: &staging,
        },
        &held,
        &draft,
        |contexts| {
            assert!(contexts.is_empty(), "the default layout names no context");
            Ok(None)
        },
    )
    .unwrap_or_else(|failure| panic!("{failure:?}"));
    assert_eq!(page.output_id, "svg__preview-draft");
    assert_eq!(page.content_type, "image/svg+xml");
    assert!(
        page.page.trim_start().starts_with("<svg"),
        "not an SVG page"
    );
    assert_eq!(page.size_bytes, page.page.len() as u64);
    assert_eq!(
        page.sha256,
        format!("{:x}", Sha256::digest(page.page.as_bytes()))
    );
    assert!(
        !page.print_diagnostics.is_empty(),
        "the engine assesses its page"
    );
    assert_eq!(page.print_diagnostics[0]["output_id"], "svg__preview-draft");
    assert!(page.context_layers.is_empty());
    assert!(page.context_omitted.is_empty());
    // A preview leaves nothing on disk.
    assert!(!staging.exists() || std::fs::read_dir(&staging).unwrap().next().is_none());
}

#[test]
#[ignore = "requires DS_REPORT_BIN and the reporter printing fixtures"]
fn the_command_execution_answers_the_same_page_over_the_synthetic_room() {
    let binary = std::env::var_os("DS_REPORT_BIN")
        .map(PathBuf::from)
        .expect("DS_REPORT_BIN must name the locally built reporter");
    let examples = examples().expect("the reporter printing examples must be present");
    let engine = ProcessEngine { binary };
    let receipt = receipt_from_example(&examples.join("network-config.a0-a3.example.json"));
    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("staging");
    let inputs = room(&examples.join("rooms/tr_example_a.json"), "tr_example_a");
    let mut draft = ds_command_kernel::printing::default_layout();
    draft.id = "preview-draft".into();
    draft.name = "Preview draft".into();
    // The default layout names no context, so the held context is empty and
    // no holdings root is needed: the page is the engine's SVG.
    let page = execute(
        &engine,
        &PreviewSettings {
            project_id: "example-project",
            receipt: &receipt,
            admin_bounds: None,
            staging_root: &staging,
        },
        &PreviewRequest {
            transformer: "tr_example_a".into(),
            layout: draft,
        },
        &HeldState {
            room: HeldRoom {
                transformer: inputs.transformer.clone(),
                server_version: inputs.server_version,
                layers: inputs.layers.clone(),
                content_digest: None,
                media_grant: None,
                sheet: None,
            },
            context: HeldContext::default(),
        },
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(page.output_id, "svg__preview-draft");
    assert_eq!(page.filename, "tr_example_a.preview-draft.svg");
    assert!(
        page.page.trim_start().starts_with("<svg"),
        "not an SVG page"
    );
    assert_eq!(page.receipt["transformer"], "tr_example_a");
    assert!(page.context_layers.is_empty());
    assert!(page.context_omitted.is_empty());
    let data = answer("example-project", "stable", &[(&page, PageRef::Inline)]);
    assert_eq!(data["results"][0]["page"]["svg"], page.page);
    assert_eq!(data["results"][0]["filename"], page.filename);
    assert_eq!(data["preview"]["output_id"], "svg__preview-draft");
    assert_eq!(data["batch"]["completed"], 1);
    assert!(!staging.exists() || std::fs::read_dir(&staging).unwrap().next().is_none());
}
