#[path = "common/prepared_cases_tests.rs"]
mod cases;

#[test]
fn native_file_task_matches_cloud_semantics_and_makes_zero_provider_requests() {
    cases::native_file_task_matches_cloud_semantics_and_makes_zero_provider_requests(env!(
        "CARGO_BIN_EXE_ds-solar"
    ));
}

#[test]
fn malformed_or_unprepared_requests_leave_no_output_or_stage() {
    cases::malformed_or_unprepared_requests_leave_no_output_or_stage(env!(
        "CARGO_BIN_EXE_ds-solar"
    ));
}
