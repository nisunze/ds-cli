//! The execution context — one immutable record per admitted operation, and
//! the five decisions that keep it honest.
//!
//! A Server admits work for several of its owner's projects at once. Nothing
//! about that work may be read from a process-wide "current project", a saved
//! CLI selection captured at startup, or an id a caller happens to hold. Every
//! admitted operation instead carries a record decided here, once: who is
//! authenticated, on which lane and deployment, from which install, for which
//! project, from which client, which operation, which job, which exact input
//! bytes, and when it was admitted. The record is written with the job row and
//! never rewritten — a retry, a restart, a claim, a publication and a
//! cancellation all present the same context they started with.
//!
//! **The boundary.** The Server is the desktop's core, wrapped by the desktop,
//! and it stands on the desktop's side of the desktop's one boundary with
//! ds-brain: entitlement is decided at that boundary and never re-decided
//! here. So this owner fetches, caches and refreshes no project directory, and
//! carries no online/offline conditional. An admission is the authenticated
//! connection, the project the work names — a sealed input's own project
//! outranks the named one — and the caller's key with the digest of its exact
//! bytes. Nothing else. Offline-first falls out of that: admission, queueing,
//! execution, retry, restart recovery and capacity are all decided with no
//! upstream present at all.
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
//! - **A named project is the operation's project.** A sealed input
//!   that already names its project (Solar) outranks it; an explicit request
//!   that contradicts the bytes is `scope_mismatch`. Naming a project is not
//!   being entitled to it: that is the gateway's answer on the effect that
//!   leaves the machine, at publication and at sync.
//! - **A project id is one path segment.** No `/`, no `\`, not `.` or `..`,
//!   no whitespace, no control character, at most 200 characters. Hosts
//!   partition durable state by this name — store rows, dataset caches,
//!   staging directories, report names — so an id that could mean a different
//!   place on disk is `context_corrupt` at every door it reaches.
//! - **An id never authorizes.** `visible` answers one word, `job not found`,
//!   to a foreign principal, a foreign lane, a foreign deployment, a caller
//!   naming the wrong project, and a caller naming an id that does not exist.
//!   Nothing distinguishes them, so nothing is disclosed by probing.
//! - **Nothing rewrites scope.** `retain` takes `now_ms` and ignores it. That
//!   is the point: time, a retry, a restart or a publication cannot move a job
//!   to another project.
//! - **Capacity is bounded and fair.** A project may exceed its running share
//!   while nothing else is waiting, and may not while another project under
//!   its own share has queued work; two projects both at their share never
//!   idle a worker between them. Saturation is a typed answer with retry
//!   guidance, never a dropped request.
//! - **A share is smaller than the pool it divides.** Limits whose
//!   `per_project_*` is not below its `global_*` give one project the whole
//!   host while another waits, which is the thing the share exists to prevent,
//!   so they are refused here as a host configuration fault — not silently
//!   honoured because a host's own flag parser happened to check. The one
//!   exception is a pool of exactly one: a single-worker host (the smallest
//!   rented VM) has no share to give, its one project at a time holds
//!   everything by definition, and its `per_project_*` must therefore be 1.
//! - **Recovery reconstructs; it does not guess.** A row written by a released
//!   Server gets its project from its own sealed input (Solar, which carries
//!   it) and from nothing else — never from whichever project happens to be
//!   selected now. A transformer row carries no project by design, so it is
//!   `context_unrecoverable`: readable, never claimed, never published, until
//!   its owner reads the bytes it was admitted with and submits them again
//!   naming the project. Not its result — such a row never ran and has none.
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
/// The kernel's own project bound, as `survey_query` and
/// `survey_entries_changes` already read it. It is deliberately not
/// `layer_document`'s 500-character free-form bound: a project id that reaches
/// a durable job row is a name hosts partition by, so it is bounded by
/// [`valid_project`]'s grammar as well as by this number.
pub const MAX_PROJECT_CHARS: usize = 200;
pub const MAX_CLIENT_CHARS: usize = 120;
pub const MAX_KEY_CHARS: usize = 200;
pub const MAX_JOB_ID_CHARS: usize = 200;
pub const MAX_SLOTS: usize = 100_000;
pub const MAX_LIMIT: usize = 4_096;

/// Retry guidance grows with the work that must go first and stops at a
/// minute, so a saturated host never invites a hot retry loop.
pub const RETRY_AFTER_BASE_MS: u64 = 1_000;
pub const RETRY_AFTER_MAX_MS: u64 = 60_000;

/// What a reconstructed context records instead of the client and key a
/// released Server never persisted. The durable id is a digest and cannot be
/// inverted, so the key is not recoverable; reuse is decided by project,
/// operation and input digest, never by the text.
pub const RECOVERED_CLIENT: &str = "recovery:legacy-row";
pub const RECOVERED_KEY: &str = "recovered";

/// The one thing the owner of a row this owner cannot place can actually do,
/// said in the refusal itself so it travels with the answer to every host.
///
/// Deliberately **not** "read its result": such a row was never claimed and
/// never ran, so it has none. What it has is the bytes it was admitted with,
/// which a host returns (`GET /v1/jobs/:id/input`, `ds server input`);
/// submitting them again under a named project is a new job with its own id.
pub const UNRECOVERABLE_REMEDY: &str =
    "read its stored input and submit it again naming the project";

// ── The operation vocabulary ────────────────────────────────────────────
pub const TRANSFORMER_PROCESSING: &str = "transformer_processing";
pub const TILE_PROCESSING: &str = "tile_processing";
pub const SOLAR_PROCESSING: &str = "solar_processing";
pub const REPORT_EXPORT: &str = "report_export";
pub const LAYER_READ: &str = "layer_read";
pub const LAYER_WRITE: &str = "layer_write";
pub const SYNC_RECOVERY: &str = "sync_recovery";
/// Retained native Profile sessions carry the same principal/project context.
pub const PROFILE_SESSION: &str = "profile_session";
pub const OPERATIONS: [&str; 8] = [
    TRANSFORMER_PROCESSING,
    SOLAR_PROCESSING,
    TILE_PROCESSING,
    REPORT_EXPORT,
    LAYER_READ,
    LAYER_WRITE,
    SYNC_RECOVERY,
    PROFILE_SESSION,
];

