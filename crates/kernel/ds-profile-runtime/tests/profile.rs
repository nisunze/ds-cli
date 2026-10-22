use ds_command_kernel::profile_display::{DisplayOption, DisplayPatch};
use ds_grid_engine::GridSession;
use ds_profile_runtime::{InitializeRequest as Request, Runtime};
mod support;
use serde_json::{Value, json};
fn request() -> Request {
    let session = GridSession::open(Default::default());
    support::initialize(&session, "profile-test", "one", Default::default(), &[])
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
    let changed = runtime
        .execute(support::refresh(&one, &initial, None))
        .unwrap();
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
    assert!(runtime
        .execute(support::refresh(&invalid, &initial, None))
        .is_err());
    one.patch = DisplayPatch::default();
    one.analyze = true;
    for _ in 0..2 {
        let result = runtime
            .execute(support::refresh(&one, &initial, None))
            .unwrap();
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
    let initial = runtime.execute(request()).unwrap();
    let mut wrong = support::refresh(&request(), &initial, None);
    wrong.expected_revision = serde_json::from_value(json!("old")).unwrap();
    assert!(runtime
        .execute(wrong)
        .unwrap_err()
        .starts_with("profile_replay_diverged"));
    let mut wrong = support::refresh(&request(), &initial, None);
    wrong.style_document = Value::Null;
    assert!(runtime
        .execute(wrong)
        .unwrap_err()
        .starts_with("profile_styles_unavailable"));
    assert_eq!(
        runtime
            .execute(support::refresh(&request(), &initial, None))
            .unwrap()["scene"]["display"]["state"]["visibility"]["review"],
        true
    );
}

#[test]
fn standalone_analysis_exposes_unknown_blockers_but_runtime_requires_governed_styles() {
    let mut runtime = Runtime::default();
    let mut input = request();
    input.style_document = Value::Null;
    input.analyze = true;
    assert!(runtime
        .execute(input)
        .unwrap_err()
        .starts_with("profile_styles_unavailable:"));
    let session = GridSession::open(Default::default());
    let result = ds_profile_runtime::analyze(
        &session,
        &Default::default(),
        None,
        Some("no governed Profile projection was admitted"),
    );
    assert_eq!(result["engineering_case_scope"], "model_bound_envelope");
    assert_eq!(
        result["model_revision"],
        serde_json::json!(session.current_revision().revision_id)
    );
    assert!(result["issues"].is_object());
    assert!(
        result["blocker_kinds"]["profile_projection"]
            .as_u64()
            .unwrap()
            > 0
    );
    let admitted = runtime.execute(request()).unwrap();
    assert_eq!(
        admitted["history"],
        serde_json::to_value(session.history_state()).unwrap()
    );
    assert_eq!(admitted["ran"], false);
}

#[test]
fn cached_weather_is_display_only_and_revision_fenced() {
    use ds_grid_engine::profile_display_case::ProfileDisplayCase;
    use ds_profile_runtime::{CaseRequest, Request as Operation};
    let mut runtime = Runtime::default();
    let mut initial = request();
    initial.analyze = true;
    initial.patch.display_case =
        Some(serde_json::from_value(json!({"mode":"sag_reference"})).unwrap());
    let projected = runtime.execute(initial).unwrap();
    let revision = projected["history"]["model_revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let analysis = projected["command_view"]["analysis"].clone();
    let mut case = CaseRequest {
        scope: "one".into(),
        model_id: "profile-test".into(),
        expected_history: serde_json::from_value(projected["history"].clone()).unwrap(),
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
    assert_eq!(
        selected["command_view"]["review"]["display_case"],
        json!({"mode":"sag_reference"})
    );
    assert_eq!(selected["command_view"]["review"]["selected_case_index"], 1);
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
fn governed_profile_document_admits_every_native_display_option() {
    let backend: Value = serde_json::from_str(include_str!(
        "../../ds-command-kernel/tests/fixtures/profile-display-style.json"
    ))
    .unwrap();
    assert_eq!(
        backend["metadata"]["profile_pens"]
            .as_object()
            .unwrap()
            .len(),
        17
    );
    let state = ds_command_kernel::profile_display::DisplayState::default();
    let rows = ds_command_kernel::profile_encoding::resolve(&state, &backend).unwrap();
    assert_eq!(rows.len(), DisplayOption::ALL.len());
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
    let opened = runtime.execute(original.clone()).unwrap();
    let case = |sequence| CaseRequest {
        scope: original.scope.clone(),
        model_id: "profile-test".into(),
        expected_history: serde_json::from_value(opened["history"].clone()).unwrap(),
        expected_revision: opened["history"]["model_revision"]
            .as_str()
            .unwrap()
            .to_owned(),
        display_case: ProfileDisplayCase::GreatestSag,
        select: true,
        cached_only: true,
        sequence,
    };
    runtime.execute(case(7)).unwrap();
    let mut invalid = original.clone();
    invalid.patch.vertical_exaggeration = Some(101.0);
    assert!(runtime
        .execute(support::refresh(&invalid, &opened, None))
        .is_err());
    let mut invalid = original.clone();
    invalid.style_document["metadata"]["profile_pens"]
        .as_object_mut()
        .unwrap()
        .remove("grid");
    assert!(runtime
        .execute(support::refresh(&invalid, &opened, None))
        .is_err());
    let mut invalid = support::refresh(&original, &opened, None);
    invalid.expected_revision = serde_json::from_value(json!("foreign-checkpoint")).unwrap();
    assert!(runtime.execute(invalid).is_err());
    assert!(runtime
        .execute(case(7))
        .unwrap_err()
        .contains("profile_selection_stale"));
    runtime
        .execute(case(8))
        .expect("previous prepared case must still be usable");
    assert!(runtime
        .execute(support::refresh(&original, &opened, None))
        .unwrap()["scene"]["display"]
        .is_object());
}

#[test]
fn native_structure_labels_keep_the_vertical_profile_position() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid"
    ))
    .unwrap();
    let (package, _) = ds_grid_exchange::dsgrid::ingest(&bytes).unwrap();
    let session = ds_grid_exchange::linked_models::open_session(&package);
    let mut packet = support::initialize(
        &session,
        package.manifest.model.model_id.as_str(),
        "one",
        package.manifest.model.presentation.clone(),
        &package.assets,
    );
    let mut runtime = Runtime::default();
    let identity = |p: &&Value| {
        [
            "structure_numbers",
            "structure_names",
            "structure_comments",
            "structure_labels",
        ]
        .contains(&p["option"].as_str().unwrap())
    };
    let option = |p: &Value, name: &str| p["option"].as_str() == Some(name);
    // Owner, 2026-10-05: names and comments are visible alongside numbers.
    let admitted = runtime.execute(packet.clone()).unwrap();
    let opened = admitted["scene"]["display"]["authored_structures"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|group| group["primitives"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert!(opened.iter().any(|p| option(p, "structure_numbers")));
    assert!(opened.iter().any(|p| option(p, "structure_names")));
    packet.patch = serde_json::from_str(r#"{"visibility":{"structure_names":true}}"#).unwrap();
    let result = runtime
        .execute(support::refresh(&packet, &admitted, None))
        .unwrap();
    let primitives = result["scene"]["display"]["authored_structures"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|group| group["primitives"].as_array().unwrap())
        .cloned()
        .collect::<Vec<_>>();
    assert!(primitives.iter().any(|p| option(p, "structure_names")));
    let labels = primitives
        .iter()
        .filter(identity)
        .flat_map(|p| p["labels"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert!(!labels.is_empty());
    for label in labels {
        assert_eq!(
            label["rotation_radians"].as_f64().unwrap(),
            -std::f64::consts::FRAC_PI_2
        );
        // Owner, 2026-10-05: identities start above the attachment boxes (-8..8 px).
        assert_eq!(label["offset_px"][1], -12.0);
    }
}

#[test]
fn cli_and_renderer_share_native_controls_and_colour_state() {
    // Reset availability is native metadata, not a host decision.
    let styles = serde_json::from_str(include_str!(
        "../../ds-command-kernel/tests/fixtures/profile-display-style.json"
    ))
    .unwrap();
    let state = ds_command_kernel::profile_display::DisplayState::default();
    let rows = ds_command_kernel::profile_encoding::resolve(&state, &styles).unwrap();
    let colours = std::collections::BTreeMap::from([("Cable A".to_owned(), "#aabbcc".to_owned())]);
    let cables =
        ds_command_kernel::profile_encoding::cable_pens(["Cable A", "Cable B"], &rows, &colours);
    assert!(cables[0].can_reset);
    assert!(!cables[1].can_reset);
    let mut runtime = Runtime::default();
    let mut one = request();
    one.patch = serde_json::from_value(json!({"styles":{"ground":{"color":"#112233","width_mm":0.8,"line_type":"dotted"},"strain":{"symbol":"▲","color":"#ABCDEF"}},"vertical_exaggeration": 7.5, "cable_colors":{"Cable A":"#AABBCC"},
        "terrain":{"corridor_half_width_m":25.0,"gap_tolerance_m":42.0,"ground_clearance_offset_m":6.0,"lowest_wire_drop_m":2.0}})).unwrap();
    let result = runtime.execute(one.clone()).unwrap();
    assert_eq!(
        result["scene"]["display"]["groups"],
        result["command_view"]["display"]["groups"]
    );
    assert_eq!(
        result["command_view"]["display"]["state"]["cable_colors"]["Cable A"],
        "#aabbcc"
    );
    assert_eq!(
        result["scene"]["display"]["groups"][0]["numbers"][0]["value"],
        7.5
    );
    assert_eq!(
        result["scene"]["display"]["groups"][1]["numbers"][1]["value"],
        42.0
    );
    assert_eq!(
        result["scene"]["display"]["style_controls"],
        result["command_view"]["display"]["style_controls"]
    );
    assert_eq!(
        result["scene"]["display"]["legend"],
        result["command_view"]["display"]["legend"]
    );
    assert_eq!(result["scene"]["display"]["legend"][0]["glyph"], "▲");
    assert_eq!(
        result["scene"]["display"]["legend"][0]["encoding"]["color"],
        "#abcdef"
    );
    let ground = result["scene"]["display"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["option"] == "ground")
        .unwrap();
    assert_eq!(ground["encoding"]["color"], "#112233");
    assert!((ground["encoding"]["width_px"].as_f64().unwrap() - 0.8 * 96.0 / 25.4).abs() < 1e-9);
    assert_eq!(ground["encoding"]["dash_px"].as_array().unwrap().len(), 2);
    let mut two = request();
    two.scope = "two".into();
    let other = runtime.execute(two).unwrap();
    assert_eq!(
        other["scene"]["display"]["groups"][0]["numbers"][0]["value"],
        5.0
    );
    assert!(other["display_state"].get("cable_colors").is_none());
    one.patch = serde_json::from_value(
        json!({"visibility":{"wire":false},"cable_colors":{"Cable A":"red"}}),
    )
    .unwrap();
    assert!(runtime
        .execute(support::refresh(&one, &result, None))
        .unwrap_err()
        .starts_with("invalid_profile_view:"));
    one.patch = Default::default();
    let retained = runtime
        .execute(support::refresh(&one, &result, None))
        .unwrap();
    assert_eq!(
        retained["scene"]["display"]["groups"],
        result["scene"]["display"]["groups"]
    );
    one.patch = serde_json::from_value(json!({"cable_colors":{"Cable A":null}})).unwrap();
    assert!(runtime
        .execute(support::refresh(&one, &result, None))
        .unwrap()["display_state"]
        .get("cable_colors")
        .is_none());
}
