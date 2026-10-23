use super::*;
use crate::gateway_exchange::retained::testing::{Answer, TestEdge, block_on};
const PHOTO: &str = "demo/forms/poles/entry/photo.jpg";
const BUCKET: &str = "demo.firebasestorage.app";
fn answer(path: &str, generation: &str) -> Value {
    json!({"data": {"blob_path": path, "bucket": BUCKET, "generation": generation,
        "source_url": format!("https://storage.googleapis.com/{BUCKET}/{path}?generation={generation}&X-Goog-Signature=test")}})
}
#[test]
fn original_address_and_pinned_generation_are_preserved() {
    for path in [
        PHOTO,
        "projects/demo/forms/poles/entry/photo.jpg",
        "projects/demo/forms/poles/entry/photo_1/photo.jpg",
    ] {
        let edge = TestEdge::new();
        edge.json(answer(path, "1234"));
        let found = block_on(inspect(&edge, path.into(), false))
            .unwrap()
            .unwrap();
        assert_eq!(found.blob_path, path);
        assert_eq!(found.generation, "1234");
        let sent = edge.requests();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].path, PATH);
        let mut expected = survey_media_address::wire(&survey_media_address::parse(path).unwrap());
        expected["action"] = json!("survey_upload_inspect");
        assert_eq!(sent[0].body, Some(expected));
    }
}
#[test]
fn optional_absence_is_only_404_and_invalid_addresses_send_nothing() {
    let edge = TestEdge::new();
    edge.answer(Answer::Json(404, json!({"error": "missing"})));
    assert!(
        block_on(inspect(&edge, PHOTO.into(), true))
            .unwrap()
            .is_none()
    );
    edge.answer(Answer::Json(403, json!({"error": "denied"})));
    assert_eq!(
        block_on(inspect(&edge, PHOTO.into(), true))
            .unwrap_err()
            .status,
        Some(403)
    );
    edge.answer(Answer::Json(404, json!({"error": "missing"})));
    assert_eq!(
        block_on(inspect(&edge, PHOTO.into(), false))
            .unwrap_err()
            .status,
        Some(404)
    );
    let count = edge.requests().len();
    assert!(block_on(inspect(&edge, "demo/forms/poles/entry".into(), false)).is_err());
    assert_eq!(edge.requests().len(), count);
}
#[test]
fn wrong_target_or_unpinned_source_is_refused_without_an_upload() {
    let edge = TestEdge::new();
    edge.json(answer("demo/forms/poles/other/photo.jpg", "1234"));
    assert_eq!(
        block_on(inspect(&edge, PHOTO.into(), false))
            .unwrap_err()
            .code,
        INVALID
    );
    let mut unpinned = answer(PHOTO, "1234");
    unpinned["data"]["source_url"] = json!(format!(
        "https://storage.googleapis.com/{BUCKET}/{PHOTO}?generation=1233"
    ));
    edge.json(unpinned);
    assert_eq!(
        block_on(inspect(&edge, PHOTO.into(), false))
            .unwrap_err()
            .code,
        INVALID
    );
    assert_eq!(edge.requests().len(), 2);
}
#[test]
fn the_declared_size_is_kept_for_the_read_and_a_malformed_one_is_refused() {
    let edge = TestEdge::new();
    let mut sized = answer(PHOTO, "1234");
    sized["data"]["size"] = json!(48_213);
    sized["data"]["content_type"] = json!("image/jpeg");
    edge.json(sized);
    let found = block_on(inspect(&edge, PHOTO.into(), false)).unwrap().unwrap();
    assert_eq!(found.size, Some(48_213));
    // An answer that declares none still inspects: the read is then bounded only.
    edge.json(answer(PHOTO, "1234"));
    assert_eq!(block_on(inspect(&edge, PHOTO.into(), false)).unwrap().unwrap().size, None);
    for size in [json!("48213"), json!(-1), json!(1.5)] {
        let mut malformed = answer(PHOTO, "1234");
        malformed["data"]["size"] = size.clone();
        edge.json(malformed);
        assert_eq!(
            block_on(inspect(&edge, PHOTO.into(), false)).unwrap_err().code,
            INVALID,
            "{size}"
        );
    }
}
