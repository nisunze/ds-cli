use super::*;

#[test]
fn case_only_tag_refusal_has_one_actionable_public_code() {
    let failure = Failure::invalid(
        "design_request_invalid",
        "phase allows \"Phase II\", not \"phase ii\"; use the authored spelling exactly",
    )
    .detail(json!({ "http_status": 400 }));

    let classified = classify_design_failure(failure);
    assert_eq!(classified.code(), "tag_value_case_mismatch");
    assert_eq!(
        classified.remedy_text(),
        Some(TAG_VALUE_CASE_MISMATCH.remedy)
    );
}

#[test]
fn unrelated_design_refusal_stays_generic() {
    let failure = Failure::invalid("design_request_invalid", "the transformer does not exist")
        .detail(json!({ "http_status": 400 }));

    assert_eq!(
        classify_design_failure(failure).code(),
        "design_request_invalid"
    );
}

/// A signed-out or paired-window identity is not this domain's to refine:
/// the headless client answers `headless_signed_out` itself, and nothing
/// here renames a code it does not own.
#[test]
fn foreign_identities_pass_through_untouched() {
    let failure = Failure::unauthorized("headless_signed_out", "no native user is signed in")
        .detail(json!({ "detail": "permission denied" }));
    assert_eq!(
        classify_design_failure(failure).code(),
        "headless_signed_out"
    );
}

/// `ds auth` types a refused envelope from its HTTP status before this
/// domain sees it, so its conditions arrive with a coarse code and the
/// route's prose in `message`. They are still the same conditions, and
/// their remedies are not interchangeable: no admin can grant a capability
/// on an archived project.
#[test]
fn a_status_typed_refusal_is_still_refined_to_its_own_condition() {
    let archived = Failure::unauthorized(
        "design_not_permitted",
        "this project is archived and accepts no design changes (403)",
    )
    .detail(json!({ "http_status": 422 }));
    let archived = classify_design_failure(archived);
    assert_eq!(archived.code(), "design_project_read_only");
    assert_eq!(archived.remedy_text(), Some(READ_ONLY.remedy));

    let case_only = Failure::invalid(
        "design_request_invalid",
        "city allows \"Kigali\", not \"kigali\"; use the authored spelling exactly (400)",
    )
    .detail(json!({ "http_status": 422 }));
    let case_only = classify_design_failure(case_only);
    assert_eq!(case_only.code(), "tag_value_case_mismatch");
    assert_eq!(
        case_only.remedy_text(),
        Some(TAG_VALUE_CASE_MISMATCH.remedy)
    );

    // An API that never answered says nothing about authority, so it is
    // never refined into one — even when its message happens to carry a
    // word one of the branches reads.
    let unreachable = Failure::unavailable(
        "auth_transient",
        "the Data Solutions API did not answer; the project may be archived",
    );
    assert_eq!(
        classify_design_failure(unreachable).code(),
        "auth_transient"
    );

    // A status-typed refusal with no condition in it keeps the identity the
    // application gave it, rather than being renamed for want of a match.
    let plain = Failure::invalid(
        "design_request_invalid",
        "tag query matched more than limit 200; raise limit explicitly (400)",
    )
    .detail(json!({ "http_status": 422 }));
    assert_eq!(
        classify_design_failure(plain).code(),
        "design_request_invalid"
    );
}
