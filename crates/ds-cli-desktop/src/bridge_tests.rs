use super::*;

#[test]
fn signed_in_local_desktop_is_carried_as_local_and_an_unknown_lane_is_unsupported() {
    // A development build's fence is served, as `local` (2026-09-20).
    let local = IdentityFence::from_session(&json!({
        "uid": "uid-1",
        "lane": "local",
        "credential_audience_sha256": "a".repeat(64),
        "project": "project-1",
        "session_revision": 7,
    }))
    .expect("a local Desktop is served on its own lane");
    assert_eq!(local.lane, "local");
    let error = IdentityFence::from_session(&json!({
        "uid": "uid-1",
        "lane": "nightly",
        "credential_audience_sha256": "a".repeat(64),
        "project": "project-1",
        "session_revision": 7,
    }))
    .expect_err("an unknown lane must not be fenced as if it were provisioned");
    assert_eq!(error.code(), "desktop_operation_unsupported");
    // The remedy is the one the command contracts publish under that
    // code, not a second text minted here: one code, one recovery.
    assert_eq!(error.remedy_text(), Some(crate::ops::UNSUPPORTED.remedy));
}

#[test]
fn structured_desktop_refusals_keep_code_class_and_remedy() {
    let failure = structured_desktop_refusal(
        "data.admin_bounds.read",
        &json!({"error": {
            "class": "unavailable",
            "code": "admin_authority_unavailable",
            "message": "authority did not answer",
            "remedy": "retry unchanged"
        }}),
        422,
    )
    .expect("typed refusal");
    assert_eq!(failure.code(), "admin_authority_unavailable");
    assert_eq!(failure.class().token(), "unavailable");
    assert_eq!(failure.message(), "authority did not answer");
    assert_eq!(failure.remedy_text(), Some("retry unchanged"));
    assert_eq!(failure.detail_value().unwrap()["http_status"], 422);
}

#[test]
fn malformed_structured_desktop_refusals_do_not_invent_codes() {
    // Shape, not membership, is what disqualifies a refusal now. An
    // unknown class is still refused rather than guessed, because the
    // class is what tells a caller whether retrying can ever work.
    for error in [
        json!({"class": "success", "code": "admin_authority_unavailable", "message": "bad class"}),
        json!({"class": "unavailable", "code": "NOT-STABLE", "message": "bad code"}),
        json!({"class": "unavailable", "code": "admin_authority_unavailable", "message": ""}),
        json!({"code": "admin_authority_unavailable", "message": "no class"}),
        json!("a legacy string error"),
    ] {
        assert!(
            structured_desktop_refusal(
                "data.admin_bounds.read",
                &json!({"error": error}),
                422,
            )
            .is_none()
        );
    }
}

/// Any operation may now carry a typed refusal, and every class maps.
///
/// This replaces an allowlist of two operations. It was there so the
/// application could not mint a code `ds` does not document; that
/// guarantee now lives in `every_application_refusal_code_is_documented`,
/// which fails the build instead of silently degrading the refusal. The
/// old behaviour is what a stress session met as `desktop_refused` for six
/// unrelated causes, including an input error reported as class `failed`.
#[test]
fn any_operation_may_carry_a_typed_refusal_and_every_class_maps() {
    for (class, expected) in [
        ("invalid_input", "invalid_input"),
        ("unavailable", "unavailable"),
        ("unauthorized", "unauthorized"),
        ("conflict", "conflict"),
        ("failed", "failed"),
        ("internal", "internal"),
    ] {
        let failure = structured_desktop_refusal(
            "design.process.batch",
            &json!({"error": {
                "class": class,
                "code": "report_grouping_not_prepared",
                "message": "prepare the applied grouping first",
                "remedy": "run consumer-grouping apply while online"
            }}),
            422,
        )
        .expect("typed refusal for a non-allowlisted operation");
        assert_eq!(failure.code(), "report_grouping_not_prepared");
        assert_eq!(failure.class().token(), expected);
        assert_eq!(
            failure.remedy_text(),
            Some("run consumer-grouping apply while online")
        );
    }
}

/// A refusal without a remedy stays a refusal, and says nothing it cannot.
#[test]
fn a_missing_or_empty_remedy_is_omitted_rather_than_invented() {
    for error in [
        json!({"class": "conflict", "code": "report_grouping_not_prepared", "message": "m"}),
        json!({"class": "conflict", "code": "report_grouping_not_prepared", "message": "m", "remedy": ""}),
    ] {
        let failure =
            structured_desktop_refusal("design.process.batch", &json!({"error": error}), 422)
                .expect("typed refusal");
        assert_eq!(failure.remedy_text(), None);
    }
}

/// The view a result would be written into is the view it was captured
/// in, or it is refused. `ds` carries the owner window's generation so the
/// application can answer that; an instance that publishes no roster is
/// fenced on the revision alone, as it always was.
#[test]
fn the_fence_carries_the_owner_windows_generation_when_there_is_one() {
    let session = json!({
        "uid": "uid-1",
        "lane": "stable",
        "credential_audience_sha256": "a".repeat(64),
        "project": "project-1",
        "session_revision": 7,
        "windows": [
            {"label": "workspace-2", "project": "project-2", "generation": 9},
            {"label": crate::discover::OWNER_WINDOW_LABEL, "project": "project-1", "generation": 3},
        ],
    });
    let fence = IdentityFence::from_session(&session).expect("a complete fence");
    assert_eq!(
        fence.context_generation,
        Some(3),
        "the owner window's generation is the one an operation is fenced on"
    );
    let sent = serde_json::to_value(&fence).expect("encodes");
    assert_eq!(sent["context_generation"], json!(3));

    let mut older = session.clone();
    older.as_object_mut().expect("an object").remove("windows");
    let fence = IdentityFence::from_session(&older).expect("a complete fence");
    assert_eq!(fence.context_generation, None);
    // The shell's fence denies unknown fields, so a null here would refuse
    // every call to a desktop that predates the roster.
    assert!(
        serde_json::to_value(&fence)
            .expect("encodes")
            .get("context_generation")
            .is_none()
    );
}

/// The context fence answers 409 with the same structured refusal an
/// operation's own answers 422 with, and its code survives the trip.
#[test]
fn a_stale_context_generation_keeps_the_code_the_owner_decided() {
    let failure = structured_desktop_refusal(
        "map.zoom_to",
        &json!({"error": {
            "class": "conflict",
            "code": "context_generation_stale",
            "message": "this view's project or account changed while that was in flight",
            "remedy": "retry the operation against the view as it is now"
        }}),
        409,
    )
    .expect("the context fence's refusal is structured");
    assert_eq!(failure.code(), "context_generation_stale");
    assert_eq!(failure.class().token(), "conflict");
    assert_eq!(
        failure.remedy_text(),
        Some("retry the operation against the view as it is now")
    );
}

#[test]
fn auth_context_code_accepts_legacy_and_structured_desktop_shapes() {
    assert_eq!(
        desktop_error_code(&json!({"error": "auth_context_mismatch"})),
        Some("auth_context_mismatch")
    );
    assert_eq!(
        desktop_error_code(&json!({"error": {"code": "auth_context_mismatch"}})),
        Some("auth_context_mismatch")
    );
}
