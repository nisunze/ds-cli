//! Controls for the progress-following job wait: it must still refuse a job
//! that never completes, however it fails to.

#[path = "common/job_wait_tests.rs"]
mod job_wait;
use ds_command_kernel::compute_jobs::{Job, Phase};
use job_wait::{
    CEILING, STALL, wait_for_completed_job, wait_for_completed_job_within, wait_for_completed_jobs,
    wait_for_completed_jobs_within,
};
use serde_json::json;
use std::time::Duration;

fn job(phase: &str, updated: u64) -> Job {
    serde_json::from_value(json!({
        "id": "j", "owner": "o", "lane": "canary", "input_sha256": "d",
        "engine": "fast_lv", "input_tag": "ds.fast-lv.request/v1", "phase": phase,
        "attempts": 1, "created_at_ms": 1u64, "updated_at_ms": updated,
        "worker": null, "lease_until_ms": 0u64, "result_sha256": null, "error": null
    }))
    .unwrap()
}

const SHORT: Duration = Duration::from_millis(300);

#[test]
#[should_panic(expected = "the submitted job row must remain persisted")]
fn a_row_that_was_never_written_is_refused() {
    wait_for_completed_job_within(|| None, SHORT, SHORT);
}

#[test]
#[should_panic(expected = "made no lifecycle or lease progress")]
fn a_job_that_stops_changing_is_refused() {
    wait_for_completed_job_within(|| Some(job("running", 5)), Duration::from_secs(30), SHORT);
}

#[test]
#[should_panic(expected = "exceeded the")]
fn a_job_that_keeps_moving_still_meets_the_ceiling() {
    let mut tick = 0;
    wait_for_completed_job_within(
        || {
            tick += 1;
            Some(job("running", tick))
        },
        SHORT,
        Duration::from_secs(30),
    );
}

#[test]
#[should_panic(expected = "stopped before completion")]
fn a_failed_job_is_refused_at_once() {
    wait_for_completed_job_within(|| Some(job("failed", 1)), Duration::from_secs(30), SHORT);
}

#[test]
fn a_slow_job_that_keeps_progressing_is_waited_for() {
    let mut tick = 0;
    let done = wait_for_completed_job_within(
        || {
            tick += 1;
            Some(job(if tick < 12 { "running" } else { "completed" }, tick))
        },
        Duration::from_secs(30),
        SHORT,
    );
    assert_eq!(done.phase, Phase::Completed);
}

#[test]
#[should_panic(expected = "made no lifecycle or lease progress")]
fn too_few_completed_jobs_are_not_enough() {
    wait_for_completed_jobs_within(
        || vec![job("completed", 1)],
        2,
        Duration::from_secs(30),
        SHORT,
    );
}

#[test]
fn the_default_wait_returns_completed_jobs_at_once() {
    assert!(STALL < CEILING);
    assert_eq!(
        wait_for_completed_job(|| Some(job("completed", 1)), CEILING).phase,
        Phase::Completed
    );
    assert_eq!(
        wait_for_completed_jobs(|| vec![job("completed", 1)], 1, CEILING).len(),
        1
    );
}
