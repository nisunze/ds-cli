//! Exact, generation-pinned Solar portfolio bytes from a governed artifact head.
use crate::{
    authorized::Authority, solar_project::call, AuthorizedTransport, ClientError, ClientProfile,
    ErrorKind,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// A host can execute this handoff, but cannot construct caller-selected URLs.
pub struct SolarArtifactDownloadCall<'a> {
    url: &'a str,
    response_limit: usize,
}
impl SolarArtifactDownloadCall<'_> {
    pub fn url(&self) -> &str {
        self.url
    }
    pub fn response_limit(&self) -> usize {
        self.response_limit
    }
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "The published Solar portfolio does not match its exact artifact head",
    )
}

pub(crate) fn portfolio<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    portfolio: &str,
    membership: &Value,
) -> Result<Option<(Vec<u8>, Value)>, ClientError> {
    let identity = json!({"project_id":project,"engine":"solar","operation":format!("portfolio-{portfolio}"),"variant":"aggregate-v3"});
    let mut request = identity.clone();
    request["action"] = json!("download");
    request["output_id"] = json!("portfolio-result");
    let mut download = match call(profile, transport, authority, true, request) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::ResourceNotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // Remove the private capability before any receipt can escape this owner.
    let mut uri = download
        .as_object_mut()
        .and_then(|m| m.remove("download_url"))
        .and_then(|v| {
            if let Value::String(uri) = v {
                Some(uri)
            } else {
                None
            }
        })
        .ok_or_else(unreadable)?;
    let result = (|| {
        let head = &download["head"];
        let output = &download["output"];
        let expected = membership
            .as_str()
            .and_then(|s| s.strip_prefix("sha256:"))
            .ok_or_else(unreadable)?;
        if head["engine"] != identity["engine"]
            || head["operation"] != identity["operation"]
            || head["variant"] != identity["variant"]
            || head["input_base_fingerprint"] != expected
            || !head["head_revision"].as_u64().is_some_and(|n| n > 0)
            || output["output_id"] != "portfolio-result"
            || output["format"] != "json"
            || output["content_type"] != "application/json"
            || !head["outputs"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|r| r == output))
        {
            return Err(unreadable());
        }
        let size = output["size_bytes"]
            .as_u64()
            .filter(|n| (1..=8 * 1024 * 1024).contains(n))
            .ok_or_else(unreadable)? as usize;
        let digest = output["sha256"]
            .as_str()
            .filter(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or_else(unreadable)?;
        let url = url::Url::parse(&uri).map_err(|_| unreadable())?;
        let host = url.host_str().ok_or_else(unreadable)?;
        let generations = url
            .query_pairs()
            .filter(|(key, _)| key == "generation")
            .collect::<Vec<_>>();
        if uri.len() > 16 * 1024
            || url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.port_or_known_default() != Some(443)
            || generations.len() != 1
            || !generations[0].1.parse::<u64>().ok().is_some_and(|n| n > 0)
            || !(matches!(
                host,
                "storage.googleapis.com" | "firebasestorage.googleapis.com"
            ) || host.ends_with(".storage.googleapis.com"))
        {
            return Err(unreadable());
        }
        let expires = time::OffsetDateTime::parse(
            download["expires_at"].as_str().ok_or_else(unreadable)?,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| unreadable())?;
        if expires <= time::OffsetDateTime::now_utc() {
            return Err(unreadable());
        }
        let mut response = transport.download_solar_artifact(SolarArtifactDownloadCall {
            url: &uri,
            response_limit: size,
        })?;
        if response.status != 200
            || response.body.len() != size
            || format!("{:x}", Sha256::digest(&response.body)) != digest
        {
            return Err(unreadable());
        }
        // Re-read once after transfer. A new head cannot lend its identity to old bytes.
        let mut request = identity;
        request["action"] = json!("read");
        if call(profile, transport, authority, true, request)? != *head {
            return Err(unreadable());
        }
        Ok(Some((
            std::mem::take(&mut response.body),
            json!({"work_id":head["work_id"],"head_revision":head["head_revision"],"output_sha256":digest,"byte_count":size,"engine_version":head["engine_version"],"engine_build_manifest_sha256":head["engine_build_manifest_sha256"]}),
        )))
    })();
    uri.zeroize();
    result
}
