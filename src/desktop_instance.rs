//! Which live runtime is this operation for, and may this view's result still
//! be applied?
//!
//! A Desktop instance IS a Server instance with a webview, so there is one
//! instance registry and one selection rule for both: a running process
//! publishes a descriptor naming itself, a client enumerates the descriptors,
//! probes each, and asks this owner which one an operation belongs to. The
//! answer is a decision — the host performs it.
//!
//! Four rules the product is judged on, and where each lives here:
//!
//! | Rule | Op |
//! |---|---|
//! | May this published descriptor be used at all, and who is it? | `admit_descriptor` |
//! | Which live instance is this operation for? | `select` |
//! | What may a client show about the instances it found? | `list` |
//! | May a reply captured before a switch still be applied? | `advance_generation` / `admit_result` |
//!
//! And one grammar: a request id is `cli-<instance_id>-<n>`, so a completion
//! names the instance it came from (`mint_request_id` / `read_request_id`).
//!
//! Rules worth stating up front:
//!
//! - **Order never decides.** `select` is a function of a *set*. The corpus
//!   replays every multi-candidate case in several permutations and requires
//!   the same answer, and every list this owner emits is ordered by instance
//!   id, never by the order a host happened to enumerate in.
//! - **An explicit target never falls through.** A named instance that is not
//!   live is `desktop_target_not_live` even when exactly one other instance
//!   would otherwise have been the automatic answer; a named instance that is
//!   live but signed in elsewhere is `desktop_target_mismatch`. Neither ever
//!   routes anywhere.
//! - **A saved CLI selection never moves a live map.** A project the caller
//!   named narrows *which instance may serve the operation*; it is never an
//!   instruction to switch a window's project. When no live instance has that
//!   project open the answer is `desktop_project_not_open`, whose remedy is an
//!   explicit, instance-qualified switch — see [`crate::project_context`].
//! - **A refusal discloses only safe metadata.** Instance id, profile, lane,
//!   project id, build, start and window count. Never a pairing token, never a
//!   bridge url, never an account uid or email. The token in a descriptor is
//!   read, checked for shape and dropped; nothing this owner emits can carry it.
//! - **Identity is per process, not per profile.** A descriptor that predates
//!   instance identity gets one derived from what it does say — profile,
//!   origin and pid — so it can be enumerated and targeted within one
//!   enumeration, and so a restart (new ephemeral port, new pid) is a
//!   different instance rather than the same one impersonated.
//!
//! Pure: every list is bounded, no clock and no I/O enter it, and the same
//! request always has the same answer.
//! `tests/fixtures/desktop-instance-cases.json` is the named proof, replayed
//! by the kernel and by ds-web over the vendored WASM (`desktopInstance`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::execution_context::{MAX_PROJECT_CHARS, MAX_UID_CHARS};

pub const SCHEMA: &str = "ds.desktop-instance/v1";
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

// ── Bounds ──────────────────────────────────────────────────────────────
/// The descriptor a host reads from disk. The real file is a few hundred
/// bytes; the bound lives here so every host reads the same number.
pub const MAX_DESCRIPTOR_BYTES: u64 = 16 * 1024;
/// A minted or derived instance id: 32 lowercase hex characters.
pub const INSTANCE_ID_CHARS: usize = 32;
/// The credential audience digest, as the paired session publishes it.
pub const AUDIENCE_CHARS: usize = 64;
/// The pairing secret's shape. Its bytes are never inspected further and never
/// leave this module.
pub const MIN_TOKEN_CHARS: usize = 32;
pub const MAX_TOKEN_CHARS: usize = 512;
pub const MAX_URL_CHARS: usize = 200;
pub const MAX_LANE_CHARS: usize = 40;
pub const MAX_PROFILE_CHARS: usize = 120;
pub const MAX_BUILD_CHARS: usize = 120;
pub const MAX_WINDOW_LABEL_CHARS: usize = 120;
pub const MAX_WINDOWS: usize = 64;
pub const MAX_CANDIDATES: usize = 64;
/// Generations and request sequences cross into a browser, where every number
/// is a double. Bounding both at `Number.MAX_SAFE_INTEGER` keeps the value the
/// kernel decided and the value JavaScript compares the same integer.
pub const MAX_GENERATION: u64 = 9_007_199_254_740_991;
pub const MAX_SEQUENCE: u64 = 9_007_199_254_740_991;
/// A request id names the instance that minted it: `cli-<instance_id>-<n>`.
pub const REQUEST_ID_PREFIX: &str = "cli-";
/// How much of a caller's own malformed target is echoed back to it.
const MAX_ECHO_CHARS: usize = 64;

// ── The refusal vocabulary (closed) ─────────────────────────────────────
/// A published descriptor cannot be used: wrong version, not the numeric
/// loopback origin, or a field outside its bound.
pub const DESCRIPTOR_UNUSABLE: &str = "descriptor_unusable";
/// A live instance answered the handshake with something that is not the
/// contract, so it cannot be a candidate for anything.
pub const DESKTOP_CONTRACT_MISMATCH: &str = "desktop_contract_mismatch";
/// No live instance is compatible with this caller.
pub const DESKTOP_NOT_PAIRED: &str = "desktop_not_paired";
/// More than one is, and nothing but the caller may choose.
pub const DESKTOP_AMBIGUOUS: &str = "desktop_ambiguous";
/// The caller named an instance that is not among the live ones.
pub const DESKTOP_TARGET_NOT_LIVE: &str = "desktop_target_not_live";
/// The caller named a live instance that cannot serve this operation.
pub const DESKTOP_TARGET_MISMATCH: &str = "desktop_target_mismatch";
/// The operation is about a project no eligible instance has open.
pub const DESKTOP_PROJECT_NOT_OPEN: &str = "desktop_project_not_open";
/// The reply was captured before this view's project or account changed.
pub const CONTEXT_GENERATION_STALE: &str = "context_generation_stale";
/// A request id that is not `cli-<instance_id>-<n>`.
pub const REQUEST_ID_MALFORMED: &str = "request_id_malformed";
pub const REFUSALS: [&str; 9] = [
    DESCRIPTOR_UNUSABLE,
    DESKTOP_CONTRACT_MISMATCH,
    DESKTOP_NOT_PAIRED,
    DESKTOP_AMBIGUOUS,
    DESKTOP_TARGET_NOT_LIVE,
    DESKTOP_TARGET_MISMATCH,
    DESKTOP_PROJECT_NOT_OPEN,
    CONTEXT_GENERATION_STALE,
    REQUEST_ID_MALFORMED,
];

// ── Refusals ────────────────────────────────────────────────────────────

/// Why a decision refused, in the shape `ds` already re-raises: the code, the
/// sentence, the remedy, and a bounded detail that carries only safe metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub remedy: &'static str,
    /// Boxed: a refusal travels in every `Result` this module returns, and a
    /// `Value` inline would make the error variant of each of them large.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Box<Value>>,
}

/// A request the host built wrong (`Hard`) against a decision a caller must be
/// told (`Refused`). Hosts relay a refusal; a hard fault is a bug in the host.
#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    Hard(String),
    Refused(Refusal),
}

