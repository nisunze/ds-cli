//! The governed global DS Grid library and example catalog, headlessly.
//!
//! This catalog is **global**: a library release or an example revision
//! belongs to the product, not to a project, so nothing here carries a project
//! id or a project fence. Authority is the signed-in user and the gateway's
//! own publish capability; reads are open to any restored user and every write
//! is refused server-side without `grid.catalog.publish`.
//!
//! Until now `ds library global …` reached this endpoint through the paired
//! desktop, which forwarded exactly these bodies to exactly this path. That
//! made a global, project-free catalog operation require an open window, and
//! made the commands declare `Available` while refusing `desktop_not_paired`
//! on any machine without one. The transport moves here; the wire does not
//! change.
//!
//! Closed by construction: the [`Command`] enum is the complete set of actions
//! this owner can send, each variant naming the body keys the backend's
//! `DisallowUnknownFields` decoder accepts for it. A caller cannot reach an
//! action that is not listed, cannot add a key beside those its action
//! declares, and cannot choose the path, the method or the origin.

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    GridCatalogCall,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// An exact legacy catalog artifact. Coordinates are required; a mutable head
/// and a caller-supplied URL cannot select bytes for backup.
pub enum ArtifactSelection {
    LibraryManifest { library: String, release: String },
    LibraryValidation { library: String, release: String },
    ExampleModel { example: String, revision: String },
}

impl ArtifactSelection {
    pub fn read_command(&self) -> Command {
        match self {
            Self::LibraryManifest { library, release }
            | Self::LibraryValidation { library, release } => Command::GetLibrary {
                library: library.clone(),
                release: Some(release.clone()),
            },
            Self::ExampleModel { example, revision } => Command::GetExample {
                example: example.clone(),
                revision: Some(revision.clone()),
            },
        }
    }

    /// Consume only the signed delivery from the exact authorized read. The
    /// transfer shares project models' fixed Storage host, no-redirect transport
    /// and exact object/size/hash checks; no DS credential reaches Storage.
    pub fn download<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        data: &Value,
        expected_digest: &str,
    ) -> Result<crate::grid_models::Receipt, ClientError> {
        self.read_command().validate()?;
        if !sha256(expected_digest) {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "expected digest must be 64 lowercase hexadecimal characters",
            ));
        }
        let (artifact, url, mut receipt) = match self {
            Self::LibraryManifest { library, release }
            | Self::LibraryValidation { library, release } => {
                if data["head"]["library_id"] != *library
                    || data["release"]["library_id"] != *library
                    || data["release"]["release_id"] != *release
                {
                    return Err(unreadable());
                }
                let (kind, artifact, url) = match self {
                    Self::LibraryManifest { .. } => {
                        ("library-manifest", "manifest", "manifest_download_url")
                    }
                    _ => (
                        "library-validation",
                        "validation_report",
                        "validation_report_download_url",
                    ),
                };
                (
                    &data["release"][artifact],
                    &data[url],
                    json!({"kind":kind,"library_id":library,"release_id":release}),
                )
            }
            Self::ExampleModel { example, revision } => {
                if data["head"]["example_id"] != *example
                    || data["revision"]["example_id"] != *example
                    || data["revision"]["revision_id"] != *revision
                {
                    return Err(unreadable());
                }
                (
                    &data["revision"]["model"],
                    &data["model_download_url"],
                    json!({"kind":"example-model","example_id":example,"revision_id":revision}),
                )
            }
        };
        let digest = artifact["digest"].as_str().ok_or_else(unreadable)?;
        if digest != expected_digest {
            return Err(unreadable());
        }
        let object = artifact["object"].as_str().ok_or_else(unreadable)?;
        if object != format!("ds-grid/objects/sha256/{digest}") {
            return Err(unreadable());
        }
        let size = artifact["byte_length"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 256 * 1024 * 1024)
            .ok_or_else(unreadable)? as usize;
        let url = url.as_str().ok_or_else(unreadable)?;
        let call = crate::grid_models::locator(url, object, size)?;
        let mut response = transport.grid_model_bytes(call)?;
        if response.status != 200
            || response.body.len() != size
            || format!("{:x}", Sha256::digest(&response.body)) != digest
        {
            return Err(unreadable());
        }
        receipt["sha256"] = json!(digest);
        receipt["bytes"] = json!(size);
        receipt["verified"] = json!(true);
        Ok(crate::grid_models::Receipt {
            data: receipt,
            bytes: Some(std::mem::take(&mut response.body)),
        })
    }
}

