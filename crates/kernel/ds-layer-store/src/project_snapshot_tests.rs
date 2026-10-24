use super::*;
use ds_columnar::{schema_digest, ArtifactReader, ConversionStream, Provenance};
use serde_json::json;

fn fixture() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .prefix("snapshot-store-")
        .tempdir_in(dirs::home_dir().unwrap())
        .unwrap()
}
fn point(id: &str, x: f64, revision: &str) -> Value {
    json!({"type":"Feature","id":id,"geometry":{"type":"Point","coordinates":[x,x]},"properties":{"revision":revision}})
}
fn context() -> Context {
    serde_json::from_value(json!({"principal":"reader","lane":"canary","audience":"native-dev","owner":{"kind":"project","project_id":"project-a"},"visibility":"visible-1"})).unwrap()
}
// Independent fixture construction from logical source features: the
// admission path must derive the same digest from actual Parquet bytes.
fn snapshot(
    root: &Path,
    parts: Vec<Vec<Value>>,
    revision: &str,
) -> (Manifest, BTreeMap<String, PathBuf>) {
    let mut chunks = Vec::new();
    let mut inputs = BTreeMap::new();
    let mut features: Vec<Value> = Vec::new();
    let mut schema_sha = "a".repeat(64);
    for part in parts {
        let path = root.join(format!("{}.parquet", uuid::Uuid::new_v4()));
        let mut writer = ConversionStream::create(&path, Provenance::default()).unwrap();
        writer.append(&part, &[]).unwrap();
        writer.finish("poles").unwrap();
        let reader = ArtifactReader::open(&path, 128).unwrap();
        schema_sha = schema_digest(reader.schema()).unwrap();
        let hash = digest_file(&path).unwrap();
        chunks.push(json!({"sha256":hash,"bytes":fs::metadata(&path).unwrap().len(),"rows":part.len(),"artifact":format!("chunk-{hash}")}));
        inputs.insert(hash, path);
        features.extend(part);
    }
    features.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let mut digest = Sha256::new();
    digest.update(b"ds.snapshot-logical-rows/v1\0");
    for feature in &features {
        for value in [
            "poles".to_string(),
            feature["id"].as_str().unwrap().to_string(),
            feature["properties"]["revision"]
                .as_str()
                .unwrap()
                .to_string(),
            canonical_hash(b"ds.snapshot-feature/v1\0", feature).unwrap(),
        ] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    let source_revision = json!({"epoch":"epoch-1","vector":{"survey":revision},"coverage":"coverage-1","change_cursor":revision,"schema_sha256":schema_sha,"vocabulary_revision":"tags-1"});
    let byte_count: u64 = chunks
        .iter()
        .map(|chunk| chunk["bytes"].as_u64().unwrap())
        .sum();
    let mut manifest: Manifest = serde_json::from_value(json!({
        "schema":policy::SCHEMA,"context":context(),"source_kind":"survey","read_source":"bigquery_survey_mirror","selection":"project_complete","acquisition_filter":null,
        "revision":source_revision,
        "layers":[{"name":"poles","geometry":"point","crs":"OGC:CRS84","schema_sha256":schema_sha,"stable_id_column":ds_columnar::FEATURE_ID_COLUMN,"revision_column":"revision","row_count":features.len(),"byte_count":byte_count,"chunks":chunks}],
        "acquisition":{"attestation_id":format!("receipt-{revision}"),"revision":source_revision,"expected_layers":["poles"],"observed_layers":["poles"],"expected_pages":1,"completed_pages":1,"terminal":true,"whole_authorized_scope":true,"watermark_kind":"bigquery_mirror","watermark":revision,"mirror_behind":false,"deletion_history_complete":true},
        "saved_design":null,"original_asset":null,"logical_data_sha256":format!("{:x}",digest.finalize()),"manifest_sha256":"a".repeat(64),"created_at_ms":42
    })).unwrap();
    manifest.manifest_sha256 = manifest.computed_sha256().unwrap();
    (manifest, inputs)
}

#[test]
fn retained_real_artifacts_reuse_unchanged_chunks_and_mask_without_acquisition() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (first, inputs) = snapshot(
        root,
        vec![vec![point("p1", 0.0, "r1")], vec![point("p2", 10.0, "r1")]],
        "head-1",
    );
    let receipt = store.admit(&first, &inputs, None, &|| false).unwrap();
    assert_eq!(receipt.imported_chunks, 2);
    let pin = store.pin(&first.manifest_sha256).unwrap();
    let (second, mut changed) = snapshot(
        root,
        vec![vec![point("p1", 4.0, "r2")], vec![point("p2", 10.0, "r1")]],
        "head-2",
    );
    assert_eq!(
        first.layers[0].chunks[1].sha256,
        second.layers[0].chunks[1].sha256
    );
    changed.remove(&second.layers[0].chunks[1].sha256);
    let receipt = store
        .admit(&second, &changed, Some(&first.manifest_sha256), &|| false)
        .unwrap();
    assert_eq!((receipt.imported_chunks, receipt.reused_chunks), (1, 1));
    let before = store.head().unwrap().unwrap();
    // No input remains available: local visits must consume retained bytes.
    for path in inputs.values().chain(changed.values()) {
        fs::remove_file(path).unwrap();
    }
    let mask = json!({"type":"Polygon","coordinates":[[[-1.0,-1.0],[1.0,-1.0],[1.0,1.0],[-1.0,1.0],[-1.0,-1.0]]]});
    let mut selected = Vec::new();
    assert_eq!(
        pin.visit_layer("poles", Some(&mask), &|| false, &mut |feature| {
            selected.push(feature);
            Ok(())
        })
        .unwrap(),
        1
    );
    assert_eq!(selected[0]["id"], "p1");
    let current = store.pin(&second.manifest_sha256).unwrap();
    assert_eq!(
        current
            .visit_layer("poles", Some(&mask), &|| false, &mut |_| Ok(()))
            .unwrap(),
        0
    );
    assert_eq!(store.head().unwrap().unwrap(), before);
    assert!(
        !store
            .admit(&second, &BTreeMap::new(), Some("stale"), &|| false)
            .unwrap()
            .changed
    );
    assert_eq!(
        pin.visit_layer("poles", None, &|| false, &mut |_| Ok(()))
            .unwrap(),
        2
    );
    let retained = store.retain(&BTreeSet::new()).unwrap();
    assert_eq!(
        (retained.removed_manifests, retained.pinned_manifests),
        (0, 1)
    );
    drop(pin);
    let retired = store.retain(&BTreeSet::new()).unwrap();
    assert_eq!(retired.removed_manifests, 1);
    assert!(retired.removed_chunk_bytes > 0);
    assert!(store.pin(&first.manifest_sha256).is_err());
    assert_eq!(
        current
            .visit_layer("poles", None, &|| false, &mut |_| Ok(()))
            .unwrap(),
        2
    );
}