impl Fault {
    /// The typed code a host relays, or `None` when the fault is its own bug.
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

fn refused(code: &'static str, message: impl Into<String>, remedy: &'static str) -> Fault {
    Fault::Refused(Refusal {
        code,
        message: message.into(),
        remedy,
        detail: None,
    })
}
fn refused_with(
    code: &'static str,
    message: impl Into<String>,
    remedy: &'static str,
    detail: Value,
) -> Fault {
    Fault::Refused(Refusal {
        code,
        message: message.into(),
        remedy,
        detail: Some(Box::new(detail)),
    })
}
fn hard(message: impl Into<String>) -> Fault {
    Fault::Hard(message.into())
}
fn unusable(reason: &str) -> Fault {
    refused_with(
        DESCRIPTOR_UNUSABLE,
        format!("the published descriptor {reason}"),
        "restart DS GridDesign, or name a descriptor with --desktop-descriptor <path>",
        json!({ "reason": reason }),
    )
}
fn contract_mismatch(field: &str) -> Fault {
    refused_with(
        DESKTOP_CONTRACT_MISMATCH,
        format!("a live instance published {field} outside its contract"),
        "update DS GridDesign and retry",
        json!({ "field": field }),
    )
}

/// The same predicate [`crate::execution_context`] applies to every text field
/// it records. The bounds themselves are imported from there, so the two can
/// disagree about a value but never about a number.
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}
fn optional_bounded(value: Option<&str>, max: usize) -> bool {
    value.is_none_or(|value| bounded(value, max))
}
fn lowercase_hex(value: &str, chars: usize) -> bool {
    value.len() == chars
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether a text is a minted or derived instance id. Public because a client
/// parses `--target desktop:<instance_id>` before it has any candidate.
pub fn instance_id_valid(value: &str) -> bool {
    lowercase_hex(value, INSTANCE_ID_CHARS)
}

/// What a caller's own malformed input may be told back: bounded, control-free
/// and truncated, so a refusal cannot be made to carry a payload.
fn echo(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_ECHO_CHARS)
        .collect();
    cleaned
}

// ── The descriptor ──────────────────────────────────────────────────────

/// What a running instance publishes so a client can reach it.
///
/// Version stays **1** and every field added since is optional: an older `ds`
/// reading a newer file sees the four fields it knows, and a newer `ds`
/// reading an older file gets a derived identity rather than a refusal.
///
/// `token` is the pairing secret. It is validated for shape and then dropped —
/// there is no `Serialize` here and the `Debug` below redacts it, so no answer
/// this module can produce is able to carry it.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u8,
    pub url: String,
    pub token: String,
    pub pid: u32,
    #[serde(default)]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    #[serde(default)]
    pub profile: Option<String>,
}

impl std::fmt::Debug for Descriptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Descriptor")
            .field("version", &self.version)
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("pid", &self.pid)
            .field("instance_id", &self.instance_id)
            .field("lane", &self.lane)
            .field("build", &self.build)
            .field("started_at_ms", &self.started_at_ms)
            .field("profile", &self.profile)
            .finish()
    }
}

/// Where an admitted instance id came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    /// The instance minted it at start and publishes it.
    Minted,
    /// The descriptor predates instance identity, so it was derived from what
    /// the file does say. It names a live descriptor, not a process that will
    /// survive a restart.
    Derived,
}

impl Identity {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Minted => "minted",
            Self::Derived => "derived",
        }
    }
}

/// One admitted descriptor: who published it and where to reach it. Carries no
/// token, by construction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Admission {
    pub instance_id: String,
    #[serde(serialize_with = "identity_wire")]
    pub identity: Identity,
    /// The normalised numeric loopback origin, which is the only address a
    /// pairing token may be sent to.
    pub origin: String,
    pub profile: Option<String>,
    pub lane: Option<String>,
    pub build: Option<String>,
    pub started_at_ms: Option<u64>,
    pub pid: u32,
}

fn identity_wire<S: serde::Serializer>(
    identity: &Identity,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(identity.wire())
}

/// A descriptor with no instance id still needs one, or it can be neither
/// enumerated nor named. Derive it from what the file says: the profile it
/// belongs to, the origin it publishes and the process that wrote it. A
/// restart re-rolls the ephemeral port and the pid, so it derives a *different*
/// instance — a dead descriptor's id can never be answered by its successor.
pub fn derived_instance_id(profile: Option<&str>, origin: &str, pid: u32) -> String {
    let mut digest = Sha256::new();
    digest.update(SCHEMA.as_bytes());
    digest.update(b"\nderived\n");
    digest.update(profile.unwrap_or("").as_bytes());
    digest.update(b"\n");
    digest.update(origin.as_bytes());
    digest.update(b"\n");
    digest.update(pid.to_string().as_bytes());
    format!("{:x}", digest.finalize())[..INSTANCE_ID_CHARS].to_owned()
}

/// Admit one published descriptor, or say why it cannot be used.
///
/// `profile` is the install profile the host read the file *under* — the
/// directory it came from. A descriptor that names a different one has been
/// copied out of its own install and is refused before its token is used.
pub fn admit_descriptor(
    descriptor: &Descriptor,
    profile: Option<&str>,
) -> Result<Admission, Fault> {
    if descriptor.version != 1 {
        return Err(unusable("speaks another version"));
    }
    if !bounded(&descriptor.url, MAX_URL_CHARS) {
        return Err(unusable("names no bounded endpoint"));
    }
    if !bounded(&descriptor.token, MAX_TOKEN_CHARS)
        || descriptor.token.chars().count() < MIN_TOKEN_CHARS
        || !descriptor.token.is_ascii()
    {
        return Err(unusable("carries no usable pairing secret"));
    }
    if descriptor.pid == 0 {
        return Err(unusable("names no process"));
    }
    let origin = loopback_origin(&descriptor.url)?;
    if !optional_bounded(profile, MAX_PROFILE_CHARS)
        || !optional_bounded(descriptor.profile.as_deref(), MAX_PROFILE_CHARS)
    {
        return Err(unusable("names an unbounded profile"));
    }
    if let (Some(named), Some(read_under)) = (descriptor.profile.as_deref(), profile) {
        if named != read_under {
            return Err(unusable("belongs to another install profile"));
        }
    }
    if !optional_bounded(descriptor.lane.as_deref(), MAX_LANE_CHARS) {
        return Err(unusable("names an unbounded lane"));
    }
    if !optional_bounded(descriptor.build.as_deref(), MAX_BUILD_CHARS) {
        return Err(unusable("names an unbounded build"));
    }
    if descriptor.started_at_ms == Some(0) {
        return Err(unusable("names no start time"));
    }
    let profile = descriptor.profile.as_deref().or(profile).map(str::to_owned);
    let (instance_id, identity) = match descriptor.instance_id.as_deref() {
        Some(minted) if instance_id_valid(minted) => (minted.to_owned(), Identity::Minted),
        Some(_) => return Err(unusable("names a malformed instance id")),
        None => (
            derived_instance_id(profile.as_deref(), &origin, descriptor.pid),
            Identity::Derived,
        ),
    };
    Ok(Admission {
        instance_id,
        identity,
        origin,
        profile,
        lane: descriptor.lane.clone(),
        build: descriptor.build.clone(),
        started_at_ms: descriptor.started_at_ms,
        pid: descriptor.pid,
    })
}

