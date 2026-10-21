use super::*;

fn engine_message(label: &str) -> writes::Refusal {
    writes::Refusal::InvalidValue(format!(
        "{label} must be a non-empty string no longer than 400 characters."
    ))
}

#[test]
fn an_over_long_title_is_text_too_long_with_the_bound() {
    let title = "a".repeat(20_000);
    let failure = refused_texts(engine_message("title"), &[("title", Some(&title))]);
    assert_eq!(failure.code(), "text_too_long");
    let detail = failure.detail_value().unwrap();
    assert_eq!(detail["flag"], "title");
    assert_eq!(detail["max_characters"], 400);
    assert_eq!(detail["given_characters"], 20_000);
}

#[test]
fn an_empty_title_keeps_the_vocabulary_refusal() {
    let failure = refused_texts(engine_message("title"), &[("title", Some(" "))]);
    assert_eq!(failure.code(), "invalid_choice");
}

#[test]
fn a_bound_about_another_flag_is_not_misattributed() {
    let long = "a".repeat(1_000);
    let failure = refused_texts(
        engine_message("description"),
        &[("title", Some(&long)), ("description", None)],
    );
    assert_eq!(failure.code(), "invalid_choice");
}

#[test]
fn the_boundary_itself_is_allowed_by_the_cli_classification() {
    let at_bound = "a".repeat(400);
    let failure = refused_texts(engine_message("title"), &[("title", Some(&at_bound))]);
    assert_eq!(failure.code(), "invalid_choice");
    assert_eq!(stated_bound("title must be a string no longer than 12 characters.", "title"), Some(12));
    assert_eq!(stated_bound("titles must be a string no longer than 12 characters.", "title"), None);
}
