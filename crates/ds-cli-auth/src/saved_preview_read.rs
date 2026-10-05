//! One fixed retained A4 head and verified byte download. Opening never reads
//! current engineering inputs or renders, and never wakes publication.
use ds_cli_contract::Failure;
use ds_command_kernel::{compute_artifact_inventory::Head, printing::retained_preview};
use ds_sync_runtime::{Gateway, Reads, SyncRoute, VerifiedReads};
use serde_json::json;
use std::path::Path;

pub fn retained_a4_for_project(
    lane: &str,
    project: &str,
    transformer: &str,
    destination: &Path,
) -> Result<Head, Failure> {
    ds_client_core::validate_project_id(project).map_err(|error| unreadable(error.to_string()))?;
    ds_command_kernel::report_export::reportable_transformer(transformer).map_err(unreadable)?;
    let fence = crate::capture_layer_scope_fence_for_project(lane, project)?;
    let principal = crate::headless_identity_for_named_project(lane)?;
    let session = crate::sync::NativeSyncSession::open(
        crate::profile::Lane::parse(lane)?,
        principal,
        project.into(),
        crate::runtime_credential_binding(lane)?,
    )
    .map_err(|detail| Failure::unavailable("publication_unavailable", detail))?;
    let guard = || crate::verify_layer_scope_fence_for_project(lane, &fence, fence.uid(), project);
    guard()?;
    let operation = format!("export-{transformer}");
    let value = session.post(SyncRoute::ComputeArtifacts, &json!({"action":"read","project_id":project,"engine":"network_reporter","operation":operation,"variant":retained_preview::VARIANT})).map_err(crate::sync::publication_read_error)?;
    let head = ds_command_kernel::compute_artifact_inventory::decode_head(
        &serde_json::to_vec(&value).map_err(|error| unreadable(error.to_string()))?,
    )
    .map_err(unreadable)?;
    retained_preview::validate_head(project, transformer, &head).map_err(unreadable)?;
    guard()?;
    let output = &head.outputs[0];
    let ticket = session.post(SyncRoute::ComputeArtifacts, &json!({"action":"download","project_id":project,"engine":"network_reporter","operation":operation,"variant":retained_preview::VARIANT,"output_id":output.output_id})).map_err(crate::sync::publication_read_error)?;
    let url = admitted_ticket_url(&head, &ticket)?;
    guard()?;
    VerifiedReads::new(
        destination
            .parent()
            .ok_or_else(|| unreadable("The destination has no parent."))?
            .into(),
    )
    .download_verified(url, destination, &output.sha256, output.size_bytes)
    .map_err(unreadable)?;
    guard()?;
    Ok(head)
}

fn admitted_ticket_url<'a>(head: &Head, ticket: &'a serde_json::Value) -> Result<&'a str, Failure> {
    let resolved = ds_command_kernel::compute_artifact_inventory::decode_head(
        &serde_json::to_vec(&ticket["head"]).map_err(|error| unreadable(error.to_string()))?,
    )
    .map_err(unreadable)?;
    let output: ds_command_kernel::compute_artifact_inventory::Declaration =
        serde_json::from_value(ticket["output"].clone())
            .map_err(|error| unreadable(error.to_string()))?;
    if resolved != *head || output != head.outputs[0] {
        return Err(Failure::conflict(
            "publication_conflict",
            "The retained head moved while its download ticket was being read.",
        )
        .remedy("Retry the same retained read; do not render or substitute a ticket."));
    }
    ds_command_kernel::compute_artifact_inventory::evaluate(
        &serde_json::to_vec(&json!({"engine":head.engine,"operation":head.operation,"outputs":head.outputs,"download":{"output":ticket["output"],"file_name":ticket["file_name"]}})).map_err(|error| unreadable(error.to_string()))?,
    ).map_err(unreadable)?;
    ticket["download_url"]
        .as_str()
        .ok_or_else(|| unreadable("The retained PDF has no governed download ticket."))
}
fn unreadable(message: impl Into<String>) -> Failure {
    Failure::unavailable("publication_unreadable", message).remedy("Preserve the shared head and retry the same read; do not recalculate or bypass a refused download.")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn head() -> Head {
        use ds_command_kernel::printing::retained_preview::{Binding, SCHEMA, TEMPLATE, VARIANT};
        let binding = Binding {
            schema: SCHEMA.into(),
            template_id: TEMPLATE.into(),
            template_revision_id: "a".repeat(64),
            template_html_sha256: "b".repeat(64),
            analysis_sha256: "c".repeat(64),
            sheets_sha256: "d".repeat(64),
            report_input_fingerprint: "e".repeat(64),
            renderer_style_ref: "renderer_print".into(),
            renderer_revision_id: "renderer_revision".into(),
            renderer_content_sha256: "f".repeat(64),
            transformer_revision: 1,
            content_digest: "1".repeat(64),
        };
        let fingerprint = binding
            .input_base_fingerprint("project_a", "transformer_a")
            .unwrap();
        serde_json::from_value(json!({"printing_preview":binding,"work_id":"work_a","engine":"network_reporter","engine_version":format!("ds-network-reporter@0.1.0+{}","2".repeat(40)),"engine_build_manifest_sha256":"3".repeat(64),"operation":"export-transformer_a","variant":VARIANT,"input_base_fingerprint":fingerprint,"head_revision":1,"updated_at":"2026-10-05T00:00:00Z","outputs":[{"output_id":"voltage_drop_pdf","filename":ds_command_kernel::report_formats::report_filename("transformer_a","voltage_drop_pdf").unwrap(),"format":"voltage_drop_pdf","content_type":"application/pdf","sha256":"4".repeat(64),"size_bytes":123,"paper_size":"A4"}]})).unwrap()
    }
    #[test]
    fn exact_download_ticket_is_admitted_without_current_input_capture() {
        let head = head();
        let ticket = json!({"head":head,"output":head.outputs[0],"file_name":head.outputs[0].filename,"download_url":"https://storage.googleapis.com/b/object?generation=1"});
        assert_eq!(
            admitted_ticket_url(&head, &ticket).unwrap(),
            ticket["download_url"]
        );
    }
    #[test]
    fn moved_equal_byte_head_and_crossed_output_ticket_are_refused() {
        let head = head();
        let mut ticket = json!({"head":head,"output":head.outputs[0],"file_name":head.outputs[0].filename,"download_url":"https://storage.googleapis.com/b/object?generation=1"});
        ticket["head"]["work_id"] = json!("work_b");
        ticket["head"]["head_revision"] = json!(2);
        assert_eq!(
            admitted_ticket_url(&head, &ticket).unwrap_err().code(),
            "publication_conflict"
        );
        ticket["head"] = json!(head);
        ticket["output"]["sha256"] = json!("5".repeat(64));
        assert_eq!(
            admitted_ticket_url(&head, &ticket).unwrap_err().code(),
            "publication_conflict"
        );
    }
}
