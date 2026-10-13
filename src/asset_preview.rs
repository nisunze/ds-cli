//! Exact attachment-preview source facts and retention admission. No IO,
//! URL, credential, selected project, clock or host protocol lives here.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
}
fn fail(code: &'static str, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum Retention {
    #[default]
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
}
impl Retention {
    pub fn millis(self) -> u64 {
        match self {
            Self::Hours24 => 24 * 60 * 60 * 1000,
            Self::Days7 => 7 * 24 * 60 * 60 * 1000,
        }
    }
}
pub const REQUEST_SCHEMA: &str = "ds.asset-preview.acquire/v1";
pub const RECEIPT_SCHEMA: &str = "ds.asset-preview.receipt/v1";
pub const MAX_BYTES: u64 = crate::assets::MAX_PDF_BYTES;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub asset_id: String,
    #[serde(default)]
    pub expected_revision: Option<String>,
    #[serde(default)]
    pub expected_generation: Option<String>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub retention: Retention,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceProof {
    pub project_id: String,
    pub asset_id: String,
    pub source_kind: String,
    pub source_id: String,
    pub revision: String,
    pub generation: String,
    #[serde(default)]
    pub advertised_sha256: Option<String>,
    pub size_bytes: u64,
    pub content_type: String,
    pub sensitivity: String,
    pub name: String,
}
fn exact(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(|c| c.is_control())
}
pub fn normalized_sha(value: &str) -> Option<&str> {
    let raw = value.strip_prefix("sha256:").unwrap_or(value);
    (raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(raw)
}
impl Request {
    pub fn validate(&self) -> Result<(), Failure> {
        if self.schema != REQUEST_SCHEMA
            || !exact(&self.asset_id)
            || !(self.asset_id.starts_with("a_") || self.asset_id.starts_with("sys:pm_attachment:"))
            || self
                .expected_sha256
                .as_deref()
                .is_some_and(|s| normalized_sha(s).is_none())
            || self.expected_revision.as_deref().is_some_and(|s| !exact(s))
            || self
                .expected_generation
                .as_deref()
                .is_some_and(|s| !exact(s))
            || self.offline
                && (self.expected_revision.is_none()
                    || self.expected_generation.is_none()
                    || self.expected_sha256.is_none())
        {
            return Err(fail("asset_preview_pins_invalid","Preview needs an exact selected asset; offline requires its retained revision, generation and SHA."));
        }
        Ok(())
    }
}
impl SourceProof {
    pub fn validate(&self, project_id: &str, request: &Request) -> Result<(), Failure> {
        if self.project_id != project_id
            || self.asset_id != request.asset_id
            || !matches!(
                self.source_kind.as_str(),
                "catalogue_asset" | "pm_attachment"
            )
            || !exact(&self.source_id)
            || !exact(&self.revision)
            || !exact(&self.generation)
            || self
                .advertised_sha256
                .as_deref()
                .is_some_and(|s| normalized_sha(s) != Some(s))
            || self.size_bytes == 0
            || self.size_bytes > MAX_BYTES
            || self.content_type != "application/pdf"
            || self.name.trim().is_empty()
            || self.name.len() > 1024
            || self.name.chars().any(char::is_control)
        {
            return Err(fail(
                "asset_preview_source_invalid",
                "The native source proof does not admit one exact bounded PDF in this project.",
            ));
        }
        if request
            .expected_revision
            .as_deref()
            .is_some_and(|v| v != self.revision)
            || request
                .expected_generation
                .as_deref()
                .is_some_and(|v| v != self.generation)
            || request
                .expected_sha256
                .as_deref()
                .and_then(normalized_sha)
                .zip(self.advertised_sha256.as_deref())
                .is_some_and(|(expected, actual)| expected != actual)
        {
            return Err(fail(
                "asset_preview_source_changed",
                "The selected PDF pins differ from source authority; re-read the source row.",
            ));
        }
        // Brain's fresh grant decides which member may read this class.
        // A native DTO cannot remove an otherwise authorized PDF preview.
        if !crate::assets::Sensitivity::ALL
            .iter()
            .any(|class| class.as_str() == self.sensitivity)
        {
            return Err(fail(
                "asset_preview_source_invalid",
                "The source grant has an undeclared classification.",
            ));
        }
        Ok(())
    }
    pub fn validate_measured(&self, request: &Request, measured: &str) -> Result<(), Failure> {
        if normalized_sha(measured) != Some(measured)
            || self
                .advertised_sha256
                .as_deref()
                .is_some_and(|v| v != measured)
            || request
                .expected_sha256
                .as_deref()
                .and_then(normalized_sha)
                .is_some_and(|v| v != measured)
        {
            return Err(fail("asset_preview_source_changed","Acquired bytes do not match the selected or advertised SHA; no preview is admitted."));
        }
        Ok(())
    }
}
