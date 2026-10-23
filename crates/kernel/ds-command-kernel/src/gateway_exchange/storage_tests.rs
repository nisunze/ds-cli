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

// ---- Which stored object a record's reference names ----------------------

const MEDIA: &str = "demo.firebasestorage.app";

fn object(bucket: &str, name: &str) -> Option<StoredObject> {
    Some(StoredObject { bucket: bucket.into(), object: name.into() })
}

#[test]
fn a_reference_names_its_own_bucket_or_the_deployments_media_bucket() {
    for (reference, expected) in [
        // Storage addresses name their own bucket, whatever the deployment's.
        ("gs://shared-media.firebasestorage.app/projects/p/forms/f/e/photo_1/1.jpg",
            object("shared-media.firebasestorage.app", "projects/p/forms/f/e/photo_1/1.jpg")),
        ("https://firebasestorage.googleapis.com/v0/b/electrification-rural.firebasestorage.app/o/projects%2Fp1%2Fforms%2Fpole%2Fe1%2Fphoto%2Fa.jpg?alt=media&token=t",
            object("electrification-rural.firebasestorage.app", "projects/p1/forms/pole/e1/photo/a.jpg")),
        ("https://storage.googleapis.com/electrification-rural.firebasestorage.app/projects/demo/forms/field_notes/entry-9/photo/customer_thunder.jpeg?X-Goog-Algorithm=GOOG4-RSA-SHA256",
            object("electrification-rural.firebasestorage.app", "projects/demo/forms/field_notes/entry-9/photo/customer_thunder.jpeg")),
        // A bare name lies in the one bucket the deployment signs in.
        ("projects/demo/forms/poles/e1/photo.jpg", object(MEDIA, "projects/demo/forms/poles/e1/photo.jpg")),
        ("demo/forms/poles/entry/photo.jpg", object(MEDIA, "demo/forms/poles/entry/photo.jpg")),
        // …unless it carries a bucket domain as its first segment.
        ("shared-media.firebasestorage.app/projects/demo/media/customer.jpg",
            object("shared-media.firebasestorage.app", "projects/demo/media/customer.jpg")),
        ("Legacy.AppSpot.com/p/forms/f/e/x.jpg", object("Legacy.AppSpot.com", "p/forms/f/e/x.jpg")),
        // Only the decoration a host put around the name goes.
        (" /projects/demo/media/customer.jpg?X-Goog-Algorithm=GOOG4#frag ",
            object(MEDIA, "projects/demo/media/customer.jpg")),
        // Nothing inside the name is repaired: the signer refuses it as written.
        ("demo\\forms\\poles\\entry\\photo.jpg", object(MEDIA, "demo\\forms\\poles\\entry\\photo.jpg")),
        ("demo/forms//entry/photo.jpg", object(MEDIA, "demo/forms//entry/photo.jpg")),
        ("project/forms//customers/entry-1/walkthrough.mp4/",
            object(MEDIA, "project/forms//customers/entry-1/walkthrough.mp4/")),
        // A project segment never names a bucket.
        ("arjgpydw_survey_test/forms/lv/6c2e/photo/1.jpg", object(MEDIA, "arjgpydw_survey_test/forms/lv/6c2e/photo/1.jpg")),
    ] {
        assert_eq!(StoredObject::resolve(reference, Some(MEDIA)), expected, "{reference}");
    }
}

#[test]
fn a_reference_that_is_not_a_stored_object_names_none() {
    for reference in [
        "",
        "   ",
        "?alt=media",
        "/",
        "data:image/jpeg;base64,AAAA",
        "blob:https://app.example/5e1",
        "https://example.com/media/inspection.webm",
        "file:///etc/passwd",
        "C:\\Users\\field\\photo.jpg",
        "javascript:alert(1)",
        "gs://bucket-only",
        "shared-media.firebasestorage.app/",
    ] {
        assert_eq!(StoredObject::resolve(reference, Some(MEDIA)), None, "{reference:?}");
    }
    // Without a media bucket a bare name lies nowhere; an address still resolves.
    assert_eq!(StoredObject::resolve("demo/forms/f/e/a.jpg", None), None);
    assert_eq!(StoredObject::resolve("demo/forms/f/e/a.jpg", Some("")), None);
    assert_eq!(StoredObject::resolve("gs://b/a.jpg", None), object("b", "a.jpg"));
}