fn delta(
    base: &Manifest,
    target: &Manifest,
    tombstones: Vec<policy::Tombstone>,
) -> Vec<policy::DeltaPage> {
    let old: BTreeSet<String> = base.layers[0]
        .chunks
        .iter()
        .map(|chunk| chunk.sha256.clone())
        .collect();
    let new: BTreeSet<String> = target.layers[0]
        .chunks
        .iter()
        .map(|chunk| chunk.sha256.clone())
        .collect();
    let first = policy::DeltaPage {
        context: base.context.clone(),
        batch_id: format!("batch-{}", target.revision.change_cursor),
        base_manifest_sha256: base.manifest_sha256.clone(),
        from: base.revision.clone(),
        target: target.revision.clone(),
        page: 0,
        next_page: Some(1),
        changed_chunks: BTreeMap::new(),
        retired_chunks: BTreeMap::from([("poles".into(), old.difference(&new).cloned().collect())]),
        tombstones,
        terminal_manifest: None,
    };
    let last = policy::DeltaPage {
        page: 1,
        next_page: None,
        retired_chunks: BTreeMap::new(),
        tombstones: Vec::new(),
        changed_chunks: BTreeMap::from([(
            "poles".into(),
            target.layers[0]
                .chunks
                .iter()
                .filter(|chunk| !old.contains(&chunk.sha256))
                .cloned()
                .collect(),
        )]),
        terminal_manifest: Some(target.clone()),
        ..first.clone()
    };
    vec![first, last]
}

fn local_area(limit: f64) -> Value {
    json!({"type":"Polygon","coordinates":[[[-limit,-limit],[limit,-limit],[limit,limit],[-limit,limit],[-limit,-limit]]]})
}

#[cfg(unix)]
#[test]
fn optional_sidecar_guard_handles_unlink_without_accepting_links_or_public_files() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let workspace = fixture();
    let path = workspace.path().join("catalog.sqlite-wal");
    let create = || {
        private::open_options()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
    };
    create();
    guard_sqlite_sidecar_if_present_checked(&path, |path| {
        fs::remove_file(path).unwrap();
        snapshot_privacy::file(path)
    })
    .unwrap();
    assert!(fs::symlink_metadata(&path).is_err());
    create();
    let mut calls = 0;
    guard_sqlite_sidecar_if_present_checked(&path, |path| {
        calls += 1;
        if calls == 1 {
            fs::remove_file(path).unwrap();
            private::open_options()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            Err("snapshot privacy: prior opened sidecar was unlinked".into())
        } else {
            snapshot_privacy::file(path)
        }
    })
    .unwrap();
    assert_eq!(calls, 2);
    fs::remove_file(&path).unwrap();
    create();
    assert!(guard_sqlite_sidecar_if_present_checked(&path, |path| {
        fs::remove_file(path).unwrap();
        symlink("missing-target", path).unwrap();
        snapshot_privacy::file(path)
    })
    .is_err());
    // The initial dangling link must refuse even if a checker would remove
    // it. The verification callback must never be reached for that entry.
    assert!(
        guard_sqlite_sidecar_if_present_checked(&path, |_| panic!("link was admitted")).is_err()
    );
    fs::remove_file(&path).unwrap();
    create();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(guard_sqlite_sidecar_if_present(&path).is_err());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&path, workspace.path().join("alias")).unwrap();
    assert!(guard_sqlite_sidecar_if_present(&path).is_err());
}