/// The largest governed catalog body this owner will send, matching the
/// reviewed descriptor's response cap.
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// Inventory names are bounded independently of the catalog envelope.
pub const MAX_MEMBER_PATH_BYTES: usize = 512;

/// One governed catalog action.
///
/// Read variants carry identifiers; write variants carry the one typed object
/// the backend names for that action (`library`, `example`, `upload`,
/// `library_lifecycle`, `example_lifecycle`). The object stays opaque here —
/// its schema is the backend service's, validated there — but it must be a
/// bounded JSON object, so a caller cannot smuggle a second action or an
/// arbitrary body through it.
pub enum Command {
    ListLibraries,
    GetLibrary {
        library: String,
        release: Option<String>,
    },
    ListLibraryReleases {
        library: String,
    },
    /// Resolve only this immutable inventory pin. No head selection, project,
    /// object path or bucket can be supplied, and no member bytes are fetched.
    ResolveLibraryMember {
        library_id: String,
        release_id: String,
        relative_path: String,
        expected_digest: String,
    },
    ListExamples,
    GetExample {
        example: String,
        revision: Option<String>,
    },
    ListExampleRevisions {
        example: String,
    },
    PublishLibraryRelease {
        library: Value,
    },
    PublishExampleRevision {
        example: Value,
    },
    SetLibraryLifecycle {
        lifecycle: Value,
    },
    SetExampleLifecycle {
        lifecycle: Value,
    },
    StartArtifactUpload {
        upload: Value,
    },
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

fn pinned_identifier(value: &str) -> bool {
    (2..=128).contains(&value.len())
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

fn member_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_MEMBER_PATH_BYTES
        && value.trim() == value
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && segment.len() <= 128
                && segment.as_bytes()[0].is_ascii_alphanumeric()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b' ' | b'-'))
        })
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A governed write object: a bounded JSON object, never a scalar, a list, or
/// something larger than the descriptor's bound.
fn governed_object(value: &Value) -> bool {
    value.is_object()
        && serde_json::to_vec(value).is_ok_and(|bytes| {
            !bytes.is_empty() && bytes.len() <= MAX_REQUEST_BYTES && value.as_object().is_some()
        })
}

fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "global catalog selection or governed object violates its bounded contract",
    )
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "global catalog response violates its envelope contract",
    )
}

