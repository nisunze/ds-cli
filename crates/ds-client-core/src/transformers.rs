use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{ClientError, ErrorKind, TransportResponse};

pub(crate) const MAX_TRANSFORMER_NAME_CHARS: usize = 160;
const MAX_LAYER_NAME_CHARS: usize = 120;
const MAX_CONTENT_DIGEST_CHARS: usize = 256;

/// One full saved transformer snapshot for headless context, selection and reports.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformerContext {
    ds_project: String,
    transformer_name: String,
    layers: BTreeMap<String, Value>,
    metadata: TransformerContextMetadata,
    voltage_drop_metadata: Option<Value>,
}

impl TransformerContext {
    pub fn ds_project(&self) -> &str {
        &self.ds_project
    }

    pub fn transformer_name(&self) -> &str {
        &self.transformer_name
    }

    pub fn layers(&self) -> &BTreeMap<String, Value> {
        &self.layers
    }

    /// Same-snapshot compact receipt; full analysis is never in this response.
    pub fn voltage_drop_metadata(&self) -> Option<&Value> {
        self.voltage_drop_metadata.as_ref()
    }

    pub const fn metadata(&self) -> &TransformerContextMetadata {
        &self.metadata
    }
}

/// Same-snapshot revision metadata. Both fields are optional for explicit
/// legacy-state reporting; the client never fabricates either value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformerContextMetadata {
    version: Option<u64>,
    content_digest: Option<String>,
}

impl TransformerContextMetadata {
    pub const fn version(&self) -> Option<u64> {
        self.version
    }

    pub fn content_digest(&self) -> Option<&str> {
        self.content_digest.as_deref()
    }
}

#[derive(Deserialize)]
struct TransformerContextPayload {
    success: bool,
    data: TransformerBatch,
}

#[derive(Deserialize)]
struct TransformerBatch {
    total: usize,
    found_count: usize,
    failed_count: usize,
    results: Vec<TransformerBatchItem>,
}

#[derive(Deserialize)]
struct TransformerBatchItem {
    transformer_name: String,
    ok: bool,
    #[serde(default)]
    data: Option<TransformerData>,
    #[serde(default)]
    error_code: Option<String>,
}

#[derive(Deserialize)]
struct TransformerData {
    #[serde(default)]
    voltage_drop_metadata: Option<Value>,
    layers: BTreeMap<String, Value>,
    #[serde(default)]
    metadata: Option<TransformerMetadataWire>,
}

#[derive(Default, Deserialize)]
struct TransformerMetadataWire {
    #[serde(default)]
    version: Option<u64>,
    #[serde(default)]
    content_digest: Option<String>,
}

pub(crate) fn validate_context_input(
    ds_project: &str,
    transformer: &str,
) -> Result<(), ClientError> {
    crate::validate_project_id(ds_project)?;
    if !bounded_identifier(transformer, MAX_TRANSFORMER_NAME_CHARS) || transformer == "__all__" {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "the transformer must be one explicit bounded transformer name",
        ));
    }
    Ok(())
}

pub(crate) fn decode_context(
    response: TransportResponse,
    response_limit: usize,
    ds_project: &str,
    transformer: &str,
) -> Result<TransformerContext, ClientError> {
    if response.body.len() > response_limit {
        return Err(unreadable());
    }
    let payload: TransformerContextPayload =
        serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if !payload.success {
        return Err(unreadable());
    }
    let batch = payload.data;
    if batch.total != 1 || batch.results.len() != 1 {
        return Err(unreadable());
    }
    let item = batch.results.into_iter().next().ok_or_else(unreadable)?;
    if item.transformer_name != transformer {
        return Err(unreadable());
    }
    if !item.ok {
        if batch.found_count != 0 || batch.failed_count != 1 || item.data.is_some() {
            return Err(unreadable());
        }
        return match item.error_code.as_deref() {
            Some("TRANSFORMER_NOT_FOUND") => Err(ClientError::new(
                ErrorKind::ResourceNotFound,
                "the selected transformer does not exist in the project",
            )),
            Some("TRANSFORMER_READ_FAILED") => Err(ClientError::new(
                ErrorKind::Transient,
                "the transformer context service could not read the selected transformer",
            )),
            _ => Err(unreadable()),
        };
    }
    if batch.found_count != 1 || batch.failed_count != 0 {
        return Err(unreadable());
    }
    let data = item.data.ok_or_else(unreadable)?;
    if data
        .layers
        .keys()
        .any(|name| !bounded_identifier(name, MAX_LAYER_NAME_CHARS))
    {
        return Err(unreadable());
    }
    let metadata = data.metadata.unwrap_or_default();
    if metadata
        .content_digest
        .as_deref()
        .is_some_and(|digest| !bounded_identifier(digest, MAX_CONTENT_DIGEST_CHARS))
    {
        return Err(unreadable());
    }
    Ok(TransformerContext {
        ds_project: ds_project.to_owned(),
        transformer_name: item.transformer_name,
        layers: data.layers,
        voltage_drop_metadata: data.voltage_drop_metadata,
        metadata: TransformerContextMetadata {
            version: metadata.version,
            content_digest: metadata.content_digest,
        },
    })
}

fn bounded_identifier(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the response is outside the closed transformer-context contract",
    )
}
