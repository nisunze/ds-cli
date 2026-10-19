use super::*;
use ds_layer_store::local_models::{execute_at, package_path, read_at, scope_dir};
fn held(root: &std::path::Path, bytes: &[u8], revision: u64) -> Located {
    let scope = Scope {
        lane: "stable".into(),
        uid: "local-migration-test".into(),
    };
    let outcome = execute_at(
        root,
        &scope,
        Op::Register {
            id: "local-migration-test".into(),
            display_name: "Original named copy".into(),
            origin: Origin::Imported,
            crs: "EPSG:32735".into(),
            model_revision: revision,
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            created_at: None,
            project: None,
            head_revision: None,
            activate: true,
        },
        Some(bytes),
    )
    .unwrap();
    let row = outcome.model.unwrap();
    let path = package_path(&scope_dir(root, &scope).unwrap(), &row.id).unwrap();
    Located { scope, row, path }
}
fn current() -> Vec<u8> {
    ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest {
        coordinate_system: Some("EPSG:32735"),
        ..Default::default()
    })
    .unwrap()
    .bytes
}
#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn current_named_open_is_noop_and_keeps_original_identity() {
    let root = tempfile::tempdir().unwrap();
    let bytes = current();
    let revision = ds_grid_exchange::package::unpack(&bytes)
        .unwrap()
        .manifest
        .model
        .model_revision;
    let located = held(root.path(), &bytes, revision);
    let before = read_at(root.path(), &located.scope).unwrap();
    assert!(prepare_open_at(root.path(), &located).unwrap().is_none());
    assert_eq!(read_at(root.path(), &located.scope).unwrap(), before);
    assert_eq!(std::fs::read(&located.path).unwrap(), bytes);
    assert!(!root.path().join("model-format-migrations").exists());
}
#[test]
fn mismatched_catalogue_package_pin_refuses_without_effect() {
    let root = tempfile::tempdir().unwrap();
    let bytes = current();
    let mut located = held(
        root.path(),
        &bytes,
        ds_grid_exchange::package::unpack(&bytes)
            .unwrap()
            .manifest
            .model
            .model_revision,
    );
    located.row.sha256 = "a".repeat(64);
    assert!(prepare_open_at(root.path(), &located).is_err());
    assert_eq!(std::fs::read(&located.path).unwrap(), bytes);
    assert!(!root.path().join("model-format-migrations").exists());
}
#[test]
#[ignore = "mandatory genuine old package local-open gate; execute explicitly with DS_GRID_LOCAL_OPEN_SOURCE and its pinned SHA"]
fn genuine_old_named_open_preserves_archive_saves_current_and_is_idempotent() {
    let source_path = std::env::var("DS_GRID_LOCAL_OPEN_SOURCE").unwrap();
    let expected = std::env::var("DS_GRID_LOCAL_OPEN_SOURCE_SHA256").unwrap();
    let bytes = std::fs::read(source_path).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected);
    let output = ds_grid_exchange::package_migration::migrate(&bytes).unwrap();
    assert!(!output.receipt.already_current);
    let target = identity(&output.bytes).unwrap();
    let root = tempfile::tempdir().unwrap();
    let located = held(root.path(), &bytes, target.model_revision);
    let old_row = located.row.clone();
    let receipt = prepare_open_at(root.path(), &located)
        .unwrap()
        .expect("actual migration");
    assert_eq!(receipt["source_sha256"], expected);
    assert_eq!(receipt["current_sha256"], target.sha256);
    assert_eq!(
        std::fs::read(receipt["original_package_path"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(std::fs::read(&located.path).unwrap(), output.bytes);
    let catalogue = read_at(root.path(), &located.scope).unwrap();
    let row = catalogue
        .models
        .iter()
        .find(|row| row.id == old_row.id)
        .unwrap()
        .clone();
    assert_eq!(row.model_revision, old_row.model_revision);
    assert_eq!(row.display_name, old_row.display_name);
    assert_eq!(row.origin, old_row.origin);
    assert_eq!(row.crs, old_row.crs);
    assert_eq!(row.project, old_row.project);
    assert_eq!(row.created_at, old_row.created_at);
    assert_eq!(row.pls_source, old_row.pls_source);
    let current = Located {
        scope: located.scope,
        row,
        path: located.path,
    };
    assert!(prepare_open_at(root.path(), &current).unwrap().is_none());
    assert_eq!(read_at(root.path(), &current.scope).unwrap(), catalogue);
    // A stale captured row cannot overwrite the already migrated package.
    let stale = Located {
        scope: current.scope.clone(),
        row: old_row,
        path: current.path.clone(),
    };
    assert!(prepare_open_at(root.path(), &stale).is_err());
    assert_eq!(std::fs::read(&current.path).unwrap(), output.bytes);
}
#[test]
#[ignore = "mandatory old package pending-draft control; same pinned DS_GRID_LOCAL_OPEN_SOURCE"]
fn old_named_open_does_not_rebase_a_pending_draft_or_overwrite_incomplete_cache() {
    let bytes = std::fs::read(std::env::var("DS_GRID_LOCAL_OPEN_SOURCE").unwrap()).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        std::env::var("DS_GRID_LOCAL_OPEN_SOURCE_SHA256").unwrap()
    );
    let output = ds_grid_exchange::package_migration::migrate(&bytes).unwrap();
    assert!(!output.receipt.already_current);
    let root = tempfile::tempdir().unwrap();
    let mut located = held(
        root.path(),
        &bytes,
        identity(&output.bytes).unwrap().model_revision,
    );
    located.row.draft = Some(ds_command_kernel::local_models::LocalDraft {
        bytes: 1,
        pin: ds_command_kernel::local_models::DraftCheckpointPin {
            binding: ds_command_kernel::local_models::DraftBinding {
                local_model_id: located.row.id.clone(),
                base_artifact_sha256: located.row.sha256.clone(),
            },
            head_revision: format!("rev:{}", "a".repeat(64)),
            sequence: 1,
            content_digest: "b".repeat(64),
        },
    });
    assert!(prepare_open_at(root.path(), &located).is_err());
    assert!(!root.path().join("model-format-migrations").exists());
    located.row.draft = None;
    let codec = serde_json::to_vec(&output.receipt.output_codec_fingerprints).unwrap();
    let cache = root.path().join("model-format-migrations").join(format!(
        "{}-{:x}",
        located.row.sha256,
        Sha256::digest(codec)
    ));
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("original.dsgrid"), &bytes).unwrap();
    assert!(prepare_open_at(root.path(), &located).is_err());
    assert_eq!(std::fs::read(&located.path).unwrap(), bytes);
    assert!(!cache.join("migration.receipt.json").exists());
}
