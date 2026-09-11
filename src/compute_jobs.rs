//! Browser-independent lifecycle and resource admission for native compute jobs.
//! Hosts persist these transitions atomically; a UI never schedules or owns a job.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// The closed engine request admitted with a durable compute job.  It is
/// persisted instead of inferred from a failed decoder, so a legacy endpoint
/// can never silently become an authority for another engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    FastLv,
    SolarPrepared,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub owner: String,
    pub lane: String,
    pub input_sha256: String,
    pub engine: EngineKind,
    /// Exact sealed input vocabulary admitted for this engine.
    pub input_tag: String,
    pub phase: Phase,
    pub attempts: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub worker: Option<String>,
    pub lease_until_ms: u64,
    pub result_sha256: Option<String>,
    pub error: Option<String>,
}

pub enum Event<'a> {
    Claim {
        worker: &'a str,
        lease_ms: u64,
    },
    Renew {
        worker: &'a str,
        lease_ms: u64,
    },
    Complete {
        worker: &'a str,
        result_sha256: &'a str,
    },
    Fail {
        worker: &'a str,
        error: &'a str,
    },
    Cancel,
}

pub fn transition(job: &Job, event: Event<'_>, now: u64) -> Result<Job, &'static str> {
    let mut next = job.clone();
    match event {
        Event::Claim { worker, lease_ms } => {
            if job.phase != Phase::Queued
                && !(job.phase == Phase::Running && job.lease_until_ms <= now)
            {
                return Err("job_not_claimable");
            }
            if worker.is_empty() || worker.len() > 128 || !(1000..=3_600_000).contains(&lease_ms) {
                return Err("job_lease_invalid");
            }
            next.phase = Phase::Running;
            next.worker = Some(worker.to_owned());
            next.lease_until_ms = now.saturating_add(lease_ms);
            next.attempts = next
                .attempts
                .checked_add(1)
                .ok_or("job_attempts_exhausted")?;
            next.error = None;
        }
        Event::Renew { worker, lease_ms } => {
            require_worker(job, worker, now)?;
            if !(1000..=3_600_000).contains(&lease_ms) {
                return Err("job_lease_invalid");
            }
            next.lease_until_ms = now.saturating_add(lease_ms);
        }
        Event::Complete {
            worker,
            result_sha256,
        } => {
            require_worker(job, worker, now)?;
            if !digest(result_sha256) {
                return Err("job_digest_invalid");
            }
            next.phase = Phase::Completed;
            next.result_sha256 = Some(result_sha256.to_owned());
            next.worker = None;
            next.lease_until_ms = 0;
        }
        Event::Fail { worker, error } => {
            require_worker(job, worker, now)?;
            next.phase = Phase::Failed;
            next.error = Some(error.chars().take(1024).collect());
            next.worker = None;
            next.lease_until_ms = 0;
        }
        Event::Cancel => {
            if !matches!(job.phase, Phase::Queued | Phase::Running) {
                return Err("job_already_terminal");
            }
            next.phase = Phase::Cancelled;
            next.worker = None;
            next.lease_until_ms = 0;
        }
    }
    next.updated_at_ms = now;
    Ok(next)
}
fn require_worker(job: &Job, worker: &str, now: u64) -> Result<(), &'static str> {
    if job.phase != Phase::Running
        || job.worker.as_deref() != Some(worker)
        || job.lease_until_ms <= now
    {
        Err("job_worker_fenced")
    } else {
        Ok(())
    }
}
pub fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Same resource policy for Desktop and Server. Native hosts measure resources;
/// the kernel decides admission. Rayon owns parallelism inside each request.
pub fn concurrent_jobs(logical_cpus: usize, available_memory_bytes: Option<u64>) -> usize {
    let memory_jobs = available_memory_bytes
        .map(|bytes| bytes.saturating_sub(1024 * 1024 * 1024) / (768 * 1024 * 1024))
        .unwrap_or(2);
    logical_cpus
        .max(1)
        .min(memory_jobs.max(1).min(usize::MAX as u64) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queued() -> Job {
        Job {
            id: "1".repeat(64),
            owner: "owner".into(),
            lane: "stable".into(),
            input_sha256: "2".repeat(64),
            engine: EngineKind::FastLv,
            input_tag: "ds.fast-lv.request/v1".into(),
            phase: Phase::Queued,
            attempts: 0,
            created_at_ms: 1,
            updated_at_ms: 1,
            worker: None,
            lease_until_ms: 0,
            result_sha256: None,
            error: None,
        }
    }
    #[test]
    fn expired_workers_cannot_commit_after_recovery() {
        let first = transition(
            &queued(),
            Event::Claim {
                worker: "first",
                lease_ms: 1000,
            },
            1,
        )
        .unwrap();
        assert!(transition(
            &first,
            Event::Claim {
                worker: "second",
                lease_ms: 1000
            },
            1000
        )
        .is_err());
        let recovered = transition(
            &first,
            Event::Claim {
                worker: "second",
                lease_ms: 1000,
            },
            1001,
        )
        .unwrap();
        assert_eq!(recovered.attempts, 2);
        assert!(transition(
            &recovered,
            Event::Complete {
                worker: "first",
                result_sha256: &"3".repeat(64)
            },
            1002
        )
        .is_err());
        let done = transition(
            &recovered,
            Event::Complete {
                worker: "second",
                result_sha256: &"3".repeat(64),
            },
            1002,
        )
        .unwrap();
        assert!(transition(
            &done,
            Event::Claim {
                worker: "third",
                lease_ms: 1000
            },
            5000
        )
        .is_err());
    }
    #[test]
    fn cancellation_fences_in_flight_completion() {
        let running = transition(
            &queued(),
            Event::Claim {
                worker: "first",
                lease_ms: 1000,
            },
            1,
        )
        .unwrap();
        let cancelled = transition(&running, Event::Cancel, 2).unwrap();
        assert!(transition(
            &cancelled,
            Event::Complete {
                worker: "first",
                result_sha256: &"3".repeat(64)
            },
            3
        )
        .is_err());
    }
    #[test]
    fn resource_limits_keep_parallelism_with_memory_reserve() {
        assert_eq!(concurrent_jobs(16, Some(7 * 1024 * 1024 * 1024)), 8);
        assert_eq!(concurrent_jobs(4, Some(32 * 1024 * 1024 * 1024)), 4);
        assert_eq!(concurrent_jobs(16, Some(100)), 1);
    }
}
