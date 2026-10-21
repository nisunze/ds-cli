//! Rendered prints attached to a project's report slot: Brain issues a
//! session for one page, verifies the uploaded bytes against the page's
//! declaration when it attaches them, and removes one exact reference on
//! request (the bytes stay).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::uploads::{session, UploadSession};
use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/report_artifacts";
/// Attaching verifies every byte of a printout on Brain.
const ATTACH_TIMEOUT_MS: u64 = 600_000;
pub const INVALID: &str = "report_artifact_receipt_invalid";

/// One rendered page, as Brain verifies it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ArtifactDeclaration {
    pub project_id: String,
    /// `transformer`, `combined` or `mv`.
    pub scope: String,
    pub transformer: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub sha256: String,
    pub map_family: String,
    pub layout_name: String,
    pub paper_size: String,
    pub orientation: String,
    /// `sheet`, `atlas` or `joined`.
    pub page_role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_receipt_sha256: Option<String>,
}

/// What Brain attached.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AttachedArtifact {
    pub project_id: String,
    pub scope: String,
    pub transformer: String,
    pub file_name: String,
    pub sha256: String,
    pub gcs_path: String,
    pub map_family: String,
}

fn declared(action: &str, declaration: &ArtifactDeclaration) -> Value {
    let mut body = json!(declaration);
    body["action"] = action.into();
    body
}

/// Open a session for one page's bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactStart(pub ArtifactDeclaration);

impl Exchange for ArtifactStart {
    type Output = UploadSession;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::post(PATH, declared("upload_start", &self.0)).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        session(&body).map(Admitted::Done)
    }
}

/// Attach uploaded bytes to the page's report slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactAttach {
    pub declaration: ArtifactDeclaration,
    pub blob_path: String,
}

impl Exchange for ArtifactAttach {
    type Output = AttachedArtifact;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = declared("attach", &self.declaration);
        body["blob_path"] = self.blob_path.clone().into();
        Ok(Request::post(PATH, body)
            .timeout_ms(ATTACH_TIMEOUT_MS)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        AttachedArtifact::deserialize(data(&body))
            .ok()
            .filter(|attached| {
                attached.sha256 == self.declaration.sha256
                    && attached.project_id == self.declaration.project_id
                    && !attached.gcs_path.is_empty()
            })
            .map(Admitted::Done)
            .ok_or_else(|| Refusal::new(INVALID, "Brain answered an invalid attachment receipt."))
    }
}

/// Remove one exact published reference; Brain keeps its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRemove {
    pub project_id: String,
    pub scope: String,
    pub transformer: String,
    pub file_name: String,
    pub gcs_path: String,
    pub sha256: String,
}

impl Exchange for ArtifactRemove {
    type Output = ();

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"action": "remove", "project_id": self.project_id,
            "scope": self.scope, "transformer": self.transformer, "file_name": self.file_name,
            "blob_path": self.gcs_path, "sha256": self.sha256});
        Ok(Request::post(PATH, body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let receipt = data(&body);
        let same = |field: &str, expected: &str| {
            receipt.get(field).and_then(Value::as_str) == Some(expected)
        };
        let exact = same("project_id", &self.project_id)
            && same("scope", &self.scope)
            && same("transformer", &self.transformer)
            && same("file_name", &self.file_name)
            && same("gcs_path", &self.gcs_path)
            && same("sha256", &self.sha256)
            && receipt.get("bytes_deleted") == Some(&Value::Bool(false))
            && receipt.get("removed").and_then(Value::as_u64).is_some();
        if !exact {
            return Err(Refusal::new(
                INVALID,
                "Invalid print removal receipt. Refresh the project before trying again.",
            ));
        }
        Ok(Admitted::Done(()))
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use wasm_bindgen::prelude::*;

    use super::{ArtifactAttach, ArtifactDeclaration, ArtifactRemove, ArtifactStart};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};

    fn declaration(value: JsValue) -> Result<ArtifactDeclaration, JsValue> {
        tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|error| js_sys::Error::new(&error.to_string()).into())
    }

    /// Open a session for one rendered page's bytes.
    #[wasm_bindgen(js_name = reportArtifactStart, unchecked_return_type = "UploadSession")]
    pub async fn report_artifact_start(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "ArtifactDeclaration")] declaration: JsValue,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, ArtifactStart(self::declaration(declaration)?)).await?)
    }

    /// Attach uploaded bytes; Brain verifies them against the declaration.
    #[wasm_bindgen(js_name = reportArtifactAttach, unchecked_return_type = "AttachedArtifact")]
    pub async fn report_artifact_attach(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "ArtifactDeclaration")] declaration: JsValue,
        blob_path: String,
    ) -> Result<JsValue, JsValue> {
        let call = ArtifactAttach {
            declaration: self::declaration(declaration)?,
            blob_path,
        };
        typed(&run(&host, call).await?)
    }

    /// Remove one exact published reference.
    #[wasm_bindgen(js_name = reportArtifactRemove)]
    pub async fn report_artifact_remove(
        host: GatewayHost,
        project_id: String,
        scope: String,
        transformer: String,
        file_name: String,
        gcs_path: String,
        sha256: String,
    ) -> Result<(), JsValue> {
        let call = ArtifactRemove {
            project_id,
            scope,
            transformer,
            file_name,
            gcs_path,
            sha256,
        };
        run(&host, call).await
    }
}

#[cfg(test)]
#[path = "report_artifacts_tests.rs"]
mod tests;
