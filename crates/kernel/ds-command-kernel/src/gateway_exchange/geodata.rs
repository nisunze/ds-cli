//! Geographic uploads a person keeps: their own (`user_data`) or a
//! project's (`project_data`). Brain issues a resumable session, the Kernel
//! sends the bytes, and registering them starts the tiler; a stuck tiling is
//! retried by registering the stored file again.

use ds_gateway_client::Operation;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::uploads::{session, UploadSession};
use super::{data, Admitted, Exchange, Plan, Refusal, Request};

pub const INVALID: &str = "geodata_answer_invalid";

fn operation(project: Option<&str>) -> Operation {
    match project {
        Some(_) => Operation::ProjectDataAction,
        None => Operation::UserDataAction,
    }
}

fn post(action: &str, project: Option<&str>, mut body: Value) -> Result<Request, Refusal> {
    body["action"] = action.into();
    if let Some(project) = project {
        body["project_id"] = project.into();
    }
    Request::generated(operation(project), Some(body))
}

fn invalid() -> Refusal {
    Refusal::new(INVALID, "The data service answered an invalid list.")
}

/// One upload as its list shows it; fields this client does not name pass
/// through unchanged.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GeodataDoc {
    pub upload_id: String,
    pub file_name: String,
    /// The owner of a person's upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    /// Who uploaded a project's file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uploaded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_status: Option<String>,
    /// Layer name to its metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub layers: Option<Map<String, Value>>,
    #[serde(flatten)]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub rest: Map<String, Value>,
}

/// One row of a project's data-provenance audit: metadata only.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AuditDataset {
    /// `project_data`, `user_data`, `vector_tile` or `bq_dataset`.
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_count: Option<i64>,
    /// Epoch milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// `owned`, `linked`, `tile` or `bigquery`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pmtiles_path: Option<String>,
}

/// Rows a list answers under `member`; Go writes an empty list as `null`.
fn rows<T: for<'de> Deserialize<'de>>(body: &Value, member: &str) -> Result<Vec<T>, Refusal> {
    match data(body).get(member) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(rows) => Vec::<T>::deserialize(rows).map_err(|_| invalid()),
    }
}

/// Every upload the caller owns or shares, or every upload of a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeodataList {
    pub project: Option<String>,
}

impl Exchange for GeodataList {
    type Output = Vec<GeodataDoc>;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(post("list", self.project.as_deref(), json!({}))?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        rows(&body, "uploads").map(Admitted::Done)
    }
}

/// Open a session for one geodata file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeodataStart {
    pub project: Option<String>,
    pub file_name: String,
    pub size: u64,
    pub content_type: String,
}

impl Exchange for GeodataStart {
    type Output = UploadSession;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"file_name": self.file_name, "size": self.size,
            "content_type": self.content_type});
        Ok(post("upload_start", self.project.as_deref(), body)?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        session(&body).map(Admitted::Done)
    }
}

/// What registering a geodata file started.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GeodataRegistered {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_status: Option<String>,
}

/// Register a stored geodata file; Brain starts its tiling. Registering the
/// same file again retries a stuck tiling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeodataRegister {
    pub project: Option<String>,
    pub blob_path: String,
    pub file_name: String,
}

impl Exchange for GeodataRegister {
    type Output = GeodataRegistered;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"blob_path": self.blob_path, "file_name": self.file_name});
        Ok(post("upload", self.project.as_deref(), body)?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(
            GeodataRegistered::deserialize(data(&body)).unwrap_or_default(),
        ))
    }
}

/// Delete one upload: its record, and its stored files when it owns them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeodataDelete {
    pub project: Option<String>,
    pub upload_id: String,
}

impl Exchange for GeodataDelete {
    type Output = ();

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"upload_id": self.upload_id});
        Ok(post("delete", self.project.as_deref(), body)?.into())
    }

    fn admit(self, _status: u16, _body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(()))
    }
}

/// A project's data-provenance audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeodataAudit {
    pub project: String,
}

impl Exchange for GeodataAudit {
    type Output = Vec<AuditDataset>;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(post("audit", Some(&self.project), json!({}))?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        rows(&body, "datasets").map(Admitted::Done)
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use js_sys::Function;
    use wasm_bindgen::prelude::*;

    use super::{GeodataAudit, GeodataDelete, GeodataList, GeodataRegister, GeodataStart};
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use crate::transfer::browser::exports::{send, UploadSource};

    /// Every upload the caller owns or shares, or every upload of `project`.
    #[wasm_bindgen(js_name = geodataList, unchecked_return_type = "GeodataDoc[]")]
    pub async fn geodata_list(
        host: GatewayHost,
        project: Option<String>,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, GeodataList { project }).await?)
    }

    /// Upload one geodata file, the caller's own or `project`'s, and start
    /// its tiling.
    #[wasm_bindgen(js_name = geodataUpload, unchecked_return_type = "GeodataRegistered")]
    pub async fn geodata_upload(
        host: GatewayHost,
        project: Option<String>,
        source: UploadSource,
        file_name: String,
        content_type: String,
        #[wasm_bindgen(
            unchecked_param_type = "((progress: UploadProgress) => void) | null | undefined"
        )]
        progress: Option<Function>,
    ) -> Result<JsValue, JsValue> {
        let start = GeodataStart {
            project: project.clone(),
            file_name: file_name.clone(),
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
            progress,
            None,
        )
        .await
        .finished()
        .map_err(|refusal| rejection(&refusal))?;
        let register = GeodataRegister {
            project,
            blob_path: session.blob_path,
            file_name,
        };
        typed(&run(&host, register).await?)
    }

    /// Retry a stuck or failed tiling of a stored upload.
    #[wasm_bindgen(js_name = geodataRetry, unchecked_return_type = "GeodataRegistered")]
    pub async fn geodata_retry(
        host: GatewayHost,
        project: Option<String>,
        blob_path: String,
        file_name: String,
    ) -> Result<JsValue, JsValue> {
        let call = GeodataRegister {
            project,
            blob_path,
            file_name,
        };
        typed(&run(&host, call).await?)
    }

    /// Delete one upload.
    #[wasm_bindgen(js_name = geodataDelete)]
    pub async fn geodata_delete(
        host: GatewayHost,
        project: Option<String>,
        upload_id: String,
    ) -> Result<(), JsValue> {
        run(&host, GeodataDelete { project, upload_id }).await
    }

    /// A project's data-provenance audit.
    #[wasm_bindgen(js_name = geodataAudit, unchecked_return_type = "AuditDataset[]")]
    pub async fn geodata_audit(host: GatewayHost, project: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, GeodataAudit { project }).await?)
    }
}

#[cfg(test)]
#[path = "geodata_tests.rs"]
mod tests;
