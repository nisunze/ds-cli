//! Owner, 2026-10-05: the analysis report is on by default. Rust says when a held
//! revision still owes it; the host only runs the explicit Analyze action then.
#[path = "../../../../ds-network/crates/ds-grid-engine/tests/common/mod.rs"]
mod common;
use ds_grid_engine::profile_display_case::ProfileDisplayCase;
use ds_grid_engine::GridSession;
use ds_grid_model::{EntityId, GridModelSnapshot, ModelCrs};
use ds_profile_runtime::{CaseRequest, ProjectRequest, Request, Runtime};
use serde_json::{json, Value};

fn request(snapshot: &GridModelSnapshot) -> ProjectRequest {
    let session = GridSession::open(snapshot.clone());
    let (artifacts, _) = ds_grid_exchange::dsgrid::emit(
        snapshot,
        &ds_grid_exchange::PackOptions {
            model_id: EntityId::new("default-analysis").unwrap(),
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
    ProjectRequest {
        scope: "default-analysis".into(),
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

fn weather(project: &ProjectRequest) -> Request {
    CaseRequest {
        scope: project.scope.clone(),
        expected_revision: project.expected_revision.clone(),
        display_case: ProfileDisplayCase::SagReference,
        select: false,
        cached_only: false,
        sequence: 1,
    }
    .into()
}

fn state(result: &Value) -> &Value {
    &result["command_view"]["analysis_state"]
}

#[test]
fn default_analysis_is_due_once_per_revision_and_one_switch_turns_it_off() {
    let mut runtime = Runtime::default();
    let first = request(&common::fixture());
    let opened = runtime.execute(first.clone()).unwrap();
    assert_eq!(opened["display_state"]["visibility"]["analysis"], true);
    assert_eq!(state(&opened), "due");
    assert!(opened["analysis"].is_null());
    assert_eq!(opened["ran"], false);

    let mut off = first.clone();
    off.patch = serde_json::from_value(json!({"visibility":{"analysis":false}})).unwrap();
    let switched_off = runtime.execute(off).unwrap();
    assert_eq!(state(&switched_off), "off");
    // The switch draws nothing: the projected review is unchanged.
    assert_eq!(
        switched_off["scene"]["engineering"],
        opened["scene"]["engineering"]
    );
    let mut on = first.clone();
    on.patch = serde_json::from_value(json!({"visibility":{"analysis":true}})).unwrap();
    assert_eq!(state(&runtime.execute(on).unwrap()), "due");

    // The host runs the same explicit Analyze action once; the revision is then current.
    let mut analyze = first.clone();
    analyze.analyze = true;
    let analyzed = runtime.execute(analyze).unwrap();
    assert_eq!(analyzed["ran"], true);
    assert_eq!(
        analyzed["analysis"]["model_revision"],
        first.expected_revision.as_str()
    );
    assert_eq!(state(&analyzed), "current");

    // Later display patches and weather receipts at that revision stay current, so a
    // host never runs it again for the same revision.
    let mut patch = first.clone();
    patch.patch.vertical_exaggeration = Some(8.0);
    let patched = runtime.execute(patch).unwrap();
    assert_eq!(patched["ran"], false);
    assert_eq!(state(&patched), "current");
    let case = runtime.execute(weather(&first)).unwrap();
    assert_eq!(state(&case), "current");
    assert_eq!(
        case["command_view"]["analysis"]["model_revision"],
        first.expected_revision.as_str()
    );
}

#[test]
fn an_edited_revision_owes_its_own_analysis_and_never_reads_the_previous_one() {
    let mut runtime = Runtime::default();
    let first = request(&common::fixture());
    let mut analyze = first.clone();
    analyze.analyze = true;
    assert_eq!(state(&runtime.execute(analyze).unwrap()), "current");

    let mut edited = common::fixture();
    edited.structures[1].description = Some("edited after analysis".into());
    let second = request(&edited);
    assert_ne!(second.expected_revision, first.expected_revision);
    let projected = runtime.execute(second.clone()).unwrap();
    assert!(projected["analysis"].is_null());
    assert_eq!(state(&projected), "due");
    // Negative control: a weather receipt at the edited revision must not carry the
    // previous revision's analysis, or the edit would never be analyzed.
    let case = runtime.execute(weather(&second)).unwrap();
    assert!(case["command_view"]["analysis"].is_null());
    assert_eq!(state(&case), "due");
}

#[test]
fn an_empty_model_owes_no_analysis() {
    let mut runtime = Runtime::default();
    let empty = runtime
        .execute(request(&GridModelSnapshot::default()))
        .unwrap();
    assert_eq!(state(&empty), "not_applicable");
}
