//! Immutable exports of one project model revision: derived files — a
//! delivered PLS-CADD `.bak`, a GIS layer — recorded against the revision's
//! exact model digest. Publishing one never moves the head, and an export id
//! once written cannot change. Bytes travel through the same authenticated
//! storage grant and native transfer as model publication; a read is
//! verified against the digest the export record declares.
use super::publication::{session_upload, stored_artifact};
use super::*;
use ds_command_kernel::grid_publication::Artifact;

/// Most export records one revision lists (the catalog's own bound).
pub const MAX_EXPORTS: usize = 500;

fn artifact(value: &Value) -> Option<Artifact> {
    serde_json::from_value::<Artifact>(value.clone())
        .ok()
        .filter(|a| {
            sha256_hex(&a.digest)
                && a.byte_length > 0
                && a.byte_length <= MAX_BYTES as u64
                && a.object == canonical_object(&a.digest)
        })
}

/// The content-addressed object the storage grant names for a digest.
pub(crate) fn canonical_object(digest: &str) -> String {
    format!("ds-grid/objects/sha256/{}/{}", &digest[..2], digest)
}

/// The export records of one revision, each checked to belong to it.
pub(super) fn list<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    model: &str,
    revision: &str,
) -> Result<Vec<Value>, ClientError> {
    let data = request(
        profile,
        transport,
        authority,
        json!({"action":"list_exports","project_id":project,"model_id":model,"revision_id":revision}),
    )?;
    let exports = if data["exports"].is_null() {
        vec![]
    } else {
        data["exports"].as_array().ok_or_else(bad)?.clone()
    };
    if exports.len() > MAX_EXPORTS
        || exports.iter().any(|export| {
            export["project_id"] != project
                || export["model_id"] != model
                || export["revision_id"] != revision
                || !export["export_id"].as_str().is_some_and(catalog_identifier)
                || !export["model_digest"].as_str().is_some_and(sha256_hex)
                || !export["items"].as_array().is_some_and(|items| {
                    items.iter().all(|item| {
                        item["output_id"].as_str().is_some_and(catalog_identifier)
                            && artifact(&item["artifact"]).is_some()
                    })
                })
        })
    {
        return Err(bad());
    }
    Ok(exports)
}

pub(super) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Receipt, ClientError> {
    match command {
        Command::PublishExport {
            model,
            revision,
            export,
            output,
            format,
            bytes,
        } => {
            // The export is pinned to the digest the catalog holds for this
            // revision, read now, never one the caller asserts.
            let record = revision_record(profile, transport, authority, project, model, revision)?;
            let model_digest = record["model"]["digest"].as_str().ok_or_else(bad)?;
            let digest = format!("{:x}", Sha256::digest(bytes));
            let artifact = Artifact {
                object: canonical_object(&digest),
                digest,
                byte_length: bytes.len() as u64,
            };
            let upload = json!({"action":"start_upload","project_id":project,
                "upload":{"model_id":model,"digest":artifact.digest,"byte_length":artifact.byte_length,"purpose":"export"}});
            let grant = request(profile, transport, authority, upload.clone())?;
            let skipped = match session_upload(&grant, &artifact, bytes)? {
                None => true,
                Some(call) => {
                    transport.grid_model_upload(call)?;
                    let stored = request(profile, transport, authority, upload)?;
                    stored_artifact(&stored, &artifact)?;
                    false
                }
            };
            let item =
                json!({"output_id":output,"scope":"model","format":format,"artifact":artifact});
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"publish_export","project_id":project,"export":{"model_id":model,
                    "revision_id":revision,"model_digest":model_digest,"export_id":export,"items":[item]}}),
            )?;
            let published = data.get("export").ok_or_else(bad)?;
            // An existing export id answers with the stored record; only an
            // identical one is this publication. Anything else is refused.
            if published["project_id"] != project
                || published["model_id"] != *model
                || published["revision_id"] != *revision
                || published["model_digest"] != model_digest
                || published["export_id"] != *export
                || published["items"].as_array().map(Vec::len) != Some(1)
                || published["items"][0]["output_id"] != *output
                || published["items"][0]["format"] != *format
                || artifact_of(&published["items"][0]) != Some(artifact.clone())
            {
                return Err(bad());
            }
            Ok(Receipt {
                data: json!({"status":"published","project":project,"model_id":model,"revision_id":revision,
                    "version":record["version"],"model_digest":model_digest,"export_id":export,"output_id":output,
                    "format":format,"digest":artifact.digest,"byte_length":artifact.byte_length,
                    "upload_skipped":skipped,"created_at":published["created_at"],
                    "created_by":published["created_by"],"head_moved":false,"verified":true}),
                bytes: None,
            })
        }
        Command::DownloadExport {
            model,
            revision,
            export,
            output,
        } => {
            let exports = list(profile, transport, authority, project, model, revision)?;
            let not_found = || {
                ClientError::with_service_refusal(
                    ErrorKind::ResourceNotFound,
                    "the revision holds no such export output",
                    ServiceRefusal::new(404, Some("grid_model_not_found"), None),
                )
            };
            let record = exports
                .iter()
                .find(|e| e["export_id"] == *export)
                .ok_or_else(not_found)?;
            let item = record["items"]
                .as_array()
                .and_then(|items| items.iter().find(|i| i["output_id"] == *output))
                .ok_or_else(not_found)?;
            let declared = artifact(&item["artifact"]).ok_or_else(bad)?;
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"get_export_artifact","project_id":project,"model_id":model,
                    "revision_id":revision,"export_id":export,"output_id":output}),
            )?;
            // A Server that names the artifact must name the listed one.
            for named in [
                data.get("artifact"),
                data.get("item").map(|i| &i["artifact"]),
            ]
            .into_iter()
            .flatten()
            {
                if artifact(named).as_ref() != Some(&declared) {
                    return Err(bad());
                }
            }
            let url = data["download_url"].as_str().ok_or_else(bad)?;
            let size = declared.byte_length as usize;
            let mut response = transport.grid_model_bytes(locator(url, &declared.object, size)?)?;
            if response.status != 200
                || response.body.len() != size
                || format!("{:x}", Sha256::digest(&response.body)) != declared.digest
            {
                return Err(bad());
            }
            Ok(Receipt {
                data: json!({"project":project,"model_id":model,"revision_id":revision,
                    "model_digest":record["model_digest"],"export_id":export,"output_id":output,
                    "format":item["format"],"sha256":declared.digest,"bytes":size,"verified":true}),
                bytes: Some(std::mem::take(&mut response.body)),
            })
        }
        _ => unreachable!("export commands only"),
    }
}

fn artifact_of(item: &Value) -> Option<Artifact> {
    artifact(&item["artifact"])
}
