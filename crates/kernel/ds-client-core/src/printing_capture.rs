//! Pure held-transport seam for the same saved-source reads as the native CLI.
//! A host supplies exact response entities. This module never selects an account,
//! changes an API lane, fetches data, calculates an analysis or repairs a seed.
use crate::{ClientError, TransformerContext, TransportResponse};
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

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn saved_read_plan_cannot_acquire_retired_configuration_document_or_styles() {
        let plan = plan("project", "T1").unwrap();
        let wire = serde_json::to_value(&plan).unwrap();
        assert_eq!(
            wire.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["snapshot"]
        );
        assert_eq!(plan.snapshot.body["fields"], "saved");
        assert_eq!(plan.snapshot.path, crate::TRANSFORMER_CONTEXT_PATH);
        assert!(self::plan("", "T1").is_err());
        assert!(self::plan("project", "").is_err());
    }

    #[test]
    fn saved_analysis_preserves_exact_bytes_and_rejects_changed_payload_or_headers() {
        let raw = b"{ \"saved_value\": 1.0 }".to_vec();
        let sha = format!("{:x}", Sha256::digest(&raw));
        let response = || Response {
            status: 200,
            bytes: raw.clone(),
            content_type: Some("application/json".into()),
            analysis_sha256: Some(sha.clone()),
            cache_control: Some("no-store".into()),
        };
        assert_eq!(analysis(response(), &sha).unwrap(), raw);
        for change in 0..4 {
            let mut held = response();
            match change {
                0 => held.bytes.push(b' '),
                1 => held.analysis_sha256 = Some("f".repeat(64)),
                2 => held.cache_control = None,
                _ => held.content_type = Some("text/plain".into()),
            }
            assert!(analysis(held, &sha).is_err(), "change {change}");
        }
    }
}
