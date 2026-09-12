//! The execution context — one immutable record per admitted operation, and
//! the five decisions that keep it honest.
//!
//! A Server admits work for several authorized projects at once. Nothing about
//! that work may be read from a process-wide "current project", a saved CLI
//! selection captured at startup, or an id a caller happens to hold. Every
//! admitted operation instead carries a record decided here, once: who is
//! authenticated, on which lane and deployment, from which install, for which
//! **verified** project, from which client, which operation, which job, which
//! exact input bytes, and when it was admitted. The record is written with the
//! job row and never rewritten — a retry, a restart, a claim, a publication and
//! a cancellation all present the same context they started with.
//!
//! The five questions this owner answers:
//!
//! | Question | Op |
//! |---|---|
//! | May this request be admitted, and under which project? | `admit` |
//! | May this caller see this job at all? | `visible` |
//! | Does this lifecycle event change the scope? (it never does) | `retain` |
//! | Is there room for this project's work right now, fairly? | `capacity` |
//! | What context does a row written before this slice have? | `recover` |
//!
//! Rules worth stating up front:
//!
//! - **A saved selection is a default, not authority.** `saved_project` is
//!   verified against fresh membership exactly like an explicitly requested
//!   one. A sealed input that already names its project (Solar) outranks both;
//!   an explicit request that contradicts the bytes is `scope_mismatch`.
//! - **An id never authorizes.** `visible` answers one word, `job not found`,
//!   to a foreign principal, a foreign lane, a foreign deployment, a caller
//!   naming the wrong project, and a caller naming an id that does not exist.
//!   Nothing distinguishes them, so nothing is disclosed by probing.
//! - **Nothing rewrites scope.** `retain` takes `now_ms` and ignores it. That
//!   is the point: time, a retry, a restart or a publication cannot move a job
//!   to another project.
//! - **Capacity is bounded and fair.** A project may exceed its running share
//!   while nothing else is waiting, and may not while another project has
//!   queued work. Saturation is a typed answer with retry guidance, never a
//!   dropped request.
//! - **Recovery reconstructs; it does not authorize.** A row written by a
//!   released Server gets its project from the sealed input (Solar, which
//!   carries it) or from the saved selection (transformer work, which by
//!   design carries none) — and from nothing else. The host re-verifies
//!   membership before the next effect either way.
//!
//! Pure: `now_ms` is an input, every list is bounded, and the same request
//! always has the same answer. `tests/fixtures/execution-context-cases.json`
//! is the named proof, replayed by the kernel and by ds-web over the vendored
//! WASM (`executionContext`).

use crate::compute_jobs::{self, EngineKind, Job};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SCHEMA: &str = "ds.execution-context/v1";
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

// ── Bounds ──────────────────────────────────────────────────────────────
pub const MAX_UID_CHARS: usize = 200;
pub const MAX_DEPLOYMENT_CHARS: usize = 300;
pub const MAX_INSTALL_CHARS: usize = 200;
/// The same project bound `layer_document::request_body` admits.
pub const MAX_PROJECT_CHARS: usize = 500;
pub const MAX_CLIENT_CHARS: usize = 120;
pub const MAX_KEY_CHARS: usize = 200;
pub const MAX_JOB_ID_CHARS: usize = 200;
pub const MAX_MEMBERSHIP_PROJECTS: usize = 5_000;
pub const MAX_MEMBERSHIP_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_SLOTS: usize = 100_000;
pub const MAX_LIMIT: usize = 4_096;

/// Retry guidance grows with the work that must go first and stops at a
/// minute, so a saturated host never invites a hot retry loop.
pub const RETRY_AFTER_BASE_MS: u64 = 1_000;
pub const RETRY_AFTER_MAX_MS: u64 = 60_000;

/// What a reconstructed context records instead of the client and key a
/// released Server never persisted. The durable id is the digest of
/// `(owner, lane, key)` and cannot be inverted, so the key is not recoverable;
/// reuse is decided by project, operation and input digest, never by the text.
pub const RECOVERED_CLIENT: &str = "recovery:legacy-row";
pub const RECOVERED_KEY: &str = "recovered";

// ── The operation vocabulary ────────────────────────────────────────────
pub const TRANSFORMER_PROCESSING: &str = "transformer_processing";
pub const SOLAR_PROCESSING: &str = "solar_processing";
pub const REPORT_EXPORT: &str = "report_export";
pub const LAYER_READ: &str = "layer_read";
pub const LAYER_WRITE: &str = "layer_write";
pub const OPERATIONS: [&str; 5] = [
    TRANSFORMER_PROCESSING,
    SOLAR_PROCESSING,
    REPORT_EXPORT,
    LAYER_READ,
    LAYER_WRITE,
];

// ── The refusal vocabulary (closed) ─────────────────────────────────────
pub const PROJECT_REQUIRED: &str = "project_required";
pub const PROJECT_NOT_VISIBLE: &str = "project_not_visible";
pub const SCOPE_MISMATCH: &str = "scope_mismatch";
pub const SCOPE_MISMATCH_FOR_KEY: &str = "scope_mismatch_for_key";
pub const PAYLOAD_CHANGED_FOR_KEY: &str = "payload_changed_for_key";
pub const PRINCIPAL_MISMATCH: &str = "principal_mismatch";
pub const NOT_VISIBLE: &str = "not_visible";
pub const CAPACITY_EXHAUSTED: &str = "capacity_exhausted";
pub const CONTEXT_CORRUPT: &str = "context_corrupt";
pub const CONTEXT_UNRECOVERABLE: &str = "context_unrecoverable";
pub const REFUSALS: [&str; 10] = [
    PROJECT_REQUIRED,
    PROJECT_NOT_VISIBLE,
    SCOPE_MISMATCH,
    SCOPE_MISMATCH_FOR_KEY,
    PAYLOAD_CHANGED_FOR_KEY,
    PRINCIPAL_MISMATCH,
    NOT_VISIBLE,
    CAPACITY_EXHAUSTED,
    CONTEXT_CORRUPT,
    CONTEXT_UNRECOVERABLE,
];

/// The one sentence a caller gets for every invisible job, whatever the reason.
pub const NOT_FOUND: &str = "job not found";
/// The one sentence a caller gets for a project they may not act in, whether
/// they are not a member or the host's membership snapshot has expired. The
/// host knows which — it holds `fetched_at_ms`; the caller learns nothing.
pub const NOT_VISIBLE_MESSAGE: &str = "that project is not visible to this connection";

// ── The record ──────────────────────────────────────────────────────────

/// Immutable once admitted. Serialised in declaration order as the durable
/// `context` member of a compute job row, so the field names below are the
/// on-disk names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContext {
    /// The authenticated native account behind the Server connection.
    pub principal_uid: String,
    /// `stable` or `canary`; never inferred from a build.
    pub lane: String,
    /// The gateway origin / audience the credential is bound to.
    pub deployment: String,
    /// The registered native install this work was admitted from.
    pub install_id: String,
    /// The **authorized** project. Verified membership at admission.
    pub project: String,
    /// The originating client or session label, e.g. `cli:4711`.
    pub client: String,
    /// One of [`OPERATIONS`].
    pub operation: String,
    /// The durable job id, or the request id for a read.
    pub job_id: String,
    /// The caller's idempotency key for a submission.
    pub idempotency_key: String,
    /// The digest of the exact admitted input bytes.
    pub input_sha256: String,
    pub admitted_at_ms: u64,
}

