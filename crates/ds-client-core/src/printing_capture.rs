//! Pure held-transport seam for the same saved-source reads as the native CLI.
//! A host supplies exact response entities. This module never selects an account,
//! changes an API lane, fetches data, calculates an analysis or repairs a seed.
use crate::{
    ClientError, PrintingStandardKind, PrintingStandardRequest, TransformerContext,
    TransportResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Read {
    pub path: String,
    pub method: &'static str,
    pub body: Value,
    pub response_limit: usize,
    pub timeout_seconds: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
    pub status: u16,
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub analysis_sha256: Option<String>,
    pub cache_control: Option<String>,
}
impl Response {
    fn transport(self) -> TransportResponse {
        TransportResponse::new(self.status, self.bytes)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPlan {
    pub snapshot: Read,
    pub config: Read,
    pub document: Read,
    pub styles: Read,
}

fn a4() -> PrintingStandardRequest {
    PrintingStandardRequest::Get {
        kind: PrintingStandardKind::A4,
        id: "voltage-drop-a4-v1".into(),
    }
}
fn styles(project: &str) -> ds_command_kernel::style_governance::Request {
    ds_command_kernel::style_governance::Request {
        project: project.into(),
        command: ds_command_kernel::style_governance::Command::Table,
    }
}
fn read(path: impl Into<String>, body: Vec<u8>, limit: usize, timeout: u64) -> Read {
    Read {
        path: path.into(),
        method: "POST",
        body: serde_json::from_slice(&body).expect("closed native request"),
        response_limit: limit,
        timeout_seconds: timeout,
    }
}
pub fn plan(project: &str, transformer: &str) -> Result<ReadPlan, ClientError> {
    crate::transformers::validate_context_input(project, transformer)?;
    Ok(ReadPlan {
        snapshot: read(
            crate::TRANSFORMER_CONTEXT_PATH,
            crate::transformers::context_body(
                project,
                transformer,
                crate::TransformerContextProjection::Saved,
            ),
            crate::TRANSFORMER_CONTEXT_RESPONSE_LIMIT,
            crate::TRANSFORMER_CONTEXT_TIMEOUT_SECONDS,
        ),
        config: Read {
            path: crate::project_feeders::read_path(project),
            method: "GET",
            body: serde_json::json!({}),
            response_limit: 8 * 1024 * 1024,
            timeout_seconds: 120,
        },
        document: read(
            crate::printing_standard::PRINTING_STANDARD_PATH,
            a4().body(project),
            crate::printing_standard::PRINTING_STANDARD_RESPONSE_LIMIT,
            60,
        ),
        styles: read(
            crate::STYLES_PATH,
            serde_json::to_vec(
                &ds_command_kernel::style_governance::request_body(&styles(project))
                    .map_err(|_| invalid("Invalid governed style-table read."))?,
            )
            .expect("closed style table request"),
            crate::LAYERS_RESPONSE_LIMIT,
            120,
        ),
    })
}
pub fn snapshot(
    project: &str,
    transformer: &str,
    response: Response,
) -> Result<TransformerContext, ClientError> {
    crate::transformers::validate_context_input(project, transformer)?;
    if response.status != 200 {
        return Err(invalid("Saved snapshot is outside its read contract."));
    }
    crate::transformers::decode_context(
        response.transport(),
        crate::TRANSFORMER_CONTEXT_RESPONSE_LIMIT,
        project,
        transformer,
    )
}
pub fn analysis_plan(
    project: &str,
    transformer: &str,
    snapshot: Response,
) -> Result<Read, ClientError> {
    let snapshot = self::snapshot(project, transformer, snapshot)?;
    let (version, digest, sha) = crate::saved_lv_analysis_pins(
        snapshot.voltage_drop_metadata(),
        snapshot.metadata().version(),
        snapshot.metadata().content_digest(),
    )?
    .ok_or_else(|| invalid("The saved head has no current saved analysis."))?;
    Ok(read(
        crate::TRANSFORMER_ANALYSIS_PATH,
        crate::transformer_analysis::request_body(project, transformer, version, &digest, &sha),
        crate::TRANSFORMER_ANALYSIS_RESPONSE_LIMIT,
        crate::TRANSFORMER_ANALYSIS_TIMEOUT_SECONDS,
    ))
}
pub fn configuration(response: Response) -> Result<crate::FeederConfiguration, ClientError> {
    if response.bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid("Project configuration exceeds its read bound."));
    }
    let document = crate::project_feeders::decode(response.transport())?;
    crate::project_feeders::rows(&document)?;
    Ok(crate::FeederConfiguration {
        document,
        summary: Value::Null,
    })
}
pub fn document(project: &str, response: Response) -> Result<Value, ClientError> {
    crate::printing_standard::decode(response.transport(), project, &a4())
}
pub fn style_table(project: &str, response: Response) -> Result<Value, ClientError> {
    if response.status != 200 || response.bytes.len() > crate::LAYERS_RESPONSE_LIMIT {
        return Err(invalid("Style table is outside its read contract."));
    }
    let value: Value =
        serde_json::from_slice(&response.bytes).map_err(|_| invalid("Unreadable style table."))?;
    ds_command_kernel::style_governance::admit(&styles(project), &value)
        .map_err(|_| invalid("Invalid governed style-table response."))
}
pub fn analysis(response: Response, sha: &str) -> Result<Vec<u8>, ClientError> {
    let headers = crate::TransformerAnalysisHeaders {
        content_type: response.content_type,
        analysis_sha256: response.analysis_sha256,
        cache_control: response.cache_control,
    };
    crate::transformer_analysis::accept(
        crate::TransformerAnalysisResponse {
            response: TransportResponse::new(response.status, response.bytes),
            headers,
        },
        sha,
    )
}
fn invalid(message: &'static str) -> ClientError {
    ClientError::new(crate::ErrorKind::UnreadableResponse, message)
}
