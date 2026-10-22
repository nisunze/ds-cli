use super::*;
use ds_command_kernel::{survey_media_rotation, survey_photo_media as pixels};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ds-photo-contract-{}-{}-{}",
            std::process::id(),
            now_ms(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        ds_layer_store::private::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn prepared() -> survey_photo::Prepared {
    // A real PNG, retained verbatim rather than normalized or reencoded.
    let original = vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5,
        1, 1, 39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let path = "p1/forms/poles/e1/photo.png";
    let turn = survey_media_rotation::turn(&serde_json::Map::new(), path, "1729", 90).unwrap();
    let thumbnail = pixels::thumbnail(&original, pixels::Rotation::Deg90).unwrap();
    let dimensions = pixels::display_dimensions(&original, pixels::Rotation::Deg90).unwrap();
    let manifest = Manifest {
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
        original_size: original.len(),
        original_sha256: hash(&original),
        previous_quarter_turns: 0,
        quarter_turns: 1,
        expected_version: 3,
        updated_data: turn.updated_data,
        entry_committed_version: None,
        thumbnail_expected_generation: None,
        thumbnail_size: thumbnail.bytes.len(),
        thumbnail_sha256: hash(&thumbnail.bytes),
        width: dimensions.0,
        height: dimensions.1,
        thumbnail_width: thumbnail.width,
        thumbnail_height: thumbnail.height,
    };
    survey_photo::Prepared {
        manifest,
        original,
        thumbnail: thumbnail.bytes,
    }
}

fn reload(root: &Path) -> survey_photo::Prepared {
    survey_photo::Prepared {
        manifest: held_manifest(root).unwrap(),
        original: read(root, "original.bin", pixels::MAX_INPUT_BYTES).unwrap(),
        thumbnail: read(root, "thumbnail.jpeg", pixels::MAX_INPUT_BYTES).unwrap(),
    }
}

#[test]
fn held_turns_survive_reload_accumulate_and_preserve_original_and_generation() {
    let dir = TestDir::new();
    let root = dir.0.join("held");
    let first = prepared();
    let original = first.original.clone();
    write_bundle(&root, &first).unwrap();
    let initial_record = record_of(&first, None, 10);
    let mut record = initial_record.clone();
    for turns in [2, 3, 0] {
        let held = reload(&root);
        let next =
            survey_photo::turn_pending("p1", "uid", &"a".repeat(64), &held, 90, Some("1729"))
                .unwrap();
        replace_bundle(&root, &next).unwrap();
        let reloaded = reload(&root);
        assert_eq!(reloaded.original, original);
        assert_eq!(reloaded.manifest.generation, "1729");
        assert_eq!(reloaded.manifest.original_sha256, hash(&original));
        assert_eq!(reloaded.manifest.quarter_turns, turns);
        assert_eq!(reloaded.manifest.expected_version, 3);
        record = record_of(&reloaded, Some(&record), 20);
        assert_eq!(record.cached_at_ms, initial_record.cached_at_ms);
        assert_eq!(record.source_generation, "1729");
        assert_eq!(record.degrees, u16::from(turns) * 90);
    }
    assert_eq!(
        reload(&root).manifest.updated_data,
        json!({survey_media_rotation::KEY:{}})
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["manifest.json", "original.bin", "thumbnail.jpeg"] {
            assert_eq!(
                fs::metadata(root.join(name)).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn committed_receipt_is_retained_without_replacing_either_byte_file() {
    let dir = TestDir::new();
    let root = dir.0.join("held");
    let mut first = prepared();
    write_bundle(&root, &first).unwrap();
    first.manifest.entry_committed_version = Some(4);
    replace_manifest(&root, &first.manifest).unwrap();
    let held = reload(&root);
    assert_eq!(held.manifest.entry_committed_version, Some(4));
    assert_eq!(held.original, first.original);
    assert_eq!(held.thumbnail, first.thumbnail);
    // A new turn after a metadata commit uses its actual version as the fence.
    let next = survey_photo::turn_pending("p1", "uid", &"a".repeat(64), &held, 90, None).unwrap();
    assert_eq!(next.manifest.expected_version, 4);
    assert_eq!(next.manifest.previous_quarter_turns, 1);
    assert_eq!(next.manifest.quarter_turns, 2);
    assert_eq!(next.manifest.entry_committed_version, None);
}

#[test]
fn changed_original_owner_generation_or_thumbnail_is_refused_before_staging() {
    let mut held = prepared();
    assert!(survey_photo::turn_pending("p1", "other", &"a".repeat(64), &held, 90, None).is_err());
    assert!(
        survey_photo::turn_pending("p1", "uid", &"a".repeat(64), &held, 90, Some("1730")).is_err()
    );
    held.original[0] ^= 1;
    assert!(survey_photo::turn_pending("p1", "uid", &"a".repeat(64), &held, 90, None).is_err());
    let mut held = prepared();
    held.thumbnail[0] ^= 1;
    assert!(survey_photo::turn_pending("p1", "uid", &"a".repeat(64), &held, 90, None).is_err());
}
