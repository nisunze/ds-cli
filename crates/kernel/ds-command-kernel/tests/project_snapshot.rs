use ds_command_kernel::project_snapshot::*;
use serde_json::{json, Value};

fn document() -> Value {
    let hash = "a".repeat(64);
    let revision = json!({"epoch":"epoch-1","vector":{"survey":"head-7"},"coverage":"coverage-3", "change_cursor":"cursor-7", "schema_sha256":hash,"vocabulary_revision":"tags-2"});
    json!({
        "schema":SCHEMA,
        "context":{"principal":"reader","lane":"canary","audience":"native-dev","owner":{"kind":"project","project_id":"project-a"},"visibility":"all-visible-1"},
        "source_kind":"survey","read_source":"bigquery_survey_mirror","selection":"project_complete","acquisition_filter":null,
        "revision":revision,
        "layers":[{"name":"poles","geometry":"point","crs":"EPSG:4326","schema_sha256":hash,"stable_id_column":"object_id","revision_column":"object_revision","row_count":2,"byte_count":512,
            "chunks":[{"sha256":hash,"bytes":512,"rows":2,"artifact":"chunk-1"}]}],
        "acquisition":{"attestation_id":"server-receipt-7","revision":revision,"expected_layers":["poles"],"observed_layers":["poles"],"expected_pages":2,"completed_pages":2,"terminal":true,"whole_authorized_scope":true,"watermark_kind":"bigquery_mirror","watermark":"mirror-ledger-7","mirror_behind":false,"deletion_history_complete":true},
        "saved_design":null,"original_asset":null,"logical_data_sha256":hash,"manifest_sha256":hash,"created_at_ms":42
    })
}
fn decode(value: Value) -> Manifest {
    serde_json::from_value(value).unwrap()
}

#[test]
fn manifest_identity_binds_all_captured_fields_and_refuses_mutation() {
    let mut manifest = decode(document());
    assert_eq!(
        manifest.verify_sha256().unwrap_err().code,
        RefusalCode::SnapshotCorrupt
    );
    manifest.manifest_sha256 = manifest.computed_sha256().unwrap();
    manifest.verify_sha256().unwrap();
    let digest = manifest.manifest_sha256.clone();
    manifest.created_at_ms += 1;
    assert_ne!(digest, manifest.computed_sha256().unwrap());
    assert_eq!(
        manifest.verify_sha256().unwrap_err().code,
        RefusalCode::SnapshotCorrupt
    );
    manifest.created_at_ms -= 1;
    manifest.context.principal = "other".into();
    assert_ne!(digest, manifest.computed_sha256().unwrap());
    manifest.context.principal = "reader".into();
    manifest.layers[0].chunks[0].artifact = "replacement".into();
    assert_ne!(digest, manifest.computed_sha256().unwrap());
}
fn code(value: Value) -> RefusalCode {
    let manifest = decode(value);
    manifest.validate(&manifest.context).unwrap_err().code
}

#[test]
fn survey_cannot_select_design_authority_and_design_requires_no_bigquery_gate() {
    let valid = decode(document());
    assert!(valid.complete_project_candidate(&valid.context).unwrap());
    let mut bad = document();
    bad["read_source"] = json!("saved_firestore_and_dsgrid");
    assert_eq!(code(bad), RefusalCode::SnapshotSourcePolicy);
    let mut design = document();
    design["source_kind"] = json!("saved_design");
    design["read_source"] = json!("saved_firestore_and_dsgrid");
    design["acquisition"]["watermark_kind"] = json!("authoritative");
    design["saved_design"] = json!({"saved_models":{"model-1":"b".repeat(64)},"transformer_revisions":{"t1":"rev-4"},"tag_revision":"tags-2","unsaved":false});
    let manifest = decode(design.clone());
    assert!(manifest
        .complete_project_candidate(&manifest.context)
        .unwrap());
    design["read_source"] = json!("bigquery_survey_mirror");
    assert_eq!(code(design), RefusalCode::SnapshotSourcePolicy);
}