#[test]
fn concurrent_sqlite_sidecar_lifecycles_keep_shared_snapshot_reads_valid() {
    use std::sync::{Arc, Barrier};
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
    store.admit(&base, &files, None, &|| false).unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let mut threads = Vec::new();
    for _ in 0..4 {
        let root = root.to_owned();
        let barrier = barrier.clone();
        let expected = base.manifest_sha256.clone();
        threads.push(std::thread::spawn(move || {
            let store = Store::open(&root, context(), SourceKind::Survey).unwrap();
            barrier.wait();
            for _ in 0..64 {
                assert_eq!(store.head().unwrap().unwrap().manifest_sha256, expected);
                assert!(store.pending_delta("absent").unwrap().is_none());
            }
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(store.head().unwrap().unwrap(), base);
}
#[cfg(unix)]
#[test]
fn sqlite_sidecars_persist_with_private_identity_after_last_connection_closes() {
    use std::os::unix::fs::MetadataExt;
    let workspace = fixture();
    let store = Store::open(workspace.path(), context(), SourceKind::Survey).unwrap();
    let paths = [
        store.root.join("catalog.sqlite-wal"),
        store.root.join("catalog.sqlite-shm"),
    ];
    let identities: Vec<_> = paths
        .iter()
        .map(|path| {
            snapshot_privacy::file(path).unwrap();
            let m = fs::symlink_metadata(path).unwrap();
            (m.dev(), m.ino())
        })
        .collect();
    for _ in 0..32 {
        assert!(store.head().unwrap().is_none());
        for (path, identity) in paths.iter().zip(&identities) {
            snapshot_privacy::file(path).unwrap();
            let m = fs::symlink_metadata(path).unwrap();
            assert_eq!((m.dev(), m.ino()), *identity);
        }
    }
    assert!(fs::metadata(&paths[0]).unwrap().len() <= 4 * 1024 * 1024);
}

fn read_local(view: &LocalView) -> (Vec<Value>, LocalReadReceipt) {
    let mut features = Vec::new();
    let receipt = view
        .visit_layer("poles", &|| false, &mut |f| {
            features.push(f);
            Ok(())
        })
        .unwrap();
    (features, receipt)
}

#[test]
fn saved_local_masks_reopen_shrink_expand_clear_and_revisit_without_source_changes() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, files) = snapshot(
        root,
        vec![vec![point("p1", 1.0, "r1"), point("p2", 10.0, "r1")]],
        "base",
    );
    store.admit(&base, &files, None, &|| false).unwrap();
    let chunk = store
        .root
        .join("chunks")
        .join(&base.layers[0].chunks[0].sha256);
    let before = fs::read(&chunk).unwrap();
    let mut mask = store
        .set_work_area("desk", Some(&local_area(12.0)), None)
        .unwrap()
        .mask;
    assert_eq!(
        read_local(
            &store
                .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
                .unwrap()
        )
        .0
        .len(),
        2
    );
    drop(store);
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    assert_eq!(store.work_area("desk").unwrap().unwrap(), mask);
    for (area, count) in [
        (Some(local_area(2.0)), 1),
        (Some(local_area(12.0)), 2),
        (None, 2),
        (Some(local_area(2.0)), 1),
    ] {
        mask = store
            .set_work_area("desk", area.as_ref(), Some(&mask.revision))
            .unwrap()
            .mask;
        let view = store
            .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
            .unwrap();
        let (_, receipt) = read_local(&view);
        assert_eq!(receipt.rows, count);
        assert_eq!(receipt.work_area, mask);
        assert_eq!(receipt.source_revision, base.revision);
        assert_eq!(receipt.acquisition, base.acquisition);
        assert_eq!(receipt.selection, base.selection);
        assert_eq!(
            receipt.predicate,
            if area.is_some() {
                ViewPredicate::Intersects
            } else {
                ViewPredicate::AllRetainedRows
            }
        );
    }
    assert_eq!(store.head().unwrap().unwrap(), base);
    assert_eq!(fs::read(chunk).unwrap(), before);
    assert_eq!(
        base.layers[0].chunks[0].sha256,
        digest_file(files.values().next().unwrap()).unwrap()
    );
    assert!(store.remove_work_area("desk", &mask.revision).unwrap());
    assert!(store.work_area("desk").unwrap().is_none());
    assert!(store
        .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
        .is_err());
}

#[test]
fn captured_local_view_survives_mask_changes_source_moves_deletions_and_retention() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, files) = snapshot(
        root,
        vec![vec![point("p1", 1.0, "r1"), point("p2", 10.0, "r1")]],
        "base",
    );
    store.admit(&base, &files, None, &|| false).unwrap();
    let first = store
        .set_work_area("desk", Some(&local_area(2.0)), None)
        .unwrap()
        .mask;
    let captured = store
        .capture_view("desk", store.pin(&base.manifest_sha256).unwrap())
        .unwrap();
    let expanded = store
        .set_work_area("desk", Some(&local_area(25.0)), Some(&first.revision))
        .unwrap()
        .mask;
    let (target, files) = snapshot(root, vec![vec![point("p1", 20.0, "r2")]], "target");
    let pages = delta(
        &base,
        &target,
        vec![policy::Tombstone {
            layer: "poles".into(),
            object_id: "p2".into(),
            object_revision: "r2".into(),
        }],
    );
    store.apply_delta(&pages, &files, &|| false).unwrap();
    assert_eq!(store.retain(&BTreeSet::new()).unwrap().pinned_manifests, 1);
    let (old, receipt) = read_local(&captured);
    assert_eq!(old, vec![point("p1", 1.0, "r1")]);
    assert_eq!(receipt.work_area, first);
    assert_eq!(receipt.manifest_sha256, base.manifest_sha256);
    let new = store
        .capture_view("desk", store.pin(&target.manifest_sha256).unwrap())
        .unwrap();
    let (moved, receipt) = read_local(&new);
    assert_eq!(moved, vec![point("p1", 20.0, "r2")]);
    assert_eq!(receipt.work_area, expanded);
    assert_eq!(receipt.source_revision, target.revision);
    let shrinking = store
        .set_work_area("desk", Some(&local_area(2.0)), Some(&expanded.revision))
        .unwrap()
        .mask;
    let empty = store
        .capture_view("desk", store.pin(&target.manifest_sha256).unwrap())
        .unwrap();
    assert_eq!(read_local(&empty).1.rows, 0);
    assert_eq!(read_local(&new).0, moved);
    assert_eq!(empty.work_area(), &shrinking);
    assert!(new.visit_layer("poles", &|| true, &mut |_| Ok(())).is_err());
    assert_eq!(store.head().unwrap().unwrap(), target);
}

