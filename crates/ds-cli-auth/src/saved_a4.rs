//! One native session captures every authority consumed by a saved A4 preview.
//! No calculation, repair, save, synthetic context or default document fallback.
use std::collections::BTreeMap;

use ds_cli_contract::Failure;
use ds_client_core::{
    FeederConfiguration, PrintingStandardKind, PrintingStandardRequest, TransformerContext,
};
use ds_command_kernel::printing::catalogue::Location;
use ds_command_kernel::style_resolution::{Binding, Snapshot};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::HeadlessProjectReport;

pub struct SavedA4Capture {
    pub location: Location,
    /// Genuine server receipt, or its explicit refusal; pure rendering does
    /// not synthesize publication provenance when a receipt is unavailable.
    pub input_receipt: Result<ds_command_kernel::report_export::InputReceipt, String>,
    pub analysis: Vec<u8>,
    pub analysis_sha256: String,
    pub version: u64,
    pub content_digest: String,
    pub layers: BTreeMap<String, Value>,
    pub network_config: Value,
    pub renderer_defaults: Binding,
    pub report_format: String,
}

struct RawCapture {
    snapshot: TransformerContext,
    config: FeederConfiguration,
    document: Value,
    styles: Value,
    analysis: Option<Vec<u8>>,
}

/// Snapshot, config, A4 and Styles use ONE restored user/device session. The
/// fenced raw-analysis read happens last, so a moved saved head refuses after
/// the other authority reads, rather than rendering an obsolete result.
pub fn saved_a4_for_project(
    lane: &str,
    project: &str,
    transformer: &str,
    location: &Location,
) -> Result<HeadlessProjectReport<SavedA4Capture>, Failure> {
    match location {
        Location::GovernedHtml { project_id, id, .. }
            if project_id == project && id == "voltage-drop-a4-v1" => {}
        _ => {
            return Err(invalid(
                "The A4 location belongs to another project or document.",
            ));
        }
    }
    let held = capture_raw(lane, project, transformer)?;
    let capture = admit(project, transformer, location, held.result)?;
    Ok(HeadlessProjectReport {
        identity: held.identity,
        user_email: held.user_email,
        lane: held.lane,
        project_id: held.project_id,
        project_name: held.project_name,
        project_status: held.project_status,
        result: capture,
    })
}

/// Refresh captures the current owned document in the SAME native session,
/// rather than reading a location separately and restoring another session.
pub fn saved_owned_a4_for_project(
    lane: &str,
    project: &str,
    transformer: &str,
) -> Result<HeadlessProjectReport<SavedA4Capture>, Failure> {
    let held = capture_raw(lane, project, transformer)?;
    let text = |field: &str| {
        held.result.document[field]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| invalid(format!("The owned A4 response has no {field}.")))
    };
    let location = Location::GovernedHtml {
        project_id: project.into(),
        id: "voltage-drop-a4-v1".into(),
        revision_id: text("revision_id")?,
        content_sha256: text("content_sha256")?,
        html_sha256: text("html_sha256")?,
    };
    let capture = admit(project, transformer, &location, held.result)?;
    Ok(HeadlessProjectReport {
        identity: held.identity,
        user_email: held.user_email,
        lane: held.lane,
        project_id: held.project_id,
        project_name: held.project_name,
        project_status: held.project_status,
        result: capture,
    })
}

fn capture_raw(
    lane: &str,
    project: &str,
    transformer: &str,
) -> Result<HeadlessProjectReport<RawCapture>, Failure> {
    let request = PrintingStandardRequest::Get {
        kind: PrintingStandardKind::A4,
        id: "voltage-drop-a4-v1".into(),
    };
    macro_rules! capture {
        ($client:ident, $project:ident $(, $now:expr)?) => {{
            let snapshot = $client.transformer_context_saved($project, transformer $(, $now)?)?;
            let pins = ds_client_core::saved_lv_analysis_pins(
                snapshot.voltage_drop_metadata(), snapshot.metadata().version(), snapshot.metadata().content_digest(),
            )?;
            let config = $client.feeder_configuration($project, Some(&ds_client_core::ProjectConfigurationChange::ReadSettings) $(, $now)?)?;
            let document = $client.printing_standard($project, &request $(, $now)?)?;
            let styles = $client.style_governance($project, &ds_command_kernel::style_governance::Command::Table $(, $now)?)?;
            let analysis = pins.map(|(version, digest, sha)| $client.transformer_analysis($project, transformer, version, &digest, &sha $(, $now)?)).transpose()?;
            Ok(RawCapture { snapshot, config, document, styles, analysis })
        }};
    }
    crate::headless_named_report(
        lane,
        project,
        |device, project| capture!(device, project),
        |client, project| capture!(client, project, crate::now()),
    )
}

