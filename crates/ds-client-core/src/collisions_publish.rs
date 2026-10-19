//! Governed save of an edge-computed project collisions result.
//!
//! DS Core computes collisions with the Reporter addon from the saved rooms it
//! holds; this owner only carries the result and the transformer heads it was
//! computed from to ds-brain's `save_collisions` storage action, over the same
//! closed `/api/v1/data` save transport transformer saves use. ds-brain proves
//! the basis against the current heads and commits the stored document set;
//! it never computes. A result whose basis moved is refused, never merged.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ServiceRefusal,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const PUBLICATION_SCHEMA: &str = "ds.collisions-publication/v1";
/// The governed route admits a 32 MiB body; leave room for the envelope.
pub const MAX_PUBLICATION_BYTES: usize = 32 * 1024 * 1024 - 64 * 1024;
const MAX_TRANSFORMERS: usize = 2_000;
const LAYERS: [&str; 3] = [
    "collisions",
    "collisions_lv_lines",
    "collisions_service_cables",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionsPublication {
    pub schema: String,
    pub computed_at: String,
    pub engine: Value,
    pub basis: Vec<CollisionsBasisEntry>,
    pub input_errors: Vec<CollisionsInputError>,
    pub layers: BTreeMap<String, Value>,
    pub outcome: Value,
}

/// One transformer room the result was computed from, at the saved version
/// the edge actually read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionsBasisEntry {
    pub name: String,
    pub version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionsInputError {
    pub transformer: String,
    pub error_message: String,
}

/// ds-brain's commit receipt for one publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionsPublicationReceipt {
    pub project_id: String,
    pub saved: bool,
    pub regions: u64,
    pub layers: BTreeMap<String, u64>,
    pub chunk_count: u64,
    pub transformers: u64,
    pub computed_at: String,
    pub updated_at: i64,
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl CollisionsPublication {
    /// Client-side preflight: the shape ds-brain admits, before any transport.
    /// ds-brain repeats every check and adds the source-basis proof.
    pub fn validate(&self) -> Result<(), ClientError> {
        if self.schema != PUBLICATION_SCHEMA
            || self.computed_at.trim().is_empty()
            || !self.engine.is_object()
            || self.outcome["stage"] != "collision_detection"
            || self.outcome["status"] != "success"
        {
            return Err(invalid("collisions_publication_invalid"));
        }
        if self.basis.len() + self.input_errors.len() > MAX_TRANSFORMERS
            || (self.basis.is_empty() && !self.input_errors.is_empty())
        {
            return Err(invalid("collisions_publication_invalid"));
        }
        let mut seen = BTreeSet::new();
        for transformer in self.basis.iter().map(|entry| entry.name.as_str()).chain(
            self.input_errors
                .iter()
                .map(|entry| entry.transformer.as_str()),
        ) {
            if !name(transformer) || !seen.insert(transformer) {
                return Err(invalid("collisions_publication_invalid"));
            }
        }
        if self.layers.iter().any(|(layer, value)| {
            !LAYERS.contains(&layer.as_str()) || value["type"] != "FeatureCollection"
        }) {
            return Err(invalid("collisions_publication_invalid"));
        }
        Ok(())
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    publication: &CollisionsPublication,
) -> Result<CollisionsPublicationReceipt, ClientError> {
    publication.validate()?;
    let body = serde_json::to_vec(&json!({
        "action": "save_collisions",
        "eds_project_id": project,
        "collisions": publication,
    }))
    .map_err(|_| invalid("collisions_publication_invalid"))?;
    if body.len() > MAX_PUBLICATION_BYTES {
        return Err(invalid("collisions_publication_too_large"));
    }
    let response = transport.transformer_save(crate::TransformerSaveCall {
        profile,
        authority: authority.call(),
        body: &body,
    })?;
    if response.body.len() > 1024 * 1024 {
        return Err(unreadable("collisions_publication_response_too_large"));
    }
    let raw: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if response.status != 200 {
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
            "the collisions publication was refused by its owner",
            refusal,
        ));
    }
    if raw.get("success").is_some_and(|value| value != true) {
        return Err(unreadable("collisions_publication_response_failed"));
    }
    let receipt = raw.get("data").unwrap_or(&raw);
    let layers = receipt["layers"]
        .as_object()
        .map(|layers| {
            layers
                .iter()
                .map(|(name, count)| count.as_u64().map(|count| (name.clone(), count)))
                .collect::<Option<BTreeMap<_, _>>>()
        })
        .unwrap_or_else(|| Some(BTreeMap::new()));
    let parsed = (|| {
        Some(CollisionsPublicationReceipt {
            project_id: project.to_owned(),
            saved: receipt["saved"].as_bool()?,
            regions: receipt["regions"].as_u64()?,
            layers: layers?,
            chunk_count: receipt["chunk_count"].as_u64()?,
            transformers: receipt["transformers"].as_u64()?,
            computed_at: receipt["computed_at"].as_str()?.to_owned(),
            updated_at: receipt["updated_at"].as_i64()?,
        })
    })()
    .ok_or_else(|| unreadable("collisions_publication_receipt_invalid"))?;
    if !parsed.saved
        || parsed.computed_at != publication.computed_at
        || parsed.transformers != publication.basis.len() as u64
        || parsed.chunk_count == 0
    {
        return Err(unreadable("collisions_publication_receipt_mismatch"));
    }
    Ok(parsed)
}

#[cfg(test)]
#[path = "collisions_publish_tests.rs"]
mod tests;