/// The pairing token is sent to this endpoint, so accept only the exact
/// numeric loopback origin an instance publishes. Prefix matching can be
/// parsed as userinfo and send the secret to a remote host instead; a default
/// port means the bridge is not on its own ephemeral port; a path, query or
/// fragment means the address is not an origin at all.
fn loopback_origin(url: &str) -> Result<String, Fault> {
    let Ok(endpoint) = Url::parse(url) else {
        return Err(unusable("does not point at a valid bridge endpoint"));
    };
    if endpoint.scheme() != "http"
        || endpoint.host_str() != Some("127.0.0.1")
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || !matches!(endpoint.path(), "" | "/")
        || !endpoint.port().is_some_and(|port| port > 0)
    {
        return Err(unusable("does not point at the numeric loopback origin"));
    }
    Ok(endpoint.origin().ascii_serialization())
}

// ── Candidates ──────────────────────────────────────────────────────────

/// One independently routed view inside an instance.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub label: String,
    #[serde(default)]
    pub project: Option<String>,
    /// Advanced by this view's own project or account change, and by nothing
    /// else. Never the process-wide session revision.
    pub generation: u64,
}

/// A live instance: its admitted descriptor identity plus what its
/// authenticated handshake published.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub instance_id: String,
    #[serde(default)]
    pub profile: Option<String>,
    /// `stable`, `canary`, or a development build's own lane. Any lane but the
    /// caller's is simply not a candidate for its work.
    pub lane: String,
    pub uid: String,
    pub audience_sha256: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    pub session_revision: u64,
    #[serde(default)]
    pub windows: Vec<Window>,
}

impl Candidate {
    fn validate(&self) -> Result<(), Fault> {
        if !instance_id_valid(&self.instance_id) {
            return Err(contract_mismatch("an instance id"));
        }
        if !optional_bounded(self.profile.as_deref(), MAX_PROFILE_CHARS) {
            return Err(contract_mismatch("a profile"));
        }
        if !bounded(&self.lane, MAX_LANE_CHARS) {
            return Err(contract_mismatch("a lane"));
        }
        if !bounded(&self.uid, MAX_UID_CHARS) {
            return Err(contract_mismatch("an account"));
        }
        if !lowercase_hex(&self.audience_sha256, AUDIENCE_CHARS) {
            return Err(contract_mismatch("a credential audience"));
        }
        if !optional_bounded(self.project.as_deref(), MAX_PROJECT_CHARS) {
            return Err(contract_mismatch("a project"));
        }
        if !optional_bounded(self.build.as_deref(), MAX_BUILD_CHARS) {
            return Err(contract_mismatch("a build"));
        }
        if self.started_at_ms == Some(0) {
            return Err(contract_mismatch("a start time"));
        }
        if self.session_revision == 0 {
            return Err(contract_mismatch("a session revision"));
        }
        if self.windows.len() > MAX_WINDOWS {
            return Err(contract_mismatch("more windows than the bound"));
        }
        for (index, window) in self.windows.iter().enumerate() {
            if !bounded(&window.label, MAX_WINDOW_LABEL_CHARS)
                || !optional_bounded(window.project.as_deref(), MAX_PROJECT_CHARS)
                || window.generation == 0
                || window.generation > MAX_GENERATION
            {
                return Err(contract_mismatch("a window"));
            }
            if self.windows[..index]
                .iter()
                .any(|earlier| earlier.label == window.label)
            {
                return Err(contract_mismatch("the same window twice"));
            }
        }
        Ok(())
    }

    /// Whether this instance has that project open anywhere: as the session's
    /// own project, or in one of its windows.
    pub fn opens(&self, project: &str) -> bool {
        self.project.as_deref() == Some(project)
            || self
                .windows
                .iter()
                .any(|window| window.project.as_deref() == Some(project))
    }

    fn has_window(&self, label: &str) -> bool {
        self.windows.iter().any(|window| window.label == label)
    }

    /// Everything a client may show or a refusal may name about this instance.
    /// No token, no url, no account.
    pub fn summary(&self) -> InstanceSummary {
        InstanceSummary {
            instance_id: self.instance_id.clone(),
            profile: self.profile.clone(),
            lane: self.lane.clone(),
            project: self.project.clone(),
            build: self.build.clone(),
            started_at_ms: self.started_at_ms,
            windows: self.windows.len(),
        }
    }
}

/// The safe metadata of one live instance. Every field here is publishable to
/// the machine's own operator; the ones that are not simply do not exist on
/// this type, so no host can decide to include them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstanceSummary {
    pub instance_id: String,
    pub profile: Option<String>,
    pub lane: String,
    pub project: Option<String>,
    pub build: Option<String>,
    pub started_at_ms: Option<u64>,
    pub windows: usize,
}

/// What the operation needs of the instance that serves it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub lane: String,
    pub uid: String,
    pub audience_sha256: String,
    /// The project the operation is about, when it is about one.
    #[serde(default)]
    pub project: Option<String>,
    /// A project-independent operation runs on any compatible instance; the
    /// project it happens to have open is not a condition.
    #[serde(default)]
    pub project_independent: bool,
}

impl Requirement {
    fn validate(&self) -> Result<(), Fault> {
        if !matches!(self.lane.as_str(), "stable" | "canary") {
            return Err(hard("a requirement names the caller's own lane"));
        }
        if !bounded(&self.uid, MAX_UID_CHARS) {
            return Err(hard("a requirement names the caller's own account"));
        }
        if !lowercase_hex(&self.audience_sha256, AUDIENCE_CHARS) {
            return Err(hard("a requirement names the caller's own audience"));
        }
        if !optional_bounded(self.project.as_deref(), MAX_PROJECT_CHARS) {
            return Err(hard("a requirement names a bounded project"));
        }
        Ok(())
    }

    /// The project that narrows the choice, if any.
    fn narrowing_project(&self) -> Option<&str> {
        if self.project_independent {
            return None;
        }
        self.project.as_deref()
    }

    /// Why this instance cannot serve the operation, or `None`.
    fn mismatch(&self, candidate: &Candidate) -> Option<&'static str> {
        if candidate.lane != self.lane {
            return Some("lane");
        }
        if candidate.uid != self.uid {
            return Some("account");
        }
        if candidate.audience_sha256 != self.audience_sha256 {
            return Some("audience");
        }
        None
    }
}

/// The instance, and optionally the window, a caller named.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub instance_id: String,
    #[serde(default)]
    pub window: Option<String>,
}

/// Where the operation goes. The host holds the descriptor for this id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Route {
    pub instance_id: String,
    pub window: Option<String>,
}

