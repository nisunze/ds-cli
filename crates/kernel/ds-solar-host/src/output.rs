//! Complete, bounded byte delivery. Publication remains the caller's job.
use base64::Engine;
use ds_solar_contracts::{ArtifactEntry, SolarError, SolarResult};
use serde::{Deserialize, Serialize};

pub const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_OUTPUT_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_OUTPUT_FILES: usize = 512;

/// One generated document, bytes plus the description the batch will carry.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedDocument {
    /// The manifest description: logical name, media type, size and digest.
    pub entry: ArtifactEntry,
    /// The bytes themselves. The adapter writes these; they never cross an IPC
    /// response.
    pub bytes: Vec<u8>,
}

impl GeneratedDocument {
    /// Describe a document from its logical name and bytes.
    pub fn new(name: impl Into<String>, media_type: &str, bytes: Vec<u8>) -> Self {
        Self {
            entry: ArtifactEntry::from_bytes(name, media_type, &bytes),
            bytes,
        }
    }

    /// The logical name, e.g. `apd-draft-fr.md`.
    pub fn name(&self) -> &str {
        &self.entry.name
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFile {
    pub entry: ArtifactEntry,
    pub body_base64: String,
}

/// Encode already produced bytes; this layer never computes or renders them.
pub fn encode_outputs<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str, &'a [u8])>,
) -> SolarResult<Vec<OutputFile>> {
    let mut output = Vec::new();
    let mut names = std::collections::HashSet::new();
    let mut total = 0;
    for (name, media_type, bytes) in files {
        if output.len() >= MAX_OUTPUT_FILES || bytes.len() > MAX_OUTPUT_FILE_BYTES {
            return Err(SolarError::artifact_failed(
                "Solar output exceeds its file/count bound",
            ));
        }
        if name.is_empty()
            || name.contains('\\')
            || name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !names.insert(name)
        {
            return Err(SolarError::artifact_failed(
                "Solar output has an invalid or duplicate logical name",
            ));
        }
        total += bytes.len();
        if total > MAX_OUTPUT_BYTES {
            return Err(SolarError::artifact_failed("Solar output exceeds 128 MiB"));
        }
        output.push(OutputFile {
            entry: ArtifactEntry::from_bytes(name, media_type, bytes),
            body_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }
    Ok(output)
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
