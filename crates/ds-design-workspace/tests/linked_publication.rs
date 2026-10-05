use ds_design_workspace::grid_publication::{prepare_linked, Request};
use ds_grid_exchange::{pack, PackOptions};
use ds_grid_model::{EntityId, GridModelSnapshot, ModelCrs};

fn package() -> Vec<u8> {
    pack(
        &GridModelSnapshot::default(),
        &PackOptions {
            model_id: EntityId::new("native-composite").unwrap(),
            model_revision: 17,
            presentation: Default::default(),
            coordinate_system: ModelCrs::new("EPSG:32735").unwrap(),
            library_pins: Vec::new(),
            library_needs: Vec::new(),
            assets: Vec::new(),
            exchange_bindings: Default::default(),
        },
    )
    .unwrap()
}
#[test]
fn linked_publication_captures_explicit_creation_and_existing_head_with_native_lineage() {
    let bytes = package();
    let mut request = Request {
        project: "project-1".into(),
        model_kind: "general".into(),
        display_name: Some("Combined".into()),
        ..Default::default()
    };
    let created = prepare_linked(&request, &bytes, "explicit-project-model", "").unwrap();
    assert_eq!(created.model_id, "explicit-project-model");
    assert_eq!(created.expected_head_revision_id, "");
    assert_eq!(created.manifest_model_id, "native-composite");
    assert_eq!(created.manifest_model_revision, 17);
    request.display_name = None;
    let existing =
        prepare_linked(&request, &bytes, "explicit-project-model", "reviewed-head").unwrap();
    assert_eq!(existing.expected_head_revision_id, "reviewed-head");
    assert_eq!(existing.source_revision_id, created.source_revision_id);
    request.display_name = Some("Rename".into());
    assert!(
        prepare_linked(&request, &bytes, "explicit-project-model", "reviewed-head")
            .unwrap_err()
            .contains("rename_unsupported")
    );
    assert!(
        prepare_linked(&request, b"invalid package", "explicit-project-model", "")
            .unwrap_err()
            .contains("model_invalid")
    );
}

#[test]
fn the_combined_models_binding_and_publication_revision_are_derived() {
    use ds_design_workspace::grid_publication::{
        combined_model_id, publication_revision, COMBINED_DISPLAY_NAME, COMBINED_GRAPH_ID,
    };
    assert_eq!(COMBINED_GRAPH_ID, "combined");
    assert_eq!(COMBINED_DISPLAY_NAME, "Combined model");
    let parts = ["north".to_string(), "south".to_string()].into();
    let identity = ds_grid_engine::composite::combined_identity(&parts);
    let model_id = combined_model_id(&identity).unwrap();
    assert_eq!(model_id, identity.replacen("combined.", "combined-", 1));
    assert_eq!(
        combined_model_id("combined.0123456789abcdef").unwrap(),
        "combined-0123456789abcdef"
    );
    for refused in [
        "north",
        "combined-0123456789abcdef",
        "combined.0123456789ABCDEF",
        "combined.0123",
    ] {
        assert!(
            combined_model_id(refused)
                .unwrap_err()
                .starts_with("combined_identity_invalid"),
            "{refused}"
        );
    }
    // The revision a later generation expects as the combined model's head is
    // exactly the one publication assigns to the same package bytes.
    let bytes = package();
    let request = Request {
        project: "project-1".into(),
        model_kind: "general".into(),
        display_name: Some(COMBINED_DISPLAY_NAME.into()),
        ..Default::default()
    };
    let created = prepare_linked(&request, &bytes, &model_id, "").unwrap();
    assert_eq!(
        publication_revision(&bytes).unwrap(),
        created.plan("project-1", &bytes).unwrap().revision_id
    );
    assert!(publication_revision(b"invalid package")
        .unwrap_err()
        .contains("model_invalid"));
}