fn summaries(candidates: &[&Candidate]) -> Vec<InstanceSummary> {
    let mut listed: Vec<InstanceSummary> = candidates
        .iter()
        .map(|candidate| candidate.summary())
        .collect();
    // Sorted by identity, never by the order a host enumerated in: two hosts
    // that found the same instances answer the same bytes.
    listed.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
    listed
}

fn validated(candidates: &[Candidate]) -> Result<Vec<&Candidate>, Fault> {
    if candidates.len() > MAX_CANDIDATES {
        return Err(hard("the candidate set exceeds its bound"));
    }
    for (index, candidate) in candidates.iter().enumerate() {
        candidate.validate()?;
        if candidates[..index]
            .iter()
            .any(|earlier| earlier.instance_id == candidate.instance_id)
        {
            // One instance publishes one descriptor. Enumerating it twice —
            // its own registry file and the legacy per-profile copy, say — is
            // the host's de-duplication to do before it asks.
            return Err(hard("the candidate set names one instance twice"));
        }
    }
    Ok(candidates.iter().collect())
}

/// Which live instance is this operation for?
///
/// A function of a *set*: the answer never depends on the order the candidates
/// arrive in. An explicit target is honoured or refused and never falls
/// through; without one, exactly one compatible instance routes and anything
/// else refuses by name before an effect.
pub fn select(
    candidates: &[Candidate],
    target: Option<&Target>,
    requirement: &Requirement,
) -> Result<Route, Fault> {
    requirement.validate()?;
    let candidates = validated(candidates)?;
    let compatible: Vec<&Candidate> = candidates
        .iter()
        .copied()
        .filter(|candidate| requirement.mismatch(candidate).is_none())
        .collect();

    if let Some(target) = target {
        return select_target(&candidates, &compatible, target, requirement);
    }
    if compatible.is_empty() {
        return Err(not_paired());
    }
    let Some(project) = requirement.narrowing_project() else {
        return route_one(&compatible, None);
    };
    let opening: Vec<&Candidate> = compatible
        .iter()
        .copied()
        .filter(|candidate| candidate.opens(project))
        .collect();
    if opening.is_empty() {
        return Err(project_not_open(project, &[]));
    }
    route_one(&opening, None)
}

fn select_target(
    candidates: &[&Candidate],
    compatible: &[&Candidate],
    target: &Target,
    requirement: &Requirement,
) -> Result<Route, Fault> {
    if !instance_id_valid(&target.instance_id) {
        return Err(target_mismatch(
            &target.instance_id,
            "malformed",
            "that target is not an instance id",
        ));
    }
    if !optional_bounded(target.window.as_deref(), MAX_WINDOW_LABEL_CHARS) {
        return Err(target_mismatch(
            &target.instance_id,
            "malformed",
            "that target is not a window label",
        ));
    }
    let Some(named) = candidates
        .iter()
        .copied()
        .find(|candidate| candidate.instance_id == target.instance_id)
    else {
        // Never a fall-through: one other instance being live and compatible
        // is exactly the case this refusal exists for.
        return Err(refused_with(
            DESKTOP_TARGET_NOT_LIVE,
            "no live DS GridDesign instance answers to that target",
            "list the live instances with `ds desktop list` and name one of them",
            json!({ "target": echo(&target.instance_id) }),
        ));
    };
    if let Some(reason) = requirement.mismatch(named) {
        return Err(target_mismatch(
            &target.instance_id,
            reason,
            match reason {
                "lane" => "that instance runs on another lane",
                "account" => "that instance is signed in as another account",
                _ => "that instance is bound to another credential audience",
            },
        ));
    }
    if let Some(window) = target.window.as_deref() {
        if !named.has_window(window) {
            return Err(target_mismatch(
                &target.instance_id,
                "window",
                "that instance has no such window open",
            ));
        }
    }
    if let Some(project) = requirement.narrowing_project() {
        // Every instance that does hold it, the named one included: a caller
        // who pinned the wrong window of the right instance is told so, and a
        // caller who pinned the wrong instance is told which one to name.
        let holding: Vec<&Candidate> = compatible
            .iter()
            .copied()
            .filter(|candidate| candidate.opens(project))
            .collect();
        let open_here = match target.window.as_deref() {
            Some(window) => named
                .windows
                .iter()
                .any(|entry| entry.label == window && entry.project.as_deref() == Some(project)),
            None => named.opens(project),
        };
        if !open_here {
            return Err(project_not_open(project, &holding));
        }
    }
    Ok(Route {
        instance_id: named.instance_id.clone(),
        window: target.window.clone(),
    })
}

fn route_one(eligible: &[&Candidate], window: Option<String>) -> Result<Route, Fault> {
    match eligible {
        [] => Err(not_paired()),
        [only] => Ok(Route {
            instance_id: only.instance_id.clone(),
            window,
        }),
        several => Err(refused_with(
            DESKTOP_AMBIGUOUS,
            "more than one DS GridDesign instance can serve this operation",
            "name one with --target desktop:<instance_id>",
            json!({ "candidates": summaries(several) }),
        )),
    }
}

fn not_paired() -> Fault {
    // Nothing is named: an instance signed in as another account is another
    // person's session on this machine, and its existence is not this
    // caller's to learn.
    refused(
        DESKTOP_NOT_PAIRED,
        "no DS GridDesign session on this machine is signed in for this account",
        "start DS GridDesign and sign in, then retry",
    )
}

fn project_not_open(project: &str, elsewhere: &[&Candidate]) -> Fault {
    refused_with(
        DESKTOP_PROJECT_NOT_OPEN,
        "no targeted DS GridDesign instance has that project open",
        "open it in the app, or switch one explicitly with `ds desktop project switch --target desktop:<instance_id> --project <project>`",
        json!({ "project": echo(project), "instances": summaries(elsewhere) }),
    )
}

fn target_mismatch(target: &str, reason: &'static str, message: &'static str) -> Fault {
    refused_with(
        DESKTOP_TARGET_MISMATCH,
        message,
        "name an instance that can serve this operation, from `ds desktop list`",
        json!({ "target": echo(target), "reason": reason }),
    )
}

/// What a client may show for every live instance it found, and which of them
/// can serve this caller's work. The projection is here so that no host has to
/// decide which fields of a handshake are safe to print.
pub fn list(
    candidates: &[Candidate],
    requirement: Option<&Requirement>,
) -> Result<(Vec<InstanceSummary>, Option<Vec<String>>), Fault> {
    let candidates = validated(candidates)?;
    let compatible = match requirement {
        Some(requirement) => {
            requirement.validate()?;
            let mut ids: Vec<String> = candidates
                .iter()
                .copied()
                .filter(|candidate| requirement.mismatch(candidate).is_none())
                .map(|candidate| candidate.instance_id.clone())
                .collect();
            ids.sort();
            Some(ids)
        }
        None => None,
    };
    Ok((summaries(&candidates), compatible))
}

// ── The context generation ──────────────────────────────────────────────

/// Why a view's context generation advances. Nothing else advances it — not a
/// map move, not a publish, not a second window's work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvanceReason {
    ProjectSwitch,
    AccountChange,
    SignOut,
}

