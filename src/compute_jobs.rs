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
    /// The immutable execution context admitted with this job: principal,
    /// lane, deployment, install, the project it is about, client, operation,
    /// job id and input digest (`crate::execution_context`). Rows are stored
    /// as one JSON blob, so this is a compatible addition with no SQL schema
    /// change: a row written by a Server released before it deserialises with
    /// `None` and re-serialises byte-identically. `transition` never touches
    /// it — no lifecycle event moves a job to another project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<crate::execution_context::ExecutionContext>,
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
            context: None,
        }
    }
    fn context() -> crate::execution_context::ExecutionContext {
        crate::execution_context::ExecutionContext {
            principal_uid: "uid-a".into(),
            lane: "stable".into(),
            deployment: "https://gateway.example".into(),
            install_id: "install-1".into(),
            project: "project-a".into(),
            client: "cli:4711".into(),
            operation: crate::execution_context::TRANSFORMER_PROCESSING.into(),
            job_id: "1".repeat(64),
            idempotency_key: "key-a".into(),
            input_sha256: "2".repeat(64),
            admitted_at_ms: 1,
        }
    }
    /// A row written by a Server released before execution contexts existed.
    /// Literal, not generated: the point is that TODAY's bytes still load.
    const LEGACY_ROW: &str = r#"{"id":"1111111111111111111111111111111111111111111111111111111111111111","owner":"owner","lane":"stable","input_sha256":"2222222222222222222222222222222222222222222222222222222222222222","engine":"fast_lv","input_tag":"ds.fast-lv.request/v1","phase":"queued","attempts":0,"created_at_ms":1,"updated_at_ms":1,"worker":null,"lease_until_ms":0,"result_sha256":null,"error":null}"#;

    #[test]
    fn a_row_stored_before_execution_contexts_still_loads_and_round_trips() {
        let job: Job = serde_json::from_str(LEGACY_ROW).expect("a released row still deserialises");
        assert_eq!(job, queued());
        assert_eq!(job.context, None);
        // A context-less row re-serialises to exactly the bytes it came from,
        // so adding the member cannot rewrite a released Server's queue.
        assert_eq!(serde_json::to_string(&job).unwrap(), LEGACY_ROW);
        // The member is additive, not required: a row that carries one is the
        // same row plus its context.
        let mut carrying = queued();
        carrying.context = Some(context());
        let text = serde_json::to_string(&carrying).unwrap();
        assert!(text.starts_with(LEGACY_ROW.trim_end_matches('}')));
        assert_eq!(
            serde_json::from_str::<Job>(&text).unwrap().context,
            Some(context())
        );
    }

    #[test]
    fn no_lifecycle_event_touches_the_execution_context() {
        let mut job = queued();
        job.context = Some(context());
        let claimed = transition(
            &job,
            Event::Claim {
                worker: "w",
                lease_ms: 1000,
            },
            1,
        )
        .unwrap();
        assert_eq!(claimed.context, job.context);
        for event in [
            Event::Renew {
                worker: "w",
                lease_ms: 1000,
            },
            Event::Complete {
                worker: "w",
                result_sha256: &"3".repeat(64),
            },
            Event::Fail {
                worker: "w",
                error: "boom",
            },
            Event::Cancel,
        ] {
            let next = transition(&claimed, event, 2).unwrap();
            assert_eq!(
                next.context, job.context,
                "a lifecycle event rewrote the execution context"
            );
        }
        // A retry after a lease takeover keeps it too.
        let retaken = transition(
            &claimed,
            Event::Claim {
                worker: "second",
                lease_ms: 1000,
            },
            1_002,
        )
        .unwrap();
        assert_eq!(retaken.context, job.context);
        assert_eq!(retaken.attempts, 2);
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
