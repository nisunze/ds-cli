//! One no-process publication boundary for selected, evidence-derived cells.
//! Native re-derivation precedes the server's independent CAS transaction.
use crate::{
    authorized::{self, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, ServiceRefusal,
};
use ds_command_kernel::report_export::jcs::layers_content_digest;
use ds_command_kernel::{
    design::Snapshot,
    design_repair::{self, ApplySelection, EvidenceSource, Proposal},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformerRepairPublication {
    pub operation_id: String,
    pub source: Option<EvidenceSource>,
    pub target: Snapshot,
    pub proposal: Proposal,
    pub selection: ApplySelection,
}
fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}
fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}
fn repair_error(error: design_repair::RepairError) -> ClientError {
    invalid(match error.code.as_str() {
        "repair_scope_mismatch" => "repair_scope_mismatch",
        "repair_target_changed" => "repair_target_changed",
        "repair_source_changed" => "repair_source_changed",
        "repair_field_not_allowed" => "repair_field_not_allowed",
        "repair_proposal_changed" => "repair_proposal_changed",
        "repair_selection_invalid" => "repair_selection_invalid",
        "repair_identity_ambiguous" => "repair_identity_ambiguous",
        "repair_effect_unconfirmed" => "repair_effect_unconfirmed",
        "repair_bound_exceeded" => "repair_bound_exceeded",
        "repair_crs_required" => "repair_crs_required",
        "repair_geometry_invalid" => "repair_geometry_invalid",
        "repair_key_invalid" => "repair_key_invalid",
        "repair_field_invalid" => "repair_field_invalid",
        "repair_source_required" => "repair_source_required",
        "repair_match_ambiguous" => "repair_match_ambiguous",
        "repair_unit_mismatch" => "repair_unit_mismatch",
        "repair_value_invalid" => "repair_value_invalid",
        "repair_layer_unknown" => "repair_layer_unknown",
        "repair_scope_invalid" => "repair_scope_invalid",
        "repair_target_invalid" => "repair_target_invalid",
        "repair_identity_required" => "repair_identity_required",
        "repair_change_ambiguous" => "repair_change_ambiguous",
        "repair_preservation_refused" => "repair_preservation_refused",
        "repair_root_unknown" => "repair_root_unknown",
        _ => "repair_document_invalid",
    })
}
impl TransformerRepairPublication {
    pub fn project(&self) -> &str {
        &self.proposal.request.scope.project
    }
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        ds_command_kernel::design::identifier(&self.operation_id)
            .map_err(|_| invalid("repair_operation_id_invalid"))?;
        if project != self.project()
            || self.proposal.request.target.server_version.is_none()
            || self.proposal.request.target.layers_content_digest.is_none()
        {
            return Err(invalid("repair_server_fence_required"));
        }
        if self.selection.change_ids.len() > 1000 {
            return Err(invalid("repair_bound_exceeded"));
        }
        self.projected()?;
        self.wire(project)?;
        Ok(())
    }
    fn projected(&self) -> Result<design_repair::AppliedRepair, ClientError> {
        design_repair::apply_selected(
            self.source.as_ref(),
            &self.target,
            &self.proposal,
            &self.selection,
        )
        .map_err(repair_error)
    }
    fn wire(&self, project: &str) -> Result<Vec<u8>, ClientError> {
        let selected = self.selection.change_ids.iter().collect::<BTreeSet<_>>();
        let mut changes = Vec::new();
        for change in &self.proposal.changes {
            if !selected.contains(&change.id) {
                continue;
            }
            let features = self.target.layers[&change.layer]["features"]
                .as_array()
                .ok_or_else(|| invalid("repair_target_invalid"))?;
            let feature = features
                .iter()
                .find(|feature| {
                    ds_command_kernel::design_repair::feature_identity(feature)
                        .ok()
                        .is_some_and(|id| match id.value {
                            Value::String(v) => v.trim() == change.feature_id,
                            Value::Number(v) => v.to_string() == change.feature_id,
                            _ => false,
                        })
                })
                .ok_or_else(|| invalid("repair_identity_required"))?;
            changes.push(json!({"layer":change.layer,"identity":design_repair::feature_identity(feature).map_err(repair_error)?,"property":change.field,"before":change.before,"after":change.after}));
        }
        let body=serde_json::to_vec(&json!({"action":"repair_transformer_properties","eds_project_id":project,"transformer_name":self.target.transformer,
            "repair":{"operation_id":self.operation_id,"source_version":self.proposal.request.target.server_version,
            "source_content_digest":self.proposal.request.target.layers_content_digest,"changes":changes}})).map_err(|_| invalid("repair_document_invalid"))?;
        if body.len() > 256 * 1024 {
            return Err(invalid("repair_bound_exceeded"));
        }
        Ok(body)
    }
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    input: &TransformerRepairPublication,
) -> Result<Value, ClientError> {
    input.validate(project)?;
    let scope = &input.proposal.request.scope;
    if (scope.principal != authority.email && scope.principal != authority.uid)
        || scope.lane != profile.lane().token()
        || scope.audience != profile.credential_audience_sha256()
    {
        return Err(invalid("repair_scope_mismatch"));
    }
    let projected = input.projected()?;
    let expected = layers_content_digest(&projected.snapshot.layers)
        .map_err(|_| invalid("repair_target_invalid"))?;
    // The server's CAS validates the supplied saved version/content fence.
    // A preliminary download would duplicate that check and cannot close a
    // race; acquire the result only after its exact operation receipt succeeds.
    let source = input
        .proposal
        .request
        .target
        .layers_content_digest
        .as_deref()
        .unwrap();
    let body = input.wire(project)?;
    let response = transport.transformer_save(crate::TransformerSaveCall {
        profile,
        authority: authority.call(),
        body: &body,
    })?;
    if response.body.len() > 1024 * 1024 {
        return Err(unreadable("repair_response_too_large"));
    }
    if response.status != 200 {
        let raw: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        let code = raw["error"]["code"].as_str().map(str::to_ascii_lowercase);
        let refusal = ServiceRefusal::with_details(
            response.status,
            code.as_deref(),
            raw["error"]["message"].as_str(),
            raw["data"].as_object(),
        );
        let kind = match response.status {
            400 | 409 | 413 | 422 | 423 => ErrorKind::InvalidInput,
            401 | 403 => ErrorKind::AuthenticationRejected,
            404 => ErrorKind::ResourceNotFound,
            _ => ErrorKind::Transient,
        };
        return Err(ClientError::with_service_refusal(
            kind,
            "transformer repair was refused by its owner",
            refusal,
        ));
    }
    let raw: Value = serde_json::from_slice(&response.body)
        .map_err(|_| unreadable("repair_response_invalid"))?;
    if raw.get("success").is_some_and(|v| v != true) {
        return Err(unreadable("repair_response_failed"));
    }
    let receipt = raw.get("data").unwrap_or(&raw);
    let base = input.proposal.request.target.server_version.unwrap();
    if receipt["operation_id"] != input.operation_id
        || receipt["transformer_name"] != input.target.transformer
        || receipt["source_version"].as_u64() != Some(base)
        || receipt["source_content_digest"] != source
        || receipt["version"].as_u64() != base.checked_add(1)
        || receipt["content_digest"] != expected
        || receipt["changed_cells"].as_u64() != Some(projected.changed_cells as u64)
    {
        return Err(unreadable("repair_receipt_mismatch"));
    }
    let after = authorized::transformer_context(
        profile,
        transport,
        authority,
        project,
        &input.target.transformer,
        crate::TransformerContextProjection::Saved,
    )?;
    if after.metadata().version() != receipt["version"].as_u64()
        || after.metadata().content_digest() != Some(expected.as_str())
        || layers_content_digest(after.layers()).map_err(|_| invalid("repair_target_invalid"))?
            != expected
        || after.layers() != &projected.snapshot.layers
    {
        return Err(unreadable("repair_readback_unverified"));
    }
    Ok(
        json!({"verified":true,"receipt":receipt,"preserved_outside_selection":true,"process_invoked":false,
        "measurements":{"transformer_reads":1,"save_calls":1,"request_bytes":body.len(),"response_bytes":response.body.len()}}),
    )
}