/// Exact entities transported by the signed-in browser, admitted by the same
/// native decoders and saved-source checks as the restored-session CLI path.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedA4Responses {
    pub snapshot: ds_client_core::printing_capture::Response,
    pub config: ds_client_core::printing_capture::Response,
    pub document: ds_client_core::printing_capture::Response,
    pub styles: ds_client_core::printing_capture::Response,
    pub analysis: ds_client_core::printing_capture::Response,
}
pub fn admit_saved_a4_responses(
    project: &str,
    transformer: &str,
    location: &Location,
    responses: SavedA4Responses,
) -> Result<SavedA4Capture, Failure> {
    use ds_client_core::printing_capture;
    let snapshot = printing_capture::snapshot(project, transformer, responses.snapshot)
        .map_err(|e| invalid(e.to_string()))?;
    let pins = ds_client_core::saved_lv_analysis_pins(
        snapshot.voltage_drop_metadata(),
        snapshot.metadata().version(),
        snapshot.metadata().content_digest(),
    )
    .map_err(|e| invalid(e.to_string()))?
    .ok_or_else(|| {
        Failure::conflict(
            "lv_analysis_missing",
            "The saved head has no current saved analysis.",
        )
    })?;
    let raw = RawCapture {
        snapshot,
        config: printing_capture::configuration(responses.config)
            .map_err(|e| invalid(e.to_string()))?,
        document: printing_capture::document(project, responses.document)
            .map_err(|e| invalid(e.to_string()))?,
        styles: printing_capture::style_table(project, responses.styles)
            .map_err(|e| invalid(e.to_string()))?,
        analysis: Some(
            printing_capture::analysis(responses.analysis, &pins.2)
                .map_err(|e| invalid(e.to_string()))?,
        ),
    };
    admit(project, transformer, location, raw)
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::conflict("report_inputs_invalid", message)
        .remedy("Refresh the same project's templates and saved analyses; no report-time recomputation is performed.")
}