impl ExecutionContext {
    /// Every field, on the way in and on the way out. A stored record that
    /// fails this is corrupt, not merely unexpected.
    pub fn validate(&self) -> Result<(), Fault> {
        let bad = |field: &str| Err(corrupt(field));
        if !bounded(&self.principal_uid, MAX_UID_CHARS) {
            return bad("principal_uid");
        }
        if !valid_lane(&self.lane) {
            return bad("lane");
        }
        if !bounded(&self.deployment, MAX_DEPLOYMENT_CHARS) {
            return bad("deployment");
        }
        if !bounded(&self.install_id, MAX_INSTALL_CHARS) {
            return bad("install_id");
        }
        if !bounded(&self.project, MAX_PROJECT_CHARS) {
            return bad("project");
        }
        if !bounded(&self.client, MAX_CLIENT_CHARS) {
            return bad("client");
        }
        if !OPERATIONS.contains(&self.operation.as_str()) {
            return bad("operation");
        }
        if !bounded(&self.job_id, MAX_JOB_ID_CHARS) {
            return bad("job_id");
        }
        if !bounded(&self.idempotency_key, MAX_KEY_CHARS) {
            return bad("idempotency_key");
        }
        if !compute_jobs::digest(&self.input_sha256) {
            return bad("input_sha256");
        }
        if self.admitted_at_ms == 0 {
            return bad("admitted_at_ms");
        }
        Ok(())
    }
}

/// The authenticated connection an admission is made under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub uid: String,
    pub lane: String,
    pub deployment: String,
    pub install_id: String,
}

impl Principal {
    pub fn validate(&self) -> Result<(), Fault> {
        if !bounded(&self.uid, MAX_UID_CHARS) {
            return Err(corrupt("principal.uid"));
        }
        if !valid_lane(&self.lane) {
            return Err(corrupt("principal.lane"));
        }
        if !bounded(&self.deployment, MAX_DEPLOYMENT_CHARS) {
            return Err(corrupt("principal.deployment"));
        }
        if !bounded(&self.install_id, MAX_INSTALL_CHARS) {
            return Err(corrupt("principal.install_id"));
        }
        Ok(())
    }
}

/// Who is asking about a job. Deliberately without `install_id`: the same
/// account reconnecting from a second install still recovers its own jobs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caller {
    pub uid: String,
    pub lane: String,
    pub deployment: String,
}

/// The host's project-membership snapshot and how long it stays usable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    pub projects: Vec<String>,
    pub fetched_at_ms: u64,
    pub ttl_ms: u64,
}

/// One occupied or waiting unit of work, named only by its project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub project: String,
}

/// The host's configured admission limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub global_running: usize,
    pub per_project_running: usize,
    pub per_project_queued: usize,
    pub global_queued: usize,
}

/// Which moment the host is asking about. Submission bounds the queue; a claim
/// bounds the running pool and enforces the fairness share.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    #[default]
    Admit,
    Claim,
}

/// The lifecycle events that must not change a context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetainEvent {
    Retry,
    Restart,
    Claim,
    Publish,
    Cancel,
}

impl RetainEvent {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Restart => "restart",
            Self::Claim => "claim",
            Self::Publish => "publish",
            Self::Cancel => "cancel",
        }
    }
}

/// Where a recovered context came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// The row already carried it.
    Stored,
    /// Rebuilt from the sealed Solar input, which names its own project.
    SealedInput,
    /// Rebuilt from the caller's saved selection, the only project a
    /// transformer row (which carries none by design) can belong to.
    SavedSelection,
}

impl Provenance {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::SealedInput => "sealed_input",
            Self::SavedSelection => "saved_selection",
        }
    }
}

// ── Refusals ────────────────────────────────────────────────────────────

/// Why a decision refused. `retry_after_ms` and `scope` belong to
/// `capacity_exhausted` and are absent everywhere else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<&'static str>,
}

/// A malformed request the host built wrong (`Hard`) against a decision a
/// caller must be told (`Refused`). Hosts relay a refusal; a hard fault is a
/// bug in the host, never a client answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    Hard(String),
    Refused(Refusal),
}

impl Fault {
    /// The typed code a host relays to its caller, or `None` when the fault is
    /// the host's own bug and there is nothing to relay.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Hard(_) => None,
            Self::Refused(refusal) => Some(refusal.code),
        }
    }
    pub fn message(&self) -> &str {
        match self {
            Self::Hard(message) => message,
            Self::Refused(refusal) => &refusal.message,
        }
    }
}

impl std::fmt::Display for Fault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code() {
            Some(code) => write!(formatter, "{code}: {}", self.message()),
            None => formatter.write_str(self.message()),
        }
    }
}

fn refused(code: &'static str, message: impl Into<String>) -> Fault {
    Fault::Refused(Refusal {
        code,
        message: message.into(),
        retry_after_ms: None,
        scope: None,
    })
}
fn corrupt(field: &str) -> Fault {
    refused(
        CONTEXT_CORRUPT,
        format!("execution context field {field} is not a bounded value"),
    )
}
fn hard(message: impl Into<String>) -> Fault {
    Fault::Hard(message.into())
}
fn not_found() -> Refusal {
    Refusal {
        code: NOT_VISIBLE,
        message: NOT_FOUND.into(),
        retry_after_ms: None,
        scope: None,
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}
fn valid_lane(lane: &str) -> bool {
    matches!(lane, "stable" | "canary")
}

// ── admit ───────────────────────────────────────────────────────────────

/// Everything an admission decision needs, and nothing the host may substitute
/// for it. `job_id` is the host's existing durable id (`compute_jobs` already
/// derives it from owner, lane and key); the kernel only binds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmitRequest {
    pub now_ms: u64,
    pub principal: Principal,
    pub client: String,
    pub operation: String,
    pub job_id: String,
    pub idempotency_key: String,
    pub input_sha256: String,
    /// The project the caller named explicitly, if any.
    #[serde(default)]
    pub requested_project: Option<String>,
    /// The caller's saved selection — a default, verified like any other.
    #[serde(default)]
    pub saved_project: Option<String>,
    /// The project the sealed input itself names (Solar). Authoritative.
    #[serde(default)]
    pub sealed_project: Option<String>,
    pub membership: Membership,
    /// The context already stored for this key, if the host found one.
    #[serde(default)]
    pub existing: Option<ExecutionContext>,
}

/// What an admission decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admitted {
    /// A new context, stamped at `now_ms`.
    Fresh(ExecutionContext),
    /// The same key, project, operation and bytes: the context already
    /// admitted, returned unchanged. An idempotent resubmit.
    Existing(ExecutionContext),
}

impl Admitted {
    pub fn context(&self) -> &ExecutionContext {
        match self {
            Self::Fresh(context) | Self::Existing(context) => context,
        }
    }
    pub const fn outcome(&self) -> &'static str {
        match self {
            Self::Fresh(_) => "admitted",
            Self::Existing(_) => "existing",
        }
    }
}