#[test]
fn the_media_bucket_is_this_builds_firebase_projects_default_bucket() {
    assert_eq!(
        media_bucket(),
        crate::gateway_target::FIREBASE_PROJECT.map(|project| format!("{project}.firebasestorage.app"))
    );
}

// ---- Signing exactly one object ------------------------------------------

fn photo() -> StoredObject {
    StoredObject { bucket: MEDIA.into(), object: "demo/forms/poles/entry/photo.jpg".into() }
}

const SIGNED: &str = "https://storage.googleapis.com/demo.firebasestorage.app/demo/forms/poles/entry/photo.jpg?X-Goog-Algorithm=GOOG4-RSA-SHA256&X-Goog-Signature=abc";

fn signing(object: StoredObject, status: u16, answer: Value) -> Result<String, Refusal> {
    let Step::Fetch { request, continuation } = start(SignObject(object.clone())) else {
        panic!("a valid object is signed")
    };
    assert_eq!(request.path, "/report");
    assert_eq!(request.body, Some(json!({"action": "sign_url", "paths": [object.path()]})));
    let observed = Observed::Response { status, content_type: Some("application/json".into()), retry_after: None };
    match resume(continuation, observed, answer.to_string().as_bytes(), &Facts { has_session: true, ..Facts::default() }) {
        Step::Done(url) => Ok(url),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("one answer ends the signing"),
    }
}

#[test]
fn one_object_is_signed_on_its_own_and_its_url_must_read_exactly_it() {
    assert_eq!(signing(photo(), 200, json!({"urls": {photo().path(): SIGNED}, "mode": "signed"})), Ok(SIGNED.into()));
    // The success envelope and additive fields are compatible.
    let enveloped = json!({"success": true, "data": {"urls": {photo().path(): SIGNED, "other/x.jpg": "https://x/y"},
        "mode": "signed", "expires_at": "2026-10-22T10:00:00Z", "refused": {"other/z.jpg": "not a member"}}});
    assert_eq!(signing(photo(), 200, enveloped), Ok(SIGNED.into()));
    // Go escapes every reserved character; the comparison is of the name.
    let odd = StoredObject { bucket: MEDIA.into(), object: "p/forms/f/e/MV Plan (A3)+1.pdf".into() };
    let escaped = "https://storage.googleapis.com/demo.firebasestorage.app/p/forms/f/e/MV%20Plan%20%28A3%29%2B1.pdf?X-Goog-Signature=s";
    assert_eq!(signing(odd.clone(), 200, json!({"urls": {odd.path(): escaped}})), Ok(escaped.into()));
}

#[test]
fn a_signing_of_anything_else_is_a_malformed_answer() {
    let path = photo().path();
    for url in [
        "https://storage.googleapis.com/demo.firebasestorage.app/demo/forms/poles/entry/other.jpg?sig=1",
        "https://storage.googleapis.com/other-bucket/demo/forms/poles/entry/photo.jpg?sig=1",
        "http://storage.googleapis.com/demo.firebasestorage.app/demo/forms/poles/entry/photo.jpg?sig=1",
        "https://firebasestorage.googleapis.com/v0/b/demo.firebasestorage.app/o/demo%2Fforms%2Fpoles%2Fentry%2Fphoto.jpg?alt=media",
        "https://user:pw@storage.googleapis.com/demo.firebasestorage.app/demo/forms/poles/entry/photo.jpg",
        "https://storage.googleapis.com:8443/demo.firebasestorage.app/demo/forms/poles/entry/photo.jpg",
        "https://storage.googleapis.com/demo.firebasestorage.app/demo/forms/poles/entry/photo.jpg#x",
        "https://evil.example/demo.firebasestorage.app/demo/forms/poles/entry/photo.jpg",
        "javascript:alert(1)",
        "not a url",
    ] {
        let refusal = signing(photo(), 200, json!({"urls": {&path: url}})).unwrap_err();
        assert_eq!(refusal.code, READ_RESPONSE_INVALID, "{url}");
    }
    let long = format!("{SIGNED}&pad={}", "x".repeat(SIGNED_URL_MAX));
    assert_eq!(signing(photo(), 200, json!({"urls": {&path: long}})).unwrap_err().code, READ_RESPONSE_INVALID);
    for answer in [
        json!({}),
        json!({"urls": []}),
        json!({"urls": {&path: 7}}),
        json!({"urls": {&path: SIGNED}, "refused": {&path: "not a member"}}),
        json!({"urls": {}, "refused": "not a member"}),
        json!({"urls": {}, "refused": {&path: 3}}),
    ] {
        assert_eq!(signing(photo(), 200, answer.clone()).unwrap_err().code, READ_RESPONSE_INVALID, "{answer}");
    }
}

