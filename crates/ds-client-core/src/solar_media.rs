//! Copy an operator-selected map into independent, server-verified Solar media.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    UploadBytesCall,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn invalid() -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, "Solar map needs a canonical city, map role, filename and PNG, JPEG or WebP of at most 16 MiB")
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "Solar media receipt does not match the copied map",
    )
}
fn media_type(bytes: &[u8]) -> Result<&'static str, ClientError> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok("image/jpeg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Ok("image/webp")
    } else {
        Err(invalid())
    }
}
pub(crate) fn validate(
    city: &str,
    role: &str,
    file: &str,
    bytes: &[u8],
) -> Result<(), ClientError> {
    crate::solar_snapshot::validate_input("project", city)?;
    if !crate::solar_snapshot::canonical_media_role(role)
        || !crate::solar_snapshot::canonical_media_file_name(file)
        || bytes.is_empty()
        || bytes.len() > 16 * 1024 * 1024
    {
        return Err(invalid());
    }
    media_type(bytes)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn upload<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    city: &str,
    role: &str,
    file: &str,
    bytes: &[u8],
) -> Result<Value, ClientError> {
    use crate::solar_project::call;
    validate(city, role, file, bytes)?;
    let root = format!("eds_project/{project}/eds_solar");
    let content_type = media_type(bytes)?;
    let started = call(
        profile,
        transport,
        authority,
        false,
        json!({"action":"network_media_upload_start","root":root,"template_id":city,"logical_role":role,"file_name":file,"content_type":content_type,"size_bytes":bytes.len()}),
    )?;
    let id = started["media_id"]
        .as_str()
        .filter(|s| crate::solar_snapshot::canonical_uuid(s))
        .ok_or_else(unreadable)?;
    let uri = started["session_uri"].as_str().ok_or_else(unreadable)?;
    crate::solar_project::validate_upload_uri(uri)?;
    let mut reader = std::io::Cursor::new(bytes);
    let response = transport.upload_bytes(UploadBytesCall {
        uri,
        size: bytes.len() as u64,
        reader: &mut reader,
    })?;
    crate::project_data::uploaded(response)?;
    let finalized = call(
        profile,
        transport,
        authority,
        false,
        json!({"action":"network_media_finalize","root":root,"template_id":city,"logical_role":role,"media_id":id}),
    )?;
    let reference = &finalized["ref"];
    verify(reference, &root, city, role, file, id, content_type, bytes)?;
    // The signed download URL is transport authority, never a persisted input.
    Ok(
        json!({"project_id":project,"city_id":city,"reference":reference,"bytes_copied":bytes.len()}),
    )
}

#[allow(clippy::too_many_arguments)]
fn verify(
    r: &Value,
    root: &str,
    city: &str,
    role: &str,
    file: &str,
    id: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), ClientError> {
    if r["schema"] != 1
        || r["kind"] != "solar_network_media"
        || r["media_id"] != id
        || r["logical_role"] != role
        || r["file_name"] != file
        || r["media_type"] != content_type
        || r["size_bytes"].as_u64() != Some(bytes.len() as u64)
        || r["sha256"] != format!("{:x}", Sha256::digest(bytes))
        || r["object"] != format!("{root}/{city}/network_plots/_immutable/{id}")
        || !r["generation"].as_u64().is_some_and(|n| n > 0)
        || !r["bucket"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 512)
        || r.as_object().map(|o| o.len()) != Some(11)
    {
        return Err(unreadable());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_map_receipt_pins_bytes_city_role_and_generation() {
        let b = b"\x89PNG\r\n\x1a\nmap";
        let id = "12345678-1234-1234-1234-123456789abc";
        let r = json!({"schema":1,"kind":"solar_network_media","media_id":id,"logical_role":"network.map:reseau_propose","file_name":"map.png","media_type":"image/png","size_bytes":b.len(),"sha256":format!("{:x}",Sha256::digest(b)),"object":format!("root/town/network_plots/_immutable/{id}"),"generation":1,"bucket":"bucket"});
        assert!(verify(
            &r,
            "root",
            "town",
            "network.map:reseau_propose",
            "map.png",
            id,
            "image/png",
            b
        )
        .is_ok());
        assert!(verify(
            &r,
            "root",
            "other",
            "network.map:reseau_propose",
            "map.png",
            id,
            "image/png",
            b
        )
        .is_err());
        assert!(verify(
            &r,
            "root",
            "town",
            "network.map:reseau_propose",
            "map.png",
            id,
            "image/png",
            b"changed"
        )
        .is_err());
        let mut unpinned = r;
        unpinned["generation"] = json!(0);
        assert!(verify(
            &unpinned,
            "root",
            "town",
            "network.map:reseau_propose",
            "map.png",
            id,
            "image/png",
            b
        )
        .is_err());
    }
}
