use super::*;
#[test]
fn exact_bytes_and_digests_cross_transport() {
    let document = GeneratedDocument::new("plots/map.png", "image/png", vec![0, 255, 128]);
    let output = encode_outputs([(
        document.name(),
        document.entry.media_type.as_str(),
        document.bytes.as_slice(),
    )])
    .unwrap();
    assert_eq!(output[0].entry, document.entry);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&output[0].body_base64)
        .unwrap();
    assert_eq!(
        output[0].entry,
        ArtifactEntry::from_bytes("plots/map.png", "image/png", &bytes)
    );
    assert_eq!(bytes, [0, 255, 128]);
}
#[test]
fn refuses_duplicate_and_escaping_names() {
    for name in ["../map.png", "/map.png", "plots//map.png", "plots\\map.png"] {
        assert!(encode_outputs([(name, "image/png", &[1][..])]).is_err());
    }
    assert!(encode_outputs([("map.png", "image/png", &[1][..]); 2]).is_err());
}
