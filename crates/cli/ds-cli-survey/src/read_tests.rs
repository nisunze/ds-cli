use super::*;
use ds_cli_contract::args::parse as parse_args;

pub(super) fn inputs(arguments: &[&str]) -> Inputs {
    parse_args(
        &COMMAND,
        &arguments
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("closed arguments")
}

#[test]
fn filters_parse_before_auth_and_the_default_bound_is_a_hundred() {
    let request = parse(&inputs(&[
        "--project",
        "p1",
        "--form",
        "transformer_survey",
        "--bbox",
        "29.7, -2.1,29.8,-2.0",
        "--updated-after",
        "2026-09-01T00:00:00Z",
    ]))
    .unwrap();
    assert_eq!(request.limit(), 100);
    assert_eq!(request.form(), "transformer_survey");
    assert_eq!(request.updated_after(), Some("2026-09-01T00:00:00Z"));
    for bad in [
        &["--limit", "0"][..],
        &["--limit", "5001"],
        &["--bbox", "1,2,3"],
        &["--bbox", "30,-2,29,-1"],
        &["--updated-after", "yesterday"],
    ] {
        let mut arguments = vec!["--project", "p1", "--form", "f"];
        arguments.extend_from_slice(bad);
        let failure = parse(&inputs(&arguments)).unwrap_err();
        assert_eq!(failure.code(), "survey_entries_invalid", "{bad:?}");
    }
}

#[test]
fn a_partial_read_says_so_in_words() {
    let text = render(
        &json!({"project":{"ds_project":"p1"},"form":"f","returned":100,
        "total":247,"media_count":88,"truncated":true}),
    );
    assert!(text.contains("100 of 247 entries  88 photos"));
    assert!(text.contains("NOT ALL"));
}

#[test]
fn an_unreadable_read_of_a_disabled_binding_names_the_binding() {
    let unreadable =
        || Failure::unavailable("survey_entries_unreadable", "Survey layers failed: f");
    let bindings = [("f", false), ("g", true)];
    let named = name_disabled(unreadable(), "p1", "f", bindings);
    assert_eq!(named.code(), "survey_form_disabled");
    assert!(!named.retryable());
    assert!(named.message().contains("`f` is disabled in project p1"));
    assert!(REFUSALS.iter().any(|refusal| refusal.code == named.code()));
    // An enabled or unknown binding keeps the stream refusal unchanged.
    for form in ["g", "absent"] {
        let kept = name_disabled(unreadable(), "p1", form, bindings);
        assert_eq!(kept.code(), "survey_entries_unreadable", "{form}");
        assert_eq!(kept.message(), "Survey layers failed: f");
    }
}

#[test]
fn only_an_unreadable_answer_is_diagnosed() {
    // Any other refusal returns before a binding read could be attempted.
    let transient = Failure::unavailable("survey_entries_transient", "service down");
    let kept = diagnose(transient, "stable", "p1", "f");
    assert_eq!(kept.code(), "survey_entries_transient");
}