#[test]
fn local_view_receipt_preserves_filtered_scope_and_refuses_cross_namespace_pins() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (mut partial, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "partial");
    partial.selection = policy::SelectionScope::Filtered;
    partial.acquisition_filter = Some("opaque-filter".into());
    partial.acquisition.whole_authorized_scope = false;
    partial.acquisition.mirror_behind = true;
    partial.manifest_sha256 = partial.computed_sha256().unwrap();
    store.admit(&partial, &files, None, &|| false).unwrap();
    store
        .set_work_area("desk", Some(&local_area(0.5)), None)
        .unwrap();
    let (_, receipt) = read_local(
        &store
            .capture_view("desk", store.pin(&partial.manifest_sha256).unwrap())
            .unwrap(),
    );
    assert_eq!(receipt.rows, 0);
    assert_eq!(receipt.selection, policy::SelectionScope::Filtered);
    assert_eq!(receipt.acquisition_filter, partial.acquisition_filter);
    assert_eq!(receipt.acquisition, partial.acquisition);
    assert_eq!(receipt.source_revision.coverage, partial.revision.coverage);
    let mut variants = Vec::new();
    let mut other = context();
    other.principal = "other".into();
    variants.push((other, SourceKind::Survey));
    let mut other = context();
    other.lane = policy::Lane::Stable;
    variants.push((other, SourceKind::Survey));
    let mut other = context();
    other.owner = policy::Owner::Project {
        project_id: "other".into(),
    };
    variants.push((other, SourceKind::Survey));
    let mut other = context();
    other.visibility = "other".into();
    variants.push((other, SourceKind::Survey));
    variants.push((context(), SourceKind::SavedDesign));
    let mut other = context();
    other.audience = "other".into();
    variants.push((other, SourceKind::Survey));
    for (ctx, source) in variants {
        let other = Store::open(root, ctx, source).unwrap();
        assert!(other.work_area("desk").unwrap().is_none());
        assert_eq!(
            other
                .capture_view("desk", store.pin(&partial.manifest_sha256).unwrap())
                .err()
                .unwrap()
                .code,
            RefusalCode::SnapshotContextMismatch
        );
    }
}

