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
    /// Historic queue rows remain readable; newly written jobs use the
    /// single LV processing name rather than advertising another mode.
    #[serde(alias = "fast_lv")]
    LvProcessing,
    SolarPrepared,
    TilesPrepared,
}

/// The native authority lane is captured by the host, never authored input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineLane {
    Stable,
    Canary,
}
impl EngineLane {
    pub fn parse(lane: &str) -> Result<Self, &'static str> {
        match lane {
            "stable" => Ok(Self::Stable),
            "canary" => Ok(Self::Canary),
            _ => Err("compute_engine_pin_lane_invalid"),
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Canary => "canary",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnginePinSource {
    /// The native owner's explicit lane resolver, including its admitted
    /// development override or packaged fallback. This is not installation trust.
    NativeLaneCapture,
}

/// Portable storage envelope. Native owners validate their own identity DTO;
/// the kernel binds that opaque identity and executable to exactly one job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnginePin {
    pub schema: String,
    pub engine: EngineKind,
    pub owner: String,
    pub lane: EngineLane,
    pub context: crate::execution_context::ExecutionContext,
    pub source: EnginePinSource,
    pub executable: String,
    pub executable_sha256: String,
    pub identity: serde_json::Value,
    pub workers: usize,
    pub development_override: bool,
}

pub const ENGINE_PIN_REMEDY: &str = "read its stored input and submit it under a new key naming the project and an available compatible native addon";

/// Missing historical pins are parked, never silently upgraded to latest.
/// This pure check starts no addon and performs no filesystem/network read.
pub fn validate_engine_pin(job: &Job) -> Result<(), &'static str> {
    if job.engine == EngineKind::TilesPrepared {
        return if job.engine_pin.is_none() {
            Ok(())
        } else {
            Err("compute_engine_pin_invalid")
        };
    }
    let pin = job
        .engine_pin
        .as_ref()
        .ok_or("compute_engine_pin_required")?;
    let context = job
        .context
        .as_ref()
        .ok_or("compute_engine_pin_context_mismatch")?;
    if pin.schema != "ds.compute.engine-pin/v1"
        || pin.engine != job.engine
        || pin.owner != job.owner
        || pin.lane.as_str() != job.lane
        || pin.context != *context
        || context.validate().is_err()
        || context.job_id != job.id
        || context.input_sha256 != job.input_sha256
        || context.lane != job.lane
        || context.operation
            != match job.engine {
                EngineKind::LvProcessing => crate::execution_context::TRANSFORMER_PROCESSING,
                EngineKind::SolarPrepared => crate::execution_context::SOLAR_PROCESSING,
                EngineKind::TilesPrepared => unreachable!(),
            }
        || !absolute_executable(&pin.executable)
        || pin.executable.len() > 4096
        || pin.executable.chars().any(char::is_control)
        || !digest(&pin.executable_sha256)
        || !(1..=65536).contains(&pin.workers)
        || !pin.identity.is_object()
        || !serde_json::to_vec(&pin.identity).is_ok_and(|bytes| bytes.len() <= 64 * 1024)
    {
        return Err("compute_engine_pin_invalid");
    }
    Ok(())
}