impl Command {
    /// The backend action this command sends. Also the caller's evidence of
    /// which closed action ran.
    pub fn action(&self) -> &'static str {
        match self {
            Self::ListLibraries => "list_libraries",
            Self::GetLibrary { .. } => "get_library",
            Self::ListLibraryReleases { .. } => "list_library_releases",
            Self::ResolveLibraryMember { .. } => "resolve_library_member",
            Self::ListExamples => "list_examples",
            Self::GetExample { .. } => "get_example",
            Self::ListExampleRevisions { .. } => "list_example_revisions",
            Self::PublishLibraryRelease { .. } => "publish_library_release",
            Self::PublishExampleRevision { .. } => "publish_example_revision",
            Self::SetLibraryLifecycle { .. } => "set_library_lifecycle",
            Self::SetExampleLifecycle { .. } => "set_example_lifecycle",
            Self::StartArtifactUpload { .. } => "start_artifact_upload",
        }
    }

    /// Does this action change the governed catalog? Reads stay available to
    /// any restored user; writes are refused without the publish capability.
    pub const fn writes(&self) -> bool {
        matches!(
            self,
            Self::PublishLibraryRelease { .. }
                | Self::PublishExampleRevision { .. }
                | Self::SetLibraryLifecycle { .. }
                | Self::SetExampleLifecycle { .. }
                | Self::StartArtifactUpload { .. }
        )
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        let ok = match self {
            Self::ListLibraries | Self::ListExamples => true,
            Self::GetLibrary { library, release } => {
                identifier(library) && release.as_deref().is_none_or(identifier)
            }
            Self::ListLibraryReleases { library } => identifier(library),
            Self::ResolveLibraryMember {
                library_id,
                release_id,
                relative_path,
                expected_digest,
            } => {
                pinned_identifier(library_id)
                    && pinned_identifier(release_id)
                    && member_path(relative_path)
                    && sha256(expected_digest)
            }
            Self::GetExample { example, revision } => {
                identifier(example) && revision.as_deref().is_none_or(identifier)
            }
            Self::ListExampleRevisions { example } => identifier(example),
            Self::PublishLibraryRelease { library } => governed_object(library),
            Self::PublishExampleRevision { example } => governed_object(example),
            Self::SetLibraryLifecycle { lifecycle } | Self::SetExampleLifecycle { lifecycle } => {
                governed_object(lifecycle)
            }
            Self::StartArtifactUpload { upload } => governed_object(upload),
        };
        if ok {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    /// The exact body for this action, and nothing else. The backend decodes
    /// with `DisallowUnknownFields`, so a key this match does not write is a
    /// key the request cannot carry.
    fn payload(&self) -> Value {
        let action = self.action();
        match self {
            Self::ListLibraries | Self::ListExamples => json!({ "action": action }),
            Self::GetLibrary { library, release } => match release {
                Some(release) => {
                    json!({ "action": action, "library_id": library, "release_id": release })
                }
                None => json!({ "action": action, "library_id": library }),
            },
            Self::ListLibraryReleases { library } => {
                json!({ "action": action, "library_id": library })
            }
            Self::ResolveLibraryMember {
                library_id,
                release_id,
                relative_path,
                expected_digest,
            } => json!({
                "action": action,
                "library_id": library_id,
                "release_id": release_id,
                "relative_path": relative_path,
                "expected_digest": expected_digest,
            }),
            Self::GetExample { example, revision } => match revision {
                Some(revision) => {
                    json!({ "action": action, "example_id": example, "revision_id": revision })
                }
                None => json!({ "action": action, "example_id": example }),
            },
            Self::ListExampleRevisions { example } => {
                json!({ "action": action, "example_id": example })
            }
            Self::PublishLibraryRelease { library } => {
                json!({ "action": action, "library": library })
            }
            Self::PublishExampleRevision { example } => {
                json!({ "action": action, "example": example })
            }
            Self::SetLibraryLifecycle { lifecycle } => {
                json!({ "action": action, "library_lifecycle": lifecycle })
            }
            Self::SetExampleLifecycle { lifecycle } => {
                json!({ "action": action, "example_lifecycle": lifecycle })
            }
            Self::StartArtifactUpload { upload } => json!({ "action": action, "upload": upload }),
        }
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate()?;
    let response = transport.grid_catalog(GridCatalogCall {
        profile,
        authority: authority.call(),
        payload: command.payload(),
    })?;
    match response.status {
        200 => {}
        // A missing library, example, release, revision or indexed member
        // (including a legacy release without inventory) is an ordinary
        // answer to a governed read, not a transport failure.
        404 => {
            return Err(ClientError::new(
                ErrorKind::ResourceNotFound,
                "no such governed library, example, release, revision or indexed member",
            ));
        }
        // 400 is the backend's strict decoder or its service validation; 409
        // is a head that moved or an identity that already exists. Both are
        // the caller's request to fix, never a retry.
        400 | 409 => {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "the governed catalog refused this request or its expected head",
            ));
        }
        408 | 425 | 429 | 500..=599 => {
            return Err(ClientError::new(
                ErrorKind::Transient,
                "the governed catalog service is temporarily unavailable",
            ));
        }
        // 401/403 and anything else: the verified user was refused. The
        // publish capability is the usual cause for a write.
        _ => {
            return Err(ClientError::new(
                ErrorKind::AuthenticationRejected,
                "the governed catalog refused the verified user for this action",
            ));
        }
    }
    if response.body.len() > MAX_REQUEST_BYTES {
        return Err(unreadable());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if body["success"] != true {
        return Err(unreadable());
    }
    let data = body.get("data").cloned().ok_or_else(unreadable)?;
    if data.is_null() {
        return Err(unreadable());
    }
    if let Command::ResolveLibraryMember {
        library_id,
        release_id,
        relative_path,
        expected_digest,
    } = command
    {
        validate_member_response(
            &data,
            library_id,
            release_id,
            relative_path,
            expected_digest,
        )?;
    }
    Ok(data)
}

/// Check the typed evidence and exact selection, then return the original
/// server data unchanged. Inventory semantics and object integrity belong to
/// the service; this client never manufactures records or dereferences URLs.
fn validate_member_response(
    data: &Value,
    library_id: &str,
    release_id: &str,
    relative_path: &str,
    expected_digest: &str,
) -> Result<(), ClientError> {
    fn text(value: &Value) -> bool {
        value.as_str().is_some_and(|s| !s.is_empty())
    }
    fn optional_text(value: &Value, key: &str) -> bool {
        value.get(key).is_none_or(Value::is_string)
    }
    let member = &data["member"];
    let artifact = &member["artifact"];
    let provenance = &member["provenance"];
    if data["library_id"] != library_id
        || data["release_id"] != release_id
        || member["relative_path"] != relative_path
        || artifact["digest"] != expected_digest
        || !text(&member["class"])
        || !text(&artifact["object"])
        || !artifact["byte_length"].as_i64().is_some_and(|n| n > 0)
        || !text(&provenance["kind"])
        || !text(&provenance["reference"])
        || !optional_text(provenance, "source_digest")
    {
        return Err(unreadable());
    }
    if let Some(binding) = member.get("external_definition") {
        if !text(&binding["system"])
            || !text(&binding["definition_id"])
            || !optional_text(binding, "revision")
        {
            return Err(unreadable());
        }
    }
    let expires_at = data["expires_at"].as_str().ok_or_else(unreadable)?;
    if expires_at.len() > 64
        || time::OffsetDateTime::parse(expires_at, &time::format_description::well_known::Rfc3339)
            .is_err()
    {
        return Err(unreadable());
    }
    let locator = data["download_url"].as_str().ok_or_else(unreadable)?;
    if locator.len() > 8192
        || !locator.starts_with("https://")
        || locator.contains('\\')
        || locator.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(unreadable());
    }
    let url = url::Url::parse(locator).map_err(|_| unreadable())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(unreadable());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ArtifactTransport {
        body: Vec<u8>,
        status: u16,
        calls: usize,
    }
    macro_rules! no_other_call {
        ($name:ident, $call:ident) => {
            fn $name(
                &mut self,
                _: crate::$call<'_>,
            ) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("artifact export made an unrelated call")
            }
        };
    }
    impl AuthorizedTransport for ArtifactTransport {
        no_other_call!(list_projects, ProjectListCall);
        no_other_call!(transformer_context, TransformerContextCall);
        no_other_call!(project_forms, ProjectFormsCall);
        no_other_call!(project_form_editor, ProjectFormEditorCall);
        no_other_call!(solar_snapshot, SolarSnapshotCall);
        no_other_call!(survey_query, SurveyQueryCall);
        no_other_call!(survey_entries_select, SurveyEntriesSelectCall);
        no_other_call!(survey_entries_changes, SurveyEntriesChangesCall);
        no_other_call!(survey_entry_create, SurveyEntryCreateCall);
        no_other_call!(project_data, ProjectDataCall);
        no_other_call!(upload_bytes, UploadBytesCall);
        no_other_call!(survey_control, SurveyControlCall);
        no_other_call!(styles, StylesCall);
        no_other_call!(layers, LayersCall);
        no_other_call!(tiles, TileCall);
        no_other_call!(project_report, ProjectReportCall);
        fn grid_model_bytes(
            &mut self,
            call: crate::grid_models::DownloadCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            self.calls += 1;
            assert!(call.url().starts_with("https://storage.googleapis.com/"));
            Ok(crate::TransportResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    fn artifact_fixture() -> (Value, String, Vec<u8>) {
        let bytes = b"exact original native bytes\r\n".to_vec();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let object = format!("ds-grid/objects/sha256/{digest}");
        let artifact = json!({"digest":digest,"object":object,"byte_length":bytes.len()});
        let data = json!({"head":{"library_id":"library_1","example_id":"example_1"},
            "release":{"library_id":"library_1","release_id":"release_1","manifest":artifact,"validation_report":artifact},
            "revision":{"example_id":"example_1","revision_id":"revision_1","model":artifact},
            "manifest_download_url":format!("https://storage.googleapis.com/bucket/{object}?signed=private"),
            "validation_report_download_url":format!("https://storage.googleapis.com/bucket/{object}?signed=private"),
            "model_download_url":format!("https://storage.googleapis.com/bucket/{object}?signed=private")});
        (data, digest, bytes)
    }

    fn manifest_selection() -> ArtifactSelection {
        ArtifactSelection::LibraryManifest {
            library: "library_1".into(),
            release: "release_1".into(),
        }
    }

    #[test]
    fn artifact_export_preserves_exact_bytes_and_excludes_delivery_credentials() {
        let (data, digest, bytes) = artifact_fixture();
        for selection in [
            manifest_selection(),
            ArtifactSelection::LibraryValidation {
                library: "library_1".into(),
                release: "release_1".into(),
            },
            ArtifactSelection::ExampleModel {
                example: "example_1".into(),
                revision: "revision_1".into(),
            },
        ] {
            let mut transport = ArtifactTransport {
                body: bytes.clone(),
                status: 200,
                calls: 0,
            };
            let receipt = selection.download(&mut transport, &data, &digest).unwrap();
            assert_eq!(receipt.bytes.as_deref(), Some(bytes.as_slice()));
            assert_eq!(receipt.data["sha256"], digest);
            assert_eq!(receipt.data["bytes"], bytes.len());
            assert_eq!(receipt.data["verified"], true);
            assert!(!receipt.data.to_string().contains("signed"));
            assert_eq!(transport.calls, 1);
        }
    }

    #[test]
    fn artifact_export_refuses_wrong_coordinates_digest_object_and_unsafe_locator_before_fetch() {
        let (data, digest, bytes) = artifact_fixture();
        let mut bad_cases = Vec::new();
        for (pointer, value) in [
            ("/release/library_id", json!("other")),
            ("/release/release_id", json!("moved")),
            ("/release/manifest/digest", json!("a".repeat(64))),
            (
                "/release/manifest/object",
                json!("ds-grid/objects/sha256/other"),
            ),
            ("/release/manifest/byte_length", json!(0)),
            (
                "/release/manifest/byte_length",
                json!(256_u64 * 1024 * 1024 + 1),
            ),
            (
                "/manifest_download_url",
                json!("https://example.com/native"),
            ),
            (
                "/manifest_download_url",
                json!("http://storage.googleapis.com/bucket/native"),
            ),
            (
                "/manifest_download_url",
                json!(format!(
                    "https://storage.googleapis.com/bucket/ds-grid/objects/sha256/{}",
                    "b".repeat(64)
                )),
            ),
        ] {
            let mut bad = data.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            bad_cases.push(bad);
        }
        for bad in bad_cases {
            let mut transport = ArtifactTransport {
                body: bytes.clone(),
                status: 200,
                calls: 0,
            };
            assert!(manifest_selection()
                .download(&mut transport, &bad, &digest)
                .is_err());
            assert_eq!(transport.calls, 0);
        }
        let selection = ArtifactSelection::ExampleModel {
            example: "example_1".into(),
            revision: "other".into(),
        };
        let mut transport = ArtifactTransport {
            body: bytes,
            status: 200,
            calls: 0,
        };
        assert!(selection.download(&mut transport, &data, &digest).is_err());
        assert_eq!(transport.calls, 0);
    }

    #[test]
    fn artifact_export_never_returns_incomplete_corrupt_or_redirected_bytes() {
        let (data, digest, bytes) = artifact_fixture();
        let mut corrupt = bytes.clone();
        corrupt[0] ^= 1;
        for (body, status) in [
            (bytes[..bytes.len() - 1].to_vec(), 200),
            (corrupt, 200),
            (bytes, 302),
        ] {
            let mut transport = ArtifactTransport {
                body,
                status,
                calls: 0,
            };
            assert!(manifest_selection()
                .download(&mut transport, &data, &digest)
                .is_err());
            assert_eq!(transport.calls, 1);
        }
    }

    #[test]
    fn every_action_sends_only_the_key_its_backend_decoder_accepts() {
        // The backend decodes with DisallowUnknownFields, so an extra key is a
        // 400 rather than an ignored field. One case per action, asserting the
        // exact key set.
        let cases: Vec<(Command, &str, Vec<&str>)> = vec![
            (Command::ListLibraries, "list_libraries", vec![]),
            (
                Command::GetLibrary {
                    library: "pls-cadd".into(),
                    release: Some("r3".into()),
                },
                "get_library",
                vec!["library_id", "release_id"],
            ),
            (
                Command::GetLibrary {
                    library: "pls-cadd".into(),
                    release: None,
                },
                "get_library",
                vec!["library_id"],
            ),
            (
                Command::ListLibraryReleases {
                    library: "pls-cadd".into(),
                },
                "list_library_releases",
                vec!["library_id"],
            ),
            (
                Command::ResolveLibraryMember {
                    library_id: "library_1".into(),
                    release_id: "release_1".into(),
                    relative_path: "pls-cadd/criteria/Base.CRI".into(),
                    expected_digest: "a".repeat(64),
                },
                "resolve_library_member",
                vec![
                    "library_id",
                    "release_id",
                    "relative_path",
                    "expected_digest",
                ],
            ),
            (Command::ListExamples, "list_examples", vec![]),
            (
                Command::GetExample {
                    example: "feeder".into(),
                    revision: None,
                },
                "get_example",
                vec!["example_id"],
            ),
            (
                Command::ListExampleRevisions {
                    example: "feeder".into(),
                },
                "list_example_revisions",
                vec!["example_id"],
            ),
            (
                Command::PublishLibraryRelease {
                    library: json!({ "library_id": "pls-cadd" }),
                },
                "publish_library_release",
                vec!["library"],
            ),
            (
                Command::PublishExampleRevision {
                    example: json!({ "example_id": "feeder" }),
                },
                "publish_example_revision",
                vec!["example"],
            ),
            (
                Command::SetLibraryLifecycle {
                    lifecycle: json!({ "library_id": "pls-cadd", "action": "archive" }),
                },
                "set_library_lifecycle",
                vec!["library_lifecycle"],
            ),
            (
                Command::SetExampleLifecycle {
                    lifecycle: json!({ "example_id": "feeder", "action": "archive" }),
                },
                "set_example_lifecycle",
                vec!["example_lifecycle"],
            ),
            (
                Command::StartArtifactUpload {
                    upload: json!({ "purpose": "library_asset" }),
                },
                "start_artifact_upload",
                vec!["upload"],
            ),
        ];
        for (command, action, extra) in cases {
            let payload = command.payload();
            let object = payload.as_object().expect("payload is an object");
            assert_eq!(object["action"], action);
            let mut expected: Vec<&str> = extra;
            expected.push("action");
            expected.sort_unstable();
            let mut actual: Vec<&str> = object.keys().map(String::as_str).collect();
            actual.sort_unstable();
            assert_eq!(actual, expected, "{action} sends the wrong key set");
        }
    }

    #[test]
    fn an_identifier_that_could_change_a_route_is_refused() {
        // These ids reach a path segment on the backend's own REST twin, so a
        // slash or a control character is refused here rather than sent.
        for bad in ["", " leading", "trailing ", "a/b", "a\\b", "a\nb"] {
            assert!(
                Command::GetLibrary {
                    library: bad.into(),
                    release: None,
                }
                .validate()
                .is_err(),
                "{bad:?} was accepted as a library id"
            );
            assert!(
                Command::ListExampleRevisions {
                    example: bad.into(),
                }
                .validate()
                .is_err(),
                "{bad:?} was accepted as an example id"
            );
        }
        assert!(
            Command::GetLibrary {
                library: "pls-cadd".into(),
                release: Some("a/b".into()),
            }
            .validate()
            .is_err(),
            "a release id with a separator was accepted"
        );
    }

    #[test]
    fn a_governed_write_object_is_an_object_within_its_bound() {
        assert!(Command::PublishLibraryRelease {
            library: json!({ "library_id": "pls-cadd" }),
        }
        .validate()
        .is_ok());
        for bad in [
            json!("a string"),
            json!(7),
            json!(null),
            json!([{ "library_id": "pls-cadd" }]),
        ] {
            assert!(
                Command::PublishLibraryRelease { library: bad }
                    .validate()
                    .is_err(),
                "a non-object governed body was accepted"
            );
        }
        let oversized = json!({ "blob": "x".repeat(MAX_REQUEST_BYTES) });
        assert!(
            Command::StartArtifactUpload { upload: oversized }
                .validate()
                .is_err(),
            "an oversized governed body was accepted"
        );
    }

    #[test]
    fn reads_and_writes_are_told_apart_without_asking_the_backend() {
        // `ds library global read` and `… write` gate on this, so a new action
        // landing on the wrong side is a capability error, not a cosmetic one.
        assert!(!Command::ListLibraries.writes());
        assert!(!Command::ResolveLibraryMember {
            library_id: "library_1".into(),
            release_id: "release_1".into(),
            relative_path: "pls-cadd/criteria/Base.CRI".into(),
            expected_digest: "a".repeat(64),
        }
        .writes());
        assert!(!Command::GetExample {
            example: "feeder".into(),
            revision: None,
        }
        .writes());
        assert!(Command::PublishLibraryRelease { library: json!({}) }.writes());
        assert!(Command::StartArtifactUpload { upload: json!({}) }.writes());
    }
}