#[test]
fn filtered_or_permission_limited_receipts_cannot_replace_whole_project() {
    let mut filtered = document();
    filtered["selection"] = json!("filtered");
    filtered["acquisition_filter"] = json!("form_id = poles");
    filtered["acquisition"]["whole_authorized_scope"] = json!(false);
    let manifest = decode(filtered.clone());
    assert!(!manifest
        .complete_project_candidate(&manifest.context)
        .unwrap());
    filtered["selection"] = json!("project_complete");
    filtered["acquisition_filter"] = json!(null);
    assert_eq!(code(filtered), RefusalCode::SnapshotIncomplete);
    let mut empty_filter = document();
    empty_filter["acquisition_filter"] = json!("");
    assert_eq!(code(empty_filter), RefusalCode::SnapshotIncomplete);
}

#[test]
fn pagination_layer_coverage_and_deletion_history_are_terminal_obligations() {
    for (field, value) in [
        ("completed_pages", json!(1)),
        ("terminal", json!(false)),
        ("observed_layers", json!([])),
        ("deletion_history_complete", json!(false)),
    ] {
        let mut bad = document();
        bad["acquisition"][field] = value;
        assert_eq!(code(bad), RefusalCode::SnapshotIncomplete, "{field}");
    }
    let mut bad = document();
    bad["layers"] = json!([]);
    assert_eq!(code(bad), RefusalCode::SnapshotIncomplete);
}

#[test]
fn zero_rows_and_zero_layers_are_valid_when_authoritatively_attested() {
    let mut empty = document();
    empty["layers"][0]["row_count"] = json!(0);
    empty["layers"][0]["byte_count"] = json!(0);
    empty["layers"][0]["chunks"] = json!([]);
    let manifest = decode(empty.clone());
    assert!(manifest
        .complete_project_candidate(&manifest.context)
        .unwrap());
    empty["layers"] = json!([]);
    empty["acquisition"]["expected_layers"] = json!([]);
    empty["acquisition"]["observed_layers"] = json!([]);
    empty["acquisition"]["expected_pages"] = json!(0);
    empty["acquisition"]["completed_pages"] = json!(0);
    let manifest = decode(empty);
    assert!(manifest
        .complete_project_candidate(&manifest.context)
        .unwrap());
}

#[test]
fn moving_heads_and_mirror_lag_are_never_reported_current() {
    let mut moving = document();
    moving["acquisition"]["revision"]["change_cursor"] = json!("cursor-8");
    assert_eq!(code(moving), RefusalCode::SnapshotMovingHead);
    let mut lag = document();
    lag["acquisition"]["mirror_behind"] = json!(true);
    let manifest = decode(lag);
    manifest.validate(&manifest.context).unwrap();
    assert!(!manifest
        .complete_project_candidate(&manifest.context)
        .unwrap());
    let mut wrong = document();
    wrong["acquisition"]["watermark_kind"] = json!("authoritative");
    assert_eq!(code(wrong), RefusalCode::SnapshotSourcePolicy);
}

#[test]
fn account_lane_audience_project_and_visibility_are_separate_fences() {
    let manifest = decode(document());
    let contexts = [
        Context {
            principal: "other".into(),
            ..manifest.context.clone()
        },
        Context {
            lane: Lane::Stable,
            ..manifest.context.clone()
        },
        Context {
            audience: "other".into(),
            ..manifest.context.clone()
        },
        Context {
            owner: Owner::Project {
                project_id: "other".into(),
            },
            ..manifest.context.clone()
        },
        Context {
            visibility: "restricted".into(),
            ..manifest.context.clone()
        },
    ];
    for context in contexts {
        assert_eq!(
            manifest.validate(&context).unwrap_err().code,
            RefusalCode::SnapshotContextMismatch
        );
    }
}

#[test]
fn personal_upload_retains_original_identity_without_project_replacement_authority() {
    let mut upload = document();
    upload["context"]["owner"] =
        json!({"kind":"personal","project_id":"project-a","owner_id":"reader"});
    upload["source_kind"] = json!("geographic_upload");
    upload["read_source"] = json!("original_asset");
    upload["selection"] = json!("principal_complete");
    upload["acquisition"]["watermark_kind"] = json!("authoritative");
    upload["original_asset"] =
        json!({"asset_id":"upload-a","generation":"2","sha256":"c".repeat(64),"bytes":20});
    let manifest = decode(upload.clone());
    assert!(!manifest
        .complete_project_candidate(&manifest.context)
        .unwrap());
    upload["context"]["owner"]["owner_id"] = json!("another-user");
    assert_eq!(code(upload), RefusalCode::SnapshotContextMismatch);
}