#[test]
fn a_refused_signing_is_forbidden_and_an_unsigned_one_is_not_readable_now() {
    let path = photo().path();
    let forbidden = signing(photo(), 200, json!({"urls": {}, "refused": {&path: "not a member of the project\n"}})).unwrap_err();
    assert_eq!(forbidden.code, READ_FORBIDDEN);
    assert_eq!(forbidden.status, Some(403));
    assert!(forbidden.message.contains("not a member of the project"), "{}", forbidden.message);
    assert!(!forbidden.message.contains('\n'));
    let long = signing(photo(), 200, json!({"urls": {}, "refused": {&path: "r".repeat(5_000)}})).unwrap_err();
    assert!(long.message.len() < 400);
    // Brain leaves a path it failed to sign out of both maps.
    let unsigned = signing(photo(), 200, json!({"urls": {}, "mode": "signed"})).unwrap_err();
    assert_eq!(unsigned.code, READ_FAILED);
    assert!(unsigned.transient && unsigned.retryable);
}

#[test]
fn an_object_no_signer_could_be_asked_for_sends_nothing() {
    for (bucket, name) in [
        ("", "a.jpg"),
        (MEDIA, ""),
        ("two/buckets", "a.jpg"),
        ("spaced bucket", "a.jpg"),
        (MEDIA, "line\nbreak.jpg"),
    ] {
        let call = SignObject(StoredObject { bucket: bucket.into(), object: name.into() });
        let Step::Refused(refusal) = start(call) else { panic!("{bucket}/{name} must not be signed") };
        assert_eq!(refusal.code, READ_REQUEST_INVALID);
    }
    let long = SignObject(StoredObject { bucket: MEDIA.into(), object: "o".repeat(OBJECT_NAME_MAX + 1) });
    assert!(matches!(start(long), Step::Refused(_)));
}

// ---- One read: its request, storage's refusals, its bytes ----------------

fn read(value: Value) -> Result<AdmittedRead, Refusal> {
    serde_json::from_value::<StoredObjectRead>(value)
        .map_err(|error| Refusal::new(READ_REQUEST_INVALID, error.to_string()))?
        .admit(Some(MEDIA))
}

#[test]
fn a_read_names_one_object_and_bounds_what_it_holds() {
    let admitted = read(json!({"bucket": MEDIA, "object": "demo/a.jpg"})).unwrap();
    assert_eq!(admitted.max_bytes, crate::survey_photo_media::MAX_INPUT_BYTES as u64);
    assert_eq!(admitted.object, StoredObject { bucket: MEDIA.into(), object: "demo/a.jpg".into() });
    let sha = "A".repeat(64);
    let pinned = read(json!({"bucket": MEDIA, "object": "demo/a.pdf", "max_bytes": 10, "size_bytes": 4, "sha256": sha})).unwrap();
    assert_eq!(pinned.sha256.as_deref(), Some("a".repeat(64).as_str()));
    assert_eq!(pinned.size_bytes, Some(4));
    for malformed in [
        json!({"object": "demo/a.jpg"}),
        json!({"bucket": MEDIA}),
        json!({"bucket": MEDIA, "object": "demo/a.jpg", "max_bytes": 0}),
        json!({"bucket": MEDIA, "object": "demo/a.jpg", "max_bytes": -1}),
        json!({"bucket": MEDIA, "object": "demo/a.jpg", "sha256": "abc"}),
        json!({"bucket": MEDIA, "object": "demo/a.jpg", "url": "https://storage.googleapis.com/x/y"}),
        json!({"bucket": "", "object": "demo/a.jpg"}),
    ] {
        assert_eq!(read(malformed.clone()).unwrap_err().code, READ_REQUEST_INVALID, "{malformed}");
    }
    let oversized = read(json!({"bucket": MEDIA, "object": "demo/a.pdf", "max_bytes": 3, "size_bytes": 4})).unwrap_err();
    assert_eq!(oversized.code, READ_TOO_LARGE);
}