/// Admit one operation under one verified project, or refuse it by name.
///
/// The order of the checks is itself a rule: the project is resolved and its
/// membership verified **before** anything stored under the caller's key is
/// consulted, so a caller can never learn about a job in a project they are
/// not a member of by reusing a key.
pub fn admit(request: &AdmitRequest) -> Result<Admitted, Fault> {
    if request.now_ms == 0 {
        return Err(hard("admission needs the host's clock in now_ms"));
    }
    request.principal.validate()?;
    if !bounded(&request.client, MAX_CLIENT_CHARS) {
        return Err(corrupt("client"));
    }
    if !OPERATIONS.contains(&request.operation.as_str()) {
        return Err(corrupt("operation"));
    }
    if !bounded(&request.job_id, MAX_JOB_ID_CHARS) {
        return Err(corrupt("job_id"));
    }
    if !bounded(&request.idempotency_key, MAX_KEY_CHARS) {
        return Err(corrupt("idempotency_key"));
    }
    if !compute_jobs::digest(&request.input_sha256) {
        return Err(corrupt("input_sha256"));
    }
    let project = resolve_project(request)?;
    verify_membership(&request.membership, request.now_ms, &project)?;
    if let Some(existing) = &request.existing {
        existing.validate()?;
        if existing.principal_uid != request.principal.uid
            || existing.lane != request.principal.lane
            || existing.deployment != request.principal.deployment
        {
            return Err(refused(
                PRINCIPAL_MISMATCH,
                "that key was admitted under another authenticated identity",
            ));
        }
        if existing.job_id != request.job_id {
            return Err(refused(
                CONTEXT_CORRUPT,
                "the stored context is bound to another job id",
            ));
        }
        if existing.project != project || existing.operation != request.operation {
            return Err(refused(
                SCOPE_MISMATCH_FOR_KEY,
                "that idempotency key already names another project or operation",
            ));
        }
        if existing.input_sha256 != request.input_sha256 {
            return Err(refused(
                PAYLOAD_CHANGED_FOR_KEY,
                "that idempotency key was admitted with different input bytes",
            ));
        }
        return Ok(Admitted::Existing(existing.clone()));
    }
    let context = ExecutionContext {
        principal_uid: request.principal.uid.clone(),
        lane: request.principal.lane.clone(),
        deployment: request.principal.deployment.clone(),
        install_id: request.principal.install_id.clone(),
        project,
        client: request.client.clone(),
        operation: request.operation.clone(),
        job_id: request.job_id.clone(),
        idempotency_key: request.idempotency_key.clone(),
        input_sha256: request.input_sha256.clone(),
        admitted_at_ms: request.now_ms,
    };
    context.validate()?;
    Ok(Admitted::Fresh(context))
}

/// Sealed bytes outrank an explicit request, which outranks a saved default.
/// A saved default that disagrees with the bytes is simply not used — only a
/// caller who *named* a contradicting project is refused.
fn resolve_project(request: &AdmitRequest) -> Result<String, Fault> {
    let requested = optional_project(request.requested_project.as_deref(), "requested_project")?;
    let saved = optional_project(request.saved_project.as_deref(), "saved_project")?;
    let sealed = optional_project(request.sealed_project.as_deref(), "sealed_project")?;
    if let Some(sealed) = sealed {
        if let Some(requested) = requested {
            if requested != sealed {
                return Err(refused(
                    SCOPE_MISMATCH,
                    format!(
                        "this request is for {requested}, but its sealed input was prepared for another project"
                    ),
                ));
            }
        }
        return Ok(sealed.to_owned());
    }
    requested
        .or(saved)
        .map(str::to_owned)
        .ok_or_else(|| refused(PROJECT_REQUIRED, "name a project, or select one first"))
}

fn optional_project<'a>(value: Option<&'a str>, field: &str) -> Result<Option<&'a str>, Fault> {
    match value {
        Some(project) if bounded(project, MAX_PROJECT_CHARS) => Ok(Some(project)),
        Some(_) => Err(corrupt(field)),
        None => Ok(None),
    }
}

/// A project is visible only from a snapshot that is still fresh and actually
/// contains it. A snapshot stamped in the future is treated as unusable, not
/// as eternally fresh.
fn verify_membership(membership: &Membership, now_ms: u64, project: &str) -> Result<(), Fault> {
    if membership.projects.len() > MAX_MEMBERSHIP_PROJECTS {
        return Err(hard("membership snapshot exceeds its bound"));
    }
    if membership.ttl_ms == 0 || membership.ttl_ms > MAX_MEMBERSHIP_TTL_MS {
        return Err(hard("membership ttl_ms must be 1..=86400000"));
    }
    let stale = membership.fetched_at_ms == 0
        || membership.fetched_at_ms > now_ms
        || now_ms - membership.fetched_at_ms > membership.ttl_ms;
    if stale || !membership.projects.iter().any(|known| known == project) {
        return Err(refused(PROJECT_NOT_VISIBLE, NOT_VISIBLE_MESSAGE));
    }
    Ok(())
}

// ── visible ─────────────────────────────────────────────────────────────

/// Whether this caller may see this job at all. Every "no" is the same "no":
/// a foreign principal, lane or deployment, a caller narrowing to the wrong
/// project, and a context the host cannot even read all answer alike, so
/// probing an id discloses nothing.
pub fn visible(
    context: &ExecutionContext,
    caller: &Caller,
    requested_project: Option<&str>,
) -> bool {
    context.validate().is_ok()
        && bounded(&caller.uid, MAX_UID_CHARS)
        && valid_lane(&caller.lane)
        && bounded(&caller.deployment, MAX_DEPLOYMENT_CHARS)
        && context.principal_uid == caller.uid
        && context.lane == caller.lane
        && context.deployment == caller.deployment
        && requested_project.is_none_or(|project| project == context.project)
}

// ── retain ──────────────────────────────────────────────────────────────

/// The identity function, stated at the type level: no lifecycle event moves a
/// job to another project, account, lane or deployment. It refuses only a
/// context that is already corrupt.
pub fn retain(context: &ExecutionContext, _event: RetainEvent) -> Result<ExecutionContext, Fault> {
    context.validate()?;
    Ok(context.clone())
}

// ── capacity ────────────────────────────────────────────────────────────

/// Is there room, fairly, for this project's work right now?
///
/// Every input is the host's own: configured limits, and the store's current
/// queue and running set. Nothing here is caller-supplied, so a malformed
/// input is a host bug (`Fault::Hard`), never a client refusal.
///
/// `Intent::Admit` bounds the queue a submission enters. `Intent::Claim`
/// bounds the running pool and applies the fairness share: a project may hold
/// more than `per_project_running` slots while nothing else is waiting, and
/// may not while another project has queued work.
pub fn capacity(
    limits: Limits,
    intent: Intent,
    running: &[Slot],
    queued: &[Slot],
    project: &str,
) -> Result<(), Fault> {
    for (value, name) in [
        (limits.global_running, "global_running"),
        (limits.per_project_running, "per_project_running"),
        (limits.per_project_queued, "per_project_queued"),
        (limits.global_queued, "global_queued"),
    ] {
        if value == 0 || value > MAX_LIMIT {
            return Err(hard(format!("capacity limit {name} must be 1..=4096")));
        }
    }
    if running.len() > MAX_SLOTS || queued.len() > MAX_SLOTS {
        return Err(hard("capacity census exceeds its bound"));
    }
    if !bounded(project, MAX_PROJECT_CHARS)
        || running
            .iter()
            .chain(queued)
            .any(|slot| !bounded(&slot.project, MAX_PROJECT_CHARS))
    {
        return Err(hard("capacity census names an unbounded project"));
    }
    let queued_here = queued.iter().filter(|slot| slot.project == project).count();
    let queued_elsewhere = queued.len() - queued_here;
    let running_here = running
        .iter()
        .filter(|slot| slot.project == project)
        .count();
    match intent {
        Intent::Admit => {
            if queued.len() >= limits.global_queued {
                return Err(exhausted(
                    "global",
                    format!(
                        "the server queue is full ({} of {} waiting)",
                        queued.len(),
                        limits.global_queued
                    ),
                    queued.len(),
                ));
            }
            if queued_here >= limits.per_project_queued {
                return Err(exhausted(
                    "project",
                    format!(
                        "this project already has {queued_here} of {} operations waiting",
                        limits.per_project_queued
                    ),
                    queued_here,
                ));
            }
        }
        Intent::Claim => {
            if running.len() >= limits.global_running {
                return Err(exhausted(
                    "global",
                    format!(
                        "every worker is busy ({} of {} running)",
                        running.len(),
                        limits.global_running
                    ),
                    queued.len(),
                ));
            }
            if running_here >= limits.per_project_running && queued_elsewhere > 0 {
                return Err(exhausted(
                    "project",
                    format!(
                        "this project holds {running_here} of {} running slots while other projects are waiting",
                        limits.per_project_running
                    ),
                    queued_elsewhere,
                ));
            }
        }
    }
    Ok(())
}

