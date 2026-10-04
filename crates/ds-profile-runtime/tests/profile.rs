use ds_command_kernel::profile_display::{DisplayOption, DisplayPatch};
use ds_grid_engine::GridSession;
use ds_grid_model::{EntityId, ModelCrs};
use ds_profile_runtime::{ProjectRequest as Request, Runtime};
use serde_json::{json, Value};
fn request() -> Request {
    let session = GridSession::open(Default::default());
    let (artifacts, _) = ds_grid_exchange::dsgrid::emit(
        session.snapshot(),
        &ds_grid_exchange::PackOptions {
            model_id: EntityId::new("profile-test").unwrap(),
            model_revision: 0,
            coordinate_system: ModelCrs::new("EPSG:32736").unwrap(),
            presentation: Default::default(),
            library_pins: vec![],
            library_needs: vec![],
            assets: vec![],
            exchange_bindings: Default::default(),
        },
    )
    .unwrap();
    Request {
        scope: "one".into(),
        expected_revision: session.current_revision().revision_id.to_string(),
        package_bytes: artifacts.artifacts[0].bytes.clone(),
        options: Default::default(),
        style_document: serde_json::from_str(include_str!(
            "../../../tests/fixtures/profile-display-style.json"
        ))
        .unwrap(),
        patch: Default::default(),
        analyze: false,
    }
}
#[test]
fn display_is_native_atomic_and_isolated_and_unknowns_are_results() {
    let mut runtime = Runtime::default();
    let mut one = request();
    let initial = runtime.execute(one.clone()).unwrap();
    assert_eq!(
        initial["scene"]["display"]["state"]["visibility"]["review"],
        true
    );
    one.patch =
        serde_json::from_value(json!({"visibility":{"review":false,"wire":false}})).unwrap();
    let changed = runtime.execute(one.clone()).unwrap();
    assert_eq!(
        changed["scene"]["display"]["state"]["visibility"]["review"],
        false
    );
    assert_eq!(
        changed["scene"]["display"]["state"]["visibility"]["ground"],
        true
    );
    let mut two = request();
    two.scope = "two".into();
    assert_eq!(
        runtime.execute(two).unwrap()["scene"]["display"]["state"]["visibility"]["review"],
        true
    );
    let mut invalid = one.clone();
    invalid.patch.vertical_exaggeration = Some(101.0);
    assert!(runtime.execute(invalid).is_err());
    one.patch = DisplayPatch::default();
    one.analyze = true;
    for _ in 0..2 {
        let result = runtime.execute(one.clone()).unwrap();
        assert_eq!(result["ran"], true);
        assert_eq!(
            result["analysis"]["engineering_case_scope"],
            "model_bound_envelope"
        );
        assert_eq!(result["command_view"]["analysis"]["status"], "unknown");
        assert!(serde_json::to_vec(&result["command_view"]).unwrap().len() < 1024 * 1024);
        assert!(
            result["scene"].get("analysis").is_none(),
            "analysis must travel once on the native wire"
        );
        assert_eq!(result["analysis"]["status"], "unknown");
        assert!(!result["analysis"]["blockers"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(result["analysis"]["issues"].is_object());
        assert!(result["scene"]["display"]["state"]["visibility"]["review"] == true);
    }
    let _ = DisplayOption::Review;
}
#[test]
fn stale_checkpoint_and_incomplete_pen_documents_refuse_before_mutation() {
    let mut runtime = Runtime::default();
    let mut wrong = request();
    wrong.expected_revision = "old".into();
    assert!(runtime
        .execute(wrong)
        .unwrap_err()
        .starts_with("profile_selection_stale"));
    let mut wrong = request();
    wrong.style_document = Value::Null;
    assert!(runtime
        .execute(wrong)
        .unwrap_err()
        .starts_with("profile_styles_unavailable"));
    assert_eq!(
        runtime.execute(request()).unwrap()["scene"]["display"]["state"]["visibility"]["review"],
        true
    );
}

#[test]
fn analyze_runs_and_exposes_unknown_blockers_without_governed_styles() {
    let mut runtime = Runtime::default();
    let mut input = request();
    input.style_document = Value::Null;
    input.analyze = true;
    let result = runtime.execute(input).unwrap();
    assert_eq!(result["ran"], true);
    assert_eq!(
        result["analysis"]["engineering_case_scope"],
        "model_bound_envelope"
    );
    assert_eq!(result["scene"], Value::Null);
    assert!(result["analysis"]["issues"].is_object());
    assert!(
        result["analysis"]["blocker_kinds"]["profile_projection"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(result["display_state"]["visibility"]["review"], true);
}

#[test]
fn cached_weather_is_display_only_and_revision_fenced() {
    use ds_grid_engine::profile_display_case::ProfileDisplayCase;
    use ds_profile_runtime::{CaseRequest, Request as Operation};
    let mut runtime = Runtime::default();
    let mut initial = request();
    initial.analyze = true;
    let revision = initial.expected_revision.clone();
    let projected = runtime.execute(initial).unwrap();
    let analysis = projected["command_view"]["analysis"].clone();
    let mut case = CaseRequest {
        scope: "one".into(),
        expected_revision: revision,
        display_case: ProfileDisplayCase::SagReference,
        select: false,
        cached_only: false,
        sequence: 1,
    };
    let prepared = runtime.execute(Operation::CaseLayer(case.clone())).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(prepared["bytes"].clone()).unwrap();
    let layer = ds_grid_engine::profile_atlas::ProfileCaseLayer::from_bytes(&bytes).unwrap();
    assert_eq!(layer.to_bytes().unwrap(), bytes);
    case.select = true;
    case.cached_only = true;
    let selected = runtime.execute(Operation::CaseLayer(case.clone())).unwrap();
    assert!(selected["bytes"].is_null());
    assert_eq!(
        selected["display_state"]["display_case"],
        json!({"mode":"sag_reference"})
    );
    assert_eq!(selected["command_view"]["analysis"], analysis);
    assert!(runtime
        .execute(Operation::CaseLayer(case.clone()))
        .unwrap_err()
        .starts_with("profile_selection_stale:"));
    case.sequence = 2;
    case.expected_revision = "stale".into();
    assert!(runtime
        .execute(Operation::CaseLayer(case))
        .unwrap_err()
        .starts_with("profile_selection_stale:"));
    let mut broken = bytes;
    broken.push(0);
    assert!(ds_grid_engine::profile_atlas::ProfileCaseLayer::from_bytes(&broken).is_err());
}

#[test]
fn backend_owned_profile_default_is_the_native_fixture_and_admits_all_seventeen_pens() {
    // The exact sibling Brain source owns this default. No copied host palette.
    let backend: Value = serde_json::from_str(include_str!(
        "../../../../ds-brain/internal/services/profile_display_style.v1.json"
    ))
    .unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/profile-display-style.json"
    ))
    .unwrap();
    assert_eq!(
        backend, fixture,
        "native fixture must match the owning backend document"
    );
    assert_eq!(
        backend["metadata"]["profile_pens"]
            .as_object()
            .unwrap()
            .len(),
        17
    );
    let state = ds_command_kernel::profile_display::DisplayState::default();
    let rows = ds_command_kernel::profile_encoding::resolve(&state, &backend).unwrap();
    assert_eq!(rows.len(), 16);
    ds_command_kernel::profile_encoding::clearance_violation_encoding(&backend).unwrap();
    let mut request = request();
    request.style_document = backend.clone();
    let output = Runtime::default().execute(request).unwrap();
    assert!(output["scene"]["display"].is_object());
    let mut incomplete = backend;
    incomplete["metadata"]["profile_pens"]
        .as_object_mut()
        .unwrap()
        .remove("clearance_violations");
    assert!(ds_command_kernel::profile_encoding::resolve(&state, &incomplete).is_err());
}

#[test]
fn refused_updates_preserve_the_prior_checkpoint_weather_sequence_and_cached_case() {
    use ds_grid_engine::profile_display_case::ProfileDisplayCase;
    use ds_profile_runtime::CaseRequest;
    let mut runtime = Runtime::default();
    let original = request();
    runtime.execute(original.clone()).unwrap();
    let case = |sequence| CaseRequest {
        scope: original.scope.clone(),
        expected_revision: original.expected_revision.clone(),
        display_case: ProfileDisplayCase::GreatestSag,
        select: true,
        cached_only: true,
        sequence,
    };
    runtime.execute(case(7)).unwrap();
    let mut invalid = original.clone();
    invalid.patch.vertical_exaggeration = Some(101.0);
    assert!(runtime.execute(invalid).is_err());
    let mut invalid = original.clone();
    invalid.style_document["metadata"]["profile_pens"]
        .as_object_mut()
        .unwrap()
        .remove("grid");
    assert!(runtime.execute(invalid).is_err());
    let mut invalid = original.clone();
    invalid.expected_revision = "foreign-checkpoint".into();
    assert!(runtime.execute(invalid).is_err());
    assert!(runtime
        .execute(case(7))
        .unwrap_err()
        .contains("profile_selection_stale"));
    runtime
        .execute(case(8))
        .expect("previous prepared case must still be usable");
    assert!(runtime.execute(original).unwrap()["scene"]["display"].is_object());
}
