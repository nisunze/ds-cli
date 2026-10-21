//! Opt-in exact native receipts for replaying a selected-case edit through the
//! production browser transport. Inputs stay read-only; output is explicit.
use super::*;
use ds_grid_engine::profile_display_case::ProfileDisplayCase;
use ds_grid_engine::{CommandEnvelope, GridCommand};

#[test]
#[ignore = "reads an explicitly named package and writes replay receipts"]
fn named_selected_case_retype_receipts() {
    let path = std::env::var("DS_SELECTED_CASE_PACKAGE").expect("named package");
    let output = std::env::var("DS_SELECTED_CASE_RECEIPT").expect("fresh receipt path");
    assert!(!std::path::Path::new(&output).exists());
    let bytes = std::fs::read(&path).unwrap();
    let (package, _) = ds_grid_exchange::dsgrid::ingest(&bytes).unwrap();
    let model_id = package.manifest.model.model_id.clone();
    let mut worker = GridSession::open(package.snapshot.clone());
    let structure = worker
        .snapshot()
        .structures
        .iter()
        .find(|row| row.engineering_number.as_deref() == Some("18"))
        .expect("pole 18")
        .clone();
    let original_type = worker
        .snapshot()
        .structure_types
        .iter()
        .find(|row| row.id == structure.structure_type_id)
        .unwrap()
        .clone();
    let replacement = worker
        .snapshot()
        .structure_types
        .iter()
        .find(|row| row.engineering_name.eq_ignore_ascii_case("a-w-S140.012"))
        .expect("exact retained replacement")
        .clone();
    let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
        &worker,
        model_id.clone(),
        package.manifest.model.presentation.clone(),
        &package.assets,
    );
    let seed_json = serde_json::to_string(&seed).unwrap();
    let scope = "selected-case-probe";
    let mut runtime = Runtime::default();
    let initial = runtime
        .execute(Request::InitializeExact(Box::new(InitializeRequest {
            scope: scope.into(),
            publication_id: None,
            seed_json: seed_json.clone(),
            restore_state: None,
            options: Default::default(),
            style_document: serde_json::from_str(include_str!(
                "../../../tests/fixtures/profile-display-style.json"
            ))
            .unwrap(),
            patch: Default::default(),
            analyze: false,
        })))
        .unwrap();
    let refresh = |runtime: &Runtime, worker: &GridSession, patch: DisplayPatch| {
        let held = &runtime.held[scope];
        RefreshRequest {
            scope: scope.into(),
            publication_id: None,
            model_id: model_id.to_string(),
            base_revision: held.session.current_revision().revision_id.clone(),
            base_history: held.session.history_state(),
            expected_revision: worker.current_revision().revision_id.clone(),
            target_history: worker.history_state(),
            journal_json: serde_json::to_string(&worker.retained_journal()).unwrap(),
            options: Default::default(),
            style_document: serde_json::from_str(include_str!(
                "../../../tests/fixtures/profile-display-style.json"
            ))
            .unwrap(),
            patch,
            analyze: false,
            calculated_base_signature: None,
        }
    };
    let sag = runtime
        .execute(Request::Refresh(Box::new(refresh(
            &runtime,
            &worker,
            DisplayPatch {
                display_case: Some(
                    serde_json::from_value(json!({"mode":"sag_reference"})).unwrap(),
                ),
                ..Default::default()
            },
        ))))
        .unwrap();
    let select = |runtime: &Runtime, value: ProfileDisplayCase, sequence| CaseRequest {
        scope: scope.into(),
        model_id: model_id.to_string(),
        expected_revision: worker.current_revision().revision_id.to_string(),
        expected_history: runtime.held[scope].session.history_state(),
        display_case: value,
        select: true,
        cached_only: false,
        sequence,
    };
    let greatest = runtime
        .execute(select(&runtime, ProfileDisplayCase::GreatestSag, 1))
        .unwrap();
    // Advance the calculated host mirror as in the previous installed replay.
    let describe = CommandEnvelope::new(
        "selected-case-description",
        worker.current_revision().revision_id.clone(),
        GridCommand::DescribeStructure {
            id: structure.id.clone(),
            description: Some("selected-case replay".into()),
        },
    );
    worker.apply_command(describe.clone()).unwrap();
    let mut observe = refresh(&runtime, &worker, Default::default());
    observe.calculated_base_signature = Some(
        runtime.held[scope]
            .calculated_base
            .as_ref()
            .unwrap()
            .signature()
            .into(),
    );
    let described = runtime
        .execute(Request::Observe(Box::new(observe)))
        .unwrap();
    assert!(!described["calculation"]["required"].as_bool().unwrap());
    // Selection is at this new head; prepare its alternate case explicitly.
    let prepared = runtime
        .execute(Request::Refresh(Box::new(refresh(
            &runtime,
            &worker,
            DisplayPatch {
                display_case: Some(
                    serde_json::from_value(json!({"mode":"sag_reference"})).unwrap(),
                ),
                ..Default::default()
            },
        ))))
        .unwrap();
    let selected = runtime
        .execute(CaseRequest {
            scope: scope.into(),
            model_id: model_id.to_string(),
            expected_revision: worker.current_revision().revision_id.to_string(),
            expected_history: worker.history_state(),
            display_case: ProfileDisplayCase::GreatestSag,
            select: true,
            cached_only: false,
            sequence: 2,
        })
        .unwrap();
    let command = CommandEnvelope::new(
        "selected-case-retype",
        worker.current_revision().revision_id.clone(),
        GridCommand::RetypeStructure {
            id: structure.id.clone(),
            structure_type_id: replacement.id.clone(),
        },
    );
    let before_retype = worker.snapshot().clone();
    worker.apply_command(command.clone()).unwrap();
    let mut observe = refresh(&runtime, &worker, Default::default());
    observe.calculated_base_signature = Some(
        runtime.held[scope]
            .calculated_base
            .as_ref()
            .unwrap()
            .signature()
            .into(),
    );
    let edited = runtime
        .execute(Request::Observe(Box::new(observe)))
        .unwrap();
    assert_eq!(
        edited["calculation"]["computed_revision"],
        worker.current_revision().revision_id.as_str()
    );
    assert_eq!(edited["calculation"]["required"], false);
    assert!(worker.is_valid());
    assert_eq!(
        worker.snapshot().structure_types,
        package.snapshot.structure_types
    );
    let scene = runtime.held[scope]
        .calculated_base
        .as_ref()
        .unwrap()
        .scene();
    let fresh_seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
        &worker,
        model_id.clone(),
        package.manifest.model.presentation.clone(),
        &package.assets,
    );
    let fresh = Runtime::default()
        .execute(Request::InitializeExact(Box::new(InitializeRequest {
            scope: "independent-fresh".into(),
            publication_id: None,
            seed_json: serde_json::to_string(&fresh_seed).unwrap(),
            restore_state: Some(runtime.views[scope].clone()),
            options: Default::default(),
            style_document: serde_json::from_str(include_str!(
                "../../../tests/fixtures/profile-display-style.json"
            ))
            .unwrap(),
            patch: Default::default(),
            analyze: false,
        })))
        .unwrap();
    let fresh_scene: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_str(fresh["scene_json"].as_str().unwrap()).unwrap();
    for field in [
        "bands",
        "conductor_spans",
        "clearance_guides",
        "evidence",
        "axis_pin",
        "display_case",
    ] {
        assert_eq!(
            serde_json::to_value(scene).unwrap()[field],
            serde_json::to_value(&fresh_scene).unwrap()[field],
            "fresh {field}"
        );
    }
    let edited_snapshot = worker.snapshot().clone();
    let edited_history = worker.history_state();
    worker.undo().unwrap();
    assert_eq!(worker.snapshot(), &before_retype);
    assert!(worker.is_valid());
    worker.redo().unwrap();
    assert_eq!(worker.snapshot(), &edited_snapshot);
    assert_eq!(worker.history_state(), edited_history);
    let receipt = json!({"seed_json":seed_json,"model_id":model_id,"structure":structure.id,
        "original_type":original_type,"replacement":replacement,"initial":initial,"sag":sag,
        "greatest":greatest,"describe_command":describe,"described":described,
        "prepared":prepared,"selected":selected,"retype_command":command,"edited":edited,
        "expected_scene":scene,"independent_fresh_geometry_equal":true,"undo_redo_exact":true,
        "history":worker.history_state(),"type_count":worker.snapshot().structure_types.len()});
    std::fs::write(&output, serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    eprintln!(
        "selected-case retype: native head fresh, {} types retained, receipt {}",
        worker.snapshot().structure_types.len(),
        output
    );
}
