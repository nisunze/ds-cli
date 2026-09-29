//! Fenced native writes through the existing transformer save authority.
//! Hosts supply an explicit processed result; this owner checks sources,
//! configuration and published readback. It never acquires a project override.
use crate::{
    authorized::{self, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
};
use ds_command_kernel::report_export::jcs::layers_content_digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

// Match ds-brain's /api/v1/data body limit. Ordinary retained LV designs can
// exceed 2 MiB even for a single transformer; never truncate their properties.
pub const MAX_SAVE_BYTES: usize = 32 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformerSaveBatch {
    pub project_id: String,
    pub items: Vec<TransformerSaveItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformerSaveItem {
    pub transformer_name: String,
    pub base_version: u64,
    pub source_content_digest: String,
    pub source_layers: BTreeMap<String, Value>,
    pub gdfs: BTreeMap<String, Value>,
    pub config_dfs: BTreeMap<String, Value>,
    pub process_metadata: Value,
    pub operation_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformerSaveReceipt {
    pub project_id: String,
    pub results: Vec<TransformerSaved>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformerSaved {
    pub transformer_name: String,
    pub verified: bool,
    pub unchanged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}
fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}
fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.trim() == value
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn digest(layers: &BTreeMap<String, Value>) -> Result<String, ClientError> {
    layers_content_digest(layers).map_err(|_| invalid("transformer_save_layer_digest_invalid"))
}
fn validate_layers(layers: &BTreeMap<String, Value>) -> Result<(), ClientError> {
    if layers.is_empty() || layers.len() > 64 {
        return Err(invalid("transformer_save_invalid_layers"));
    }
    let mut count = 0usize;
    for (name, fc) in layers {
        if !token(name) || fc["type"] != "FeatureCollection" {
            return Err(invalid("transformer_save_invalid_layers"));
        }
        let features = fc["features"]
            .as_array()
            .ok_or_else(|| invalid("transformer_save_invalid_layers"))?;
        count += features.len();
        if count > 100_000
            || features
                .iter()
                .any(|f| f["type"] != "Feature" || !f["properties"].is_object())
        {
            return Err(invalid("transformer_save_invalid_features"));
        }
    }
    Ok(())
}
impl TransformerSaveBatch {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if self.project_id != project
            || !token(project)
            || self.items.is_empty()
            || self.items.len() > 4
        {
            return Err(invalid("transformer_save_project_or_batch_invalid"));
        }
        let mut names = BTreeSet::new();
        for item in &self.items {
            if !token(&item.transformer_name)
                || matches!(
                    item.transformer_name.as_str(),
                    "mv_data" | "combined_transformer" | "collisions" | "__all__"
                )
                || !names.insert(item.transformer_name.as_str())
                || item.base_version == 0
                || item.base_version >= i64::MAX as u64
                || !valid_sha(&item.source_content_digest)
                || !token(&item.operation_id)
                || item.config_dfs.is_empty()
                || item.config_dfs.len() > 64
                || !item.process_metadata.is_object()
                || item.process_metadata["lane"] != "fast"
                || !matches!(
                    item.process_metadata["status"].as_str(),
                    Some("success" | "warning")
                )
            {
                return Err(invalid("transformer_save_invalid_item"));
            }
            validate_layers(&item.source_layers)?;
            validate_layers(&item.gdfs)?;
            if digest(&item.source_layers)? != item.source_content_digest {
                return Err(invalid("transformer_save_source_digest_mismatch"));
            }
        }
        if self.wire_body()?.len() > MAX_SAVE_BYTES {
            return Err(invalid("transformer_save_payload_too_large"));
        }
        Ok(())
    }
    fn wire_body(&self) -> Result<Vec<u8>, ClientError> {
        let items = self
            .items
            .iter()
            .map(|item| {
                Ok(json!({
                    "transformer_name":item.transformer_name,"base_version":item.base_version,
                    "gdfs":item.gdfs,"content_digest":digest(&item.gdfs)?,
                    "process_metadata":item.process_metadata,"operation_id":item.operation_id
                }))
            })
            .collect::<Result<Vec<_>, ClientError>>()?;
        serde_json::to_vec(&json!({"action":"save_transformers_data","eds_project_id":self.project_id,"transformer_saves":items})).map_err(|_|invalid("transformer_save_payload_invalid"))
    }
}
fn check_config(item: &TransformerSaveItem, sheets: &Value) -> Result<(), ClientError> {
    for (name, expected) in &item.config_dfs {
        if !token(name) || sheets.get(name) != Some(expected) {
            return Err(invalid("transformer_save_config_changed"));
        }
    }
    Ok(())
}
fn preflight(
    item: &TransformerSaveItem,
    snapshot: &crate::TransformerContext,
    result_digest: &str,
) -> Result<bool, ClientError> {
    if snapshot.transformer_name() != item.transformer_name {
        return Err(unreadable("transformer_save_context_identity_mismatch"));
    }
    let metadata = snapshot.metadata();
    let actual_digest = digest(snapshot.layers())?;
    if metadata.content_digest() != Some(actual_digest.as_str()) {
        return Err(unreadable("transformer_save_context_digest_mismatch"));
    }
    if actual_digest == result_digest
        && metadata
            .version()
            .is_some_and(|version| version >= item.base_version)
    {
        return Ok(true);
    }
    if metadata.version() != Some(item.base_version) || actual_digest != item.source_content_digest
    {
        return Err(invalid("transformer_save_source_changed"));
    }
    Ok(false)
}
fn verified(
    item: &TransformerSaveItem,
    snapshot: &crate::TransformerContext,
    unchanged: bool,
) -> TransformerSaved {
    TransformerSaved {
        transformer_name: item.transformer_name.clone(),
        verified: true,
        unchanged,
        version: snapshot.metadata().version(),
        content_digest: snapshot.metadata().content_digest().map(str::to_owned),
        error_code: None,
    }
}
fn failure(item: &TransformerSaveItem, code: &str) -> TransformerSaved {
    TransformerSaved {
        transformer_name: item.transformer_name.clone(),
        verified: false,
        unchanged: false,
        version: None,
        content_digest: None,
        error_code: Some(code.into()),
    }
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    batch: &TransformerSaveBatch,
) -> Result<TransformerSaveReceipt, ClientError> {
    batch.validate(project)?;
    let config = authorized::feeder_configuration(
        profile,
        transport,
        authority,
        project,
        Some(&crate::ProjectConfigurationChange::ReadSettings),
    )?;
    let mut receipts = BTreeMap::new();
    let mut pending = Vec::new();
    // Every source/config preflight precedes the first write. Server CAS still
    // fences edits occurring after these reads; no force fields are exposed.
    for item in &batch.items {
        check_config(item, &config.document["sheets"])?;
        let snapshot = authorized::transformer_context(
            profile,
            transport,
            authority,
            project,
            &item.transformer_name,
            crate::TransformerContextProjection::Saved,
        )?;
        if preflight(item, &snapshot, &digest(&item.gdfs)?)? {
            receipts.insert(
                item.transformer_name.clone(),
                verified(item, &snapshot, true),
            );
        } else {
            pending.push(item.clone());
        }
    }
    if !pending.is_empty() {
        let submission = TransformerSaveBatch {
            project_id: project.into(),
            items: pending,
        };
        let bytes = submission.wire_body()?;
        let response = transport.transformer_save(TransformerSaveCall {
            profile,
            authority: authority.call(),
            body: &bytes,
        })?;
        if response.body.len() > 1024 * 1024 {
            return Err(unreadable("transformer_save_response_too_large"));
        }
        if response.status != 200 {
            return Err(authorized::classify_tile_error(response.status));
        }
        let raw: Value = serde_json::from_slice(&response.body)
            .map_err(|_| unreadable("transformer_save_response_invalid"))?;
        if raw.get("success").is_some_and(|v| v != true) {
            return Err(unreadable("transformer_save_response_failed"));
        }
        let body = raw.get("data").unwrap_or(&raw);
        let rows = body["results"]
            .as_array()
            .ok_or_else(|| unreadable("transformer_save_results_missing"))?;
        if body["total"].as_u64() != Some(submission.items.len() as u64)
            || rows.len() != submission.items.len()
            || rows.iter().any(|row| !row["ok"].is_boolean())
            || body["saved_count"].as_u64()
                != Some(rows.iter().filter(|row| row["ok"] == true).count() as u64)
            || body["saved_count"]
                .as_u64()
                .zip(body["failed_count"].as_u64())
                .and_then(|(a, b)| a.checked_add(b))
                != Some(rows.len() as u64)
        {
            return Err(unreadable("transformer_save_result_count_mismatch"));
        }
        let mut seen = BTreeSet::new();
        for row in rows {
            let name = row["transformer_name"]
                .as_str()
                .ok_or_else(|| unreadable("transformer_save_result_name_missing"))?;
            let item = submission
                .items
                .iter()
                .find(|i| i.transformer_name == name)
                .ok_or_else(|| unreadable("transformer_save_result_name_mismatch"))?;
            if !seen.insert(name)
                || row["operation_id"].as_str() != Some(item.operation_id.as_str())
            {
                return Err(unreadable("transformer_save_result_operation_mismatch"));
            }
            if row["ok"] != true {
                let code = row["error_code"]
                    .as_str()
                    .filter(|v| token(v))
                    .unwrap_or("TRANSFORMER_SAVE_FAILED");
                receipts.insert(name.into(), failure(item, code));
                continue;
            }
            let result_digest = digest(&item.gdfs)?;
            let result = &row["result"];
            let unchanged = result["unchanged"].as_bool().unwrap_or(false);
            if result["transformer_name"] != name
                || result["content_digest"] != result_digest
                || result["version"].as_u64().is_none_or(|version| {
                    if unchanged {
                        version < item.base_version
                    } else {
                        version != item.base_version + 1
                    }
                })
            {
                receipts.insert(
                    name.into(),
                    failure(item, "TRANSFORMER_SAVE_RECEIPT_MISMATCH"),
                );
                continue;
            }
            match authorized::transformer_context(
                profile,
                transport,
                authority,
                project,
                name,
                crate::TransformerContextProjection::Saved,
            ) {
                Ok(snapshot)
                    if snapshot.metadata().version() == result["version"].as_u64()
                        && snapshot.metadata().content_digest() == Some(result_digest.as_str())
                        && digest(snapshot.layers())? == result_digest =>
                {
                    receipts.insert(name.into(), verified(item, &snapshot, unchanged));
                }
                _ => {
                    receipts.insert(
                        name.into(),
                        failure(item, "TRANSFORMER_SAVE_READBACK_UNVERIFIED"),
                    );
                }
            }
        }
    }
    Ok(TransformerSaveReceipt {
        project_id: project.into(),
        results: batch
            .items
            .iter()
            .map(|i| {
                receipts
                    .remove(&i.transformer_name)
                    .expect("each item has receipt")
            })
            .collect(),
    })
}

/// Closed bearer transport. Its body can only be constructed after preflight.
pub struct TransformerSaveCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) authority: crate::transport::CallAuthority<'a>,
    pub(crate) body: &'a [u8],
}
impl TransformerSaveCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        "/api/v1/data"
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        180
    }
    pub fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn body(&self) -> crate::SecretRequestBody {
        crate::SecretRequestBody::new(self.body.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item() -> TransformerSaveItem {
        let layers = BTreeMap::from([(
            "lv_poles".into(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[15.0,9.0]},"properties":{"material":"concrete"}}]}),
        )]);
        let mut result = layers.clone();
        result.get_mut("lv_poles").unwrap()["features"][0]["properties"]["material"] =
            json!("steel");
        TransformerSaveItem {
            transformer_name: "T1".into(),
            base_version: 7,
            source_content_digest: digest(&layers).unwrap(),
            source_layers: layers,
            gdfs: result,
            config_dfs: BTreeMap::from([("lv_poles_struct_type".into(), json!([]))]),
            process_metadata: json!({"lane":"fast","status":"success"}),
            operation_id: "save-T1-7".into(),
        }
    }
    fn snapshot(
        item: &TransformerSaveItem,
        layers: &BTreeMap<String, Value>,
        version: u64,
    ) -> crate::TransformerContext {
        let body = serde_json::to_vec(&json!({"total":1,"found_count":1,"failed_count":0,"results":[{
            "transformer_name":item.transformer_name,"ok":true,"data":{"layers":layers,"metadata":{"version":version,"content_digest":digest(layers).unwrap()}}
        }]})).unwrap();
        crate::transformers::decode_context(
            crate::TransportResponse { status: 200, body },
            1024 * 1024,
            "project",
            &item.transformer_name,
        )
        .unwrap()
    }
    #[test]
    fn save_fences_source_and_retries_already_published_result() {
        let item = item();
        let result_digest = digest(&item.gdfs).unwrap();
        assert!(!preflight(
            &item,
            &snapshot(&item, &item.source_layers, 7),
            &result_digest
        )
        .unwrap());
        assert!(preflight(
            &item,
            &snapshot(&item, &item.source_layers, 8),
            &result_digest
        )
        .is_err());
        assert!(preflight(&item, &snapshot(&item, &item.gdfs, 8), &result_digest).unwrap());
        assert!(preflight(&item, &snapshot(&item, &item.gdfs, 6), &result_digest).is_err());
    }
    #[test]
    fn save_binds_project_config_and_result_digest_without_source_or_force_fields_on_wire() {
        let item = item();
        let mut batch = TransformerSaveBatch {
            project_id: "project".into(),
            items: vec![item.clone()],
        };
        batch.validate("project").unwrap();
        assert!(batch.validate("other").is_err());
        check_config(&item, &json!({"lv_poles_struct_type":[]})).unwrap();
        assert!(check_config(
            &item,
            &json!({"lv_poles_struct_type":[{"material":"concrete"}]})
        )
        .is_err());
        let wire: Value = serde_json::from_slice(&batch.wire_body().unwrap()).unwrap();
        let saved = &wire["transformer_saves"][0];
        assert_eq!(saved["content_digest"], digest(&item.gdfs).unwrap());
        assert_ne!(saved["content_digest"], item.source_content_digest);
        assert!(saved.get("force_version_conflict").is_none());
        assert!(saved.get("source_layers").is_none());
        batch.items[0].source_layers = item.gdfs;
        assert!(batch.validate("project").is_err());
    }
    #[test]
    fn save_refuses_duplicate_items_and_oversized_payloads() {
        let item = item();
        let mut batch = TransformerSaveBatch {
            project_id: "project".into(),
            items: vec![item.clone(), item.clone()],
        };
        assert!(batch.validate("project").is_err());
        batch.items = vec![item];
        let held = "x".repeat(3 * 1024 * 1024);
        batch.items[0].gdfs.get_mut("lv_poles").unwrap()["features"][0]["properties"]["retained"] =
            json!(held);
        batch
            .validate("project")
            .expect("one full retained design fits the server contract");
        let body: Value = serde_json::from_slice(&batch.wire_body().unwrap()).unwrap();
        assert_eq!(
            body["transformer_saves"][0]["gdfs"]["lv_poles"]["features"][0]["properties"]
                ["retained"],
            held
        );
        batch.items[0].gdfs.get_mut("lv_poles").unwrap()["features"][0]["properties"]
            ["oversized"] = json!("x".repeat(MAX_SAVE_BYTES));
        assert!(batch.validate("project").is_err());
    }
}
