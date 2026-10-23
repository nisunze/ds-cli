use ds_design_workspace::grid_publication::{prepare, publication_revision, Request};
use ds_grid_exchange::blank_model::{create_blank_model, BlankModelRequest};
use ds_grid_exchange::library::bundle_digest;
use ds_grid_exchange::model_library::{
    attach_model_library, create_model_library, detach_model_library,
};
use ds_grid_exchange::package::unpack;
use ds_grid_exchange::structure_import::import_structure_package;
use ds_grid_model::EntityId;

#[test]
#[ignore = "requires an explicitly qualified native Network grid-publication executable"]
fn immutable_library_adoption_and_detach_publish_distinct_revisions_with_project_fences() {
    let blank = create_blank_model(&BlankModelRequest::default()).unwrap();
    let imported = import_structure_package(&blank.bytes, "hp-m1-strain.012", include_bytes!("../../../../../ds-network/fixtures/pls-public/humble-pole/workspace/structures/hp-m1-strain.012"), None, None).unwrap();
    let source = imported.bytes;
    let library = create_model_library(
        &source,
        &bundle_digest(&source),
        EntityId::new("neutral-library").unwrap(),
        EntityId::new("release-1").unwrap(),
    )
    .unwrap();
    let attached = attach_model_library(
        &source,
        &bundle_digest(&source),
        &library,
        &bundle_digest(&library),
        &[],
    )
    .unwrap();
    let detached = detach_model_library(
        &attached.bytes,
        &attached.resulting_digest,
        &attached.library_pin,
    )
    .unwrap();
    let original_snapshot = unpack(&source).unwrap().snapshot;
    assert_eq!(unpack(&attached.bytes).unwrap().snapshot, original_snapshot);
    assert_eq!(unpack(&detached.bytes).unwrap().snapshot, original_snapshot);
    let base = publication_revision(&source, "stable").unwrap();
    let adopted = publication_revision(&attached.bytes, "stable").unwrap();
    let removed = publication_revision(&detached.bytes, "stable").unwrap();
    assert_ne!(base, adopted);
    assert_ne!(adopted, removed);
    assert_ne!(base, removed);
    let request = Request {
        project: "reviewed-project".into(),
        model_id: Some("reviewed-model".into()),
        model_kind: "general".into(),
        expected_head_revision_id: Some(base.clone()),
        ..Default::default()
    };
    let intent = prepare(&request, &attached.bytes, "stable").unwrap();
    assert_eq!(intent.project, "reviewed-project");
    assert_eq!(intent.model_id, "reviewed-model");
    assert_eq!(intent.expected_head_revision_id, base);
    assert_eq!(
        intent.manifest_model_revision,
        unpack(&source).unwrap().manifest.model.model_revision + 1
    );
    assert_eq!(
        ds_command_kernel::grid_publication::revision_identifier(&intent.source_revision_id)
            .unwrap(),
        adopted
    );
    assert_eq!(
        intent.source_revision_id,
        prepare(&request, &attached.bytes, "stable")
            .unwrap()
            .source_revision_id
    );
    let missing = Request {
        expected_head_revision_id: None,
        ..request
    };
    assert!(prepare(&missing, &attached.bytes, "stable")
        .unwrap_err()
        .starts_with("publish_expected_head_required"));
}
