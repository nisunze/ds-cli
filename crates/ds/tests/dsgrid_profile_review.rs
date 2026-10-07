//! The public file command returns the same native Profile review facts as the renderer.
use ds_grid_engine::profile_engineering::{ProfileEngineeringMarkerKind, ProfileReviewColourRole};
use ds_grid_engine::{GridSession, ProfileAtlasOptions};
use serde_json::Value;
mod common;

#[test]
fn profile_review_json_retains_native_per_structure_summaries_without_authoring() {
    let model = common::fixture();
    let before = std::fs::read(&model).unwrap();
    let package = ds_grid_exchange::unpack(&before).unwrap();
    let session = GridSession::open(package.snapshot);
    let native = session
        .profile_atlas_scene(ProfileAtlasOptions::default())
        .unwrap();
    let (envelope, code) = common::json(&[
        "dsgrid",
        "run",
        "--model",
        &model,
        "--operation",
        "project_profile_atlas",
        "--limit",
        "10000",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{envelope}");
    let data = &envelope["data"];
    assert_eq!(data["staged"], false);
    assert_eq!(data["persisted"], false);
    let result = &data["result"];
    assert_eq!(
        result["engineering"],
        serde_json::to_value(&native.engineering).unwrap()
    );
    assert!(!native.engineering.markers.is_empty());
    for marker in &native.engineering.markers {
        assert_eq!(marker.labels.len(), 1);
        assert!(marker.labels[0].summary.is_some());
        assert!(!marker.labels[0].text.to_lowercase().contains("unknown"));
        if marker.kind == ProfileEngineeringMarkerKind::SectionState {
            // Native section/attachment boxes may show an issue count.
            // The owner tests its engineering meaning; this adapter retains
            // that count and every member rather than demanding blank text.
            let label = &marker.labels[0];
            if !label.text.is_empty() {
                let count = label.text.parse::<usize>().expect("native issue count");
                let summary = label.summary.as_ref().unwrap();
                assert!(count > 0 && count <= summary.members.len(), "{summary:?}");
            }
            if serde_json::to_value(marker.status).unwrap() == "unknown" {
                assert_eq!(
                    marker.labels[0].colour_role,
                    ProfileReviewColourRole::Neutral
                );
            }
        }
    }
    assert_eq!(std::fs::read(model).unwrap(), before);
    assert_ne!(result["engineering"], Value::Null);
}