#[test]
fn local_mask_cas_rejects_aba_recreation_and_two_writer_races() {
    use std::sync::{Arc, Barrier};
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let a = store
        .set_work_area("desk", Some(&local_area(2.0)), None)
        .unwrap()
        .mask;
    assert!(
        !store
            .set_work_area("desk", Some(&local_area(2.0)), Some(&a.revision))
            .unwrap()
            .changed
    );
    assert_eq!(
        store
            .set_work_area("desk", Some(&local_area(2.0)), None)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotStaleView
    );
    let b = store
        .set_work_area("desk", None, Some(&a.revision))
        .unwrap()
        .mask;
    let again = store
        .set_work_area("desk", Some(&local_area(2.0)), Some(&b.revision))
        .unwrap()
        .mask;
    assert_eq!(a.content_sha256, again.content_sha256);
    assert_ne!(a.revision, again.revision);
    assert_eq!(
        store
            .set_work_area("desk", Some(&local_area(2.0)), Some(&a.revision))
            .unwrap_err()
            .code,
        RefusalCode::SnapshotStaleView
    );
    assert!(store.remove_work_area("desk", &a.revision).is_err());
    store.remove_work_area("desk", &again.revision).unwrap();
    drop(store);
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let recreated = store
        .set_work_area("desk", Some(&local_area(2.0)), None)
        .unwrap()
        .mask;
    assert_ne!(again.revision, recreated.revision);
    assert!(recreated.generation > again.generation);
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for limit in [3.0, 4.0] {
        let root = root.to_owned();
        let barrier = barrier.clone();
        let expected = recreated.revision.clone();
        threads.push(std::thread::spawn(move || {
            let store = Store::open(&root, context(), SourceKind::Survey).unwrap();
            barrier.wait();
            store.set_work_area("desk", Some(&local_area(limit)), Some(&expected))
        }));
    }
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r,Err(e) if e.code==RefusalCode::SnapshotStaleView))
            .count(),
        1
    );
}

#[test]
fn local_mask_budgets_malformed_inputs_and_clock_failure_are_atomic() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let first = store.set_work_area("desk", None, None).unwrap().mask;
    for bad in [
        json!({"type":"LineString","coordinates":[[0,0],[1,1]]}),
        json!({"type":"Polygon","coordinates":[]}),
    ] {
        assert!(store
            .set_work_area("desk", Some(&bad), Some(&first.revision))
            .is_err());
    }
    assert!(store.set_work_area(&"x".repeat(257), None, None).is_err());
    let mut deep = Value::Null;
    for _ in 0..34 {
        deep = json!([deep]);
    }
    assert_eq!(
        store
            .set_work_area("deep", Some(&deep), None)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotBudgetExceeded
    );
    let too_large = json!({"type":"Feature","geometry":local_area(2.0),"properties":{"padding":"x".repeat(300_000)}});
    assert_eq!(
        store
            .set_work_area("large", Some(&too_large), None)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotBudgetExceeded
    );
    let large = json!({"type":"Feature","geometry":local_area(2.0),"properties":{"padding":"x".repeat(250_000)}});
    for i in 0..16 {
        store
            .set_work_area(&format!("mask-{i}"), Some(&large), None)
            .unwrap();
    }
    assert_eq!(
        store
            .set_work_area("overflow", Some(&large), None)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotBudgetExceeded
    );
    for i in 0..16 {
        let key = format!("mask-{i}");
        let mask = store.work_area(&key).unwrap().unwrap();
        store.remove_work_area(&key, &mask.revision).unwrap();
    }
    for i in 0..127 {
        store
            .set_work_area(&format!("clear-{i}"), None, None)
            .unwrap();
    }
    assert_eq!(
        store
            .set_work_area("overflow", None, None)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotBudgetExceeded
    );
    assert_eq!(store.work_area("desk").unwrap().unwrap(), first);
    let _lock = store.lock(true).unwrap();
    let c = store.connection().unwrap();
    c.execute(
        "UPDATE local_work_area_clock SET generation=?1,sha=?2 WHERE id=1",
        params![
            i64::MAX,
            local_view::clock_digest(&store.namespace_sha256().unwrap(), i64::MAX as u64).unwrap()
        ],
    )
    .unwrap();
    drop(c);
    drop(_lock);
    assert_eq!(
        store
            .set_work_area("desk", Some(&local_area(2.0)), Some(&first.revision))
            .unwrap_err()
            .code,
        RefusalCode::SnapshotBudgetExceeded
    );
    assert_eq!(store.work_area("desk").unwrap().unwrap(), first);
}