/// Deterministic guidance: one second per unit of work that must go first,
/// capped at a minute. No clock, no randomness, no project named.
pub fn retry_after_ms(ahead: usize) -> u64 {
    RETRY_AFTER_BASE_MS
        .saturating_mul(ahead as u64 + 1)
        .min(RETRY_AFTER_MAX_MS)
}

fn exhausted(scope: &'static str, message: String, ahead: usize) -> Fault {
    Fault::Refused(Refusal {
        code: CAPACITY_EXHAUSTED,
        message,
        retry_after_ms: Some(retry_after_ms(ahead)),
        scope: Some(scope),
    })
}

// ── recover ─────────────────────────────────────────────────────────────

/// What context a stored job row has — including a row written by a Server
/// released before this slice, which has none.
///
/// A row that already carries a context is returned unchanged. A legacy Solar
/// row takes its project from the sealed input, which is authoritative because
/// the bytes themselves name it. A legacy transformer row carries no project
/// by design, so the caller's saved selection is the only honest source; with
/// no saved selection the row is `context_unrecoverable` and the host must ask
/// rather than guess.
///
/// Recovery reconstructs; it does not authorize. The host re-verifies
/// membership before the next effect.
pub fn recover(
    row: Value,
    principal: &Principal,
    saved_project: Option<&str>,
    sealed_project: Option<&str>,
    now_ms: u64,
) -> Result<(ExecutionContext, Provenance), Fault> {
    if now_ms == 0 {
        return Err(hard("recovery needs the host's clock in now_ms"));
    }
    principal.validate()?;
    let saved = optional_project(saved_project, "saved_project")?;
    let sealed = optional_project(sealed_project, "sealed_project")?;
    let Ok(job) = serde_json::from_value::<Job>(row) else {
        return Err(refused(
            CONTEXT_UNRECOVERABLE,
            "that row is not a durable compute job",
        ));
    };
    // The store already reads rows by (owner, lane); the owner is a digest the
    // kernel holds no credential material to recompute, so the lane is the
    // fence it can check here.
    if job.lane != principal.lane {
        return Err(refused(
            PRINCIPAL_MISMATCH,
            "that row belongs to another deployment lane",
        ));
    }
    if let Some(context) = job.context {
        context.validate()?;
        if context.principal_uid != principal.uid
            || context.lane != principal.lane
            || context.deployment != principal.deployment
        {
            return Err(refused(
                PRINCIPAL_MISMATCH,
                "that row was admitted under another authenticated identity",
            ));
        }
        if context.job_id != job.id || context.input_sha256 != job.input_sha256 {
            return Err(refused(
                CONTEXT_CORRUPT,
                "the stored context is bound to another job id or input digest",
            ));
        }
        if sealed.is_some_and(|sealed| sealed != context.project) {
            return Err(refused(
                SCOPE_MISMATCH,
                "that row's sealed input was prepared for another project",
            ));
        }
        return Ok((context, Provenance::Stored));
    }
    let unrecoverable = |what: &str| {
        refused(
            CONTEXT_UNRECOVERABLE,
            format!("that row predates execution contexts and {what}"),
        )
    };
    let (project, provenance, operation) = match job.engine {
        EngineKind::SolarPrepared => (
            sealed.ok_or_else(|| unrecoverable("its sealed input names no project"))?,
            Provenance::SealedInput,
            SOLAR_PROCESSING,
        ),
        EngineKind::FastLv => (
            saved.ok_or_else(|| unrecoverable("no project was selected to recover it into"))?,
            Provenance::SavedSelection,
            TRANSFORMER_PROCESSING,
        ),
    };
    let context = ExecutionContext {
        principal_uid: principal.uid.clone(),
        lane: job.lane.clone(),
        deployment: principal.deployment.clone(),
        install_id: principal.install_id.clone(),
        project: project.to_owned(),
        client: RECOVERED_CLIENT.into(),
        operation: operation.into(),
        job_id: job.id.clone(),
        idempotency_key: RECOVERED_KEY.into(),
        input_sha256: job.input_sha256.clone(),
        // The row's own admission moment, not the moment it was reopened.
        admitted_at_ms: if job.created_at_ms > 0 {
            job.created_at_ms
        } else {
            now_ms
        },
    };
    if context.validate().is_err() {
        return Err(unrecoverable("its stored identity is not a bounded value"));
    }
    Ok((context, provenance))
}

