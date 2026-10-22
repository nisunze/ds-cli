use super::*;

const PATH: &str = "p1/forms/poles/e1/a.jpg";

fn manifest(thumbnail: &[u8], turns: u8) -> Manifest {
    let turned =
        survey_media_rotation::turn(&serde_json::Map::new(), PATH, "1729", i64::from(turns) * 90)
            .unwrap();
    Manifest {
        schema: SCHEMA.into(),
        project_id: "p1".into(),
        account_id: "uid".into(),
        credential_audience_sha256: "a".repeat(64),
        bucket: "bucket".into(),
        object_path: PATH.into(),
        thumbnail_path: "p1/forms/poles/e1/a_thunder.jpeg".into(),
        form_id: "poles".into(),
        entry_id: "e1".into(),
        generation: "1729".into(),
        media_type: "image/jpeg".into(),
        original_size: 4,
        original_sha256: hash(b"jpeg"),
        previous_quarter_turns: 0,
        quarter_turns: turned.quarter_turns,
        expected_version: 3,
        updated_data: turned.updated_data,
        entry_committed_version: None,
        thumbnail_expected_generation: Some("77".into()),
        thumbnail_size: thumbnail.len(),
        thumbnail_sha256: hash(thumbnail),
        width: 2,
        height: 3,
        thumbnail_width: 2,
        thumbnail_height: 3,
    }
}

#[test]
fn a_bundle_publishes_only_the_metadata_and_thumbnail_it_names() {
    let thumbnail = b"thumb".as_slice();
    let good = manifest(thumbnail, 1);
    validate_manifest("p1", "uid", &good, thumbnail).unwrap();
    // The turn the manifest claims must be the turn its patch records.
    let mut lying = good.clone();
    lying.quarter_turns = 2;
    assert!(validate_manifest("p1", "uid", &lying, thumbnail).is_err());
    // Changed thumbnail bytes, another account or project, another entry.
    assert!(validate_manifest("p1", "uid", &good, b"other").is_err());
    assert!(validate_manifest("p1", "someone", &good, thumbnail).is_err());
    assert!(validate_manifest("p2", "uid", &good, thumbnail).is_err());
    let mut retargeted = good.clone();
    retargeted.entry_id = "e2".into();
    assert!(validate_manifest("p1", "uid", &retargeted, thumbnail).is_err());
    // A turn record chosen for other bytes is not this generation's turn.
    let mut stale = good;
    stale.generation = "1730".into();
    assert!(validate_manifest("p1", "uid", &stale, thumbnail).is_err());
}

#[test]
fn a_full_circle_bundle_records_no_turn() {
    let thumbnail = b"thumb".as_slice();
    let mut back = manifest(thumbnail, 1);
    back.updated_data = serde_json::json!({survey_media_rotation::KEY: {}});
    back.quarter_turns = 0;
    validate_manifest("p1", "uid", &back, thumbnail).unwrap();
}

#[test]
fn a_local_rotation_takes_only_a_quarter_turn_of_a_readable_image() {
    assert!(rotate_local(b"\x89PNG\r\n\x1a\n", 45).is_err());
    assert!(rotate_local(b"not an image", 90).is_err());
}

use crate::*;

fn original_png() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap()
}

fn test_profile() -> ClientProfile {
    ClientProfile::from_parts(crate::profile::ProfileParts {
        lane: DeploymentLane::Canary,
        source_revision: "independent-core-release",
        descriptor_sha256: &"a".repeat(64),
        firebase_project_id: "fixture-project",
        firebase_api_key: "fixture-public-key",
        gateway_api_key: "fixture-public-key",
        gateway_origin: "https://fixture-canary.ue.gateway.dev",
    })
    .unwrap()
}

fn authority() -> Authority<'static> {
    Authority {
        bearer_token: "fixture-token",
        uid: "uid",
        email: "fixture@example.invalid",
        device_id: None,
    }
}

/// Only transport responses are fixtures. The actual native owner, metadata
/// decoder, mutation constructor, generation admission and pixel code execute.
struct PhotoWire {
    source: Vec<u8>,
    generation: String,
    version: u64,
    data: Value,
    packets: Vec<Value>,
    updates: Vec<Value>,
    uploaded: Vec<Vec<u8>>,
    reads: usize,
    downloads: usize,
    fail_thumbnail: bool,
    conflict: bool,
}

impl PhotoWire {
    fn new() -> Self {
        Self {
            source: original_png(),
            generation: "1729".into(),
            version: 3,
            data: json!({"photo":PATH,"name":"preserved field"}),
            packets: Vec::new(),
            updates: Vec::new(),
            uploaded: Vec::new(),
            reads: 0,
            downloads: 0,
            fail_thumbnail: false,
            conflict: false,
        }
    }
}

macro_rules! unused_transport {
    ($name:ident, $call:ty) => {
        fn $name(&mut self, _call: $call) -> Result<TransportResponse, TransportError> {
            panic!("unexpected transport: {}", stringify!($name));
        }
    };
}

