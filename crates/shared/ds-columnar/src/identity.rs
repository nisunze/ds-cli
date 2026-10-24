//! The identity of one conversion (§0.8a.3): the digest of its source bytes
//! and of every parameter that can change the artifact.

use crate::contract::GEOPARQUET_VERSION;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Bytes hashed per read while digesting a source. Bounded on purpose: the
/// point of streaming identity is that a 1.15 GiB source never becomes a 1.15
/// GiB allocation.
pub(crate) const DIGEST_CHUNK_BYTES: usize = 1024 * 1024;

/// sha256 of a file's bytes, read in bounded chunks.
///
/// The same value [`digest`] returns for the same bytes — it is
/// the source identity a conversion receipt records, and a second definition
/// of it would mean two artifacts for one file.
pub fn digest_file(path: &Path) -> Result<String, String> {
    let file =
        File::open(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut reader = BufReader::with_capacity(DIGEST_CHUNK_BYTES, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; DIGEST_CHUNK_BYTES];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
        "geoparquet_version": GEOPARQUET_VERSION,
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