fn admit(
    project: &str,
    transformer: &str,
    location: &Location,
    held: RawCapture,
) -> Result<SavedA4Capture, Failure> {
    if held.snapshot.ds_project() != project || held.snapshot.transformer_name() != transformer {
        return Err(invalid(
            "The saved snapshot belongs to another project or transformer.",
        ));
    }
    let metadata = held.snapshot.voltage_drop_metadata();
    let pins = ds_client_core::saved_lv_analysis_pins(
        metadata,
        held.snapshot.metadata().version(),
        held.snapshot.metadata().content_digest(),
    )
    .map_err(|error| invalid(error.to_string()))?;
    let Some((version, content_digest, analysis_sha256)) = pins else {
        if matches!(
            metadata.and_then(|value| value["state"].as_str()),
            Some("stale" | "needs_reprocess")
        ) {
            return Err(Failure::conflict(
                "lv_analysis_stale",
                "The saved analysis is stale; no calculation was attempted.",
            ));
        }
        return Err(Failure::conflict(
            "lv_analysis_missing",
            "The saved head has no current saved analysis; no calculation was attempted.",
        ));
    };
    let analysis = held
        .analysis
        .ok_or_else(|| invalid("The exact saved analysis bytes are missing."))?;
    if analysis.len() > ds_client_core::TRANSFORMER_ANALYSIS_RESPONSE_LIMIT
        || format!("{:x}", Sha256::digest(&analysis)) != analysis_sha256
        || ds_command_kernel::report_export::jcs::layers_content_digest(held.snapshot.layers())
            .map_err(invalid)?
            != content_digest
    {
        return Err(invalid(
            "The captured analysis or saved layers do not match their immutable pins.",
        ));
    }
    ds_command_kernel::printing::catalogue::read_governed(location.clone(), held.document.clone())
        .map_err(invalid)?;
    let config = held.config.document["sheets"].clone();
    if !config.is_object()
        || config["printing_context"]["project_id"] != project
        || config["printing_a4"] != held.document["document"]
    {
        return Err(invalid(
            "The captured project configuration differs from the held A4 document or project context.",
        ));
    }
    let styles: Snapshot =
        serde_json::from_value(held.styles).map_err(|error| invalid(error.to_string()))?;
    if styles.project_id != project {
        return Err(invalid("The renderer policy belongs to another project."));
    }
    let renderer_defaults =
        ds_command_kernel::printing::renderer_defaults::resolve(&styles).map_err(invalid)?;
    let report_format =
        ds_command_kernel::report_formats::project_voltage_drop_report_format(&config)
            .map_err(invalid)?
            .token()
            .into();
    Ok(SavedA4Capture {
        location: location.clone(),
        input_receipt: ds_command_kernel::report_export::InputReceipt::from_config(
            &held.config.document,
        ),
        analysis,
        analysis_sha256,
        version,
        content_digest,
        layers: held.snapshot.layers().clone(),
        network_config: config,
        renderer_defaults,
        report_format,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FixtureTransport, NOW, SIGN_IN, signed_in};
    use serde_json::json;

    fn fixture() -> (Location, RawCapture) {
        let layers = BTreeMap::new();
        let digest = ds_command_kernel::report_export::jcs::layers_content_digest(&layers).unwrap();
        let analysis = b"{\"schema\":\"ds.lv-voltage-drop.analysis/v1\"}".to_vec();
        let sha = format!("{:x}", Sha256::digest(&analysis));
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.lock().transformer_context.push_back(ds_client_core::TransportResponse::new(200,
            serde_json::to_vec(&json!({"success":true,"data":{"total":1,"found_count":1,"failed_count":0,"results":[{"transformer_name":"T1","ok":true,"data":{"layers":layers,"metadata":{"version":2,"content_digest":digest},"voltage_drop_metadata":{"state":"ready","schema":ds_client_core::SAVED_LV_ANALYSIS_SCHEMA,"method":ds_client_core::SAVED_LV_ANALYSIS_METHOD,"content_digest":digest,"analysis_sha256":sha,"json_bytes":analysis.len()}}}]}})).unwrap()));
        let snapshot = signed_in(transport)
            .transformer_context_saved("project", "T1", NOW)
            .unwrap();
        let html = "<!doctype html><html><body>Authored test page</body></html>";
        let content = ds_command_kernel::printing::resolved_styles::content_digest(
            &json!({"html":html,"status_layout":{},"logo_slots":[]}),
        )
        .unwrap();
        let html_sha = format!("{:x}", Sha256::digest(html.as_bytes()));
        let document = json!({"schema":"ds.print-a4-document/v1","project_id":"project","revision_id":content,"content_sha256":content,"html":html,"status_layout":{},"logo_slots":[]});
        let location = Location::GovernedHtml {
            project_id: "project".into(),
            id: "voltage-drop-a4-v1".into(),
            revision_id: content.clone(),
            content_sha256: content.clone(),
            html_sha256: html_sha.clone(),
        };
        let response = json!({"scope":"project","project_id":"project","kind":"a4","id":"voltage-drop-a4-v1","revision_id":content,"content_sha256":content,"html_sha256":html_sha,"html_available":true,"html":html,"document":document});
        let defaults: Value = serde_json::from_str(include_str!(
            "../../../../ds-command-kernel/tests/fixtures/governed-renderer-defaults.json"
        ))
        .unwrap();
        let style = json!({"version":1,"type":"line","paint":{},"layout":{},"metadata":{"print_renderer_defaults":defaults}});
        let style_sha = ds_command_kernel::style_plan::style_digest(&style);
        let styles = json!({"schema":ds_command_kernel::style_resolution::SCHEMA,"project_id":"project","revision_id":"a".repeat(64),"bindings":[{"key":ds_command_kernel::printing::renderer_defaults::key(),"style_ref":"project/renderer","revision_id":style_sha,"content_sha256":style_sha,"scope":"project","document":style}]});
        let config = FeederConfiguration {
            document: json!({"sheets":{"printing_context":{"project_id":"project"},"printing_a4":document,"project_settings":[]}}),
            summary: Value::Null,
        };
        (
            location,
            RawCapture {
                snapshot,
                config,
                document: response,
                styles,
                analysis: Some(analysis),
            },
        )
    }

    fn browser_responses(raw: RawCapture) -> SavedA4Responses {
        use ds_client_core::printing_capture::Response;
        let response = |value: Value| Response {
            status: 200,
            bytes: serde_json::to_vec(&value).unwrap(),
            content_type: Some("application/json".into()),
            analysis_sha256: None,
            cache_control: None,
        };
        let snapshot = response(
            json!({"success":true,"data":{"total":1,"found_count":1,"failed_count":0,"results":[{"transformer_name":raw.snapshot.transformer_name(),"ok":true,"data":{"layers":raw.snapshot.layers(),"metadata":{"version":raw.snapshot.metadata().version(),"content_digest":raw.snapshot.metadata().content_digest()},"voltage_drop_metadata":raw.snapshot.voltage_drop_metadata()}}]}}),
        );
        let analysis = Response {
            status: 200,
            bytes: raw.analysis.unwrap(),
            content_type: Some("application/json".into()),
            analysis_sha256: raw.snapshot.voltage_drop_metadata().unwrap()["analysis_sha256"]
                .as_str()
                .map(str::to_owned),
            cache_control: Some("no-store".into()),
        };
        SavedA4Responses {
            snapshot,
            config: response(json!({"success":true,"data":raw.config.document})),
            document: response(json!({"success":true,"data":raw.document})),
            styles: response(json!({"success":true,"data":raw.styles})),
            analysis,
        }
    }

    #[test]
    fn browser_held_entities_use_native_read_plans_and_identical_admission() {
        let (location, raw) = fixture();
        let responses = browser_responses(raw);
        let plan = ds_client_core::printing_capture::plan("project", "T1").unwrap();
        assert_eq!(plan.snapshot.body["fields"], "saved");
        assert_eq!(plan.config.path, "/config/project");
        assert_eq!(plan.config.method, "GET");
        assert_eq!(plan.document.body["scope"], "project");
        assert_eq!(plan.styles.body["action"], "get_style_table");
        let held = ds_client_core::printing_capture::Response {
            status: responses.snapshot.status,
            bytes: responses.snapshot.bytes.clone(),
            content_type: None,
            analysis_sha256: None,
            cache_control: None,
        };
        let last = ds_client_core::printing_capture::analysis_plan("project", "T1", held).unwrap();
        assert_eq!(last.body["analysis_version"], 2);
        assert_eq!(
            last.body["analysis_sha256"],
            responses.analysis.analysis_sha256.as_deref().unwrap()
        );
        let expected = responses.analysis.bytes.clone();
        let admitted = admit_saved_a4_responses("project", "T1", &location, responses).unwrap();
        assert_eq!(admitted.analysis, expected);
        assert_eq!(admitted.version, 2);
    }

    #[test]
    fn browser_held_crossed_receipts_and_raw_headers_refuse() {
        for case in 0..5 {
            let (location, raw) = fixture();
            let mut responses = browser_responses(raw);
            match case {
                0 => responses.analysis.analysis_sha256 = Some("f".repeat(64)),
                1 => responses.analysis.cache_control = None,
                2 => responses.document.bytes = serde_json::to_vec(&json!({"success":true,"data":{"scope":"project","project_id":"other"}})).unwrap(),
                _ => responses.snapshot.bytes = serde_json::to_vec(&json!({"success":true,"data":{"total":1,"found_count":0,"failed_count":1,"results":[{"transformer_name":"other","ok":false}]}})).unwrap(),
            }
            assert!(
                admit_saved_a4_responses("project", "T1", &location, responses).is_err(),
                "case {case}"
            );
        }
    }

    #[test]
    fn exact_saved_bytes_and_project_a4_are_retained_without_report_recalculation() {
        let (location, raw) = fixture();
        let exact = raw.analysis.clone().unwrap();
        let capture = admit("project", "T1", &location, raw).unwrap();
        assert_eq!(capture.analysis, exact);
        assert_eq!(capture.version, 2);
        assert!(capture.input_receipt.is_err());
        assert_eq!(
            capture.network_config["printing_a4"]["project_id"],
            "project"
        );
    }

    #[test]
    fn publication_retains_the_genuine_server_receipt_without_hashing_raw_config() {
        let (location, mut raw) = fixture();
        let sheets = r#"{ "project_settings" : [] }"#;
        let receipt = ds_command_kernel::report_export::InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Rwanda".into(),
            sheets_json: sheets.into(),
            sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
            reference_semantic_sha256: "a".repeat(64),
        };
        raw.config.document["network_reporter_input_receipt"] = json!(receipt);
        let capture = admit("project", "T1", &location, raw).unwrap();
        assert_eq!(capture.input_receipt.unwrap(), receipt);
    }

    #[test]
    fn crossed_config_moved_template_and_altered_saved_bytes_refuse() {
        for change in 0..4 {
            let (location, mut raw) = fixture();
            match change {
                0 => {
                    raw.config.document["sheets"]["printing_context"]["project_id"] = json!("other")
                }
                1 => {
                    raw.config.document["sheets"]["printing_a4"]["revision_id"] =
                        json!("b".repeat(64))
                }
                2 => raw.analysis.as_mut().unwrap().push(b' '),
                _ => raw.styles["project_id"] = json!("other"),
            }
            assert!(
                admit("project", "T1", &location, raw).is_err(),
                "case {change}"
            );
        }
    }

    #[test]
    fn wrong_project_and_layout_locations_refuse_before_native_identity_restoration() {
        for location in [
            Location::Layout {
                id: "voltage-drop-a4-v1".into(),
            },
            Location::GovernedHtml {
                project_id: "other".into(),
                id: "voltage-drop-a4-v1".into(),
                revision_id: "a".repeat(64),
                content_sha256: "a".repeat(64),
                html_sha256: "b".repeat(64),
            },
        ] {
            let error = saved_a4_for_project("stable", "project", "T1", &location)
                .err()
                .unwrap();
            assert_eq!(error.code(), "report_inputs_invalid");
        }
    }
}
