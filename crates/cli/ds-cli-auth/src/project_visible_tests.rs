use super::project_visible::*;
use ds_cli_contract::outcome::Failure;
use serde_json::json;

#[test]
fn only_an_exact_listed_id_is_visible() {
    let rows = vec![json!({"ds_project": "abc_one"}), json!({"ds_project": "abc_two"})];
    assert!(listed(&rows, "abc_one"));
    assert!(!listed(&rows, "abc"));
    assert!(!listed(&rows, "bogus"));
    assert!(!listed(&[], "abc_one"));
}

#[test]
fn the_refusal_is_the_existing_project_not_visible_code() {
    let failure = not_visible();
    assert_eq!(failure.code(), "project_not_visible");
    assert_eq!(failure.class(), ds_cli_contract::ExitClass::InvalidInput);
    assert!(failure.remedy_text().is_some());
}

#[test]
fn only_a_network_away_directory_passes_unproven() {
    assert!(directory_unreachable(&Failure::unavailable("auth_transient", "away")));
    assert!(!directory_unreachable(&Failure::unauthorized("headless_signed_out", "out")));
    assert!(!directory_unreachable(&Failure::invalid("project_not_visible", "no")));
}

#[test]
fn only_refusals_a_missing_project_can_explain_are_rechecked() {
    assert!(could_follow_missing_project(&Failure::invalid("task_not_found", "no")));
    assert!(could_follow_missing_project(&Failure::failed("context_corrupt", "bad")));
    assert!(!could_follow_missing_project(&Failure::unauthorized("auth_rejected", "no")));
    assert!(!could_follow_missing_project(&Failure::conflict("work_revision_conflict", "moved")));
    assert!(!could_follow_missing_project(&Failure::unavailable("auth_transient", "away")));
    assert!(!could_follow_missing_project(&not_visible()));
}
