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
    let held = crate::headless_named_report(
        lane,
        project,
        |device, project| capture!(device, project),
        |client, project| capture!(client, project, crate::now()),
    )?;
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

    #[test]
    fn exact_saved_bytes_and_project_a4_are_retained_without_report_recalculation() {
        let (location, raw) = fixture();
        let exact = raw.analysis.clone().unwrap();
        let capture = admit("project", "T1", &location, raw).unwrap();
        assert_eq!(capture.analysis, exact);
        assert_eq!(capture.version, 2);
        assert_eq!(
            capture.network_config["printing_a4"]["project_id"],
            "project"
        );
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
