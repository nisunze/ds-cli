use super::*;

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn real_engine_identity_matches_full_package_crs_revision_digest_and_authored_head() {
    let client = crate::native::capture().unwrap();
    let blank = ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest {
        model_id: Some("identity-process-fixture"),
        coordinate_system: Some("EPSG:32735"),
        ..Default::default()
    })
    .unwrap()
    .bytes;
    let fixture = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../ds-network/crates/ds-grid-exchange/tests/fixtures/native-engine-boundary/v1/model.dsgrid"
    ));
    let current = ds_grid_exchange::package_migration::migrate(fixture).unwrap();
    let historical = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../ds-network/crates/ds-grid-exchange/tests/fixtures/ground-basis-schema4/schema4.dsgrid"
    ));
    // Existing read-through migration is owner behavior and retains source bytes.
    for bytes in [
        blank.as_slice(),
        current.bytes.as_slice(),
        historical.as_slice(),
    ] {
        let package = ds_grid_exchange::package::unpack(bytes).unwrap();
        let old = ds_grid_engine::GridSession::open(package.snapshot);
        let identity = identity_with_client(bytes, &client).unwrap();
        assert_eq!(
            identity.crs,
            package.manifest.model.coordinate_system.to_string()
        );
        assert_eq!(
            identity.model_revision,
            package.manifest.model.model_revision
        );
        assert_eq!(identity.bytes, bytes.len() as u64);
        assert_eq!(identity.sha256, format!("{:x}", Sha256::digest(bytes)));
        assert_eq!(
            identity.authored_revision,
            old.current_revision().revision_id.as_str()
        );
    }
    assert_eq!(
        identity_with_client(b"not a package", &client)
            .err()
            .unwrap()
            .code(),
        "not_a_dsgrid_package"
    );
    // Corrupt a table payload while leaving the manifest and ZIP directory intact.
    // A manifest-only host read must not be mistaken for full identity admission.
    let mut corrupt = blank.clone();
    let header = corrupt
        .windows(4)
        .enumerate()
        .find_map(|(offset, signature)| {
            if signature != b"PK\x03\x04" || offset + 30 > corrupt.len() {
                return None;
            }
            let name_len =
                u16::from_le_bytes([corrupt[offset + 26], corrupt[offset + 27]]) as usize;
            let extra_len =
                u16::from_le_bytes([corrupt[offset + 28], corrupt[offset + 29]]) as usize;
            let name = corrupt.get(offset + 30..offset + 30 + name_len)?;
            name.starts_with(b"tables/")
                .then_some(offset + 30 + name_len + extra_len)
        })
        .expect("fixture table member");
    corrupt[header] ^= 0x7f;
    assert!(crate::package::declared_model_id(&corrupt).is_some());
    assert!(ds_grid_exchange::package::unpack(&corrupt).is_err());
    assert!(identity_with_client(&corrupt, &client).is_err());
}
