use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

#[test]
fn every_kept_address_shape_names_its_object() {
    for (address, path) in [
        ("gs://ds-media/eds_project/p/photo.jpg", Some("ds-media/eds_project/p/photo.jpg")),
        ("gs://bucket-only", None),
        ("https://storage.googleapis.com/ds-media/a%20b/c.pdf?X-Goog-Signature=x", Some("ds-media/a b/c.pdf")),
        ("https://storage.cloud.google.com/ds-media/x.gpkg", Some("ds-media/x.gpkg")),
        ("https://firebasestorage.googleapis.com/v0/b/app.appspot.com/o/eds%2Fphoto.jpg?alt=media", Some("app.appspot.com/eds/photo.jpg")),
        ("https://evil.example/storage.googleapis.com/ds-media/x", None),
        ("ftp://storage.googleapis.com/b/o", None),
        ("not a url", None),
    ] {
        assert_eq!(storage_path(address).as_deref(), path, "{address}");
    }
}

#[test]
fn a_batch_signs_distinct_paths_and_keeps_only_signed_answers() {
    let call = SignPaths::of(&[
        "gs://b/one.pdf".into(),
        "https://storage.googleapis.com/b/one.pdf?old=1".into(),
        "https://example.com/elsewhere".into(),
        "gs://b/two.pdf".into(),
    ]);
    assert_eq!(call.paths, ["b/one.pdf", "b/two.pdf"]);
    let Step::Fetch { request, continuation } = start(call) else { panic!("expected a fetch") };
    assert_eq!(request.path, "/report");
    assert_eq!(request.body, Some(json!({"action": "sign_url", "paths": ["b/one.pdf", "b/two.pdf"]})));
    let answer = json!({"urls": {"b/one.pdf": "https://storage.googleapis.com/b/one.pdf?sig=1",
        "b/two.pdf": "javascript:alert(1)", "b/other.pdf": "https://x/y"}, "mode": "signed",
        "refused": {"b/three.pdf": "not readable"}});
    let observed = Observed::Response { status: 200, content_type: Some("application/json".into()), retry_after: None };
    let Step::Done(urls) = resume(continuation, observed, answer.to_string().as_bytes(), &Facts::default()) else {
        panic!("expected the signed URLs")
    };
    assert_eq!(urls.len(), 1);
    assert_eq!(urls["b/one.pdf"], "https://storage.googleapis.com/b/one.pdf?sig=1");
    assert!(matches!(start(SignPaths { paths: vec![] }), Step::Refused(_)));
}

#[test]
fn bytes_are_exactly_the_declared_object() {
    let bytes = b"%PDF";
    assert_eq!(verify(bytes, 3, &"0".repeat(64)).unwrap_err().code, INTEGRITY);
    assert_eq!(verify(bytes, 4, &"0".repeat(64)).unwrap_err().code, INTEGRITY);
    use sha2::{Digest, Sha256};
    let real = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(verify(bytes, 4, &real), Ok(()));
    assert_eq!(verify(bytes, 4, &real.to_uppercase()), Ok(()));
}