impl AuthorizedTransport for PhotoWire {
    unused_transport!(list_projects, ProjectListCall<'_>);
    unused_transport!(transformer_context, TransformerContextCall<'_>);
    unused_transport!(project_forms, ProjectFormsCall<'_>);
    unused_transport!(project_form_editor, ProjectFormEditorCall<'_>);
    unused_transport!(solar_snapshot, SolarSnapshotCall<'_>);
    unused_transport!(survey_query, SurveyQueryCall<'_>);
    unused_transport!(survey_entries_select, SurveyEntriesSelectCall<'_>);
    unused_transport!(survey_entries_changes, SurveyEntriesChangesCall<'_>);
    unused_transport!(survey_entry_create, SurveyEntryCreateCall<'_>);
    unused_transport!(project_data, ProjectDataCall<'_>);
    unused_transport!(survey_control, SurveyControlCall<'_>);
    unused_transport!(styles, StylesCall<'_>);
    unused_transport!(layers, LayersCall<'_>);
    unused_transport!(tiles, TileCall<'_>);
    unused_transport!(project_report, ProjectReportCall<'_>);

    fn survey_entries_read(
        &mut self,
        call: SurveyEntriesReadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let body: Value = serde_json::from_slice(call.body().as_bytes()).unwrap();
        assert_eq!(body["eds_project_id"], "p1");
        self.reads += 1;
        let feature = json!({"type":"Feature","id":"e1","geometry":null,
            "properties":{"data":self.data,"metadata":{"client_version":self.version}}});
        let summary = json!({"__type":"summary","errors":[],"total_features":1});
        Ok(TransportResponse::new(
            200,
            format!("{feature}\n{summary}\n").into_bytes(),
        ))
    }

    fn survey_photo(
        &mut self,
        call: SurveyPhotoCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), PATH_FOR_PHOTO);
        let body: Value = serde_json::from_slice(call.body().as_bytes()).unwrap();
        self.packets.push(body.clone());
        let thumbnail = body["file_name"] == "a_thunder.jpeg";
        let data = match body["action"].as_str().unwrap() {
            "survey_upload_inspect" if thumbnail => {
                return Ok(TransportResponse::new(404, b"{}".as_slice()));
            }
            "survey_upload_inspect" => {
                json!({"bucket":"bucket","blob_path":PATH,"generation":self.generation,
                "source_url":format!("https://storage.googleapis.com/bucket/{PATH}?generation={}",self.generation)})
            }
            "survey_upload_start" => {
                assert!(
                    thumbnail,
                    "camera originals must never receive an upload grant"
                );
                if self.fail_thumbnail {
                    self.fail_thumbnail = false;
                    return Ok(TransportResponse::new(503, b"{}".as_slice()));
                }
                json!({"bucket":"bucket","blob_path":"p1/forms/poles/e1/a_thunder.jpeg",
                    "session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?uploadType=resumable&upload_id=fixture"})
            }
            "survey_upload_complete" => {
                assert!(thumbnail);
                json!({"bucket":"bucket","blob_path":"p1/forms/poles/e1/a_thunder.jpeg","already_present":true,"generation":"77"})
            }
            action => panic!("unexpected photo action {action}"),
        };
        Ok(TransportResponse::new(
            200,
            serde_json::to_vec(&json!({"success":true,"data":data})).unwrap(),
        ))
    }

    fn download_survey_photo(
        &mut self,
        call: DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert!(
            call.url()
                .ends_with(&format!("generation={}", self.generation))
        );
        self.downloads += 1;
        Ok(TransportResponse::new(200, self.source.clone()))
    }

    fn survey_update(
        &mut self,
        call: SurveyUpdateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let body: Value = serde_json::from_slice(call.body().as_bytes()).unwrap();
        assert_eq!(body["expected_version"], self.version);
        assert_eq!(body["operation"], "update");
        assert!(body.get("updated_geometry").is_none());
        self.updates.push(body.clone());
        if self.conflict {
            return Ok(TransportResponse::new(409, b"{}".as_slice()));
        }
        self.version += 1;
        self.data[survey_media_rotation::KEY] =
            body["updated_data"][survey_media_rotation::KEY].clone();
        let now = "2026-10-21T00:00:00Z";
        Ok(TransportResponse::new(200, serde_json::to_vec(&json!({"success":true,"error":null,
            "service":"data-solutions-backend","version":"1.0.0","timestamp":now,
            "data":{"operation":"update","doc_id":"e1","client_version":self.version,"updated_at":now}})).unwrap()))
    }

    fn upload_bytes(
        &mut self,
        mut call: UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut bytes = Vec::new();
        call.reader().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, call.size());
        self.uploaded.push(bytes);
        Ok(TransportResponse::new(200, b"{}".as_slice()))
    }
}

const PATH_FOR_PHOTO: &str = super::PATH;

fn prepare(wire: &mut PhotoWire, profile: &ClientProfile) -> Box<Prepared> {
    match run(
        profile,
        wire,
        authority(),
        "p1",
        Command::Prepare {
            object_path: PATH,
            degrees: 90,
            expected_generation: Some("1729"),
            pending: None,
        },
    )
    .unwrap()
    {
        Receipt::Prepared(prepared) => prepared,
        Receipt::Published(_) => panic!("expected preparation"),
    }
}

#[test]
fn retained_turns_accumulate_without_transport_and_four_turns_restore_the_metadata() {
    let profile = test_profile();
    let mut wire = PhotoWire::new();
    let mut prepared = prepare(&mut wire, &profile);
    assert_eq!(prepared.original, wire.source);
    let calls = (wire.packets.len(), wire.reads, wire.downloads);
    for turns in [2, 3, 0] {
        prepared = Box::new(
            turn_pending(
                "p1",
                "uid",
                profile.credential_audience_sha256(),
                &prepared,
                90,
                Some("1729"),
            )
            .unwrap(),
        );
        assert_eq!(prepared.manifest.quarter_turns, turns);
        assert_eq!(prepared.original, wire.source);
        assert_eq!(prepared.manifest.original_sha256, hash(&wire.source));
        assert_eq!(prepared.manifest.expected_version, 3);
        assert_eq!(prepared.manifest.generation, "1729");
    }
    assert_eq!((wire.packets.len(), wire.reads, wire.downloads), calls);
    assert_eq!(
        prepared.manifest.updated_data,
        json!({survey_media_rotation::KEY:{}})
    );
    assert!(
        turn_pending(
            "p1",
            "uid",
            profile.credential_audience_sha256(),
            &prepared,
            90,
            Some("1730")
        )
        .is_err()
    );
    assert!(
        turn_pending(
            "p1",
            "other",
            profile.credential_audience_sha256(),
            &prepared,
            90,
            None
        )
        .is_err()
    );
    assert!(turn_pending("p1", "uid", &"0".repeat(64), &prepared, 90, None).is_err());
}

#[test]
fn metadata_commit_then_thumbnail_failure_resumes_without_rewriting_or_mutating_twice() {
    let profile = test_profile();
    let mut wire = PhotoWire::new();
    let mut prepared = prepare(&mut wire, &profile);
    wire.fail_thumbnail = true;
    let Receipt::Published(partial) = run(
        &profile,
        &mut wire,
        authority(),
        "p1",
        Command::Publish {
            manifest: &prepared.manifest,
            thumbnail: &prepared.thumbnail,
        },
    )
    .unwrap() else {
        panic!("expected partial receipt")
    };
    assert_eq!(partial["publication"], "entry_committed");
    assert_eq!(wire.updates.len(), 1);
    assert!(wire.uploaded.is_empty());
    prepared.manifest.entry_committed_version = partial["entry"]["client_version"].as_u64();
    let Receipt::Published(done) = run(
        &profile,
        &mut wire,
        authority(),
        "p1",
        Command::Publish {
            manifest: &prepared.manifest,
            thumbnail: &prepared.thumbnail,
        },
    )
    .unwrap() else {
        panic!("expected publication")
    };
    assert_eq!(done["publication"], "published");
    assert_eq!(done["original_rewritten"], false);
    assert_eq!(wire.updates.len(), 1);
    assert_eq!(wire.uploaded, vec![prepared.thumbnail.clone()]);
    assert_eq!(wire.data["name"], "preserved field");
    assert_eq!(wire.source, prepared.original);
    for packet in wire
        .packets
        .iter()
        .filter(|packet| packet["action"] == "survey_upload_start")
    {
        assert_eq!(packet["file_name"], "a_thunder.jpeg");
        assert_eq!(packet["source_generation"], "1729");
        assert_eq!(packet["source_quarter_turns"], 1);
    }
}

#[test]
fn a_changed_generation_or_conflicting_entry_prevents_thumbnail_publication() {
    let profile = test_profile();
    let mut wire = PhotoWire::new();
    let prepared = prepare(&mut wire, &profile);
    wire.generation = "1730".into();
    assert!(
        run(
            &profile,
            &mut wire,
            authority(),
            "p1",
            Command::Publish {
                manifest: &prepared.manifest,
                thumbnail: &prepared.thumbnail,
            }
        )
        .is_err()
    );
    assert!(wire.updates.is_empty());
    assert!(wire.uploaded.is_empty());
    wire.generation = "1729".into();
    wire.conflict = true;
    assert!(
        run(
            &profile,
            &mut wire,
            authority(),
            "p1",
            Command::Publish {
                manifest: &prepared.manifest,
                thumbnail: &prepared.thumbnail,
            }
        )
        .is_err()
    );
    assert!(wire.uploaded.is_empty());
    assert_eq!(wire.source, prepared.original);
}