// ── The refusal vocabulary (closed) ─────────────────────────────────────
pub const PROJECT_REQUIRED: &str = "project_required";
pub const SCOPE_MISMATCH: &str = "scope_mismatch";
pub const SCOPE_MISMATCH_FOR_KEY: &str = "scope_mismatch_for_key";
pub const PAYLOAD_CHANGED_FOR_KEY: &str = "payload_changed_for_key";
pub const PRINCIPAL_MISMATCH: &str = "principal_mismatch";
pub const NOT_VISIBLE: &str = "not_visible";
pub const CAPACITY_EXHAUSTED: &str = "capacity_exhausted";
pub const CONTEXT_CORRUPT: &str = "context_corrupt";
pub const CONTEXT_UNRECOVERABLE: &str = "context_unrecoverable";
pub const REFUSALS: [&str; 9] = [
    PROJECT_REQUIRED,
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

/// The two sentences the durable row states independently of [`admit`].
///
/// A row is the second, racing statement of the same rule: two first
/// submissions under one key can reach the insert together, and the one that
/// loses is refused by the row rather than by this owner, which never saw the
/// other's context. Its host then relays that as one of these codes — so the
/// sentence is written once, here, and both statements are the same answer.
/// Two spellings of one refusal are two answers to one question, and the
/// spelling that drops the remedy is the dead end §3 forbids.
pub const PAYLOAD_CHANGED_FOR_KEY_MESSAGE: &str = "that idempotency key was admitted with different input bytes; resubmit the bytes it was admitted with, or use a new key for these";
pub const SCOPE_MISMATCH_FOR_KEY_MESSAGE: &str = "that idempotency key already names another project or operation; submit this under a key of its own";

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
    /// The project this operation is about, as the owner named it or as its
    /// sealed bytes named it — one path segment ([`valid_project`]).
    /// Entitlement to it is the gateway's answer where an effect leaves the
    /// machine, never a decision made here.
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
        if !bounded(&self.principal_uid, MAX_UID_CHARS) {
            return Err(corrupt("principal_uid", text_rule(MAX_UID_CHARS)));
        }
        if !valid_lane(&self.lane) {
            return Err(corrupt("lane", lane_rule()));
        }
        if !bounded(&self.deployment, MAX_DEPLOYMENT_CHARS) {
            return Err(corrupt("deployment", text_rule(MAX_DEPLOYMENT_CHARS)));
        }
        if !bounded(&self.install_id, MAX_INSTALL_CHARS) {
            return Err(corrupt("install_id", text_rule(MAX_INSTALL_CHARS)));
        }
        if !valid_project(&self.project) {
            return Err(not_a_segment("project"));
        }
        if !bounded(&self.client, MAX_CLIENT_CHARS) {
            return Err(corrupt("client", text_rule(MAX_CLIENT_CHARS)));
        }
        if !OPERATIONS.contains(&self.operation.as_str()) {
            return Err(corrupt("operation", operation_rule()));
        }
        if !bounded(&self.job_id, MAX_JOB_ID_CHARS) {
            return Err(corrupt("job_id", text_rule(MAX_JOB_ID_CHARS)));
        }
        if !bounded(&self.idempotency_key, MAX_KEY_CHARS) {
            return Err(corrupt("idempotency_key", text_rule(MAX_KEY_CHARS)));
        }
        if !compute_jobs::digest(&self.input_sha256) {
            return Err(corrupt("input_sha256", digest_rule()));
        }
        if self.admitted_at_ms == 0 {
            return Err(corrupt(
                "admitted_at_ms",
                "the host's clock in milliseconds, never zero",
            ));
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
            return Err(corrupt("principal.uid", text_rule(MAX_UID_CHARS)));
        }
        if !valid_lane(&self.lane) {
            return Err(corrupt("principal.lane", lane_rule()));
        }
        if !bounded(&self.deployment, MAX_DEPLOYMENT_CHARS) {
            return Err(corrupt(
                "principal.deployment",
                text_rule(MAX_DEPLOYMENT_CHARS),
            ));
        }
        if !bounded(&self.install_id, MAX_INSTALL_CHARS) {
            return Err(corrupt(
                "principal.install_id",
                text_rule(MAX_INSTALL_CHARS),
            ));
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

impl Limits {
    /// Whether these four numbers are a configuration at all.
    ///
    /// Every one is 1..=[`MAX_LIMIT`], and each `per_project_*` must be
    /// smaller than the `global_*` it shares — see [`divides`] for the one
    /// exception, a pool of exactly one. A configuration that fails this is
    /// the host's own bug, so it is [`Fault::Hard`] and never a refusal a
    /// caller receives.
    ///
    /// [`capacity`] asks this before every decision, so no answer is ever
    /// given under numbers that make no sense. It is public because that is
    /// the wrong moment to find out: a host discovering it once per claim
    /// logs the same fault several times a second and admits nothing, where a
    /// host that asks before it starts its workers refuses to start and says
    /// which two numbers to change.
    pub fn validate(&self) -> Result<(), Fault> {
        for (value, name) in [
            (self.global_running, "global_running"),
            (self.per_project_running, "per_project_running"),
            (self.per_project_queued, "per_project_queued"),
            (self.global_queued, "global_queued"),
        ] {
            if value == 0 || value > MAX_LIMIT {
                return Err(hard(format!(
                    "capacity limit {name} must be 1..={MAX_LIMIT}"
                )));
            }
        }
        for (share, share_name, pool, pool_name) in [
            (
                self.per_project_running,
                "per_project_running",
                self.global_running,
                "global_running",
            ),
            (
                self.per_project_queued,
                "per_project_queued",
                self.global_queued,
                "global_queued",
            ),
        ] {
            if !divides(share, pool) {
                return Err(hard(format!(
                    "capacity limit {share_name} is {share} and {pool_name} is {pool}: a share must be smaller than the pool it divides, or one project holds the whole host while another waits; only a single-slot host ({pool_name} 1) has no share, and there {share_name} must be 1"
                )));
            }
        }
        Ok(())
    }
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
}

impl Provenance {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::SealedInput => "sealed_input",
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
/// A field that is not what the record says it is — named, **and told the
/// rule it broke**. A caller handed a refusal with no rule in it can read the
/// code and still not know what to send instead, which is a dead end rather
/// than an answer.
fn corrupt(field: &str, rule: impl std::fmt::Display) -> Fault {
    refused(
        CONTEXT_CORRUPT,
        format!("execution context field {field} is not valid: {rule}"),
    )
}
/// The rule every bounded text field of the record keeps.
fn text_rule(max: usize) -> String {
    format!("1..={max} characters, trimmed, with no control character")
}
/// The two closed vocabularies and the digest, said in full rather than
/// named, because a caller cannot look up a constant.
fn lane_rule() -> String {
    "one of stable, canary".to_owned()
}
fn operation_rule() -> String {
    format!("one of {}", OPERATIONS.join(", "))
}
fn digest_rule() -> String {
    "a 64-character lowercase sha-256 digest".to_owned()
}
/// The project fields answer with the grammar itself, so an operator reads the
/// rule off the refusal instead of guessing which character was the problem.
fn not_a_segment(field: &str) -> Fault {
    refused(
        CONTEXT_CORRUPT,
        format!(
            "execution context field {field} is not a project id: one path segment of 1..={MAX_PROJECT_CHARS} characters, with no separator, drive, traversal, trailing dot, whitespace, invisible character, wildcard or device name"
        ),
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

/// Characters no filesystem reads as part of a name: the two separators, and
/// the drive-and-stream colon plus the wildcard and redirection set Win32
/// refuses outright. The colon is the dangerous one — Rust's own
/// `Path::join("C:")` **replaces** the whole path on Windows, because `C:` is a
/// prefix, so a project id of `C:` would take a host's `state_dir.join(project)`
/// clean out of the state directory.
const NOT_IN_A_NAME: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// The names Windows resolves to a device at any depth: `<store>/NUL` swallows
/// what is written to it and `<store>/CON` reads the console, whatever
/// directory they are under and whatever extension follows. Win32 matches them
/// on the stem before the first `.`, case-insensitively, and so does this.
const DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A project id is **one path segment**, and nothing that could mean a
/// different place on disk — on any machine the product runs on, which
/// includes the Windows desktop.
///
/// The record is durable and every host partitions by the project in it — a
/// store row, a dataset cache, a staging directory, a report name. So an id is
/// refused when it traverses (`.`, `..`, `A/../B`), carries a separator (`A/B`,
/// `A\B`), is padded, split or replaced by whitespace (`A B`, ` A`), names a
/// drive or a stream (`C:`, `A:b`), holds a wildcard (`A*`), ends in a dot
/// (`A.`, which Windows strips, so `A.` and `A` would be two ids and one
/// directory), carries a character that draws nothing ([`renders_as_nothing`])
/// or is a device (`NUL`, `con.json`). Each of those is a path expression
/// rather than a name, and each is `context_corrupt` wherever it appears:
/// named by a caller, sealed in the bytes,
/// counted in a census or stored in a record. One rule, one answer, at every
/// door — a host may then use the id in a path without asking again what it
/// might be.
pub fn valid_project(project: &str) -> bool {
    !project.is_empty()
        && project.chars().count() <= MAX_PROJECT_CHARS
        // `.` and `..` end in a dot, so the trailing-dot rule is also the
        // traversal rule; nothing else needs to name them.
        && !project.ends_with('.')
        && !project.chars().any(|character| {
            NOT_IN_A_NAME.contains(&character)
                || character.is_whitespace()
                || character.is_control()
                || renders_as_nothing(character)
        })
        && !names_a_device(project)
}

/// A character that occupies no space when it is drawn.
///
/// The reason whitespace is refused is that `A B` is one thing to a shell,
/// another to a log line and a third to a path. A character with no glyph at
/// all is that failure taken further: `project\u{200B}-a` and `project-a` are
/// two ids, two directories and two sets of report names, and every screen an
/// operator can look at — the CLI, the activity list, the refusal itself —
/// draws them as the same word. Nothing legitimate needs one in a name, so
/// they are refused with the whitespace they behave like: the soft hyphen, the
/// zero-width and bidirectional format characters (including the overrides
/// that make an id read backwards), the interlinear annotations, the byte
/// order mark, the variation selectors and the tag block.
///
/// This is a named list rather than a Unicode category because the kernel
/// carries no Unicode tables; it is the whole of the invisible surface an id
/// can be spelled with, not a sample of it.
const fn renders_as_nothing(character: char) -> bool {
    matches!(character,
        '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{feff}'
        | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206f}'
        | '\u{fe00}'..='\u{fe0f}'
        | '\u{fff9}'..='\u{fffb}'
        | '\u{e0000}'..='\u{e007f}'
        | '\u{e0100}'..='\u{e01ef}')
}

/// Whether this id is one of the Windows device names, extension and case
/// ignored exactly as Win32 ignores them.
fn names_a_device(project: &str) -> bool {
    let stem = project.split('.').next().unwrap_or(project);
    DEVICE_NAMES
        .iter()
        .any(|device| stem.eq_ignore_ascii_case(device))
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
    /// The project the sealed input itself names (Solar). Authoritative.
    #[serde(default)]
    pub sealed_project: Option<String>,
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

/// Admit one operation under the project it names, or refuse it by name.
///
/// The order of the checks is itself a rule: the project is resolved **before**
/// anything stored under the caller's key is consulted, so what a key already
/// holds can never decide what the request is about.
pub fn admit(request: &AdmitRequest) -> Result<Admitted, Fault> {
    if request.now_ms == 0 {
        return Err(hard("admission needs the host's clock in now_ms"));
    }
    request.principal.validate()?;
    if !bounded(&request.client, MAX_CLIENT_CHARS) {
        return Err(corrupt("client", text_rule(MAX_CLIENT_CHARS)));
    }
    if !OPERATIONS.contains(&request.operation.as_str()) {
        return Err(corrupt("operation", operation_rule()));
    }
    if !bounded(&request.job_id, MAX_JOB_ID_CHARS) {
        return Err(corrupt("job_id", text_rule(MAX_JOB_ID_CHARS)));
    }
    if !bounded(&request.idempotency_key, MAX_KEY_CHARS) {
        return Err(corrupt("idempotency_key", text_rule(MAX_KEY_CHARS)));
    }
    if !compute_jobs::digest(&request.input_sha256) {
        return Err(corrupt("input_sha256", digest_rule()));
    }
    let project = resolve_project(request)?;
    if let Some(existing) = &request.existing {
        existing.validate()?;
        if existing.principal_uid != request.principal.uid
            || existing.lane != request.principal.lane
            || existing.deployment != request.principal.deployment
        {
            return Err(refused(
                PRINCIPAL_MISMATCH,
                "that key was admitted under another authenticated identity; sign in as that account, on the lane and deployment it used, or submit under a key of your own",
            ));
        }
        if existing.job_id != request.job_id {
            return Err(refused(
                CONTEXT_CORRUPT,
                "the stored context is bound to another job id; the host looked this key up outside the project it is submitting for",
            ));
        }
        // The reachable half of this is the operation: within one project a
        // key is one handle on one piece of work, and reusing it for a
        // different operation is a collision, not a resubmit.
        //
        // The project half is a fence, not a promise. A durable id digests the
        // project (`ds_compute_runtime::job_id`), so the same key in two
        // projects is two ids and two jobs — a host looking this key up in
        // project B finds nothing at all and admits fresh, and a host that
        // somehow handed over another project's record would already have been
        // stopped by the job-id check above. The kernel still refuses it,
        // because it does not assume any host's id scheme; the corpus records
        // no case for it, because in this build no host can produce one.
        if existing.project != project || existing.operation != request.operation {
            return Err(refused(
                SCOPE_MISMATCH_FOR_KEY,
                SCOPE_MISMATCH_FOR_KEY_MESSAGE,
            ));
        }
        if existing.input_sha256 != request.input_sha256 {
            return Err(refused(
                PAYLOAD_CHANGED_FOR_KEY,
                PAYLOAD_CHANGED_FOR_KEY_MESSAGE,
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

fn resolve_project(request: &AdmitRequest) -> Result<String, Fault> {
    project_for(
        request.requested_project.as_deref(),
        request.sealed_project.as_deref(),
    )
}

/// Which project one operation is about: sealed bytes outrank an explicit
/// request. A caller who names a contradicting project is refused.
///
/// Public because a host must know the project before it can derive the
/// durable job id it then hands to [`admit`]: the id digests the project, so
/// the same key in two projects is two pieces of work rather than a collision.
/// `admit` asks this again and gets the same answer; nothing decides twice.
pub fn project_for(requested: Option<&str>, sealed: Option<&str>) -> Result<String, Fault> {
    let requested = optional_project(requested, "requested_project")?;
    let sealed = optional_project(sealed, "sealed_project")?;
    if let Some(sealed) = sealed {
        if let Some(requested) = requested {
            if requested != sealed {
                return Err(refused(
                    SCOPE_MISMATCH,
                    format!(
                        "this request is for {requested}, but its sealed input was prepared for another project; submit it without naming a project, or prepare an input for {requested}"
                    ),
                ));
            }
        }
        return Ok(sealed.to_owned());
    }
    requested.map(str::to_owned).ok_or_else(|| {
        refused(
            PROJECT_REQUIRED,
            "no project was named for this request; pass --project or submit sealed input that names its project",
        )
    })
}

fn optional_project<'a>(value: Option<&'a str>, field: &str) -> Result<Option<&'a str>, Fault> {
    match value {
        Some(project) if valid_project(project) => Ok(Some(project)),
        Some(_) => Err(not_a_segment(field)),
        None => Ok(None),
    }
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
/// input is a host bug (`Fault::Hard`), never a client refusal — including
/// limits that do not make a share ([`Limits::validate`], asked first here so
/// no answer is given under numbers that make no sense, and asked by a host
/// before it starts its workers so it never has to find out one claim at a
/// time).
///
/// `Intent::Admit` bounds the queue a submission enters. `Intent::Claim`
/// bounds the running pool and applies the fairness share: a project may hold
/// more than `per_project_running` slots while nothing else is waiting, and
/// may not while another project **under its own share** has queued work —
/// the slot it is refused is one that project can actually take. When every
/// waiting project is at its share too, refusing would idle the worker, not
/// share it, so the claim is admitted and queue order decides.
///
/// The host asks this inside the same store transaction that claims, with a
/// census read there: a census read earlier is a fairness decision about a
/// pool that no longer exists.
pub fn capacity(
    limits: Limits,
    intent: Intent,
    running: &[Slot],
    queued: &[Slot],
    project: &str,
) -> Result<(), Fault> {
    limits.validate()?;
    if running.len() > MAX_SLOTS || queued.len() > MAX_SLOTS {
        return Err(hard("capacity census exceeds its bound"));
    }
    if !valid_project(project)
        || running
            .iter()
            .chain(queued)
            .any(|slot| !valid_project(&slot.project))
    {
        return Err(hard(
            "capacity was asked about a project id that is not one path segment",
        ));
    }
    let queued_here = queued.iter().filter(|slot| slot.project == project).count();
    let mut running_by_project: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for slot in running {
        *running_by_project.entry(slot.project.as_str()).or_default() += 1;
    }
    let running_here = running_by_project.get(project).copied().unwrap_or(0);
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
            if running_here >= limits.per_project_running {
                // Waiting work of projects that are under their share: the
                // work this slot would go to instead. A waiting project that
                // is itself at its share cannot take the slot, so it is not
                // a reason to leave the worker idle.
                let yields_to = queued
                    .iter()
                    .filter(|slot| slot.project != project)
                    .filter(|slot| {
                        running_by_project
                            .get(slot.project.as_str())
                            .copied()
                            .unwrap_or(0)
                            < limits.per_project_running
                    })
                    .count();
                if yields_to > 0 {
                    return Err(exhausted(
                        "project",
                        format!(
                            "this project holds {running_here} of {} running slots while other projects are waiting",
                            limits.per_project_running
                        ),
                        yields_to,
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Whether one project's share actually divides the host's pool.
///
/// A share equal to the pool is not a share: the first project to arrive may
/// hold every slot while a second waits, which is exactly what the share
/// exists to prevent, and a share larger than the pool says even less. The one
/// exception is a pool of one — the smallest rented VM, `--workers 1`. It
/// cannot be divided at all: whatever runs holds the machine, and the honest
/// configuration is a share of exactly 1. So `1/1` is admitted and `2/1` is
/// not, and from two slots up the share must be strictly smaller.
const fn divides(share: usize, pool: usize) -> bool {
    if pool == 1 {
        share == 1
    } else {
        share < pool
    }
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
/// row takes its project from its own sealed input, which is authoritative
/// because the bytes themselves name it. A legacy transformer row carries no
/// project by design and there is no second source for one: it is
/// `context_unrecoverable`. It is never recovered into whichever project
/// happens to be selected when the host reopens the queue — that would run one
/// owner's retained work, and publish it, somewhere it was never admitted for.
/// Such a row stays readable, and [`UNRECOVERABLE_REMEDY`] — carried in the
/// refusal — is what its owner does with it.
pub fn recover(
    row: Value,
    principal: &Principal,
    sealed_project: Option<&str>,
    now_ms: u64,
) -> Result<(ExecutionContext, Provenance), Fault> {
    if now_ms == 0 {
        return Err(hard("recovery needs the host's clock in now_ms"));
    }
    principal.validate()?;
    let sealed = optional_project(sealed_project, "sealed_project")?;
    let Ok(job) = serde_json::from_value::<Job>(row) else {
        return Err(refused(
            CONTEXT_UNRECOVERABLE,
            "that row is not a durable compute job; nothing can be reopened from it",
        ));
    };
    // The store already reads rows by (owner, lane); the owner is a digest the
    // kernel holds no credential material to recompute, so the lane is the
    // fence it can check here.
    if job.lane != principal.lane {
        return Err(refused(
            PRINCIPAL_MISMATCH,
            "that row belongs to another deployment lane; reopen the queue on the lane it was admitted on",
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
                "that row was admitted under another authenticated identity; reopen the queue as that account",
            ));
        }
        if context.job_id != job.id || context.input_sha256 != job.input_sha256 {
            return Err(refused(
                CONTEXT_CORRUPT,
                "the stored context is bound to another job id or input digest; that row was rewritten outside this owner and cannot be reopened",
            ));
        }
        if sealed.is_some_and(|sealed| sealed != context.project) {
            return Err(refused(
                SCOPE_MISMATCH,
                "that row's sealed input was prepared for another project; reopen it with the project its own bytes name, or with none",
            ));
        }
        return Ok((context, Provenance::Stored));
    }
    let unrecoverable = |what: &str| {
        refused(
            CONTEXT_UNRECOVERABLE,
            format!("that row predates execution contexts and {what}; {UNRECOVERABLE_REMEDY}"),
        )
    };
    let (project, provenance, operation) = match job.engine {
        EngineKind::TilesPrepared => {
            return Err(unrecoverable(
                "a tiling job must retain its admitted context",
            ));
        }
        EngineKind::SolarPrepared => (
            sealed.ok_or_else(|| unrecoverable("its sealed input names no project"))?,
            Provenance::SealedInput,
            SOLAR_PROCESSING,
        ),
        EngineKind::LvProcessing => {
            return Err(unrecoverable("a transformer request names no project"));
        }
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
            sealed_project: None,
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
        type Case = (&'static str, fn(&mut ExecutionContext));
        let cases: [Case; 16] = [
            ("principal_uid", |c| c.principal_uid = String::new()),
            ("principal_uid", |c| c.principal_uid = " uid".into()),
            ("lane", |c| c.lane = "development".into()),
            ("deployment", |c| c.deployment = String::new()),
            ("install_id", |c| c.install_id = "a\u{0}b".into()),
            ("project", |c| c.project = "  ".into()),
            ("project", |c| c.project = "p".repeat(MAX_PROJECT_CHARS + 1)),
            ("project", |c| c.project = "..".into()),
            ("project", |c| c.project = "a/b".into()),
            ("project", |c| c.project = "a\\b".into()),
            ("project", |c| c.project = "a b".into()),
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
            // Naming the field is half an answer. The rule it broke is the
            // other half, and without it the caller knows only that
            // something was wrong.
            let (_, rule) = refused
                .message
                .split_once(": ")
                .unwrap_or_else(|| panic!("{field}: {} states no rule", refused.message));
            assert!(rule.len() > 15, "{field}: {rule}");
        }
        // The closed vocabularies are spelled out, not named: a caller cannot
        // look up a constant it does not have.
        let mut wrong_lane = context("project-a");
        wrong_lane.lane = "development".into();
        assert!(refusal(wrong_lane.validate().unwrap_err())
            .message
            .contains("one of stable, canary"));
        let mut wrong_operation = context("project-a");
        wrong_operation.operation = "everything".into();
        let listed = refusal(wrong_operation.validate().unwrap_err()).message;
        for operation in OPERATIONS {
            assert!(listed.contains(operation), "{listed}");
        }
        let mut unstamped = context("project-a");
        unstamped.admitted_at_ms = 0;
        assert_eq!(code(&unstamped.validate().unwrap_err()), CONTEXT_CORRUPT);
    }

    #[test]
    fn profile_sessions_require_explicit_project_and_keep_native_principal_scope() {
        let mut input = request();
        input.operation = PROFILE_SESSION.into();
        assert_eq!(code(&admit(&input).unwrap_err()), PROJECT_REQUIRED);
        input.requested_project = Some("profile-fixture".into());
        let admitted = admit(&input).unwrap();
        assert_eq!(admitted.context().operation, PROFILE_SESSION);
        assert_eq!(admitted.context().project, "profile-fixture");
        assert_eq!(admitted.context().principal_uid, "uid-a");
        assert_eq!(admitted.context().lane, "canary");
        let mut crossed = caller();
        crossed.lane = "stable".into();
        assert!(!visible(
            admitted.context(),
            &crossed,
            Some("profile-fixture")
        ));
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
            "no requested or sealed project is not a default"
        );
        let mut named = request();
        named.requested_project = Some("project-a".into());
        assert_eq!(admit(&named).unwrap().context().project, "project-a");
        let mut sealed = request();
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
    fn a_project_this_host_has_never_seen_is_admitted_on_the_owner_s_word() {
        // The boundary: there is no directory here to consult, no snapshot to
        // be stale and no refusal to refresh. A project the owner names for
        // the first time is admitted, and whether its effects may leave the
        // machine is the gateway's answer at publication and sync.
        for project in ["project-a", "a-project-never-seen", "p"] {
            let mut named = request();
            named.requested_project = Some(project.into());
            assert_eq!(admit(&named).unwrap().context().project, project);
            let mut sealed = request();
            sealed.sealed_project = Some(project.into());
            assert_eq!(admit(&sealed).unwrap().context().project, project);
        }
    }

    #[test]
    fn a_project_id_is_one_path_segment_wherever_it_is_named() {
        // What a host may safely put in a path: the ids the product actually
        // uses, a single character, and the bound exactly.
        for ordinary in [
            "aderm",
            "project-a",
            "p",
            "A_b.2",
            // Not over-reaching: a dot inside a name, a name that merely
            // starts like a device, and a name that merely starts with dots.
            "v1.2",
            "console",
            "nullify",
            "..a",
            // Not over-reaching the other way either: a name is refused for
            // drawing nothing, never for being written in another script.
            "projet-économie",
            "проект",
            "プロジェクト",
            &"p".repeat(MAX_PROJECT_CHARS),
        ] {
            assert!(valid_project(ordinary), "{ordinary}");
        }
        // …and what would name a different place on disk, or no place at all.
        // The last six are Windows and only Windows: `Path::join("C:")`
        // replaces the whole path, `a.` and `a` are one directory there, and
        // a device name is a device however deep it is buried.
        let rejected = [
            ".",
            "..",
            "a/b",
            "/a",
            "a/../b",
            "a\\b",
            "a b",
            " a",
            "a\u{a0}b",
            "a\tb",
            "a\nb",
            "",
            "C:",
            "a:b",
            "a*",
            "a?",
            "a.",
            // Characters that draw nothing: two ids one operator reads as one.
            "a\u{200b}b",
            "a\u{ad}b",
            "a\u{202e}b",
            "a\u{feff}",
            "a\u{fe0f}",
            "a\u{e0041}",
            "nul",
            "CON",
            "com1",
            "LPT9.json",
        ];
        for shape in rejected {
            assert!(!valid_project(shape), "{shape:?} is not one segment");
            // Named by the caller… and told the grammar, not just refused.
            let mut named = request();
            named.requested_project = Some(shape.into());
            let refused = refusal(admit(&named).unwrap_err());
            assert_eq!(refused.code, CONTEXT_CORRUPT, "{shape:?}");
            assert!(
                refused.message.contains("requested_project")
                    && refused.message.contains("one path segment"),
                "{shape:?}: {}",
                refused.message
            );
            // …sealed in the bytes…
            let mut sealed = request();
            sealed.sealed_project = Some(shape.into());
            assert_eq!(
                code(&admit(&sealed).unwrap_err()),
                CONTEXT_CORRUPT,
                "{shape:?}"
            );
            // …carried by a stored record…
            let mut stored = request();
            stored.requested_project = Some("project-a".into());
            stored.existing = Some(ExecutionContext {
                project: shape.into(),
                ..context("project-a")
            });
            assert_eq!(
                code(&admit(&stored).unwrap_err()),
                CONTEXT_CORRUPT,
                "{shape:?}"
            );
            // …offered to a legacy row by its own sealed input…
            assert_eq!(
                code(
                    &recover(
                        row("solar_prepared", None),
                        &principal(),
                        Some(shape),
                        5_000
                    )
                    .unwrap_err()
                ),
                CONTEXT_CORRUPT,
                "{shape:?}"
            );
            // …or counted in a census, where it is the host's own bug.
            assert!(
                matches!(
                    capacity(limits(), Intent::Admit, &[], &slots(&[shape]), "project-a")
                        .unwrap_err(),
                    Fault::Hard(_)
                ),
                "{shape:?}"
            );
            assert!(
                matches!(
                    capacity(limits(), Intent::Admit, &[], &[], shape).unwrap_err(),
                    Fault::Hard(_)
                ),
                "{shape:?}"
            );
        }
        // The bound is the kernel's own 200, not layer_document's 500.
        assert_eq!(MAX_PROJECT_CHARS, 200);
        assert!(!valid_project(&"p".repeat(MAX_PROJECT_CHARS + 1)));
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
    fn the_project_is_resolved_before_any_stored_key_is_consulted() {
        // A key that already holds a context never decides what the request
        // is about: a request that names no project is refused for that, and
        // the stored record is not consulted at all.
        let mut unnamed = request();
        unnamed.existing = Some(context("project-a"));
        assert_eq!(code(&admit(&unnamed).unwrap_err()), PROJECT_REQUIRED);
        // …and a project the request cannot even spell is refused before it.
        let mut unbounded = request();
        unbounded.requested_project = Some(" project-a".into());
        unbounded.existing = Some(context("project-a"));
        assert_eq!(code(&admit(&unbounded).unwrap_err()), CONTEXT_CORRUPT);
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
            broken.requested_project = Some("project-a".into());
            break_it(&mut broken);
            assert_eq!(code(&admit(&broken).unwrap_err()), CONTEXT_CORRUPT);
        }
        let mut clockless = request();
        clockless.requested_project = Some("project-a".into());
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
        // Both at their share, both waiting, room in the pool: refusing A
        // would refuse B for the same reason and idle two workers while both
        // have work. The share only yields to a project that can take the slot.
        let roomy = Limits {
            global_running: 6,
            ..limits()
        };
        let both_at_share = slots(&["project-a", "project-a", "project-b", "project-b"]);
        let both_waiting = slots(&["project-a", "project-b"]);
        for project in ["project-a", "project-b"] {
            assert!(
                capacity(roomy, Intent::Claim, &both_at_share, &both_waiting, project).is_ok(),
                "{project} is admitted when every waiting project is at its share too"
            );
        }
        // …but the moment a third project under its share waits, both yield
        // to it, and the guidance counts only the work that can go first.
        let third_waiting = slots(&["project-a", "project-b", "project-c"]);
        for project in ["project-a", "project-b"] {
            let refused = refusal(
                capacity(
                    roomy,
                    Intent::Claim,
                    &both_at_share,
                    &third_waiting,
                    project,
                )
                .unwrap_err(),
            );
            assert_eq!(refused.scope, Some("project"), "{project}");
            assert_eq!(refused.retry_after_ms, Some(2_000), "{project}");
        }
        assert!(capacity(
            roomy,
            Intent::Claim,
            &both_at_share,
            &third_waiting,
            "project-c"
        )
        .is_ok());
    }

    #[test]
    fn a_share_that_does_not_divide_its_pool_is_a_host_configuration_fault() {
        // The fairness this op applies is only real if the numbers leave room
        // for a second project. A host that configures the share as the whole
        // pool has configured the share away, and that is its own bug — not a
        // refusal for whoever happens to submit next.
        for (broken, share_name, pool_name) in [
            (
                Limits {
                    global_running: 4,
                    per_project_running: 4,
                    ..limits()
                },
                "per_project_running",
                "global_running",
            ),
            (
                Limits {
                    global_running: 4,
                    per_project_running: 5,
                    ..limits()
                },
                "per_project_running",
                "global_running",
            ),
            (
                Limits {
                    per_project_queued: 8,
                    global_queued: 8,
                    ..limits()
                },
                "per_project_queued",
                "global_queued",
            ),
            (
                Limits {
                    per_project_queued: 9,
                    global_queued: 8,
                    ..limits()
                },
                "per_project_queued",
                "global_queued",
            ),
            // A pool of one that hands out more than it has.
            (
                Limits {
                    global_running: 1,
                    per_project_running: 2,
                    ..limits()
                },
                "per_project_running",
                "global_running",
            ),
        ] {
            for intent in [Intent::Admit, Intent::Claim] {
                let fault = capacity(broken, intent, &[], &[], "project-a").unwrap_err();
                let Fault::Hard(message) = &fault else {
                    panic!("a misconfigured share is the host's bug, not a client answer: {fault}");
                };
                // Both numbers, so the operator reads the fix off the message
                // instead of going to look the configuration up.
                let (share, pool) = if share_name == "per_project_running" {
                    (broken.per_project_running, broken.global_running)
                } else {
                    (broken.per_project_queued, broken.global_queued)
                };
                assert!(
                    message.contains(&format!("{share_name} is {share}"))
                        && message.contains(&format!("{pool_name} is {pool}")),
                    "{message}"
                );
                assert!(fault.code().is_none(), "{message}");
                // The same gate, asked without a census — what a host asks
                // before it starts its workers, so a configuration nothing
                // can honour is answered once rather than once per claim.
                assert_eq!(broken.validate().unwrap_err(), fault);
            }
        }
        assert!(limits().validate().is_ok());
    }

    #[test]
    fn a_single_worker_host_has_no_share_and_is_configured_as_such() {
        // The smallest rented VM: one worker, one project at a time, the share
        // equal to the pool because there is nothing to divide. It admits and
        // it claims, and its one slot still bounds it.
        let alone = Limits {
            global_running: 1,
            per_project_running: 1,
            per_project_queued: 1,
            global_queued: 1,
        };
        assert!(capacity(alone, Intent::Admit, &[], &[], "project-a").is_ok());
        assert!(capacity(alone, Intent::Claim, &[], &[], "project-a").is_ok());
        // The pool of one is still a bound: the second claim waits, and the
        // second submission waits, whichever project asks.
        for project in ["project-a", "project-b"] {
            let running = refusal(
                capacity(alone, Intent::Claim, &slots(&["project-a"]), &[], project).unwrap_err(),
            );
            assert_eq!(running.code, CAPACITY_EXHAUSTED);
            assert_eq!(running.scope, Some("global"));
            let queued = refusal(
                capacity(alone, Intent::Admit, &[], &slots(&["project-a"]), project).unwrap_err(),
            );
            assert_eq!(queued.code, CAPACITY_EXHAUSTED);
            assert_eq!(queued.scope, Some("global"));
        }
        // A single-slot queue in front of several workers is the same
        // statement about that queue, and it is admitted too.
        let deep_pool = Limits {
            global_running: 4,
            per_project_running: 2,
            per_project_queued: 1,
            global_queued: 1,
        };
        assert!(capacity(deep_pool, Intent::Admit, &[], &[], "project-a").is_ok());
        assert!(divides(1, 1) && !divides(2, 1) && divides(1, 2) && !divides(2, 2));
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
            assert!(matches!(broken.validate().unwrap_err(), Fault::Hard(_)));
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
            Some("project-a"),
            5_000,
        )
        .unwrap();
        assert_eq!(recovered, stored);
        assert_eq!(provenance, Provenance::Stored);
        assert_eq!(provenance.wire(), "stored");
        // Nothing outside the row can restate what the row already records.
        let (again, _) = recover(
            row("solar_prepared", Some(&stored)),
            &principal(),
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
                    Some("project-b"),
                    5_000,
                )
                .unwrap_err()
            ),
            SCOPE_MISMATCH
        );
    }

    #[test]
    fn a_legacy_row_recovers_from_its_own_sealed_input_and_from_nothing_else() {
        let (solar, provenance) = recover(
            row("solar_prepared", None),
            &principal(),
            Some("project-a"),
            5_000,
        )
        .unwrap();
        assert_eq!(provenance, Provenance::SealedInput);
        assert_eq!(solar.project, "project-a");
        assert_eq!(solar.operation, SOLAR_PROCESSING);
        assert_eq!(solar.client, RECOVERED_CLIENT);
        assert_eq!(solar.idempotency_key, RECOVERED_KEY);
        assert_eq!(solar.job_id, JOB_ID);
        assert_eq!(solar.input_sha256, DIGEST_A);
        assert_eq!(
            solar.admitted_at_ms, 1_700_000_000_000,
            "recovery keeps the row's own admission moment"
        );
    }

    #[test]
    fn a_legacy_row_with_nothing_to_recover_from_is_never_guessed() {
        // A transformer row carries no project by design, and there is no
        // second source for one: not a selection, not the last project used,
        // not the only project on the host. It stays readable and unrun.
        let refused =
            refusal(recover(row("fast_lv", None), &principal(), None, 5_000).unwrap_err());
        assert_eq!(refused.code, CONTEXT_UNRECOVERABLE);
        assert!(
            refused
                .message
                .contains("transformer request names no project"),
            "{}",
            refused.message
        );
        // …and the refusal says what to do about it, because nothing else the
        // owner reads will. Not "read its result": the row never ran.
        assert!(
            refused.message.ends_with(UNRECOVERABLE_REMEDY),
            "{}",
            refused.message
        );
        assert!(
            !UNRECOVERABLE_REMEDY.contains("result"),
            "a row that never ran has no result to read"
        );
        // Naming a project alongside such a row changes nothing: the only
        // project a legacy row can be recovered into is one its own bytes name.
        assert_eq!(
            code(&recover(row("solar_prepared", None), &principal(), None, 5_000).unwrap_err()),
            CONTEXT_UNRECOVERABLE,
            "a Solar row's project comes from its bytes or from nowhere"
        );
        assert_eq!(
            code(&recover(json!({"id": JOB_ID}), &principal(), None, 5_000).unwrap_err()),
            CONTEXT_UNRECOVERABLE
        );
        // Another lane's row is never reopened under this principal.
        let mut other_lane = row("fast_lv", None);
        other_lane["lane"] = json!("stable");
        assert_eq!(
            code(&recover(other_lane, &principal(), None, 5_000).unwrap_err()),
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
                    5_000
                )
                .unwrap_err()
            ),
            CONTEXT_CORRUPT
        );
        assert!(matches!(
            recover(row("fast_lv", None), &principal(), None, 0).unwrap_err(),
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
        }))
        .unwrap();
        assert_eq!(
            refused,
            json!({"outcome": "refused", "refusal": {"code": PROJECT_REQUIRED, "message": "no project was named for this request; pass --project or submit sealed input that names its project"}})
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
            "kind": "recover", "row": row("solar_prepared", None), "principal": principal(),
            "sealed_project": "project-b", "now_ms": 5_000
        }))
        .unwrap();
        assert_eq!(recovered["outcome"], "recovered");
        assert_eq!(recovered["provenance"], "sealed_input");
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
        assert_eq!(
            refused.message(),
            "no project was named for this request; pass --project or submit sealed input that names its project"
        );
        assert_eq!(
            refused.to_string(),
            "project_required: no project was named for this request; pass --project or submit sealed input that names its project"
        );
        let mut clockless = request();
        clockless.requested_project = Some("project-a".into());
        clockless.now_ms = 0;
        let bug = admit(&clockless).unwrap_err();
        assert_eq!(bug.code(), None, "a host bug has nothing to relay");
        assert_eq!(bug.to_string(), bug.message());
    }

    #[test]
    fn the_refusal_vocabulary_is_closed() {
        assert_eq!(REFUSALS.len(), 9);
        let unique: std::collections::BTreeSet<&str> = REFUSALS.into_iter().collect();
        assert_eq!(unique.len(), REFUSALS.len(), "a code is listed twice");
        assert_eq!(OPERATIONS.len(), 8);
        assert!(OPERATIONS.contains(&SYNC_RECOVERY));
    }
}
