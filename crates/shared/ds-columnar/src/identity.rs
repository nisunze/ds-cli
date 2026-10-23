//! The identity of one conversion (§0.8a.3): the digest of its source bytes
//! and of every parameter that can change the artifact.

use crate::geoparquet;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Identity of a conversion: the source plus every parameter that can change
/// the output. Two runs agreeing here must produce the same bytes.
///
/// Public because a caller that stores artifacts needs to ADDRESS one before it
/// exists — to answer "have I already converted this exact reading of this
/// exact file?" without reading the source. Deriving that answer from a
/// caller-side key instead would be a second identity for one artifact, which
/// is what §0.8a.3 asks the crate to own.
///
/// `input` is the confirmed table reading as its exact JSON document.
/// `caller_parameters` is for a driver that post-processes features between the
/// parse and this crate — the Desktop import promotes an elevation column and
/// locks a column set, and two such readings of one file are two artifacts. It
/// is folded into the SAME canonical document rather than hashed separately, so
/// there is still one identity and one place that computes it.
pub fn conversion_identity(
    source_digest: &str,
    name: &str,
    input: Option<&Value>,
    layer_name: Option<&str>,
    caller_parameters: Option<&Value>,
) -> Result<String, String> {
    let mut parameters = serde_json::json!({
        "source": source_digest,
        "name": name,
        "input": input,
        "layer": layer_name,
        "caller": caller_parameters,
        "geoparquet_version": geoparquet::GEOPARQUET_VERSION,
    });
    let lower = name.to_ascii_lowercase();
    if [".kml", ".kmz", ".zip"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        parameters["kml_style_contract"] = serde_json::json!("ds.kml-style/v1");
    }
    let canonical = serde_json::to_string(&parameters)
        .map_err(|error| format!("could not canonicalize the conversion parameters: {error}"))?;
    Ok(digest(canonical.as_bytes()))
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
