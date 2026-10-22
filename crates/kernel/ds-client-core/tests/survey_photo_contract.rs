//! Public bundle admission controls. The real native transport, metadata CAS,
//! thumbnail publication and resume exchange are exercised in survey_photo::tests.
use ds_client_core::survey_photo::{self, Manifest};
use ds_command_kernel::survey_media_rotation;
use sha2::{Digest, Sha256};

fn manifest(thumbnail: &[u8]) -> Manifest {
    let path = "p1/forms/poles/e1/photo.png";
    let turn = survey_media_rotation::turn(&serde_json::Map::new(), path, "1729", 90).unwrap();
    Manifest {
        schema: survey_photo::SCHEMA.into(),
        project_id: "p1".into(),
        account_id: "uid".into(),
        credential_audience_sha256: "a".repeat(64),
        bucket: "bucket".into(),
        object_path: path.into(),
        thumbnail_path: "p1/forms/poles/e1/photo_thunder.jpeg".into(),
        form_id: "poles".into(),
        entry_id: "e1".into(),
        generation: "1729".into(),
        media_type: "image/png".into(),
        original_size: 4,
        original_sha256: format!("{:x}", Sha256::digest(b"camera")),
        previous_quarter_turns: 0,
        quarter_turns: 1,
        expected_version: 3,
        updated_data: turn.updated_data,
        entry_committed_version: None,
        thumbnail_expected_generation: None,
        thumbnail_size: thumbnail.len(),
        thumbnail_sha256: format!("{:x}", Sha256::digest(thumbnail)),
        width: 2,
        height: 3,
        thumbnail_width: 2,
        thumbnail_height: 3,
    }
}

#[test]
fn public_rotation_bundle_admission_preserves_context_and_refuses_retired_replacement_shape() {
    let thumbnail = b"held thumbnail";
    let held = manifest(thumbnail);
    survey_photo::validate_manifest("p1", "uid", &held, thumbnail).unwrap();
    for (project, account) in [("p2", "uid"), ("p1", "other")] {
        assert!(survey_photo::validate_manifest(project, account, &held, thumbnail).is_err());
    }
    assert!(survey_photo::validate_manifest("p1", "uid", &held, b"changed").is_err());
    let mut retired = serde_json::to_value(&held).unwrap();
    retired["schema"] = "ds.survey-photo-rotation/v1".into();
    let retired: Manifest = serde_json::from_value(retired).unwrap();
    assert!(survey_photo::validate_manifest("p1", "uid", &retired, thumbnail).is_err());
    // A private bundle carries only the rotation patch, never an arbitrary data edit.
    let mut retargeted = held;
    retargeted.updated_data["unrelated_form_field"] = true.into();
    assert!(survey_photo::validate_manifest("p1", "uid", &retargeted, thumbnail).is_err());
}