#[test]
fn local_mask_corrupt_clock_refuses_without_repair_or_mask_mutation() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let first = store.set_work_area("desk", None, None).unwrap().mask;
    let _lock = store.lock(true).unwrap();
    let c = store.connection().unwrap();
    c.execute(
        "UPDATE local_work_area_clock SET generation=0 WHERE id=1",
        [],
    )
    .unwrap();
    drop(_lock);
    assert_eq!(
        store
            .set_work_area("desk", Some(&local_area(2.0)), Some(&first.revision))
            .unwrap_err()
            .code,
        RefusalCode::SnapshotCorrupt
    );
    assert_eq!(
        c.query_row(
            "SELECT revision FROM local_work_areas WHERE name='desk'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        first.revision
    );
    assert_eq!(
        c.query_row(
            "SELECT generation FROM local_work_area_clock WHERE id=1",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    c.execute("DELETE FROM local_work_area_clock WHERE id=1", [])
        .unwrap();
    assert!(Store::open(root, context(), SourceKind::Survey).is_err());
    assert_eq!(
        c.query_row("SELECT count(*) FROM local_work_area_clock", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn delta_metadata_reopens_revalidates_files_and_replays_after_base_retention() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
    store.admit(&base, &files, None, &|| false).unwrap();
    let (target, files) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
    let pages = delta(&base, &target, vec![]);
    let batch = &pages[0].batch_id;
    assert!(store.stage_delta_page(&pages[1]).is_err());
    let first = store.stage_delta_page(&pages[0]).unwrap();
    assert_eq!(first.state, DeltaAcquisitionState::Acquiring);
    assert_eq!(store.stage_delta_page(&pages[0]).unwrap(), first);
    let mut conflict = pages[0].clone();
    conflict.retired_chunks.clear();
    assert!(store.stage_delta_page(&conflict).is_err());
    assert_eq!(store.pending_delta(batch).unwrap().unwrap(), first);
    assert!(store.finish_delta(batch, &files, &|| false).is_err());
    drop(store);
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    assert_eq!(
        store.pending_delta_pages(batch).unwrap(),
        vec![pages[0].clone()]
    );
    let ready = store.stage_delta_page(&pages[1]).unwrap();
    assert_eq!(ready.state, DeltaAcquisitionState::Ready);
    assert!(store
        .finish_delta(batch, &BTreeMap::new(), &|| false)
        .is_err());
    assert!(store.finish_delta(batch, &files, &|| true).is_err());
    let cancellation_checks = std::cell::Cell::new(0usize);
    assert!(store
        .finish_delta(batch, &files, &|| {
            let n = cancellation_checks.get() + 1;
            cancellation_checks.set(n);
            n >= 5
        })
        .is_err());
    assert!(cancellation_checks.get() >= 5);
    assert_eq!(store.pending_delta(batch).unwrap().unwrap(), ready);
    assert_eq!(store.head().unwrap().unwrap(), base);
    assert!(
        store
            .finish_delta(batch, &files, &|| false)
            .unwrap()
            .changed
    );
    assert!(store.pending_delta(batch).unwrap().is_none());
    assert!(store.stage_delta_page(&pages[0]).is_err());
    store.retain(&BTreeSet::new()).unwrap();
    assert!(store.pin(&base.manifest_sha256).is_err());
    // A receipt is sufficient after the base is retired; no file reads.
    assert!(
        !store
            .apply_delta(&pages, &BTreeMap::new(), &|| false)
            .unwrap()
            .changed
    );
    let mut mismatch = pages.clone();
    mismatch[0].retired_chunks.clear();
    assert!(store.apply_delta(&mismatch, &files, &|| false).is_err());
    assert_eq!(store.head().unwrap().unwrap(), target);
    let (advanced, files) = snapshot(root, vec![vec![point("p1", 3.0, "r3")]], "advanced");
    store
        .admit(&advanced, &files, Some(&target.manifest_sha256), &|| false)
        .unwrap();
    assert!(store
        .apply_delta(&pages, &BTreeMap::new(), &|| false)
        .is_err());
    assert_eq!(store.head().unwrap().unwrap(), advanced);
}

#[test]
fn pending_delta_limits_discard_and_retention_are_metadata_only() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
    store.admit(&base, &files, None, &|| false).unwrap();
    let (target, files) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
    let pages = delta(&base, &target, vec![]);
    for i in 0..8 {
        let mut page = pages[0].clone();
        page.batch_id = format!("pending-{i}");
        store.stage_delta_page(&page).unwrap();
    }
    let mut extra = pages[0].clone();
    extra.batch_id = "overflow".into();
    assert!(store.stage_delta_page(&extra).is_err());
    let mut oversized = pages[1].clone();
    oversized.batch_id = "pending-0".into();
    oversized.changed_chunks.get_mut("poles").unwrap()[0].artifact = "x".repeat(1024 * 1024);
    assert!(store.stage_delta_page(&oversized).is_err());
    let mut high_index = pages[0].clone();
    high_index.page = u64::MAX;
    assert!(store.stage_delta_page(&high_index).is_err());
    assert!(store.discard_delta("pending-7").unwrap());
    assert!(!store.discard_delta("pending-7").unwrap());
    store
        .admit(&target, &files, Some(&base.manifest_sha256), &|| false)
        .unwrap();
    let retained = store.retain(&BTreeSet::new()).unwrap();
    assert_eq!(retained.pinned_manifests, 1);
    assert!(store.pin(&base.manifest_sha256).is_ok());
    // An ordinary admission cannot create an unearned delta receipt.
    assert!(store.apply_delta(&pages, &files, &|| false).is_err());
    for i in 0..7 {
        assert!(store.discard_delta(&format!("pending-{i}")).unwrap());
    }
    store.retain(&BTreeSet::new()).unwrap();
    assert!(store.pin(&base.manifest_sha256).is_err());
    assert!(files.values().all(|path| path.exists()));
    assert_eq!(store.head().unwrap().unwrap(), target);
}

#[test]
fn simultaneous_delta_callers_commit_one_receipt_without_rollback() {
    use std::sync::{Arc, Barrier};
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
    store.admit(&base, &files, None, &|| false).unwrap();
    let (target, files) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
    let pages = delta(&base, &target, vec![]);
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let root = root.to_owned();
        let pages = pages.clone();
        let files = files.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let store = Store::open(&root, context(), SourceKind::Survey).unwrap();
            barrier.wait();
            store
                .apply_delta(&pages, &files, &|| false)
                .unwrap()
                .changed
        }));
    }
    let changed: usize = threads
        .into_iter()
        .map(|t| usize::from(t.join().unwrap()))
        .sum();
    assert_eq!(changed, 1);
    assert_eq!(store.head().unwrap().unwrap(), target);
    let _lock = store.lock(false).unwrap();
    let c = store.connection().unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM delta_receipts", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM delta_receipt_pages", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn completed_delta_receipt_history_is_bounded() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (mut base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
    store.admit(&base, &files, None, &|| false).unwrap();
    let mut oldest = Vec::new();
    for i in 0..65 {
        // Only the acquisition revision changes; no repeated file import.
        let (target, files) = snapshot(
            root,
            vec![vec![point("p1", 1.0, "r1")]],
            &format!("head-{i}"),
        );
        let pages = delta(&base, &target, vec![]);
        if i == 0 {
            oldest = pages.clone();
        }
        assert!(
            store
                .apply_delta(&pages, &files, &|| false)
                .unwrap()
                .changed
        );
        base = target;
    }
    assert!(store
        .apply_delta(&oldest, &BTreeMap::new(), &|| false)
        .is_err());
    let _lock = store.lock(false).unwrap();
    let c = store.connection().unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM delta_receipts", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        64
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM delta_receipt_pages", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        128
    );
}

