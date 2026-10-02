use ds_command_kernel::survey_control::{
    form_purge_plan_response, form_purge_response, FormPurge, SurveyControlCommand,
    FORM_PURGE_COUNTS,
};
use serde_json::{json, Value};

fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}
fn counts(n: u64) -> Value {
    FORM_PURGE_COUNTS
        .iter()
        .map(|key| (key.to_string(), json!(n)))
        .collect()
}
fn request() -> FormPurge {
    FormPurge {
        slug: "field_notes_-_areas".into(),
        plan_digest: digest(),
    }
}
fn receipt() -> Value {
    json!({"receipt":{"slug":request().slug,"plan_digest":digest(),"state":"complete","scope":counts(1),"remaining":counts(0),"projects":[],"errors":[]}})
}

#[test]
fn purge_request_pins_one_slug_and_lowercase_sha256_without_arbitrary_controls() {
    SurveyControlCommand::FormPurge(request())
        .validate()
        .unwrap();
    for (slug, digest) in [
        ("../field", digest()),
        ("field_notes_-_areas", "sha256:abc".into()),
        ("field", format!("sha256:{}", "A".repeat(64))),
    ] {
        assert!(SurveyControlCommand::FormPurge(FormPurge {
            slug: slug.into(),
            plan_digest: digest
        })
        .validate()
        .is_err());
    }
    assert!(serde_json::from_value::<SurveyControlCommand>(json!({"operation":"survey.form.purge","arguments":{"slug":"field","planDigest":digest(),"url":"https://example.invalid"}})).is_err());
}
#[test]
fn plan_requires_complete_counts_identity_and_bounded_scope() {
    let base = json!({"plan":{"slug":request().slug,"plan_digest":digest(),"scope":counts(1),"projects":[],"templates":[]}});
    form_purge_plan_response(base.clone(), &request().slug).unwrap();
    let mut wrong = base.clone();
    wrong["plan"]["slug"] = json!("other");
    assert!(form_purge_plan_response(wrong, &request().slug).is_err());
    let mut missing = base.clone();
    missing["plan"]["scope"]
        .as_object_mut()
        .unwrap()
        .remove("media_versions");
    assert!(form_purge_plan_response(missing, &request().slug).is_err());
    let mut unbounded = base;
    unbounded["plan"]["projects"] = json!(vec![json!({}); 1001]);
    assert!(form_purge_plan_response(unbounded, &request().slug).is_err());
}
#[test]
fn successful_http_cannot_hide_partial_remaining_or_wrong_digest() {
    form_purge_response(receipt(), &request()).unwrap();
    let mut wire = receipt();
    wire["receipt"]["remaining"]["entry_documents"] = json!(1);
    assert!(form_purge_response(wire.clone(), &request()).is_err());
    wire["receipt"]["state"] = json!("partial");
    assert_eq!(
        form_purge_response(wire.clone(), &request()).unwrap()["receipt"]["state"],
        "partial"
    );
    wire["receipt"]["plan_digest"] = json!(format!("sha256:{}", "b".repeat(64)));
    assert!(form_purge_response(wire, &request()).is_err());
    let mut missing = receipt();
    missing["receipt"]["remaining"]
        .as_object_mut()
        .unwrap()
        .remove("schemas");
    assert!(form_purge_response(missing, &request()).is_err());
}