#[test]
fn a_read_names_its_object_by_reference_or_by_resolution_never_both() {
    let by_reference = read(json!({"reference": "gs://other.firebasestorage.app/p/forms/f/e/a.jpg"})).unwrap();
    assert_eq!(by_reference.object, StoredObject { bucket: "other.firebasestorage.app".into(), object: "p/forms/f/e/a.jpg".into() });
    assert_eq!(by_reference.given, None);
    let bare = read(json!({"reference": "demo/forms/f/e/a.jpg"})).unwrap();
    assert_eq!(bare.object.bucket, MEDIA);
    // A signed storage URL keeps its own authority for when no fresh one is granted.
    let signed = read(json!({"reference": SIGNED, "max_bytes": 1024})).unwrap();
    assert_eq!(signed.object, photo());
    assert_eq!(signed.given.as_deref(), Some(SIGNED));
    for malformed in [
        json!({}),
        json!({"reference": "data:image/jpeg;base64,AAAA"}),
        json!({"reference": "https://example.com/report.pdf"}),
        json!({"reference": "gs://b/a.jpg", "bucket": MEDIA, "object": "a.jpg"}),
        json!({"reference": "gs://b/a.jpg", "object": "a.jpg"}),
        json!({"bucket": MEDIA}),
    ] {
        assert_eq!(read(malformed.clone()).unwrap_err().code, READ_REQUEST_INVALID, "{malformed}");
    }
}

#[test]
fn only_a_storage_url_carrying_its_own_authority_is_read_as_given() {
    let token = "https://firebasestorage.googleapis.com/v0/b/ds-testing-ground.firebasestorage.app/o/p%2Fforms%2Ff%2Fe%2Fa.jpg?alt=media&token=abc";
    let v2 = "https://storage.googleapis.com/b/report.pdf?GoogleAccessId=sa&Expires=1&Signature=sig";
    for (reference, given) in [
        (SIGNED, Some(SIGNED)),
        (token, Some(token)),
        (v2, Some(v2)),
        ("https://storage.googleapis.com/b/report.pdf", None),
        ("https://storage.googleapis.com/b/report.pdf?X-Goog-Signature=", None),
        ("https://firebasestorage.googleapis.com/v0/b/b/o/a.jpg?alt=media", None),
        ("https://storage.googleapis.com/b/report.pdf?token=abc", None),
        ("https://firebasestorage.googleapis.com/v0/b/b/o/a.jpg?X-Goog-Signature=s", None),
        ("http://storage.googleapis.com/b/report.pdf?X-Goog-Signature=s", None),
        ("https://user@storage.googleapis.com/b/report.pdf?X-Goog-Signature=s", None),
        ("https://storage.googleapis.com/b/report.pdf?X-Goog-Signature=s#x", None),
        ("https://evil.example/b/report.pdf?X-Goog-Signature=s", None),
        ("https://storage.cloud.google.com/b/report.pdf?X-Goog-Signature=s", None),
        ("gs://b/report.pdf", None),
    ] {
        assert_eq!(signed_url(reference).as_deref(), given, "{reference}");
    }
}

#[test]
fn a_fresh_signature_is_read_first_and_the_references_own_authority_only_when_none_is_granted() {
    let fresh = "https://storage.googleapis.com/b/o?X-Goog-Signature=fresh";
    assert_eq!(read_target(Ok(fresh.into()), Some(SIGNED)), Ok(fresh.into()));
    let forbidden = Refusal::new(READ_FORBIDDEN, "not a project path");
    assert_eq!(read_target(Err(forbidden.clone()), Some(SIGNED)), Ok(SIGNED.into()));
    assert_eq!(read_target(Err(forbidden.clone()), None), Err(forbidden));
    let unsigned = Refusal::new(READ_FAILED, "unsigned");
    assert_eq!(read_target(Err(unsigned), Some(SIGNED)), Ok(SIGNED.into()));
    for code in [crate::gateway_exchange::SESSION_CHANGED, crate::gateway_exchange::NETWORK_OFFLINE,
        crate::gateway_exchange::SIGNED_OUT, READ_CANCELLED] {
        let stop = Refusal::new(code, "stop");
        assert_eq!(read_target(Err(stop.clone()), Some(SIGNED)), Err(stop), "{code}");
    }
}