impl AdvanceReason {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::ProjectSwitch => "project_switch",
            Self::AccountChange => "account_change",
            Self::SignOut => "sign_out",
        }
    }
}

/// A view's project or account changed: everything captured before this moment
/// belongs to the view that no longer exists.
pub fn advance(current: u64, _reason: AdvanceReason) -> Result<u64, Fault> {
    if current == 0 {
        return Err(hard("a view's generation starts at 1"));
    }
    if current >= MAX_GENERATION {
        return Err(hard("a view's generation exceeds its bound"));
    }
    Ok(current + 1)
}

/// May a reply captured at `captured` still be applied to a view now at
/// `current`? Only when they are the same generation: a late response, a
/// resumed subscription and a completed bridge operation from the old context
/// are all refused rather than written into the new view.
pub fn admit_result(captured: u64, current: u64) -> Result<(), Fault> {
    if captured == 0 || current == 0 {
        return Err(hard("a generation comparison needs two established views"));
    }
    if captured > MAX_GENERATION || current > MAX_GENERATION {
        return Err(hard("a generation exceeds its bound"));
    }
    if captured > current {
        return Err(hard("a result cannot be captured after the view it is for"));
    }
    if captured < current {
        return Err(refused_with(
            CONTEXT_GENERATION_STALE,
            "this view's project or account changed while that was in flight",
            "retry the operation against the view as it is now",
            json!({ "captured": captured, "current": current }),
        ));
    }
    Ok(())
}

// ── The request id ──────────────────────────────────────────────────────

/// `cli-<instance_id>-<n>`: a completion names the instance that dispatched it,
/// so a second instance's reply can never be admitted as this one's.
pub fn mint_request_id(instance_id: &str, sequence: u64) -> Result<String, Fault> {
    if !instance_id_valid(instance_id) {
        return Err(hard("a request id names a minted or derived instance id"));
    }
    if sequence == 0 || sequence > MAX_SEQUENCE {
        return Err(hard("a request sequence starts at 1 and is bounded"));
    }
    Ok(format!("{REQUEST_ID_PREFIX}{instance_id}-{sequence}"))
}