// The envelope is portable: accept POSIX roots and Windows drive/UNC roots
// without letting the machine reading a row reinterpret a relative path.
fn absolute_executable(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
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
    /// `None`. Historic engine spellings are read aliases, while new writes
    /// use the canonical engine name. `transition` never touches
    /// it — no lifecycle event moves a job to another project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<crate::execution_context::ExecutionContext>,
    /// Written atomically with input and context at native admission. Older
    /// rows remain readable with None and require explicit resubmission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_pin: Option<EnginePin>,
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
            engine: EngineKind::LvProcessing,
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
            engine_pin: None,
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
    fn pinned() -> Job {
        let mut job = queued();
        job.context = Some(context());
        job.engine_pin = Some(EnginePin {
            schema: "ds.compute.engine-pin/v1".into(),
            engine: job.engine.clone(),
            owner: job.owner.clone(),
            lane: EngineLane::Stable,
            context: context(),
            source: EnginePinSource::NativeLaneCapture,
            executable: "/immutable/network".into(),
            executable_sha256: "a".repeat(64),
            identity: serde_json::json!({"native_identity":"opaque to pure kernel"}),
            workers: 1,
            development_override: false,
        });
        job
    }
    #[test]
    fn engine_pin_binds_every_admitted_scope_and_legacy_rows_are_explicitly_unready() {
        assert_eq!(
            validate_engine_pin(&queued()),
            Err("compute_engine_pin_required")
        );
        assert!(validate_engine_pin(&pinned()).is_ok());
        for mutate in [
            |p: &mut EnginePin| p.owner = "other".into(),
            |p: &mut EnginePin| p.lane = EngineLane::Canary,
            |p: &mut EnginePin| p.context.project = "other".into(),
            |p: &mut EnginePin| p.context.input_sha256 = "b".repeat(64),
            |p: &mut EnginePin| p.context.job_id = "c".repeat(64),
            |p: &mut EnginePin| p.engine = EngineKind::SolarPrepared,
            |p: &mut EnginePin| p.workers = 0,
            |p: &mut EnginePin| p.executable = "relative/network".into(),
            |p: &mut EnginePin| p.executable = "C:relative.exe".into(),
            |p: &mut EnginePin| p.executable_sha256 = "unknown".into(),
        ] {
            let mut job = pinned();
            mutate(job.engine_pin.as_mut().unwrap());
            assert_eq!(validate_engine_pin(&job), Err("compute_engine_pin_invalid"));
        }
        let mut value = serde_json::to_value(pinned()).unwrap();
        value["engine_pin"]["source"] = serde_json::json!("authored_path");
        assert!(serde_json::from_value::<Job>(value).is_err());
        for executable in [
            "/native/network",
            "C:\\native\\network.exe",
            "\\\\host\\native\\network.exe",
        ] {
            let mut job = pinned();
            job.engine_pin.as_mut().unwrap().executable = executable.into();
            assert!(validate_engine_pin(&job).is_ok());
        }
    }
    /// A row written by a Server released before execution contexts existed.
    /// Literal, not generated: the point is that TODAY's bytes still load.
    const LEGACY_ROW: &str = r#"{"id":"1111111111111111111111111111111111111111111111111111111111111111","owner":"owner","lane":"stable","input_sha256":"2222222222222222222222222222222222222222222222222222222222222222","engine":"fast_lv","input_tag":"ds.fast-lv.request/v1","phase":"queued","attempts":0,"created_at_ms":1,"updated_at_ms":1,"worker":null,"lease_until_ms":0,"result_sha256":null,"error":null}"#;

    #[test]
    fn a_row_stored_before_execution_contexts_still_loads_with_the_canonical_engine_name() {
        let job: Job = serde_json::from_str(LEGACY_ROW).expect("a released row still deserialises");
        assert_eq!(job, queued());
        assert_eq!(job.context, None);
        // Decoding a stored row does not mutate its bytes in the store. When
        // this build explicitly writes a row, it uses the canonical name.
        let canonical = LEGACY_ROW.replace("\"fast_lv\"", "\"lv_processing\"");
        assert_eq!(serde_json::to_string(&job).unwrap(), canonical);
        // The member is additive, not required: a row that carries one is the
        // same row plus its context.
        let mut carrying = queued();
        carrying.context = Some(context());
        let text = serde_json::to_string(&carrying).unwrap();
        assert!(text.starts_with(canonical.trim_end_matches('}')));
        assert_eq!(
            serde_json::from_str::<Job>(&text).unwrap().context,
            Some(context())
        );
    }

    #[test]
    fn no_lifecycle_event_touches_the_execution_context() {
        let job = pinned();
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
        assert_eq!(claimed.engine_pin, job.engine_pin);
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
            assert_eq!(next.engine_pin, job.engine_pin);
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
        assert_eq!(retaken.engine_pin, job.engine_pin);
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