#[test]
fn duplicate_layers_bad_chunks_counts_unknown_crs_and_missing_saved_identity_refuse() {
    let mut duplicate = document();
    let layer = duplicate["layers"][0].clone();
    duplicate["layers"].as_array_mut().unwrap().push(layer);
    assert_eq!(code(duplicate), RefusalCode::SnapshotInvalid);
    for (field, value) in [("row_count", json!(1)), ("byte_count", json!(513))] {
        let mut mismatch = document();
        mismatch["layers"][0][field] = value;
        assert_eq!(code(mismatch), RefusalCode::SnapshotCorrupt);
    }
    let mut wrong = document();
    wrong["layers"][0]["crs"] = json!("unknown");
    assert_eq!(code(wrong), RefusalCode::SnapshotInvalid);
    let mut wrong = document();
    wrong["layers"][0]["chunks"][0]["sha256"] = json!("not-a-digest");
    assert_eq!(code(wrong), RefusalCode::SnapshotInvalid);
}

#[test]
fn wire_schema_is_closed_and_generated_from_the_rust_types() {
    let mut bad = document();
    bad["source_url"] = json!("https://arbitrary.example");
    assert!(serde_json::from_value::<Manifest>(bad).is_err());
    let schema = serde_json::to_value(schemars::schema_for!(Manifest)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema["$defs"].get("Revision").is_some());
}

fn delta(base: &Manifest) -> DeltaPage {
    let mut terminal = base.clone();
    terminal.revision.change_cursor = "cursor-8".into();
    terminal
        .revision
        .vector
        .insert("survey".into(), "head-8".into());
    terminal.acquisition.revision = terminal.revision.clone();
    terminal.manifest_sha256 = "b".repeat(64);
    DeltaPage {
        context: base.context.clone(),
        batch_id: "batch-1".into(),
        base_manifest_sha256: base.manifest_sha256.clone(),
        from: base.revision.clone(),
        target: terminal.revision.clone(),
        page: 0,
        next_page: None,
        changed_chunks: Default::default(),
        retired_chunks: Default::default(),
        tombstones: vec![],
        terminal_manifest: Some(terminal),
    }
}

#[test]
fn deltas_are_bound_to_exact_base_authority_order_and_terminal_target() {
    let base = decode(document());
    let valid = delta(&base);
    valid.validate(&base, 0).unwrap();
    let mut bad = valid.clone();
    bad.page = 1;
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotStaleDelta
    );
    let mut bad = valid.clone();
    bad.base_manifest_sha256 = "f".repeat(64);
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotStaleDelta
    );
    let mut bad = valid.clone();
    bad.context.principal = "other".into();
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotContextMismatch
    );
    let mut bad = valid.clone();
    bad.target.epoch = "reset".into();
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotResnapshotRequired
    );
    let mut bad = valid.clone();
    bad.terminal_manifest
        .as_mut()
        .unwrap()
        .revision
        .change_cursor = "cursor-9".into();
    bad.terminal_manifest
        .as_mut()
        .unwrap()
        .acquisition
        .revision
        .change_cursor = "cursor-9".into();
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotMovingHead
    );
    let mut bad = valid;
    bad.next_page = Some(1);
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotIncomplete
    );
}

#[test]
fn missing_billing_evidence_stays_unknown_instead_of_claiming_zero_cost() {
    let metrics = AcquisitionMetrics::default();
    let value = serde_json::to_value(metrics).unwrap();
    assert!(value["bigquery_billed_bytes"].is_null());
    assert!(value["firestore_document_reads"].is_null());
    assert_eq!(value["source_requests"], 0);
}

#[test]
fn delta_admission_rejects_corrupt_changes_and_repeated_tombstones() {
    let base = decode(document());
    let mut bad = delta(&base);
    bad.changed_chunks.insert(
        "poles".into(),
        vec![Chunk {
            sha256: "invalid".into(),
            bytes: 1,
            rows: 1,
            artifact: "chunk".into(),
        }],
    );
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotInvalid
    );
    let mut bad = delta(&base);
    let deleted = Tombstone {
        layer: "poles".into(),
        object_id: "p1".into(),
        object_revision: "r8".into(),
    };
    bad.tombstones = vec![deleted.clone(), deleted];
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotInvalid
    );
    let mut bad = delta(&base);
    bad.target = base.revision.clone();
    assert_eq!(
        bad.validate(&base, 0).unwrap_err().code,
        RefusalCode::SnapshotStaleDelta
    );
}
