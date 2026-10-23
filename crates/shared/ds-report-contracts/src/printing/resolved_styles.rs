//! Consumer of `style_resolution::prepare_print` API receipts. The Styles API
//! owns every choice and immutable document. This module performs no lookup,
//! precedence, cloning, seeding or physical pen generation.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VectorLayer {
    pub id: String,
    pub label: String,
    pub geojson: Value,
}

/// Isolated integration boundary. The host asks the shared kernel resolver
/// once with these exact keys and holds its `prepare_print` output plus keys.
/// The pure native/WASM consumer never chooses a document from a catalogue.
pub fn consume(
    capture: &Value,
    keys: &BTreeMap<String, Value>,
    project: &str,
) -> Result<Value, String> {
    if capture["project_id"].as_str() != Some(project) {
        return Err("project_context_changed: style receipt belongs to another project".into());
    }
    if capture["resolution_revision"]
        .as_str()
        .is_none_or(|v| !digest(v))
    {
        return Err(
            "style_resolution_invalid: receipt lacks its immutable resolution revision".into(),
        );
    }
    let mut references = BTreeMap::new();
    let mut documents = BTreeMap::new();
    let mut revisions = BTreeMap::new();
    for (layer, key) in keys {
        if key["target"] != "print" || capture["keys"][layer] != *key {
            return Err(format!(
                "style_resolution_unknown: API receipt has no exact request for {layer}"
            ));
        }
        let reference = capture["style_refs"][layer].as_str().ok_or_else(|| {
            format!("style_resolution_unknown: API receipt has no document for {layer}")
        })?;
        let document = &capture["print_styles"][reference];
        let revision = &capture["style_revisions"][reference];
        if revision["revision_id"].as_str().is_none_or(|v| !digest(v))
            || revision["content_sha256"].as_str() != Some(&content_digest(document)?)
        {
            return Err(format!(
                "style_resolution_revision_mismatch: {layer} differs from its API receipt"
            ));
        }
        crate::style_validation::validate(document)
            .map_err(|e| format!("style_resolution_invalid: {layer}: {e}"))?;
        references.insert(layer.clone(), reference.to_string());
        documents.insert(reference.to_string(), document.clone());
        revisions.insert(reference.to_string(), revision.clone());
    }
    Ok(
        json!({"style_refs":references,"print_styles":documents,"style_revisions":revisions,
        "resolution_revision":capture["resolution_revision"]}),
    )
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Same normalized canonical JSON used by the style API's immutable receipts.
pub fn content_digest(document: &Value) -> Result<String, String> {
    let mut document = document.clone();
    crate::style_document::normalize_json_numbers(&mut document);
    crate::canonical::digest(&document)
}
