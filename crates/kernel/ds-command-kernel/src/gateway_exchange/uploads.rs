//! Governed uploads: Brain issues a resumable session for one file, the
//! Kernel sends the bytes ([`crate::transfer`]), and Brain registers them.
//! A processing upload lands where a project's processing domain reads it,
//! and answers its object path.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

pub const INVALID: &str = "upload_session_invalid";

/// The session Brain issued for one upload.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct UploadSession {
    pub session_uri: String,
    pub blob_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_domain: Option<String>,
}

/// A session that names where its bytes go.
pub(crate) fn session(body: &Value) -> Result<UploadSession, Refusal> {
    UploadSession::deserialize(data(body))
        .ok()
        .filter(|session| !session.session_uri.is_empty() && !session.blob_path.is_empty())
        .ok_or_else(|| Refusal::new(INVALID, "The upload session is incomplete."))
}

/// Open a session for one file a project's processing domain reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessingStart {
    pub project: String,
    /// `sketch_lv`, `lv_drafting`, `lv_process`, `design_layer_upload` or
    /// `mv_extrapolation`.
    pub domain: String,
    pub file_name: String,
    pub size: u64,
    pub content_type: String,
}

impl Exchange for ProcessingStart {
    type Output = UploadSession;

    fn plan(&self) -> Result<Plan, Refusal> {
        let path = match self.domain.as_str() {
            "mv_extrapolation" => "/mv",
            _ => "/process",
        };
        let body = json!({"action": "upload_start", "eds_project_id": self.project,
            "upload_domain": self.domain, "file_name": self.file_name, "size": self.size,
            "content_type": self.content_type});
        Ok(Request::post(path, body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let session = session(&body)?;
        if session.upload_domain.as_deref() != Some(self.domain.as_str()) {
            return Err(Refusal::new(
                INVALID,
                "The upload session names another processing target.",
            ));
        }
        Ok(Admitted::Done(session))
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use wasm_bindgen::prelude::*;

    use super::ProcessingStart;
    use crate::gateway_exchange::browser::{rejection, run, GatewayHost};
    use crate::transfer::browser::exports::{send, UploadSource};

    /// Upload one file for a project's processing domain; answers the object
    /// path the domain reads.
    #[wasm_bindgen(js_name = processingUpload)]
    pub async fn processing_upload(
        host: GatewayHost,
        project: String,
        domain: String,
        source: UploadSource,
        file_name: String,
        content_type: String,
    ) -> Result<String, JsValue> {
        let start = ProcessingStart {
            project,
            domain,
            file_name,
            size: source.size().max(0.0) as u64,
            content_type,
        };
        let session = run(&host, start).await?;
        send(
            &host,
            &session.session_uri,
            source,
            None,
            None,
            false,
            None,
            None,
        )
        .await
        .finished()
        .map_err(|refusal| rejection(&refusal))?;
        Ok(session.blob_path)
    }
}

#[cfg(test)]
#[path = "uploads_tests.rs"]
mod tests;
