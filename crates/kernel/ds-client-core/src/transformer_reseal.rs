//! Governed heal of a saved head whose stored content digest disagrees with
//! the canonical digest of its own stored layers (owner 2026-10-23). The
//! repair source guard keeps refusing such a head; this boundary recomputes
//! the digest, and only with confirmation and the reviewed fences stores it.
//! It never sends a layer, never processes and never moves the version.
use crate::{
    authorized::{self, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, ServiceRefusal, TransformerSet,
};
use ds_command_kernel::report_export::jcs::layers_content_digest;
use serde_json::{json, Value};

/// One reviewed reseal request. Without `confirmed` it only reads and plans.
#[derive(Clone, Debug)]
pub struct TransformerDigestReseal {
    pub operation_id: String,
    pub transformer: String,
    pub confirmed: bool,
    /// Required with `confirmed`: the version the reviewed plan named.
    pub expect_version: Option<u64>,
    /// Required with `confirmed`: the stale stored digest being replaced.
    pub expect_stored_digest: Option<String>,
}

fn invalid(code: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, code)
}
fn unreadable(code: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, code)
}

impl TransformerDigestReseal {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        crate::validate_project_id(project)?;
        ds_command_kernel::design::identifier(&self.operation_id)
            .map_err(|_| invalid("repair_operation_id_invalid"))?;
        TransformerSet::new([self.transformer.clone()])?;
        if self.confirmed && (self.expect_version.is_none() || self.expect_stored_digest.is_none())
        {
            return Err(invalid("repair_reseal_expectation_required"));
        }
        Ok(())
    }
}

/// The stored metadata row the status route serves, which (unlike the
/// saved-room read) is not re-derived from the layers.
fn stored_head<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    set: &TransformerSet,
) -> Result<(Option<u64>, Option<String>), ClientError> {
    let status = authorized::transformer_status(profile, transport, authority, project, set)?;
    let wanted = set.names().first().map(String::as_str).unwrap_or_default();
    let row = status
        .rows()
        .iter()
        .find(|row| row.name().eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            ClientError::new(ErrorKind::ResourceNotFound, "transformer was not found")
        })?;
    let metadata = &row.row()["metadata"];
    Ok((
        metadata["version"].as_u64(),
        metadata["content_digest"].as_str().map(str::to_owned),
    ))
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    input: &TransformerDigestReseal,
) -> Result<Value, ClientError> {
    input.validate(project)?;
    let set = TransformerSet::new([input.transformer.clone()])?;
    let (version, stored) = stored_head(profile, transport, authority, project, &set)?;
    let context = authorized::transformer_context(
        profile,
        transport,
        authority,
        project,
        &input.transformer,
        crate::TransformerContextProjection::Saved,
    )?;
    let layers =
        layers_content_digest(context.layers()).map_err(|_| unreadable("repair_target_invalid"))?;
    let (Some(version), Some(stored)) = (version, stored) else {
        return Err(invalid("repair_reseal_not_needed"));
    };
    // The status row and the saved room must describe the same head.
    if context.metadata().version() != Some(version)
        || context.metadata().content_digest() != Some(layers.as_str())
    {
        return Err(invalid("repair_reseal_head_moved"));
    }
    let plan = json!({"transformer":input.transformer,"version":version,
        "stored_content_digest":stored,"layers_content_digest":layers,"agree":stored == layers});
    if stored == layers {
        return Err(invalid("repair_reseal_not_needed"));
    }
    if !input.confirmed {
        return Ok(json!({"plan":plan,"written":false,
            "measurements":{"status_reads":1,"transformer_reads":1,"writes":0}}));
    }
    if input.expect_version != Some(version)
        || input.expect_stored_digest.as_deref() != Some(stored.as_str())
    {
        return Err(invalid("repair_reseal_expectation_changed"));
    }
    let body = serde_json::to_vec(&json!({"action":"reseal_transformer_content_digest",
        "eds_project_id":project,"transformer_name":input.transformer,
        "reseal":{"operation_id":input.operation_id,"source_version":version,
            "stored_content_digest":stored,"layers_content_digest":layers}}))
    .map_err(|_| invalid("repair_document_invalid"))?;
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
    let receipt = raw.get("data").unwrap_or(&raw).clone();
    if receipt["operation_id"] != input.operation_id
        || receipt["transformer_name"] != input.transformer
        || receipt["version"].as_u64() != Some(version)
        || receipt["previous_content_digest"] != stored
        || receipt["content_digest"] != layers
        || receipt["layers_changed"] != false
    {
        return Err(unreadable("repair_receipt_mismatch"));
    }
    let (after_version, after_stored) = stored_head(profile, transport, authority, project, &set)?;
    if after_version != Some(version) || after_stored.as_deref() != Some(layers.as_str()) {
        return Err(unreadable("repair_readback_unverified"));
    }
    Ok(
        json!({"plan":plan,"written":true,"verified":true,"receipt":receipt,
        "layers_sent":false,"process_invoked":false,
        "measurements":{"status_reads":2,"transformer_reads":1,"writes":1,"request_bytes":body.len()}}),
    )
}