#[test]
fn staged_page_count_is_bounded_and_duplicate_tombstones_do_not_poison_cursor() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, files) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "base");
    store.admit(&base, &files, None, &|| false).unwrap();
    let (target, _) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "target");
    let mut page = delta(&base, &target, vec![])[0].clone();
    page.retired_chunks.clear();
    let tombstone = policy::Tombstone {
        layer: "poles".into(),
        object_id: "missing".into(),
        object_revision: "r2".into(),
    };
    page.tombstones = vec![tombstone.clone()];
    store.stage_delta_page(&page).unwrap();
    page.page = 1;
    page.next_page = Some(2);
    assert!(store.stage_delta_page(&page).is_err());
    assert_eq!(
        store
            .pending_delta(&page.batch_id)
            .unwrap()
            .unwrap()
            .next_page,
        Some(1)
    );
    page.tombstones.clear();
    for i in 1..255 {
        page.page = i;
        page.next_page = Some(i + 1);
        store.stage_delta_page(&page).unwrap();
    }
    let before = store.pending_delta(&page.batch_id).unwrap().unwrap();
    page.page = 255;
    page.next_page = Some(256);
    assert_eq!(
        store.stage_delta_page(&page).unwrap_err().code,
        RefusalCode::SnapshotBudgetExceeded
    );
    assert_eq!(
        store.pending_delta(&page.batch_id).unwrap().unwrap(),
        before
    );
    page.page = 256;
    page.next_page = Some(257);
    assert_eq!(
        store.stage_delta_page(&page).unwrap_err().code,
        RefusalCode::SnapshotBudgetExceeded
    );
    assert_eq!(
        store.pending_delta(&page.batch_id).unwrap().unwrap(),
        before
    );
    assert_eq!(store.head().unwrap().unwrap(), base);
}

#[test]
fn ordered_delta_deletion_restore_and_move_match_complete_artifacts() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (first, inputs) = snapshot(
        root,
        vec![vec![point("p1", 1.0, "r1")], vec![point("p2", 10.0, "r1")]],
        "head-1",
    );
    store.admit(&first, &inputs, None, &|| false).unwrap();
    let (deleted, _) = snapshot(root, vec![vec![point("p2", 10.0, "r1")]], "head-2");
    let pages = delta(&first, &deleted, Vec::new());
    assert!(store
        .apply_delta(&pages, &BTreeMap::new(), &|| false)
        .unwrap_err()
        .reason
        .contains("no delta tombstone"));
    assert_eq!(store.head().unwrap().unwrap(), first);
    let pages = delta(
        &first,
        &deleted,
        vec![policy::Tombstone {
            layer: "poles".into(),
            object_id: "p1".into(),
            object_revision: "r2".into(),
        }],
    );
    let receipt = store
        .apply_delta(&pages, &BTreeMap::new(), &|| false)
        .unwrap();
    assert_eq!((receipt.imported_chunks, receipt.reused_chunks), (0, 1));
    assert!(
        !store
            .apply_delta(&pages, &BTreeMap::new(), &|| false)
            .unwrap()
            .changed
    );
    let mut base = deleted;
    for (head, x, revision) in [("head-3", 4.0, "r3"), ("head-4", 5.0, "r4")] {
        let expected = vec![point("p1", x, revision), point("p2", 10.0, "r1")];
        let (target, mut input) = snapshot(
            root,
            expected
                .iter()
                .cloned()
                .map(|feature| vec![feature])
                .collect(),
            head,
        );
        for chunk in &base.layers[0].chunks {
            input.remove(&chunk.sha256);
        }
        let receipt = store
            .apply_delta(&delta(&base, &target, Vec::new()), &input, &|| false)
            .unwrap();
        assert_eq!((receipt.imported_chunks, receipt.reused_chunks), (1, 1));
        let mut observed = Vec::new();
        store
            .pin(&target.manifest_sha256)
            .unwrap()
            .visit_layer("poles", None, &|| false, &mut |feature| {
                observed.push(feature);
                Ok(())
            })
            .unwrap();
        assert_eq!(observed, expected);
        assert_eq!(store.head().unwrap().unwrap(), target);
        base = target;
    }
}

