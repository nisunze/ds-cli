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
//! this owner can send, each variant naming the one body key the backend's
//! `DisallowUnknownFields` decoder accepts for it. A caller cannot reach an
//! action that is not listed, cannot add a key beside the one its action
//! declares, and cannot choose the path, the method or the origin.

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    GridCatalogCall,
};
use serde_json::{json, Value};

/// The largest governed catalog body this owner will send, matching the
/// reviewed descriptor's response cap.
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;

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
        // A missing library, example, release or revision is an ordinary
        // answer to a governed read, not a transport failure.
        404 => {
            return Err(ClientError::new(
                ErrorKind::ResourceNotFound,
                "no such governed library, example, release or revision",
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
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!Command::GetExample {
            example: "feeder".into(),
            revision: None,
        }
        .writes());
        assert!(Command::PublishLibraryRelease { library: json!({}) }.writes());
        assert!(Command::StartArtifactUpload { upload: json!({}) }.writes());
    }
}
