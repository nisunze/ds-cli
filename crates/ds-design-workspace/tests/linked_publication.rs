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