/// Read a request id back. A text that is not the grammar is refused; nothing
/// is guessed from it.
pub fn read_request_id(text: &str) -> Result<(String, u64), Fault> {
    let malformed = || {
        refused_with(
            REQUEST_ID_MALFORMED,
            "that is not a bridge request id",
            "a request id is cli-<instance_id>-<n>, minted by the instance that dispatched it",
            json!({ "request_id": echo(text) }),
        )
    };
    let Some(rest) = text.strip_prefix(REQUEST_ID_PREFIX) else {
        return Err(malformed());
    };
    let Some((instance_id, sequence)) = rest.split_once('-') else {
        return Err(malformed());
    };
    if !instance_id_valid(instance_id) {
        return Err(malformed());
    }
    if sequence.is_empty() || (sequence.len() > 1 && sequence.starts_with('0')) {
        return Err(malformed());
    }
    let Ok(sequence) = sequence.parse::<u64>() else {
        return Err(malformed());
    };
    if sequence == 0 || sequence > MAX_SEQUENCE {
        return Err(malformed());
    }
    Ok((instance_id.to_owned(), sequence))
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
    AdmitDescriptor(AdmitDescriptorRequest),
    /// Boxed: a selection carries the whole candidate set, so unboxed it would
    /// set the size of the enum.
    Select(Box<SelectRequest>),
    List(Box<ListRequest>),
    AdvanceGeneration(AdvanceRequest),
    AdmitResult(AdmitResultRequest),
    MintRequestId(MintRequestIdRequest),
    ReadRequestId(ReadRequestIdRequest),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmitDescriptorRequest {
    descriptor: Descriptor,
    /// The install profile the host read the descriptor under.
    #[serde(default)]
    profile: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequest {
    candidates: Vec<Candidate>,
    #[serde(default)]
    target: Option<Target>,
    requirement: Requirement,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListRequest {
    candidates: Vec<Candidate>,
    #[serde(default)]
    requirement: Option<Requirement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvanceRequest {
    current: u64,
    reason: AdvanceReason,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmitResultRequest {
    captured: u64,
    current: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MintRequestIdRequest {
    instance_id: String,
    sequence: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequestIdRequest {
    request_id: String,
}

fn answer(op: Op) -> Result<Value, Fault> {
    Ok(match op {
        Op::AdmitDescriptor(request) => {
            let admission = admit_descriptor(&request.descriptor, request.profile.as_deref())?;
            json!({"outcome": "admitted", "instance": admission})
        }
        Op::Select(request) => {
            let route = select(
                &request.candidates,
                request.target.as_ref(),
                &request.requirement,
            )?;
            json!({"outcome": "routed", "route": route})
        }
        Op::List(request) => {
            let (instances, compatible) = list(&request.candidates, request.requirement.as_ref())?;
            json!({"outcome": "listed", "instances": instances, "compatible": compatible})
        }
        Op::AdvanceGeneration(request) => {
            let generation = advance(request.current, request.reason)?;
            json!({"outcome": "advanced", "reason": request.reason.wire(), "generation": generation})
        }
        Op::AdmitResult(request) => {
            admit_result(request.captured, request.current)?;
            json!({"outcome": "admitted", "generation": request.current})
        }
        Op::MintRequestId(request) => {
            let request_id = mint_request_id(&request.instance_id, request.sequence)?;
            json!({"outcome": "minted", "request_id": request_id})
        }
        Op::ReadRequestId(request) => {
            let (instance_id, sequence) = read_request_id(&request.request_id)?;
            json!({"outcome": "read", "instance_id": instance_id, "sequence": sequence})
        }
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("desktop instance request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid desktop instance request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported desktop instance schema; expected {SCHEMA}"
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
    use serde_json::{from_value, to_vec};

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const AUDIENCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const OTHER_AUDIENCE: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const ONE: &str = "11111111111111111111111111111111";
    const TWO: &str = "22222222222222222222222222222222";
    const THREE: &str = "33333333333333333333333333333333";

    fn descriptor(extra: Value) -> Descriptor {
        let mut base = json!({
            "version": 1,
            "url": "http://127.0.0.1:41234",
            "token": TOKEN,
            "pid": 4711,
        });
        let object = base.as_object_mut().expect("an object");
        for (key, value) in extra.as_object().expect("an object") {
            object.insert(key.clone(), value.clone());
        }
        from_value(base).expect("a descriptor")
    }
    fn candidate(instance_id: &str, project: Option<&str>) -> Candidate {
        Candidate {
            instance_id: instance_id.into(),
            profile: Some("canary".into()),
            lane: "canary".into(),
            uid: "uid-a".into(),
            audience_sha256: AUDIENCE.into(),
            project: project.map(str::to_owned),
            build: Some("2026.9.12+1".into()),
            started_at_ms: Some(1_757_000_000_000),
            session_revision: 7,
            windows: Vec::new(),
        }
    }
    fn requirement(project: Option<&str>) -> Requirement {
        Requirement {
            lane: "canary".into(),
            uid: "uid-a".into(),
            audience_sha256: AUDIENCE.into(),
            project: project.map(str::to_owned),
            project_independent: false,
        }
    }
    fn target(instance_id: &str) -> Target {
        Target {
            instance_id: instance_id.into(),
            window: None,
        }
    }
    fn refusal(fault: Fault) -> Refusal {
        match fault {
            Fault::Refused(refusal) => refusal,
            Fault::Hard(message) => panic!("expected a refusal, got a hard fault: {message}"),
        }
    }
    fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut all = Vec::new();
        for index in 0..items.len() {
            let mut rest = items.to_vec();
            let head = rest.remove(index);
            for mut tail in permutations(&rest) {
                tail.insert(0, head.clone());
                all.push(tail);
            }
        }
        all
    }
    fn wire(op: Value) -> Result<Value, String> {
        evaluate(&to_vec(&json!({"schema": SCHEMA, "op": op})).expect("encodes"))
            .map(|answer| serde_json::from_str(&answer).expect("the answer is JSON"))
    }

    // ── the descriptor ──────────────────────────────────────────────────

    #[test]
    fn version_one_admits_with_or_without_the_fields_added_since() {
        let legacy = admit_descriptor(&descriptor(json!({})), Some("canary")).expect("admitted");
        assert_eq!(legacy.identity, Identity::Derived);
        assert!(instance_id_valid(&legacy.instance_id));
        assert_eq!(legacy.profile.as_deref(), Some("canary"));
        assert_eq!(legacy.origin, "http://127.0.0.1:41234");

        let minted = admit_descriptor(
            &descriptor(json!({
                "instance_id": ONE,
                "lane": "canary",
                "build": "2026.9.12+1",
                "started_at_ms": 1_757_000_000_000u64,
                "profile": "canary",
            })),
            Some("canary"),
        )
        .expect("admitted");
        assert_eq!(minted.identity, Identity::Minted);
        assert_eq!(minted.instance_id, ONE);
        assert_eq!(
            refusal(admit_descriptor(&descriptor(json!({"version": 2})), None).unwrap_err()).code,
            DESCRIPTOR_UNUSABLE
        );
    }

    #[test]
    fn a_derived_identity_names_a_live_descriptor_and_not_a_surviving_process() {
        let first = admit_descriptor(&descriptor(json!({})), Some("canary")).expect("admitted");
        // A restart re-rolls the ephemeral port and the pid. Neither the old
        // id nor the old descriptor can be answered by the new process.
        let restarted = admit_descriptor(
            &descriptor(json!({"url": "http://127.0.0.1:41235", "pid": 4712})),
            Some("canary"),
        )
        .expect("admitted");
        assert_ne!(first.instance_id, restarted.instance_id);
        // And the same file read twice is the same instance.
        let again = admit_descriptor(&descriptor(json!({})), Some("canary")).expect("admitted");
        assert_eq!(first.instance_id, again.instance_id);
        // A second profile publishing the same origin and pid is not it.
        let elsewhere = admit_descriptor(&descriptor(json!({})), Some("stable")).expect("admitted");
        assert_ne!(first.instance_id, elsewhere.instance_id);
    }

    #[test]
    fn only_the_numeric_loopback_origin_is_ever_sent_a_pairing_token() {
        for url in [
            "https://127.0.0.1:41234",
            "http://localhost:41234",
            "http://127.0.0.2:41234",
            "http://[::1]:41234",
            "http://127.0.0.1",
            "http://127.0.0.1:80",
            "http://127.0.0.1:0",
            "http://127.0.0.1@evil.example/",
            "http://user:pass@127.0.0.1:41234",
            "http://127.0.0.1:41234/bridge",
            "http://127.0.0.1:41234/?token=1",
            "http://127.0.0.1:41234/#f",
            "not a url",
        ] {
            let fault = admit_descriptor(&descriptor(json!({ "url": url })), None)
                .expect_err(&format!("{url} must be refused"));
            assert_eq!(refusal(fault).code, DESCRIPTOR_UNUSABLE, "{url}");
        }
        assert_eq!(
            admit_descriptor(&descriptor(json!({"url": "http://127.0.0.1:41234/"})), None)
                .expect("admitted")
                .origin,
            "http://127.0.0.1:41234"
        );
    }

    #[test]
    fn a_pairing_token_cannot_reach_an_answer() {
        // No `Serialize`, and a `Debug` that redacts: the two ways a secret
        // reaches a log or a result are both closed at the type.
        let rendered = format!("{:?}", descriptor(json!({})));
        assert!(!rendered.contains(TOKEN), "{rendered}");
        assert!(rendered.contains("<redacted>"));
        let admitted = serde_json::to_string(
            &admit_descriptor(&descriptor(json!({})), Some("canary")).expect("admitted"),
        )
        .expect("encodes");
        assert!(!admitted.contains(TOKEN));
        for token in ["", "short", &"t".repeat(MAX_TOKEN_CHARS + 1)] {
            assert_eq!(
                refusal(
                    admit_descriptor(&descriptor(json!({ "token": token })), None).unwrap_err()
                )
                .code,
                DESCRIPTOR_UNUSABLE
            );
        }
    }

    #[test]
    fn a_descriptor_copied_out_of_its_install_is_refused_before_its_token_is_used() {
        let fault = admit_descriptor(
            &descriptor(json!({"profile": "stable", "instance_id": ONE})),
            Some("canary"),
        )
        .unwrap_err();
        let refusal = refusal(fault);
        assert_eq!(refusal.code, DESCRIPTOR_UNUSABLE);
        assert!(refusal.message.contains("another install profile"));
    }

    // ── selection ───────────────────────────────────────────────────────

    #[test]
    fn order_never_decides_what_a_set_of_candidates_answers() {
        let set = [
            candidate(ONE, Some("project-a")),
            candidate(TWO, Some("project-b")),
            candidate(THREE, None),
        ];
        for requirement in [
            requirement(None),
            requirement(Some("project-a")),
            requirement(Some("project-z")),
            Requirement {
                project_independent: true,
                ..requirement(Some("project-a"))
            },
        ] {
            for named in [None, Some(target(TWO)), Some(target(ONE))] {
                let expected = select(&set, named.as_ref(), &requirement);
                for permutation in permutations(&set) {
                    assert_eq!(
                        select(&permutation, named.as_ref(), &requirement),
                        expected,
                        "a permutation changed the answer"
                    );
                }
            }
        }
    }

    #[test]
    fn exactly_one_compatible_instance_routes_and_anything_else_refuses() {
        assert_eq!(
            select(
                &[candidate(ONE, Some("project-a"))],
                None,
                &requirement(None)
            ),
            Ok(Route {
                instance_id: ONE.into(),
                window: None
            })
        );
        assert_eq!(
            refusal(select(&[], None, &requirement(None)).unwrap_err()).code,
            DESKTOP_NOT_PAIRED
        );
        let ambiguous = refusal(
            select(
                &[candidate(ONE, None), candidate(TWO, None)],
                None,
                &requirement(None),
            )
            .unwrap_err(),
        );
        assert_eq!(ambiguous.code, DESKTOP_AMBIGUOUS);
        // Listed by identity, so two hosts that found the same pair answer the
        // same bytes.
        assert_eq!(
            ambiguous.detail.expect("candidates")["candidates"]
                .as_array()
                .expect("an array")
                .iter()
                .map(|entry| entry["instance_id"].as_str().unwrap_or_default().to_owned())
                .collect::<Vec<_>>(),
            vec![ONE.to_owned(), TWO.to_owned()]
        );
    }

    #[test]
    fn another_account_on_this_machine_is_not_a_candidate_and_is_not_disclosed() {
        let theirs = Candidate {
            uid: "uid-b".into(),
            ..candidate(TWO, Some("project-a"))
        };
        let fault = refusal(select(&[theirs.clone()], None, &requirement(None)).unwrap_err());
        assert_eq!(fault.code, DESKTOP_NOT_PAIRED);
        assert!(fault.detail.is_none(), "an unrelated session is not named");
        // With one of ours live, theirs never makes the choice ambiguous.
        assert_eq!(
            select(&[theirs, candidate(ONE, None)], None, &requirement(None)),
            Ok(Route {
                instance_id: ONE.into(),
                window: None
            })
        );
    }

    #[test]
    fn an_explicit_target_never_falls_through_to_the_only_other_instance() {
        let live = [candidate(ONE, Some("project-a"))];
        let fault = refusal(select(&live, Some(&target(TWO)), &requirement(None)).unwrap_err());
        assert_eq!(fault.code, DESKTOP_TARGET_NOT_LIVE);
        assert_eq!(fault.detail.expect("a target")["target"], json!(TWO));

        for (broken, reason) in [
            (
                Candidate {
                    lane: "stable".into(),
                    ..candidate(TWO, None)
                },
                "lane",
            ),
            (
                Candidate {
                    uid: "uid-b".into(),
                    ..candidate(TWO, None)
                },
                "account",
            ),
            (
                Candidate {
                    audience_sha256: OTHER_AUDIENCE.into(),
                    ..candidate(TWO, None)
                },
                "audience",
            ),
        ] {
            let fault = refusal(
                select(
                    &[candidate(ONE, None), broken],
                    Some(&target(TWO)),
                    &requirement(None),
                )
                .unwrap_err(),
            );
            assert_eq!(fault.code, DESKTOP_TARGET_MISMATCH);
            assert_eq!(fault.detail.expect("a reason")["reason"], json!(reason));
        }
        // A target that is not an instance id refuses before anything is read.
        assert_eq!(
            refusal(
                select(
                    &live,
                    Some(&Target {
                        instance_id: "the second one".into(),
                        window: None
                    }),
                    &requirement(None)
                )
                .unwrap_err()
            )
            .code,
            DESKTOP_TARGET_MISMATCH
        );
    }

    #[test]
    fn a_project_narrows_which_instance_may_serve_and_never_moves_a_map() {
        let live = [
            candidate(ONE, Some("project-a")),
            candidate(TWO, Some("project-b")),
        ];
        assert_eq!(
            select(&live, None, &requirement(Some("project-b"))),
            Ok(Route {
                instance_id: TWO.into(),
                window: None
            })
        );
        // Nobody has it: a refusal with the explicit remedy, never a switch.
        let fault = refusal(select(&live, None, &requirement(Some("project-z"))).unwrap_err());
        assert_eq!(fault.code, DESKTOP_PROJECT_NOT_OPEN);
        assert!(fault.remedy.contains("ds desktop project switch --target"));
        assert_eq!(fault.detail.expect("a detail")["instances"], json!([]));
        // A named instance without it is told which instance does have it.
        let fault = refusal(
            select(&live, Some(&target(ONE)), &requirement(Some("project-b"))).unwrap_err(),
        );
        assert_eq!(fault.code, DESKTOP_PROJECT_NOT_OPEN);
        assert_eq!(
            fault.detail.expect("a detail")["instances"][0]["instance_id"],
            json!(TWO)
        );
        // A project-independent operation runs wherever the identity matches.
        assert_eq!(
            select(
                &[candidate(ONE, Some("project-a"))],
                None,
                &Requirement {
                    project_independent: true,
                    ..requirement(Some("project-z"))
                }
            ),
            Ok(Route {
                instance_id: ONE.into(),
                window: None
            })
        );
    }

    #[test]
    fn a_window_is_an_identity_inside_an_instance() {
        let mut instance = candidate(ONE, Some("project-a"));
        instance.windows = vec![
            Window {
                label: "main".into(),
                project: Some("project-a".into()),
                generation: 3,
            },
            Window {
                label: "workspace-2".into(),
                project: Some("project-b".into()),
                generation: 1,
            },
        ];
        let live = [instance];
        // A project open only in a second window still makes this instance the
        // one that can serve the work.
        assert_eq!(
            select(&live, None, &requirement(Some("project-b"))),
            Ok(Route {
                instance_id: ONE.into(),
                window: None
            })
        );
        assert_eq!(
            select(
                &live,
                Some(&Target {
                    instance_id: ONE.into(),
                    window: Some("workspace-2".into())
                }),
                &requirement(Some("project-b"))
            ),
            Ok(Route {
                instance_id: ONE.into(),
                window: Some("workspace-2".into())
            })
        );
        // A named window that does not hold the project is refused, not
        // silently served by its sibling.
        assert_eq!(
            refusal(
                select(
                    &live,
                    Some(&Target {
                        instance_id: ONE.into(),
                        window: Some("main".into())
                    }),
                    &requirement(Some("project-b"))
                )
                .unwrap_err()
            )
            .code,
            DESKTOP_PROJECT_NOT_OPEN
        );
        assert_eq!(
            refusal(
                select(
                    &live,
                    Some(&Target {
                        instance_id: ONE.into(),
                        window: Some("workspace-9".into())
                    }),
                    &requirement(None)
                )
                .unwrap_err()
            )
            .detail
            .expect("a reason")["reason"],
            json!("window")
        );
    }

    #[test]
    fn no_answer_names_a_token_an_address_or_an_account() {
        let live = [
            candidate(ONE, Some("project-a")),
            candidate(TWO, Some("project-a")),
        ];
        for fault in [
            select(&live, None, &requirement(None)).unwrap_err(),
            select(&live, None, &requirement(Some("project-z"))).unwrap_err(),
            select(&live, Some(&target(THREE)), &requirement(None)).unwrap_err(),
        ] {
            let rendered = serde_json::to_string(&refusal(fault)).expect("encodes");
            for secret in [TOKEN, AUDIENCE, "uid-a", "http://127.0.0.1", "@"] {
                assert!(!rendered.contains(secret), "{rendered} named {secret}");
            }
        }
        let (listed, compatible) = list(&live, Some(&requirement(None))).expect("listed");
        let rendered = serde_json::to_string(&listed).expect("encodes");
        for secret in [TOKEN, AUDIENCE, "uid-a", "http://127.0.0.1"] {
            assert!(!rendered.contains(secret), "{rendered} named {secret}");
        }
        assert_eq!(compatible, Some(vec![ONE.to_owned(), TWO.to_owned()]));
    }

    #[test]
    fn one_instance_enumerated_twice_is_the_hosts_bug_to_fix() {
        let doubled = [candidate(ONE, None), candidate(ONE, Some("project-a"))];
        assert!(matches!(
            select(&doubled, None, &requirement(None)),
            Err(Fault::Hard(_))
        ));
        assert!(matches!(list(&doubled, None), Err(Fault::Hard(_))));
        let flood: Vec<Candidate> = (0..=MAX_CANDIDATES)
            .map(|index| candidate(&format!("{index:032x}"), None))
            .collect();
        assert!(matches!(
            select(&flood, None, &requirement(None)),
            Err(Fault::Hard(_))
        ));
    }

    #[test]
    fn an_instance_that_does_not_speak_the_contract_is_never_a_candidate() {
        for broken in [
            Candidate {
                instance_id: "nope".into(),
                ..candidate(ONE, None)
            },
            Candidate {
                audience_sha256: "short".into(),
                ..candidate(ONE, None)
            },
            Candidate {
                session_revision: 0,
                ..candidate(ONE, None)
            },
            Candidate {
                uid: String::new(),
                ..candidate(ONE, None)
            },
            Candidate {
                windows: vec![
                    Window {
                        label: "main".into(),
                        project: None,
                        generation: 1,
                    },
                    Window {
                        label: "main".into(),
                        project: None,
                        generation: 1,
                    },
                ],
                ..candidate(ONE, None)
            },
        ] {
            assert_eq!(
                refusal(select(&[broken], None, &requirement(None)).unwrap_err()).code,
                DESKTOP_CONTRACT_MISMATCH
            );
        }
    }

    // ── the generation ──────────────────────────────────────────────────

    #[test]
    fn a_view_admits_only_the_results_of_the_context_it_is_in_now() {
        for reason in [
            AdvanceReason::ProjectSwitch,
            AdvanceReason::AccountChange,
            AdvanceReason::SignOut,
        ] {
            assert_eq!(advance(4, reason), Ok(5));
        }
        assert_eq!(admit_result(5, 5), Ok(()));
        let stale = refusal(admit_result(4, 5).unwrap_err());
        assert_eq!(stale.code, CONTEXT_GENERATION_STALE);
        assert_eq!(
            *stale.detail.expect("a detail"),
            json!({"captured": 4, "current": 5})
        );
        // A generation from the future, or none at all, is the host's bug.
        assert!(matches!(admit_result(6, 5), Err(Fault::Hard(_))));
        assert!(matches!(admit_result(0, 5), Err(Fault::Hard(_))));
        assert!(matches!(
            advance(0, AdvanceReason::SignOut),
            Err(Fault::Hard(_))
        ));
        assert!(matches!(
            advance(MAX_GENERATION, AdvanceReason::SignOut),
            Err(Fault::Hard(_))
        ));
    }

    // ── the request id ──────────────────────────────────────────────────

    #[test]
    fn a_request_id_names_the_instance_that_minted_it() {
        let minted = mint_request_id(ONE, 7).expect("minted");
        assert_eq!(minted, format!("cli-{ONE}-7"));
        assert_eq!(read_request_id(&minted), Ok((ONE.to_owned(), 7)));
        assert!(matches!(mint_request_id("nope", 1), Err(Fault::Hard(_))));
        assert!(matches!(mint_request_id(ONE, 0), Err(Fault::Hard(_))));
        for malformed in [
            "",
            "cli-",
            format!("cli-{ONE}").as_str(),
            format!("cli-{ONE}-0").as_str(),
            format!("cli-{ONE}-07").as_str(),
            format!("cli-{ONE}-x").as_str(),
            format!("desktop-{ONE}-1").as_str(),
            "cli-4711-1",
            "cli-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA-1",
        ] {
            assert_eq!(
                refusal(read_request_id(malformed).unwrap_err()).code,
                REQUEST_ID_MALFORMED,
                "{malformed}"
            );
        }
    }

    // ── the wire ────────────────────────────────────────────────────────

    #[test]
    fn the_wire_is_closed_and_answers_in_one_shape() {
        assert_eq!(
            wire(json!({"kind": "mint_request_id", "instance_id": ONE, "sequence": 2})),
            Ok(json!({"outcome": "minted", "request_id": format!("cli-{ONE}-2")}))
        );
        assert_eq!(
            wire(json!({"kind": "advance_generation", "current": 1, "reason": "project_switch"})),
            Ok(json!({"outcome": "advanced", "reason": "project_switch", "generation": 2}))
        );
        assert_eq!(
            wire(json!({"kind": "admit_result", "captured": 1, "current": 2})).expect("an answer")
                ["refusal"]["code"],
            json!(CONTEXT_GENERATION_STALE)
        );
        assert!(wire(json!({"kind": "everything"})).is_err());
        assert!(wire(json!({"kind": "admit_result", "captured": 1})).is_err());
        assert!(
            wire(json!({"kind": "admit_result", "captured": 1, "current": 1, "extra": true}))
                .is_err()
        );
        assert!(evaluate(
            &to_vec(&json!({"schema": "ds.desktop-instance/v2", "op": {"kind": "list", "candidates": []}}))
                .expect("encodes")
        )
        .is_err());
        assert_eq!(
            wire(json!({"kind": "list", "candidates": []})),
            Ok(json!({"outcome": "listed", "instances": [], "compatible": null}))
        );
    }

    #[test]
    fn every_refusal_code_the_module_names_can_actually_be_raised() {
        let raised: std::collections::BTreeSet<&str> = [
            refusal(admit_descriptor(&descriptor(json!({"version": 3})), None).unwrap_err()).code,
            refusal(
                select(
                    &[Candidate {
                        instance_id: "nope".into(),
                        ..candidate(ONE, None)
                    }],
                    None,
                    &requirement(None),
                )
                .unwrap_err(),
            )
            .code,
            refusal(select(&[], None, &requirement(None)).unwrap_err()).code,
            refusal(
                select(
                    &[candidate(ONE, None), candidate(TWO, None)],
                    None,
                    &requirement(None),
                )
                .unwrap_err(),
            )
            .code,
            refusal(select(&[], Some(&target(ONE)), &requirement(None)).unwrap_err()).code,
            refusal(
                select(
                    &[Candidate {
                        lane: "stable".into(),
                        ..candidate(ONE, None)
                    }],
                    Some(&target(ONE)),
                    &requirement(None),
                )
                .unwrap_err(),
            )
            .code,
            refusal(
                select(
                    &[candidate(ONE, None)],
                    None,
                    &requirement(Some("project-z")),
                )
                .unwrap_err(),
            )
            .code,
            refusal(admit_result(1, 2).unwrap_err()).code,
            refusal(read_request_id("nope").unwrap_err()).code,
        ]
        .into_iter()
        .collect();
        assert_eq!(raised, REFUSALS.into_iter().collect());
    }
}