#[test]
fn storage_refusals_are_typed_by_what_storage_said() {
    let xml = |code: &str, details: &str| format!(
        "<?xml version='1.0' encoding='UTF-8'?><Error><Code>{code}</Code><Message>m</Message><Details>{details}</Details></Error>"
    );
    let cases: [(u16, String, &str, bool); 9] = [
        (404, xml("NoSuchKey", "No such object: b/o"), OBJECT_MISSING, false),
        (404, String::new(), OBJECT_MISSING, false),
        (400, xml("ExpiredToken", "Request signature expired at: 2026-10-22T09:00:00+00:00"), READ_EXPIRED, false),
        (403, xml("AccessDenied", "Request has expired: 1729587600"), READ_EXPIRED, false),
        (403, xml("AccessDenied", "Access denied."), READ_FORBIDDEN, false),
        (403, xml("SignatureDoesNotMatch", "The request signature we calculated does not match"), READ_FORBIDDEN, false),
        (401, String::new(), READ_FORBIDDEN, false),
        (503, xml("ServiceUnavailable", "backend"), READ_FAILED, true),
        (429, String::new(), READ_FAILED, true),
    ];
    for (status, body, code, transient) in cases {
        let refusal = read_refusal(status, body.as_bytes());
        assert_eq!((refusal.code.as_str(), refusal.transient), (code, transient), "{status} {body}");
        assert_eq!(refusal.status, Some(status));
    }
    let other = read_refusal(400, xml("InvalidArgument", "bad").as_bytes());
    assert_eq!((other.code.as_str(), other.transient), (READ_FAILED, false));
    // Only the head of a body is read; an expiry past it is not invented.
    let padded = format!("{}{}", " ".repeat(ERROR_BODY_CAP), xml("ExpiredToken", ""));
    assert_eq!(read_refusal(400, padded.as_bytes()).code, READ_FAILED);
}

#[test]
fn bytes_are_bounded_and_exactly_what_the_read_declared() {
    use sha2::{Digest, Sha256};
    let bytes = b"%PDF-1.7";
    let sha = format!("{:x}", Sha256::digest(bytes));
    let plain = read(json!({"bucket": MEDIA, "object": "a.pdf", "max_bytes": 64})).unwrap();
    assert_eq!(admit_bytes(bytes, &plain), Ok(()));
    let tight = read(json!({"bucket": MEDIA, "object": "a.pdf", "max_bytes": 4})).unwrap();
    assert_eq!(admit_bytes(bytes, &tight).unwrap_err().code, READ_TOO_LARGE);
    let sized = read(json!({"bucket": MEDIA, "object": "a.pdf", "max_bytes": 64, "size_bytes": 8})).unwrap();
    assert_eq!(admit_bytes(bytes, &sized), Ok(()));
    assert_eq!(admit_bytes(b"%PDF-1.", &sized).unwrap_err().code, INTEGRITY);
    let digested = read(json!({"bucket": MEDIA, "object": "a.pdf", "max_bytes": 64, "sha256": sha})).unwrap();
    assert_eq!(admit_bytes(bytes, &digested), Ok(()));
    assert_eq!(admit_bytes(b"%PDF-1.8", &digested).unwrap_err().code, INTEGRITY);
    let both = read(json!({"bucket": MEDIA, "object": "a.pdf", "max_bytes": 64, "size_bytes": 8, "sha256": sha})).unwrap();
    assert_eq!(admit_bytes(bytes, &both), Ok(()));
    assert_eq!(admit_bytes(b"%PDF-1.8", &both).unwrap_err().code, INTEGRITY);
}

#[test]
fn a_read_of_a_declared_size_is_exactly_that_size() {
    let bytes = || Ok(b"%PDF-1.7".to_vec());
    assert_eq!(admit_declared_size(Some(8), bytes()), Ok(b"%PDF-1.7".to_vec()));
    assert_eq!(admit_declared_size(None, bytes()), Ok(b"%PDF-1.7".to_vec()));
    assert_eq!(admit_declared_size(Some(9), bytes()).unwrap_err().code, INTEGRITY);
    assert_eq!(admit_declared_size(Some(7), bytes()).unwrap_err().code, INTEGRITY);
    // Storage sent more than declared: the read stopped at the declaration.
    assert_eq!(admit_declared_size(Some(7), Err(too_large(7))).unwrap_err().code, INTEGRITY);
    // Undeclared, too large stays too large; other refusals pass through.
    assert_eq!(admit_declared_size(None, Err(too_large(7))).unwrap_err().code, READ_TOO_LARGE);
    let missing = read_refusal(404, b"");
    assert_eq!(admit_declared_size(Some(7), Err(missing.clone())), Err(missing));
}
