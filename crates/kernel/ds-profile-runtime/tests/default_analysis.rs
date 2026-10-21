//! Owner, 2026-10-05: the analysis report is on by default. Rust says when a held
//! revision still owes it; the host only runs the explicit Analyze action then.
#[path = "../../../../ds-network/crates/ds-grid-engine/tests/common/mod.rs"]
mod common;
use ds_grid_engine::profile_display_case::ProfileDisplayCase;
use ds_grid_engine::GridSession;
use ds_grid_model::{GridModelSnapshot, StructureId};
use ds_profile_runtime::{CaseRequest, InitializeRequest, Request, Runtime};
mod support;
use serde_json::{json, Value};

fn request(snapshot: &GridModelSnapshot) -> InitializeRequest {
    let session = GridSession::open(snapshot.clone());
    support::initialize(
        &session,
        "default-analysis",
        "default-analysis",
        Default::default(),
        &[],
    )
}

fn weather(project: &InitializeRequest, result: &Value) -> Request {
    CaseRequest {
        scope: project.scope.clone(),
        model_id: "default-analysis".into(),
        expected_history: serde_json::from_value(result["history"].clone()).unwrap(),
        expected_revision: result["history"]["model_revision"].as_str().unwrap().into(),
        display_case: ProfileDisplayCase::GreatestSag,
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
    let revision = opened["history"]["model_revision"].as_str().unwrap();
    assert_eq!(opened["display_state"]["visibility"]["analysis"], true);
    assert_eq!(state(&opened), "due");
    assert!(opened["analysis"].is_null());
    assert_eq!(opened["ran"], false);

    let mut off = first.clone();
    off.patch = serde_json::from_value(json!({"visibility":{"analysis":false}})).unwrap();
    let switched_off = runtime
        .execute(support::refresh(&off, &opened, None))
        .unwrap();
    assert_eq!(state(&switched_off), "off");
    // The switch draws nothing: the projected review is unchanged.
    assert_eq!(
        switched_off["scene"]["engineering"],
        opened["scene"]["engineering"]
    );
    let mut on = first.clone();
    on.patch = serde_json::from_value(json!({"visibility":{"analysis":true}})).unwrap();
    assert_eq!(
        state(
            &runtime
                .execute(support::refresh(&on, &opened, None))
                .unwrap()
        ),
        "due"
    );

    // The host runs the same explicit Analyze action once; the revision is then current.
    let mut analyze = first.clone();
    analyze.analyze = true;
    let analyzed = runtime
        .execute(support::refresh(&analyze, &opened, None))
        .unwrap();
    assert_eq!(analyzed["ran"], true);
    assert_eq!(analyzed["analysis"]["model_revision"], revision);
    assert_eq!(state(&analyzed), "current");

    // Later display patches and weather receipts at that revision stay current, so a
    // host never runs it again for the same revision.
    let mut patch = first.clone();
    patch.patch.vertical_exaggeration = Some(8.0);
    let patched = runtime
        .execute(support::refresh(&patch, &opened, None))
        .unwrap();
    assert_eq!(patched["ran"], false);
    assert_eq!(state(&patched), "current");
    let case = runtime.execute(weather(&first, &patched)).unwrap();
    assert_eq!(state(&case), "current");
    assert_eq!(case["command_view"]["analysis"]["model_revision"], revision);
}

#[test]
fn an_edited_revision_owes_its_own_analysis_and_never_reads_the_previous_one() {
    let mut runtime = Runtime::default();
    let first = request(&common::fixture());
    let mut analyze = first.clone();
    analyze.analyze = true;
    let analyzed = runtime.execute(analyze).unwrap();
    assert_eq!(state(&analyzed), "current");

    let mut worker = GridSession::open(common::fixture());
    worker
        .apply_command(ds_grid_engine::CommandEnvelope::new(
            "description-edit",
            worker.current_revision().revision_id.clone(),
            ds_grid_engine::GridCommand::DescribeStructure {
                id: StructureId::new("str-2").unwrap(),
                description: Some("edited after analysis".into()),
            },
        ))
        .unwrap();
    let second = support::refresh(&first, &analyzed, Some(&worker));
    assert_ne!(
        second.expected_revision.as_str(),
        analyzed["history"]["model_revision"].as_str().unwrap()
    );
    let projected = runtime.execute(second).unwrap();
    assert!(projected["analysis"].is_null());
    assert_eq!(state(&projected), "due");
    // Negative control: a weather receipt at the edited revision must not carry the
    // previous revision's analysis, or the edit would never be analyzed.
    let case = runtime.execute(weather(&first, &projected)).unwrap();
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