#[test]
fn malformed_delta_assembly_never_changes_a_native_head() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (base, input) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "head-1");
    store.admit(&base, &input, None, &|| false).unwrap();
    let (target, input) = snapshot(root, vec![vec![point("p1", 2.0, "r2")]], "head-2");
    let valid = delta(&base, &target, Vec::new());
    for kind in 0..5 {
        let mut bad = valid.clone();
        match kind {
            0 => bad.reverse(),
            1 => {
                bad[1].batch_id = "other-batch".into();
            }
            2 => {
                bad[0].retired_chunks.clear();
            }
            3 => {
                bad[1].changed_chunks.clear();
            }
            _ => {
                bad[1].tombstones.push(policy::Tombstone {
                    layer: "poles".into(),
                    object_id: "p1".into(),
                    object_revision: "r3".into(),
                });
            }
        }
        assert!(
            store.apply_delta(&bad, &input, &|| false).is_err(),
            "case {kind}"
        );
        assert_eq!(store.head().unwrap().unwrap(), base);
    }
    assert!(
        store
            .apply_delta(&valid, &input, &|| false)
            .unwrap()
            .changed
    );
}

#[test]
fn cancellation_recovers_verified_orphans_without_a_source_refetch() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let workspace = fixture();
    let store = Store::open(workspace.path(), context(), SourceKind::Survey).unwrap();
    let (manifest, inputs) = snapshot(
        workspace.path(),
        vec![vec![point("p1", 1.0, "r1")]],
        "head-1",
    );
    let calls = AtomicUsize::new(0);
    assert!(store
        .admit(&manifest, &inputs, None, &|| calls
            .fetch_add(1, Ordering::Relaxed)
            >= 4)
        .is_err());
    assert!(store.head().unwrap().is_none());
    let chunk = &manifest.layers[0].chunks[0];
    assert!(store.root.join("chunks").join(&chunk.sha256).exists());
    for path in inputs.values() {
        fs::remove_file(path).unwrap();
    }
    assert!(
        store
            .admit(&manifest, &BTreeMap::new(), None, &|| false)
            .unwrap()
            .changed
    );
    assert_eq!(store.head().unwrap().unwrap(), manifest);
}

#[test]
fn corruption_stale_heads_and_duplicate_objects_never_advance_the_cursor() {
    let workspace = fixture();
    let root = workspace.path();
    let store = Store::open(root, context(), SourceKind::Survey).unwrap();
    let (manifest, inputs) = snapshot(root, vec![vec![point("p1", 1.0, "r1")]], "head-1");
    let path = inputs.values().next().unwrap();
    let bytes = fs::read(path).unwrap();
    fs::write(path, b"corrupt").unwrap();
    assert_eq!(
        store
            .admit(&manifest, &inputs, None, &|| false)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotCorrupt
    );
    assert!(store.head().unwrap().is_none());
    fs::write(path, bytes).unwrap();
    store.admit(&manifest, &inputs, None, &|| false).unwrap();
    let (duplicate, inputs) = snapshot(
        root,
        vec![vec![point("p1", 1.0, "r1")], vec![point("p1", 2.0, "r2")]],
        "head-2",
    );
    assert!(store
        .admit(
            &duplicate,
            &inputs,
            Some(&manifest.manifest_sha256),
            &|| false
        )
        .unwrap_err()
        .reason
        .contains("duplicate"));
    assert_eq!(store.head().unwrap().unwrap(), manifest);
    assert_eq!(
        store
            .admit(&duplicate, &inputs, Some("stale"), &|| false)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotStaleDelta
    );
    let mut other = context();
    other.visibility = "restricted".into();
    let other_store = Store::open(root, other, SourceKind::Survey).unwrap();
    assert!(other_store.head().unwrap().is_none());
    assert_eq!(
        other_store
            .admit(&manifest, &BTreeMap::new(), None, &|| false)
            .unwrap_err()
            .code,
        RefusalCode::SnapshotContextMismatch
    );
    let retained = store
        .root
        .join("chunks")
        .join(&manifest.layers[0].chunks[0].sha256);
    let mut corrupt = fs::read(&retained).unwrap();
    corrupt[0] ^= 1;
    fs::write(&retained, corrupt).unwrap();
    assert_eq!(
        store
            .pin(&manifest.manifest_sha256)
            .unwrap()
            .visit_layer("poles", None, &|| false, &mut |_| Ok(()))
            .unwrap_err()
            .code,
        RefusalCode::SnapshotCorrupt
    );
}
