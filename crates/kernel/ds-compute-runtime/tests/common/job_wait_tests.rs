//! Waiting for durable jobs by their own progress instead of a fixed clock.
//!
//! A real native calculation can legitimately run longer than any guess. The
//! wait therefore ends on the jobs' own state: success when every expected job
//! is `Completed`; failure at once when one is `Failed` or `Cancelled`, when its
//! row disappears, when its lifecycle and lease stop changing for `stall`, or
//! when the generous overall `ceiling` passes.

use ds_command_kernel::compute_jobs::{Job, Phase};
use std::time::{Duration, Instant};

/// No lifecycle transition and no lease renewal for this long means the worker
/// is gone (a live run renews its 30 s lease every few seconds).
pub(crate) const STALL: Duration = Duration::from_secs(60);
/// Upper bound for one real native calculation under a loaded machine.
pub(crate) const CEILING: Duration = Duration::from_secs(180);

pub(crate) fn wait_for_completed_job(read: impl FnMut() -> Option<Job>, ceiling: Duration) -> Job {
    wait_for_completed_job_within(read, ceiling, STALL)
}

pub(crate) fn wait_for_completed_job_within(
    mut read: impl FnMut() -> Option<Job>,
    ceiling: Duration,
    stall: Duration,
) -> Job {
    wait_for_completed_jobs_within(
        || vec![read().expect("the submitted job row must remain persisted")],
        1,
        ceiling,
        stall,
    )
    .remove(0)
}

pub(crate) fn wait_for_completed_jobs(
    read: impl FnMut() -> Vec<Job>,
    expected: usize,
    ceiling: Duration,
) -> Vec<Job> {
    wait_for_completed_jobs_within(read, expected, ceiling, STALL)
}

pub(crate) fn wait_for_completed_jobs_within(
    mut read: impl FnMut() -> Vec<Job>,
    expected: usize,
    ceiling: Duration,
    stall: Duration,
) -> Vec<Job> {
    let deadline = Instant::now() + ceiling;
    let mut seen = None;
    let mut last_progress = Instant::now();
    loop {
        let jobs = read();
        if let Some(job) = jobs
            .iter()
            .find(|job| matches!(job.phase, Phase::Failed | Phase::Cancelled))
        {
            panic!("the job stopped before completion: {job:?}");
        }
        if jobs.len() == expected && jobs.iter().all(|job| job.phase == Phase::Completed) {
            return jobs;
        }
        let observed = jobs
            .iter()
            .map(|job| {
                (
                    job.id.clone(),
                    job.phase.clone(),
                    job.attempts,
                    job.updated_at_ms,
                    job.lease_until_ms,
                )
            })
            .collect::<Vec<_>>();
        if seen.as_ref() != Some(&observed) {
            seen = Some(observed);
            last_progress = Instant::now();
        }
        assert!(
            Instant::now() < deadline,
            "jobs exceeded the {ceiling:?} ceiling: {jobs:?}"
        );
        assert!(
            last_progress.elapsed() < stall,
            "jobs made no lifecycle or lease progress for {stall:?}: {jobs:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
