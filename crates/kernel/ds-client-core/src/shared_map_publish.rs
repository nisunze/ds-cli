//! Publish an explicitly declared map through the existing Assets uploader.
//! The catalogue owns bytes, access, classification and links; no map store.
use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, SharedAssetsCall, UploadBytesCall,
};
use ds_command_kernel::assets::{format::recognise, Format, Link};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "Map publication requires PDF, PNG, JPEG or WebP bytes and at most 16 exact tag links",
    )
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "Map asset receipt does not match the selected project, name, bytes or links",
    )
}
pub(crate) fn format(
    name: &str,
    bytes: &[u8],
) -> Result<(&'static str, &'static str, &'static str), ClientError> {
    let detected = recognise(
        Some(name),
        &bytes[..bytes.len().min(65536)],
        bytes.len() as u64,
    );
    match detected.format {
        Format::Pdf => Ok(("pdf", "application/pdf", "doc")),
        Format::Png => Ok(("png", "image/png", "image")),
        Format::Jpg => Ok(("jpg", "image/jpeg", "image")),
        Format::Webp => Ok(("webp", "image/webp", "image")),
        _ => Err(invalid()),
    }
}
fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    body: Value,
) -> Result<Value, ClientError> {
    let response = transport.shared_assets(SharedAssetsCall {
        profile,
        authority: authority.call(),
        payload: body,
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(bad());
    }
    let reply: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if reply["success"] != true || !reply["data"].is_object() {
        return Err(bad());
    }
    Ok(reply["data"].clone())
}
fn verify(
    asset: &Value,
    project: &str,
    name: &str,
    digest: &str,
    size: usize,
    format: &str,
) -> Result<(), ClientError> {
    let id = asset["asset_id"].as_str().ok_or_else(bad)?;
    if asset["schema"] != "ds.assets.asset/v1"
        || asset["project_id"] != project
        || asset["name"] != name
        || asset["digest"] != digest
        || asset["bytes"].as_u64() != Some(size as u64)
        || asset["format"] != format
        || !id.starts_with("a_")
        || id.len() != 14
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(bad());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    name: &str,
    bytes: &[u8],
    tags: &[Link],
    replaces: &[String],
) -> Result<Value, ClientError> {
    let (format, mime, kind) = format(name, bytes)?;
    let sha = format!("{:x}", Sha256::digest(bytes));
    let digest = format!("sha256:{sha}");
    // Read every explicitly superseded row before uploading or changing any
    // shared state. Tags classify maps; they do not identify which map to retire.
    let mut predecessors = Vec::new();
    let requested_tags = ds_command_kernel::assets::tag_maps::tags(tags);
    for id in replaces {
        let row = call(
            profile,
            transport,
            authority,
            json!({"action":"get","ds_project":project,"asset_id":id}),
        )?;
        let links: Vec<Link> = serde_json::from_value(row["links"].clone()).map_err(|_| bad())?;
        if row["project_id"] != project
            || row["asset_id"] != *id
            || row["kind"] != "geo"
            || row["format"] != format
            || !row["reference"].is_null()
            || !matches!(row["status"].as_str(), Some("durable" | "archive"))
            || ds_command_kernel::assets::tag_maps::tags(&links) != requested_tags
            || row["version"].as_u64().is_none_or(|v| v == 0)
        {
            return Err(ClientError::new(ErrorKind::InvalidInput,
                "A replacement must be a project-owned map of the same format and exact tags; use its catalogue asset ID"));
        }
        predecessors.push(row);
    }
    // Replaying the same declaration reuses its existing verified bytes. A
    // bounded incomplete search refuses instead of silently minting a copy.
    let mut cursor = String::new();
    let mut existing = None;
    let mut complete = false;
    for _ in 0..10 {
        let mut request = json!({"action":"list","ds_project":project,"limit":200});
        if !cursor.is_empty() {
            request["cursor"] = json!(cursor);
        }
        let page = call(profile, transport, authority, request)?;
        let rows = page["assets"]
            .as_array()
            .filter(|r| r.len() <= 200)
            .ok_or_else(bad)?;
        for row in rows {
            if row["project_id"] != project {
                return Err(bad());
            }
            if row["name"] == name && row["digest"] == digest {
                verify(row, project, name, &digest, bytes.len(), format)?;
                if existing.is_none() {
                    existing = Some(row.clone());
                }
            }
        }
        let more = page["has_more"].as_bool().ok_or_else(bad)?;
        let truncated = page["truncated"].as_bool().ok_or_else(bad)?;
        if !more && !truncated {
            complete = true;
            break;
        }
        let next = page["next_cursor"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 512 && *s != cursor)
            .ok_or_else(bad)?;
        cursor = next.into();
    }
    if !complete {
        return Err(ClientError::new(ErrorKind::InvalidInput, "Map catalogue search exceeds 2000 rows; publish into a bounded catalogue before retrying"));
    }
    let reused = existing.is_some();
    let mut asset = if let Some(asset) = existing {
        asset
    } else {
        let session = call(
            profile,
            transport,
            authority,
            json!({"action":"ingest_start","ds_project":project,
            "ingest":{"name":name,"content_type":mime,"size":bytes.len(),"sha256":sha}}),
        )?;
        let id = session["asset_id"]
            .as_str()
            .filter(|s| s.starts_with("a_") && s.len() == 14)
            .ok_or_else(bad)?;
        let uri = session["upload_url"].as_str().ok_or_else(bad)?;
        crate::solar_project::validate_upload_uri(uri)?;
        let mut reader = std::io::Cursor::new(bytes);
        let uploaded = transport.upload_bytes(UploadBytesCall {
            uri,
            size: bytes.len() as u64,
            reader: &mut reader,
        })?;
        crate::project_data::uploaded(uploaded)?;
        let asset = call(
            profile,
            transport,
            authority,
            json!({"action":"ingest_finalize","ds_project":project,
            "asset_id":id,"finalize":{"asset_id":id,"sha256":sha,"kind":kind,"format":format}}),
        )?;
        verify(&asset, project, name, &digest, bytes.len(), format)?;
        asset
    };
    if asset["kind"] != "geo" || asset["status"] != "durable" {
        let version = asset["version"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or_else(bad)?;
        let classified = call(
            profile,
            transport,
            authority,
            json!({"action":"classify","ds_project":project,
            "asset_id":asset["asset_id"],"classify":{"kind":"geo","status":"durable","expected_version":version,"reason":"Operator-published project map"}}),
        )?;
        asset = classified["asset"].clone();
        verify(&asset, project, name, &digest, bytes.len(), format)?;
    }
    for tag in tags {
        if asset["links"]
            .as_array()
            .is_some_and(|links| links.contains(&json!(tag)))
        {
            continue;
        }
        asset = call(
            profile,
            transport,
            authority,
            json!({"action":"attach","ds_project":project,"asset_id":asset["asset_id"],"link":tag}),
        )?;
        verify(&asset, project, name, &digest, bytes.len(), format)?;
    }
    if asset["kind"] != "geo"
        || asset["status"] != "durable"
        || !tags.iter().all(|tag| {
            asset["links"]
                .as_array()
                .is_some_and(|links| links.contains(&json!(tag)))
        })
    {
        return Err(bad());
    }
    // Only retire predecessors after the replacement is verified durable and
    // tagged. Expected versions preserve concurrent operator edits. Archive is
    // metadata-only and recoverable; a partial refusal leaves both maps visible.
    let mut superseded = Vec::new();
    for previous in predecessors {
        if previous["asset_id"] == asset["asset_id"] {
            continue;
        }
        if previous["status"] != "archive" {
            let reply = call(
                profile,
                transport,
                authority,
                json!({"action":"classify","ds_project":project,"asset_id":previous["asset_id"],
                "classify":{"status":"archive","expected_version":previous["version"],
                "reason":format!("Superseded by verified map {}", asset["asset_id"].as_str().ok_or_else(bad)?)}}),
            )?;
            let retired = &reply["asset"];
            if retired["project_id"] != project
                || retired["asset_id"] != previous["asset_id"]
                || retired["status"] != "archive"
                || retired["digest"] != previous["digest"]
            {
                return Err(bad());
            }
        }
        superseded.push(previous["asset_id"].clone());
    }
    let index = ds_command_kernel::assets::tag_maps::index(
        project,
        &json!({"assets":[asset],"has_more":false,"truncated":false}),
    )
    .map_err(|_| bad())?;
    Ok(
        json!({"project_id":project,"asset":asset,"map":index["maps"][0],
        "published":true,"reused":reused,"superseded_asset_ids":superseded,
        "bytes_uploaded":if reused { 0 } else { bytes.len() }}),
    )
}
