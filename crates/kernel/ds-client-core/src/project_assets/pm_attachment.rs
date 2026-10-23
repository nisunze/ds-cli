//! A projected Project Work attachment (`sys:pm_attachment:<id>`) has no
//! catalogue object of its own, so its bytes come through the attachment's
//! own generation-pinned grant (`download_attachment_url`). The grant must
//! name this project and attachment and sign exactly the stored generation;
//! the bytes must have the stated size, and the advertised SHA-256 when there
//! is one.

use super::{bad, MAX_READ_BYTES};
use crate::{
    authorized::Authority, shared_assets::DownloadCall, AuthorizedTransport, ClientError,
    ClientProfile, ErrorKind, ProjectManagementCall,
};
use ds_command_kernel::assets::recognise;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(super) const PREFIX: &str = "sys:pm_attachment:";

pub(super) fn read_bytes<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    id: &str,
) -> Result<(Value, Vec<u8>), ClientError> {
    if !crate::project_management::project_note_id_valid(id) {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "a projected attachment names one exact Project Work attachment",
        ));
    }
    let response = transport.project_management(ProjectManagementCall {
        profile,
        authority: authority.call(),
        payload: json!({"action": "download_attachment_url", "ds_project": project, "id": id}),
    })?;
    if response.status != 200 {
        return Err(crate::project_management::refusal(
            response.status,
            &response.body,
        ));
    }
    if response.body.len() > crate::project_management::MAX_RESPONSE_BYTES {
        return Err(bad());
    }
    let answer: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    let (data, source) = (&answer["data"], &answer["data"]["source"]);
    let size = source["size_bytes"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(bad)?;
    let generation = source["generation"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(bad)?;
    if answer["success"] != true || source["project_id"] != project || source["attachment_id"] != id
    {
        return Err(bad());
    }
    if size > MAX_READ_BYTES {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "the asset is above the 32 MiB read bound; open it from its own surface",
        ));
    }
    let url = data["url"].as_str().ok_or_else(bad)?;
    if !signed_at(url, generation) {
        return Err(bad());
    }
    let mut bytes = transport.shared_asset_bytes(DownloadCall::new(url, size as usize))?;
    if bytes.status != 200 || bytes.body.len() != size as usize {
        return Err(bad());
    }
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes.body));
    if source["advertised_sha256"]
        .as_str()
        .is_some_and(|advertised| format!("sha256:{advertised}") != digest)
    {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the fetched bytes do not match the attachment's advertised digest",
        ));
    }
    // The attachment has no catalogue classification, so its format and kind
    // are what the kernel recognises in the verified bytes, as for any asset.
    let found = recognise(source["original_name"].as_str(), &bytes.body, size);
    let row = json!({"asset_id": format!("{PREFIX}{id}"), "project_id": project,
        "name": source["original_name"], "content_type": source["content_type"],
        "bytes": size, "version": source["version"], "generation": generation,
        "digest": digest, "format": found.format.as_str(), "kind": found.kind.as_str()});
    // The response scrubs its body on drop; the verified bytes are taken out.
    Ok((row, std::mem::take(&mut bytes.body)))
}

/// A Storage URL signed for exactly this object generation.
fn signed_at(url: &str, generation: u64) -> bool {
    let Ok(u) = url::Url::parse(url) else {
        return false;
    };
    let generations: Vec<_> = u
        .query_pairs()
        .filter(|(key, _)| key == "generation")
        .map(|(_, value)| value.into_owned())
        .collect();
    url.len() <= 8192
        && u.scheme() == "https"
        && u.host_str() == Some("storage.googleapis.com")
        && u.port().is_none()
        && u.username().is_empty()
        && u.password().is_none()
        && u.fragment().is_none()
        && generations == [generation.to_string()]
}

#[cfg(test)]
#[path = "pm_attachment_tests.rs"]
mod tests;
