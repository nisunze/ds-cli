#[test]
fn format_token_matches_the_owner() {
    assert_eq!(super::FORMAT, ds_grid_exchange::dsgrid::FORMAT);
}

#[test]
fn manifest_view_reads_the_owner_manifest() {
    let blank = ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest {
        model_id: Some("view-fixture"),
        coordinate_system: Some("EPSG:32735"),
        ..Default::default()
    })
    .unwrap();
    let inspection = ds_grid_exchange::dsgrid::inspect(&blank.bytes).unwrap();
    let manifest = inspection.detail["manifest"].clone();
    let owner: ds_grid_exchange::package::PackageManifest =
        serde_json::from_value(manifest.clone()).unwrap();
    let view: super::ManifestView = serde_json::from_value(manifest).unwrap();
    assert_eq!(view.model, owner.model);
    assert_eq!(view.members.len(), owner.members.len());
    for (name, record) in &owner.members {
        assert_eq!(view.members[name].byte_len, record.byte_len);
    }
}