// ── The wire ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    op: Op,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Op {
    /// Boxed: an admission carries every input of every other op together, so
    /// unboxed it would set the size of the whole enum.
    Admit(Box<AdmitRequest>),
    Visible(VisibleRequest),
    Retain(RetainRequest),
    Capacity(CapacityRequest),
    Recover(RecoverRequest),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibleRequest {
    caller: Caller,
    #[serde(default)]
    requested_project: Option<String>,
    context: ExecutionContext,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainRequest {
    context: ExecutionContext,
    event: RetainEvent,
    /// Accepted and ignored. That it changes nothing is the statement.
    #[serde(default)]
    now_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapacityRequest {
    limits: Limits,
    #[serde(default)]
    intent: Intent,
    running: Vec<Slot>,
    queued: Vec<Slot>,
    project: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverRequest {
    row: Value,
    principal: Principal,
    #[serde(default)]
    saved_project: Option<String>,
    #[serde(default)]
    sealed_project: Option<String>,
    now_ms: u64,
}

fn answer(op: Op) -> Result<Value, Fault> {
    Ok(match op {
        Op::Admit(request) => {
            let admitted = admit(&request)?;
            json!({"outcome": admitted.outcome(), "context": admitted.context()})
        }
        Op::Visible(request) => {
            if visible(
                &request.context,
                &request.caller,
                request.requested_project.as_deref(),
            ) {
                json!({"visible": true})
            } else {
                json!({"visible": false, "refusal": not_found()})
            }
        }
        Op::Retain(request) => {
            let _ = request.now_ms;
            let context = retain(&request.context, request.event)?;
            json!({"outcome": "retained", "event": request.event.wire(), "context": context})
        }
        Op::Capacity(request) => {
            capacity(
                request.limits,
                request.intent,
                &request.running,
                &request.queued,
                &request.project,
            )?;
            json!({"outcome": "admitted"})
        }
        Op::Recover(request) => {
            let (context, provenance) = recover(
                request.row,
                &request.principal,
                request.saved_project.as_deref(),
                request.sealed_project.as_deref(),
                request.now_ms,
            )?;
            json!({"outcome": "recovered", "provenance": provenance.wire(), "context": context})
        }
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("execution context request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid execution context request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported execution context schema; expected {SCHEMA}"
        ));
    }
    let value = match answer(request.op) {
        Ok(value) => value,
        Err(Fault::Hard(message)) => return Err(message),
        Err(Fault::Refused(refusal)) => json!({"outcome": "refused", "refusal": refusal}),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::to_vec;

    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const JOB_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn principal() -> Principal {
        Principal {
            uid: "uid-a".into(),
            lane: "canary".into(),
            deployment: "https://gateway.example".into(),
            install_id: "install-1".into(),
        }
    }
    fn caller() -> Caller {
        Caller {
            uid: "uid-a".into(),
            lane: "canary".into(),
            deployment: "https://gateway.example".into(),
        }
    }
    fn membership(projects: &[&str]) -> Membership {
        Membership {
            projects: projects.iter().map(|p| (*p).to_owned()).collect(),
            fetched_at_ms: 1_000,
            ttl_ms: 600_000,
        }
    }
    fn request() -> AdmitRequest {
        AdmitRequest {
            now_ms: 2_000,
            principal: principal(),
            client: "cli:4711".into(),
            operation: SOLAR_PROCESSING.into(),
            job_id: JOB_ID.into(),
            idempotency_key: "key-a".into(),
            input_sha256: DIGEST_A.into(),
            requested_project: None,
            saved_project: None,
            sealed_project: None,
            membership: membership(&["project-a", "project-b"]),
            existing: None,
        }
    }
    fn context(project: &str) -> ExecutionContext {
        ExecutionContext {
            principal_uid: "uid-a".into(),
            lane: "canary".into(),
            deployment: "https://gateway.example".into(),
            install_id: "install-1".into(),
            project: project.into(),
            client: "cli:4711".into(),
            operation: SOLAR_PROCESSING.into(),
            job_id: JOB_ID.into(),
            idempotency_key: "key-a".into(),
            input_sha256: DIGEST_A.into(),
            admitted_at_ms: 2_000,
        }
    }
    fn code(fault: &Fault) -> &str {
        match fault {
            Fault::Refused(refusal) => refusal.code,
            Fault::Hard(message) => panic!("expected a refusal, got a hard fault: {message}"),
        }
    }
    fn refusal(fault: Fault) -> Refusal {
        match fault {
            Fault::Refused(refusal) => refusal,
            Fault::Hard(message) => panic!("expected a refusal, got a hard fault: {message}"),
        }
    }
    fn slots(projects: &[&str]) -> Vec<Slot> {
        projects
            .iter()
            .map(|project| Slot {
                project: (*project).to_owned(),
            })
            .collect()
    }
    fn limits() -> Limits {
        Limits {
            global_running: 4,
            per_project_running: 2,
            per_project_queued: 3,
            global_queued: 8,
        }
    }
    fn wire(op: Value) -> Result<Value, String> {
        evaluate(&to_vec(&json!({"schema": SCHEMA, "op": op})).unwrap())
            .map(|answer| serde_json::from_str(&answer).expect("the answer is JSON"))
    }

    // ── the record ──────────────────────────────────────────────────────

    #[test]
    fn every_field_of_the_record_is_bounded_and_named_when_it_is_not() {
        assert!(context("project-a").validate().is_ok());
        let cases: [(&str, fn(&mut ExecutionContext)); 12] = [
            ("principal_uid", |c| c.principal_uid = String::new()),
            ("principal_uid", |c| c.principal_uid = " uid".into()),
            ("lane", |c| c.lane = "development".into()),
            ("deployment", |c| c.deployment = String::new()),
            ("install_id", |c| c.install_id = "a\u{0}b".into()),
            ("project", |c| c.project = "  ".into()),
            ("project", |c| c.project = "p".repeat(MAX_PROJECT_CHARS + 1)),
            ("client", |c| c.client = "c".repeat(MAX_CLIENT_CHARS + 1)),
            ("operation", |c| c.operation = "everything".into()),
            ("job_id", |c| c.job_id = String::new()),
            ("idempotency_key", |c| {
                c.idempotency_key = "k".repeat(MAX_KEY_CHARS + 1)
            }),
            ("input_sha256", |c| c.input_sha256 = "not-a-digest".into()),
        ];
        for (field, break_it) in cases {
            let mut broken = context("project-a");
            break_it(&mut broken);
            let refused = refusal(broken.validate().unwrap_err());
            assert_eq!(refused.code, CONTEXT_CORRUPT, "{field}");
            assert!(refused.message.contains(field), "{field}");
        }
        let mut unstamped = context("project-a");
        unstamped.admitted_at_ms = 0;
        assert_eq!(code(&unstamped.validate().unwrap_err()), CONTEXT_CORRUPT);
    }

    #[test]
    fn the_record_serialises_as_the_durable_snake_case_row() {
        let text = serde_json::to_string(&context("project-a")).unwrap();
        assert!(
            text.starts_with(r#"{"principal_uid":"uid-a","lane":"canary""#),
            "{text}"
        );
        assert!(text.ends_with(r#""admitted_at_ms":2000}"#), "{text}");
        // Closed: a row with a member the kernel does not own is not a context.
        let mut extra: Value = serde_json::from_str(&text).unwrap();
        extra["surprise"] = json!(1);
        assert!(serde_json::from_value::<ExecutionContext>(extra).is_err());
    }

    // ── admit ───────────────────────────────────────────────────────────

    #[test]
    fn an_admission_names_a_project_or_refuses_to_guess_one() {
        assert_eq!(
            code(&admit(&request()).unwrap_err()),
            PROJECT_REQUIRED,
            "no requested, saved or sealed project is not a default"
        );
        let mut saved = request();
        saved.saved_project = Some("project-a".into());
        assert_eq!(
            admit(&saved).unwrap().context().project,
            "project-a",
            "a saved selection is a usable default"
        );
        // An explicit request outranks the saved default.
        let mut both = saved.clone();
        both.requested_project = Some("project-b".into());
        assert_eq!(admit(&both).unwrap().context().project, "project-b");
        // The sealed bytes outrank both, and a saved default that disagrees is
        // simply not used.
        let mut sealed = saved.clone();
        sealed.sealed_project = Some("project-b".into());
        assert_eq!(admit(&sealed).unwrap().context().project, "project-b");
        // But a caller who NAMES a project the bytes contradict is refused.
        let mut contradicted = sealed.clone();
        contradicted.requested_project = Some("project-a".into());
        let refused = refusal(admit(&contradicted).unwrap_err());
        assert_eq!(refused.code, SCOPE_MISMATCH);
        assert!(
            refused.message.contains("project-a") && !refused.message.contains("project-b"),
            "a scope mismatch never discloses the sealed project: {}",
            refused.message
        );
    }

    #[test]
    fn a_project_is_admitted_only_from_a_fresh_membership_that_contains_it() {
        let mut fresh = request();
        fresh.requested_project = Some("project-c".into());
        assert_eq!(
            code(&admit(&fresh).unwrap_err()),
            PROJECT_NOT_VISIBLE,
            "a project outside the snapshot is not visible"
        );
        let mut saved_but_unverified = request();
        saved_but_unverified.saved_project = Some("project-c".into());
        assert_eq!(
            code(&admit(&saved_but_unverified).unwrap_err()),
            PROJECT_NOT_VISIBLE,
            "a saved selection is verified exactly like a requested one"
        );
        let mut sealed_but_unverified = request();
        sealed_but_unverified.sealed_project = Some("project-c".into());
        assert_eq!(
            code(&admit(&sealed_but_unverified).unwrap_err()),
            PROJECT_NOT_VISIBLE,
            "sealed bytes name a project; they do not authorize it"
        );
        // Stale, future-stamped and empty snapshots all refuse the same way.
        for snapshot in [
            Membership {
                projects: vec!["project-a".into()],
                fetched_at_ms: 1_000,
                ttl_ms: 500,
            },
            Membership {
                projects: vec!["project-a".into()],
                fetched_at_ms: 9_000,
                ttl_ms: 600_000,
            },
            Membership {
                projects: vec![],
                fetched_at_ms: 1_000,
                ttl_ms: 600_000,
            },
        ] {
            let mut stale = request();
            stale.requested_project = Some("project-a".into());
            stale.membership = snapshot;
            let refused = refusal(admit(&stale).unwrap_err());
            assert_eq!(refused.code, PROJECT_NOT_VISIBLE);
            assert_eq!(
                refused.message, NOT_VISIBLE_MESSAGE,
                "stale and absent read alike to the caller"
            );
        }
        // An unusable ttl is the host's bug, not the caller's refusal.
        let mut broken = request();
        broken.requested_project = Some("project-a".into());
        broken.membership.ttl_ms = 0;
        assert!(matches!(admit(&broken).unwrap_err(), Fault::Hard(_)));
    }

    #[test]
    fn a_reused_key_is_idempotent_and_a_changed_scope_or_payload_refuses() {
        let mut reuse = request();
        reuse.requested_project = Some("project-a".into());
        reuse.now_ms = 9_000;
        reuse.existing = Some(context("project-a"));
        let admitted = admit(&reuse).unwrap();
        assert!(matches!(admitted, Admitted::Existing(_)));
        assert_eq!(admitted.outcome(), "existing");
        assert_eq!(
            admitted.context(),
            &context("project-a"),
            "an idempotent resubmit returns the context unchanged, not re-stamped"
        );
        // Same key, another project.
        let mut moved = reuse.clone();
        moved.requested_project = Some("project-b".into());
        assert_eq!(code(&admit(&moved).unwrap_err()), SCOPE_MISMATCH_FOR_KEY);
        // Same key, another operation.
        let mut repurposed = reuse.clone();
        repurposed.operation = REPORT_EXPORT.into();
        assert_eq!(
            code(&admit(&repurposed).unwrap_err()),
            SCOPE_MISMATCH_FOR_KEY
        );
        // Same key, other bytes.
        let mut rewritten = reuse.clone();
        rewritten.input_sha256 = DIGEST_B.into();
        assert_eq!(
            code(&admit(&rewritten).unwrap_err()),
            PAYLOAD_CHANGED_FOR_KEY
        );
        // Another identity holding this key is a fence, whichever field moved.
        for change in [
            |c: &mut ExecutionContext| c.principal_uid = "uid-b".into(),
            |c: &mut ExecutionContext| c.lane = "stable".into(),
            |c: &mut ExecutionContext| c.deployment = "https://other.example".into(),
        ] {
            let mut foreign = reuse.clone();
            let mut stored = context("project-a");
            change(&mut stored);
            foreign.existing = Some(stored);
            assert_eq!(code(&admit(&foreign).unwrap_err()), PRINCIPAL_MISMATCH);
        }
        // A stored record bound to another job id is corrupt, not reusable.
        let mut mismatched = reuse.clone();
        let mut stored = context("project-a");
        stored.job_id = "another-id".into();
        mismatched.existing = Some(stored);
        assert_eq!(code(&admit(&mismatched).unwrap_err()), CONTEXT_CORRUPT);
        // A corrupt stored record never becomes an admission.
        let mut corrupted = reuse.clone();
        let mut stored = context("project-a");
        stored.input_sha256 = "short".into();
        corrupted.existing = Some(stored);
        assert_eq!(code(&admit(&corrupted).unwrap_err()), CONTEXT_CORRUPT);
    }

    #[test]
    fn membership_is_verified_before_any_stored_key_is_consulted() {
        // The caller reuses a key that exists for project-a, but names a
        // project they are not a member of: they learn nothing about the key.
        let mut probe = request();
        probe.requested_project = Some("project-c".into());
        probe.existing = Some(context("project-a"));
        let refused = refusal(admit(&probe).unwrap_err());
        assert_eq!(refused.code, PROJECT_NOT_VISIBLE);
        assert_eq!(refused.message, NOT_VISIBLE_MESSAGE);
    }

    #[test]
    fn a_caller_field_the_host_built_wrong_is_a_typed_refusal_not_a_crash() {
        for break_it in [
            (|r: &mut AdmitRequest| r.client = String::new()) as fn(&mut AdmitRequest),
            |r: &mut AdmitRequest| r.client = "c".repeat(MAX_CLIENT_CHARS + 1),
            |r: &mut AdmitRequest| r.operation = "delete_everything".into(),
            |r: &mut AdmitRequest| r.job_id = " id".into(),
            |r: &mut AdmitRequest| r.idempotency_key = "k\nk".into(),
            |r: &mut AdmitRequest| r.input_sha256 = "ZZ".repeat(32),
            |r: &mut AdmitRequest| r.principal.lane = "dev".into(),
            |r: &mut AdmitRequest| r.principal.uid = String::new(),
            |r: &mut AdmitRequest| r.requested_project = Some(" p".into()),
        ] {
            let mut broken = request();
            broken.saved_project = Some("project-a".into());
            break_it(&mut broken);
            assert_eq!(code(&admit(&broken).unwrap_err()), CONTEXT_CORRUPT);
        }
        let mut clockless = request();
        clockless.saved_project = Some("project-a".into());
        clockless.now_ms = 0;
        assert!(matches!(admit(&clockless).unwrap_err(), Fault::Hard(_)));
    }

    #[test]
    fn a_fresh_admission_records_the_connection_not_the_caller_s_claims() {
        let mut fresh = request();
        fresh.requested_project = Some("project-b".into());
        let admitted = admit(&fresh).unwrap();
        assert_eq!(admitted.outcome(), "admitted");
        assert_eq!(
            admitted.context(),
            &ExecutionContext {
                project: "project-b".into(),
                ..context("project-b")
            }
        );
        assert_eq!(admitted.context().admitted_at_ms, 2_000);
    }

    // ── visible ─────────────────────────────────────────────────────────

    #[test]
    fn every_reason_a_job_is_invisible_answers_the_same_words() {
        let stored = context("project-a");
        assert!(visible(&stored, &caller(), None));
        assert!(visible(&stored, &caller(), Some("project-a")));
        let foreign_uid = Caller {
            uid: "uid-b".into(),
            ..caller()
        };
        let foreign_lane = Caller {
            lane: "stable".into(),
            ..caller()
        };
        let foreign_deployment = Caller {
            deployment: "https://other.example".into(),
            ..caller()
        };
        for (label, caller, project) in [
            ("foreign principal", foreign_uid, None),
            ("foreign lane", foreign_lane, None),
            ("foreign deployment", foreign_deployment, None),
            ("wrong project", caller(), Some("project-b")),
            (
                "malformed caller",
                Caller {
                    uid: String::new(),
                    ..caller()
                },
                None,
            ),
        ] {
            assert!(!visible(&stored, &caller, project), "{label}");
            let answer = wire(json!({
                "kind": "visible",
                "caller": caller,
                "requested_project": project,
                "context": stored,
            }))
            .unwrap();
            assert_eq!(
                answer,
                json!({"visible": false, "refusal": {"code": NOT_VISIBLE, "message": NOT_FOUND}}),
                "{label} must answer exactly what an unknown id answers"
            );
        }
        // A context the host cannot even read fails closed.
        let mut unreadable = stored.clone();
        unreadable.project = String::new();
        assert!(!visible(&unreadable, &caller(), None));
        // install_id is recorded, never a visibility fence: the same account
        // reconnecting from another install still finds its own work.
        let mut reinstalled = stored.clone();
        reinstalled.install_id = "install-2".into();
        assert!(visible(&reinstalled, &caller(), None));
    }

    // ── retain ──────────────────────────────────────────────────────────

    #[test]
    fn no_lifecycle_event_and_no_clock_rewrites_a_context() {
        let stored = context("project-a");
        for event in [
            RetainEvent::Retry,
            RetainEvent::Restart,
            RetainEvent::Claim,
            RetainEvent::Publish,
            RetainEvent::Cancel,
        ] {
            assert_eq!(retain(&stored, event).unwrap(), stored);
            let answer = wire(json!({
                "kind": "retain", "context": stored, "event": event.wire(), "now_ms": 9_999_999_u64
            }))
            .unwrap();
            assert_eq!(
                answer,
                json!({"outcome": "retained", "event": event.wire(), "context": stored})
            );
        }
        let mut corrupt = stored.clone();
        corrupt.project = " ".into();
        assert_eq!(
            code(&retain(&corrupt, RetainEvent::Restart).unwrap_err()),
            CONTEXT_CORRUPT
        );
    }

    // ── capacity ────────────────────────────────────────────────────────

    #[test]
    fn a_submission_is_bounded_by_the_queue_globally_and_per_project() {
        let idle: Vec<Slot> = vec![];
        assert!(capacity(limits(), Intent::Admit, &idle, &idle, "project-a").is_ok());
        // A full running pool never refuses a submission: it queues.
        assert!(capacity(
            limits(),
            Intent::Admit,
            &slots(&["project-a"; 4]),
            &idle,
            "project-a"
        )
        .is_ok());
        let refused = refusal(
            capacity(
                limits(),
                Intent::Admit,
                &idle,
                &slots(&["project-a", "project-a", "project-a"]),
                "project-a",
            )
            .unwrap_err(),
        );
        assert_eq!(refused.code, CAPACITY_EXHAUSTED);
        assert_eq!(refused.scope, Some("project"));
        assert_eq!(refused.retry_after_ms, Some(4_000));
        // …and another project is still admitted while A is saturated.
        assert!(capacity(
            limits(),
            Intent::Admit,
            &idle,
            &slots(&["project-a", "project-a", "project-a"]),
            "project-b"
        )
        .is_ok());
        let global = refusal(
            capacity(
                limits(),
                Intent::Admit,
                &idle,
                &slots(&[
                    "project-a",
                    "project-b",
                    "project-a",
                    "project-b",
                    "project-a",
                    "project-b",
                    "project-a",
                    "project-b",
                ]),
                "project-c",
            )
            .unwrap_err(),
        );
        assert_eq!(global.scope, Some("global"));
        assert_eq!(global.retry_after_ms, Some(9_000));
        assert!(
            !global.message.contains("project-a") && !global.message.contains("project-b"),
            "a capacity refusal names counts, never another project: {}",
            global.message
        );
    }

    #[test]
    fn a_claim_shares_the_running_pool_only_while_another_project_waits() {
        let idle: Vec<Slot> = vec![];
        // Over its share, with nothing else waiting: the machine is used.
        assert!(capacity(
            limits(),
            Intent::Claim,
            &slots(&["project-a", "project-a", "project-a"]),
            &slots(&["project-a"]),
            "project-a"
        )
        .is_ok());
        // Over its share while another project waits: fairness refuses.
        let refused = refusal(
            capacity(
                limits(),
                Intent::Claim,
                &slots(&["project-a", "project-a"]),
                &slots(&["project-b", "project-a"]),
                "project-a",
            )
            .unwrap_err(),
        );
        assert_eq!(refused.code, CAPACITY_EXHAUSTED);
        assert_eq!(refused.scope, Some("project"));
        assert_eq!(refused.retry_after_ms, Some(2_000));
        assert!(
            !refused.message.contains("project-b"),
            "{}",
            refused.message
        );
        // The waiting project claims freely.
        assert!(capacity(
            limits(),
            Intent::Claim,
            &slots(&["project-a", "project-a"]),
            &slots(&["project-b"]),
            "project-b"
        )
        .is_ok());
        // Global exhaustion refuses every project, fairly or not.
        let global = refusal(
            capacity(
                limits(),
                Intent::Claim,
                &slots(&["project-a", "project-b", "project-c", "project-d"]),
                &idle,
                "project-b",
            )
            .unwrap_err(),
        );
        assert_eq!(global.scope, Some("global"));
        assert_eq!(global.retry_after_ms, Some(1_000));
    }

    #[test]
    fn capacity_guidance_is_deterministic_and_bounded() {
        assert_eq!(retry_after_ms(0), 1_000);
        assert_eq!(retry_after_ms(58), 59_000);
        assert_eq!(retry_after_ms(1_000_000), RETRY_AFTER_MAX_MS);
        // A nonsense configuration is the host's bug, never a client answer.
        for broken in [
            Limits {
                global_running: 0,
                ..limits()
            },
            Limits {
                per_project_running: MAX_LIMIT + 1,
                ..limits()
            },
            Limits {
                per_project_queued: 0,
                ..limits()
            },
            Limits {
                global_queued: 0,
                ..limits()
            },
        ] {
            assert!(matches!(
                capacity(broken, Intent::Admit, &[], &[], "project-a").unwrap_err(),
                Fault::Hard(_)
            ));
        }
        assert!(matches!(
            capacity(limits(), Intent::Admit, &[], &slots(&[" p"]), "project-a").unwrap_err(),
            Fault::Hard(_)
        ));
    }

    // ── recover ─────────────────────────────────────────────────────────

    fn row(engine: &str, context: Option<&ExecutionContext>) -> Value {
        let mut row = json!({
            "id": JOB_ID,
            "owner": "owner-digest",
            "lane": "canary",
            "input_sha256": DIGEST_A,
            "engine": engine,
            "input_tag": if engine == "fast_lv" { "ds.fast-lv.request/v1" } else { "ds.solar.calculate.prepared/v1" },
            "phase": "queued",
            "attempts": 0,
            "created_at_ms": 1_700_000_000_000_u64,
            "updated_at_ms": 1_700_000_000_000_u64,
            "worker": Value::Null,
            "lease_until_ms": 0,
            "result_sha256": Value::Null,
            "error": Value::Null,
        });
        if let Some(context) = context {
            row["context"] = serde_json::to_value(context).unwrap();
        }
        row
    }

    #[test]
    fn a_row_that_carries_a_context_is_returned_unchanged() {
        let stored = context("project-a");
        let (recovered, provenance) = recover(
            row("solar_prepared", Some(&stored)),
            &principal(),
            Some("project-z"),
            Some("project-a"),
            5_000,
        )
        .unwrap();
        assert_eq!(recovered, stored);
        assert_eq!(provenance, Provenance::Stored);
        assert_eq!(provenance.wire(), "stored");
        // A saved selection cannot overwrite what the row already records.
        let (again, _) = recover(
            row("solar_prepared", Some(&stored)),
            &principal(),
            Some("project-z"),
            None,
            5_000,
        )
        .unwrap();
        assert_eq!(again.project, "project-a");
        // Sealed bytes that disagree with the stored context are a mismatch.
        assert_eq!(
            code(
                &recover(
                    row("solar_prepared", Some(&stored)),
                    &principal(),
                    None,
                    Some("project-b"),
                    5_000,
                )
                .unwrap_err()
            ),
            SCOPE_MISMATCH
        );
    }

    #[test]
    fn a_legacy_row_recovers_from_the_sealed_input_or_the_saved_selection() {
        let (solar, provenance) = recover(
            row("solar_prepared", None),
            &principal(),
            Some("project-b"),
            Some("project-a"),
            5_000,
        )
        .unwrap();
        assert_eq!(provenance, Provenance::SealedInput);
        assert_eq!(
            solar.project, "project-a",
            "the sealed input outranks a saved selection at recovery too"
        );
        assert_eq!(solar.operation, SOLAR_PROCESSING);
        assert_eq!(solar.client, RECOVERED_CLIENT);
        assert_eq!(solar.idempotency_key, RECOVERED_KEY);
        assert_eq!(solar.job_id, JOB_ID);
        assert_eq!(solar.input_sha256, DIGEST_A);
        assert_eq!(
            solar.admitted_at_ms, 1_700_000_000_000,
            "recovery keeps the row's own admission moment"
        );
        let (transformer, provenance) = recover(
            row("fast_lv", None),
            &principal(),
            Some("project-b"),
            None,
            5_000,
        )
        .unwrap();
        assert_eq!(provenance, Provenance::SavedSelection);
        assert_eq!(transformer.project, "project-b");
        assert_eq!(transformer.operation, TRANSFORMER_PROCESSING);
    }

    #[test]
    fn a_legacy_row_with_nothing_to_recover_from_is_never_guessed() {
        assert_eq!(
            code(&recover(row("fast_lv", None), &principal(), None, None, 5_000).unwrap_err()),
            CONTEXT_UNRECOVERABLE,
            "a transformer row carries no project by design"
        );
        assert_eq!(
            code(
                &recover(
                    row("solar_prepared", None),
                    &principal(),
                    Some("project-b"),
                    None,
                    5_000
                )
                .unwrap_err()
            ),
            CONTEXT_UNRECOVERABLE,
            "a Solar row's project comes from its bytes, never from a selection"
        );
        assert_eq!(
            code(&recover(json!({"id": JOB_ID}), &principal(), None, None, 5_000).unwrap_err()),
            CONTEXT_UNRECOVERABLE
        );
        // Another lane's row is never reopened under this principal.
        let mut other_lane = row("fast_lv", None);
        other_lane["lane"] = json!("stable");
        assert_eq!(
            code(&recover(other_lane, &principal(), Some("project-b"), None, 5_000).unwrap_err()),
            PRINCIPAL_MISMATCH
        );
        // Nor is a stored context belonging to another identity.
        let mut foreign = context("project-a");
        foreign.principal_uid = "uid-b".into();
        assert_eq!(
            code(
                &recover(
                    row("solar_prepared", Some(&foreign)),
                    &principal(),
                    None,
                    None,
                    5_000
                )
                .unwrap_err()
            ),
            PRINCIPAL_MISMATCH
        );
        // A stored context bound to another job is corrupt.
        let mut misbound = context("project-a");
        misbound.job_id = DIGEST_B.into();
        assert_eq!(
            code(
                &recover(
                    row("solar_prepared", Some(&misbound)),
                    &principal(),
                    None,
                    None,
                    5_000
                )
                .unwrap_err()
            ),
            CONTEXT_CORRUPT
        );
        assert!(matches!(
            recover(row("fast_lv", None), &principal(), Some("p"), None, 0).unwrap_err(),
            Fault::Hard(_)
        ));
    }

    // ── the wire ────────────────────────────────────────────────────────

    #[test]
    fn every_op_answers_over_the_wire() {
        let admitted = wire(json!({
            "kind": "admit",
            "now_ms": 2_000,
            "principal": principal(),
            "client": "cli:4711",
            "operation": SOLAR_PROCESSING,
            "job_id": JOB_ID,
            "idempotency_key": "key-a",
            "input_sha256": DIGEST_A,
            "requested_project": "project-a",
            "membership": membership(&["project-a"]),
        }))
        .unwrap();
        assert_eq!(admitted["outcome"], "admitted");
        assert_eq!(admitted["context"]["project"], "project-a");
        let refused = wire(json!({
            "kind": "admit",
            "now_ms": 2_000,
            "principal": principal(),
            "client": "cli:4711",
            "operation": SOLAR_PROCESSING,
            "job_id": JOB_ID,
            "idempotency_key": "key-a",
            "input_sha256": DIGEST_A,
            "membership": membership(&["project-a"]),
        }))
        .unwrap();
        assert_eq!(
            refused,
            json!({"outcome": "refused", "refusal": {"code": PROJECT_REQUIRED, "message": "name a project, or select one first"}})
        );
        assert_eq!(
            wire(json!({"kind": "visible", "caller": caller(), "context": context("project-a")}))
                .unwrap(),
            json!({"visible": true})
        );
        assert_eq!(
            wire(json!({
                "kind": "capacity", "limits": limits(), "running": [], "queued": [], "project": "project-a"
            }))
            .unwrap(),
            json!({"outcome": "admitted"})
        );
        let saturated = wire(json!({
            "kind": "capacity", "intent": "claim", "limits": limits(),
            "running": slots(&["project-a"; 4]), "queued": [], "project": "project-a"
        }))
        .unwrap();
        assert_eq!(saturated["refusal"]["code"], CAPACITY_EXHAUSTED);
        assert_eq!(saturated["refusal"]["scope"], "global");
        assert_eq!(saturated["refusal"]["retry_after_ms"], 1_000);
        let recovered = wire(json!({
            "kind": "recover", "row": row("fast_lv", None), "principal": principal(),
            "saved_project": "project-b", "now_ms": 5_000
        }))
        .unwrap();
        assert_eq!(recovered["outcome"], "recovered");
        assert_eq!(recovered["provenance"], "saved_selection");
        assert_eq!(recovered["context"]["project"], "project-b");
    }

    #[test]
    fn malformed_requests_are_refused_before_any_decision() {
        assert!(evaluate(b"{").is_err());
        assert!(evaluate(&to_vec(&json!({"schema": "x", "op": {"kind": "retain", "context": context("p"), "event": "retry"}})).unwrap()).is_err());
        assert!(
            evaluate(&to_vec(&json!({"schema": SCHEMA, "op": {"kind": "sideways"}})).unwrap())
                .is_err()
        );
        // Every op's body is closed.
        assert!(wire(json!({
            "kind": "retain", "context": context("project-a"), "event": "retry", "surprise": 1
        }))
        .is_err());
        assert!(wire(json!({
            "kind": "retain", "context": context("project-a"), "event": "reassign_project"
        }))
        .is_err());
        assert!(wire(json!({"kind": "visible", "caller": caller()})).is_err());
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    }

    #[test]
    fn a_fault_says_whether_a_host_may_relay_it() {
        let refused = admit(&request()).unwrap_err();
        assert_eq!(refused.code(), Some(PROJECT_REQUIRED));
        assert_eq!(refused.message(), "name a project, or select one first");
        assert_eq!(
            refused.to_string(),
            "project_required: name a project, or select one first"
        );
        let mut clockless = request();
        clockless.saved_project = Some("project-a".into());
        clockless.now_ms = 0;
        let bug = admit(&clockless).unwrap_err();
        assert_eq!(bug.code(), None, "a host bug has nothing to relay");
        assert_eq!(bug.to_string(), bug.message());
    }

    #[test]
    fn the_refusal_vocabulary_is_closed() {
        assert_eq!(REFUSALS.len(), 10);
        let unique: std::collections::BTreeSet<&str> = REFUSALS.into_iter().collect();
        assert_eq!(unique.len(), REFUSALS.len(), "a code is listed twice");
        assert_eq!(OPERATIONS.len(), 5);
    }
}
