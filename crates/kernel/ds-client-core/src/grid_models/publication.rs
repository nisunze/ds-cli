//! Headless publication: plan, authenticated storage grant, native transfer,
//! immutable commit, exact revision readback. A partial transfer never commits.
use super::*;
use ds_command_kernel::grid_publication::{Artifact, Intent};

/// Constructed only from the authenticated, validated storage grant.
pub struct UploadCall<'a> {
    uri: String,
    bytes: &'a [u8],
    digest: &'a str,
}
impl UploadCall<'_> {
    pub fn uri(&self) -> &str {
        &self.uri
    }
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
    pub fn digest(&self) -> &str {
        self.digest
    }
}

/// Validate one storage grant against the exact artifact this owner planned.
/// `None` means the object is already stored with that size and digest.
pub(super) fn session_upload<'a>(
    data: &Value,
    artifact: &'a Artifact,
    bytes: &'a [u8],
) -> Result<Option<UploadCall<'a>>, ClientError> {
    if serde_json::from_value::<Artifact>(data["artifact"].clone())
        .ok()
        .as_ref()
        != Some(artifact)
    {
        return Err(bad());
    }
    match data["already_exists"].as_bool() {
        Some(true) => return Ok(None),
        Some(false) => {}
        None => return Err(bad()),
    }
    let uri = data["session_uri"]
        .as_str()
        .filter(|s| s.len() <= 8192)
        .ok_or_else(bad)?;
    let url = url::Url::parse(uri).map_err(|_| bad())?;
    if url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/upload/storage/v1/b/")
        || !url.path().ends_with("/o")
        || !url
            .query_pairs()
            .any(|(k, v)| k == "upload_id" && !v.is_empty())
    {
        return Err(bad());
    }
    Ok(Some(UploadCall {
        uri: uri.into(),
        bytes,
        digest: &artifact.digest,
    }))
}

/// After a transfer the storage authority is asked again: it checks the
/// actual size and digest, and must now report the object as stored.
pub(super) fn stored_artifact(data: &Value, artifact: &Artifact) -> Result<(), ClientError> {
    match session_upload(data, artifact, &[])? {
        None => Ok(()),
        Some(_) => Err(bad()),
    }
}

pub(super) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    intent: &Intent,
    bytes: &[u8],
) -> Result<Receipt, ClientError> {
    let plan = intent
        .plan(project, bytes)
        .map_err(|message| ClientError::new(ErrorKind::InvalidInput, message))?;
    let grant = request(profile, transport, authority, plan.upload_request())?;
    let skipped = match session_upload(&grant, &plan.artifact, bytes)? {
        None => true,
        Some(call) => {
            // The native transfer owner supplies success only after its sealed
            // bytes reach storage. A 412 alone is not proof of identical bytes.
            transport.grid_model_upload(call)?;
            // Ask the storage authority again: it checks actual size + digest.
            let stored = request(profile, transport, authority, plan.upload_request())?;
            stored_artifact(&stored, &plan.artifact)?;
            false
        }
    };
    let committed = request(profile, transport, authority, plan.version_request())?;
    let version = committed
        .get("version")
        .or_else(|| committed.get("revision"))
        .unwrap_or(&committed);
    if !plan.matches_version(version) {
        return Err(bad());
    }
    let readback = request(
        profile,
        transport,
        authority,
        json!({"action":"get_version","project_id":project,"model_id":intent.model_id,"revision_id":plan.revision_id}),
    )?;
    let verified = readback
        .get("revision")
        .or_else(|| readback.get("version"))
        .ok_or_else(bad)?;
    // A Server that stores ordinals says where the revision landed; an
    // explicit bump must have started the version (ordinal 1).
    let ordinal = ds_command_kernel::grid_versions::stored_ordinal(verified);
    if !plan.matches_version(verified)
        || verified["version"] != version["version"]
        || (intent.bump_version && ordinal.is_some_and(|n| n != 1))
    {
        return Err(bad());
    }
    // Project bytes are committed even if a presentation host later fails to
    // record its local working-copy binding. This receipt is independent of it.
    Ok(Receipt {
        data: json!({"status":"published","project":project,"project_model":intent.model_id,
        "revision":plan.revision_id,"version":verified["version"],"kind":intent.model_kind,
        "expected_head":intent.expected_head_revision_id,"parent_revision":verified["parent_revision_id"],
        "digest":plan.artifact.digest,"byte_length":plan.artifact.byte_length,"upload_skipped":skipped,
        "verified":true,"local_revision":intent.source_revision_id,"bump_version":intent.bump_version,
        "milestone":verified["milestone"],"approval":verified["approval"],
        "revision_ordinal_within_version":ordinal}),
        bytes: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_cannot_replace_the_object_or_redirect_a_native_upload() {
        let intent = Intent {
            project: "p".into(),
            model_id: "m".into(),
            model_kind: "mv_line".into(),
            expected_head_revision_id: String::new(),
            source_revision_id: "rev:one".into(),
            manifest_model_id: "native-one".into(),
            manifest_model_revision: 1,
            model_schema_version: "19".into(),
            engine_version: "1".into(),
            display_name: None,
            reason: None,
            ..Default::default()
        };
        let plan = intent.plan("p", b"bytes").unwrap();
        let mut grant = json!({"artifact":plan.artifact,"already_exists":false,"session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?upload_id=issued"});
        assert!(session_upload(&grant, &plan.artifact, b"bytes")
            .unwrap()
            .is_some());
        for uri in [
            "https://evil.example/upload/storage/v1/b/b/o?upload_id=x",
            "http://storage.googleapis.com/upload/storage/v1/b/b/o?upload_id=x",
            "https://user@storage.googleapis.com/upload/storage/v1/b/b/o?upload_id=x",
        ] {
            grant["session_uri"] = json!(uri);
            assert!(session_upload(&grant, &plan.artifact, b"bytes").is_err());
        }
        grant["already_exists"] = json!(true);
        assert!(session_upload(&grant, &plan.artifact, b"bytes")
            .unwrap()
            .is_none());
        grant["artifact"]["byte_length"] = json!(1);
        assert!(session_upload(&grant, &plan.artifact, b"bytes").is_err());
    }
}
