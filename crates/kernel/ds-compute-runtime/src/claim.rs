//! The exact ds-solar prepared-publication handoff consumed by Native Server.
//!
//! This is intentionally a kernel boundary: a host may transport the sealed
//! envelope, but it cannot mint or repair provenance for a prepared request.

use serde::Deserialize;
use serde_json::Value;

use crate::{
    validate_provenance, validate_solar_request, PreparedSolarRequest, SolarSnapshotProvenance,
};

const SUBMISSION_SCHEMA: &str = "ds.solar.server-submission/v1";
const CLAIM_SCHEMA: &str = "ds-solar.prepared-publication-claim/v1";
const CLAIM_KIND: &str = "server-revalidated-publication-handoff";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedPublicationClaim {
    schema_version: String,
    claim_kind: String,
    project_id: String,
    city_id: String,
    city_content_digest: String,
    prepared_input_digest: String,
    source_snapshot_sha256: String,
    input_base_fingerprint: String,
    snapshot_receipt_id: String,
    snapshot_receipt_expires_at: String,
}

/// Produce the Server envelope from the exact two artifacts emitted by Solar
/// preparation. Verification reuses the admission decoder before bytes leave
/// this owner; hosts never assemble or repair publication provenance.
pub fn encode_solar_server_submission(prepared: &[u8], claim: &[u8]) -> Result<Vec<u8>, String> {
    if prepared.len().saturating_add(claim.len()) > 64 * 1024 * 1024 {
        return Err("solar_server_submission_too_large".into());
    }
    let prepared: Value = serde_json::from_slice(prepared)
        .map_err(|_| "solar_server_submission_invalid".to_string())?;
    let claim: Value =
        serde_json::from_slice(claim).map_err(|_| "solar_publication_claim_invalid".to_string())?;
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": SUBMISSION_SCHEMA,
        "prepared": prepared,
        "publication_claim": claim,
        "render_charts": true,
    }))
    .map_err(|_| "solar_server_submission_invalid".to_string())?;
    decode_solar_server_submission(&bytes)?;
    Ok(bytes)
}

/// Decode the two-document ds-solar handoff into the existing kernel request
/// bytes and verified provenance. The caller persists only the returned bytes
/// through `submit_solar_with_provenance`.
pub fn decode_solar_server_submission(
    bytes: &[u8],
) -> Result<(Vec<u8>, SolarSnapshotProvenance), String> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("solar_server_submission_too_large".into());
    }
    let mut submitted: Value =
        serde_json::from_slice(bytes).map_err(|_| "solar_server_submission_invalid".to_string())?;
    let object = submitted
        .as_object_mut()
        .ok_or("solar_server_submission_invalid")?;
    if object.get("schema_version").and_then(Value::as_str) != Some(SUBMISSION_SCHEMA) {
        return Err("solar_server_submission_invalid".into());
    }
    let claim: PreparedPublicationClaim = serde_json::from_value(
        object
            .remove("publication_claim")
            .ok_or("solar_publication_claim_missing")?,
    )
    .map_err(|_| "solar_publication_claim_invalid".to_string())?;
    object.remove("schema_version");
    if object.contains_key("provenance") {
        return Err("solar_server_submission_invalid".into());
    }
    let sealed = serde_json::to_vec(&submitted)
        .map_err(|_| "solar_server_submission_invalid".to_string())?;
    let request: PreparedSolarRequest = serde_json::from_slice(&sealed)
        .map_err(|_| "solar_server_submission_invalid".to_string())?;
    validate_solar_request(&request)?;
    if claim.schema_version != CLAIM_SCHEMA
        || claim.claim_kind != CLAIM_KIND
        || !lower_sha256(&claim.source_snapshot_sha256)
        || !lower_sha256(&claim.input_base_fingerprint)
        || !canonical_uuid(&claim.snapshot_receipt_id)
        || !bounded_text(&claim.snapshot_receipt_expires_at, 128)
        || claim.project_id != request.prepared.city.identity.project_id
        || claim.city_id != request.prepared.city.identity.city_id
        || claim.city_content_digest != request.prepared.city.content_digest
        || claim.prepared_input_digest != request.prepared.input_digest
        || request.prepared.city.revision.source != "ds-brain"
        || request.prepared.city.revision.revision.as_deref()
            != Some(claim.input_base_fingerprint.as_str())
    {
        return Err("solar_publication_claim_invalid_or_stale".into());
    }
    let provenance = SolarSnapshotProvenance {
        project_id: claim.project_id,
        root: request.prepared.city.identity.root.clone(),
        template_id: claim.city_id,
        input_base_fingerprint: claim.input_base_fingerprint,
        source_snapshot_sha256: claim.source_snapshot_sha256,
        snapshot_receipt_id: claim.snapshot_receipt_id,
    };
    validate_provenance(&provenance, &request.prepared)?;
    Ok((sealed, provenance))
}

fn lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn canonical_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            14 => matches!(*byte, b'1'..=b'5'),
            19 => matches!(*byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f'),
        })
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
