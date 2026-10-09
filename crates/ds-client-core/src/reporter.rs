//! `ds` reliability reporting: one SRE event for each failed, crashed or
//! refused invocation, and one daily success rollup per host.
//!
//! Contract: `ds-command-kernel/docs/contracts/ds-cli-reliability-and-feedback.md`
//! §3. The store side is `ds-sre` `REQUEST_EVENT_STORE_CONTRACT.md` §2.3, which
//! ds-brain admits on `POST /api/v1/sre/requests`
//! (`domains.sre.client_request_event`) with `source = "cli_command"` or
//! `"cli_rollup"` and stamps `service = "ds"` itself. Everything this module
//! sends is a field of that body; nothing else is.
//!
//! ## What this module decides, and what a host supplies
//!
//! The decisions are here, so every host — the CLI process, the MCP adapter,
//! a Desktop or a Server — reports the same way: how an ending is classified
//! ([`classify`]), what an event holds ([`command_event`]), what never leaves
//! the machine ([`sanitize_message`], [`flag_names`]), the bounded outbox,
//! dedupe, the per-minute cap and the daily rollup ([`record`], [`flush`]).
//!
//! Like the rest of this crate it does no IO of its own. A host supplies the
//! persistence ([`ReporterStore`]: one owner-only state document on disk,
//! never `/tmp` or `/dev/shm`) and the send ([`EventSink`]: the fixed gateway
//! call). The reporter never writes to stdout or stderr and returns nothing a
//! host may print, which is what keeps a command's output, envelope and exit
//! code byte-identical with reporting on or off, online or offline.
//!
//! ## Rules that are easy to break later
//!
//! - **One event per invocation, at most.** `ok` is never sent per call; it is
//!   counted into the daily `cli_rollup`.
//! - **A refusal is recorded, never counted against availability.** It is sent
//!   with `outcome = "success"`: the contract worked.
//! - **No recursion.** A failed send only keeps the event in the outbox; the
//!   reporter never reports its own failure.
//! - **Off means off.** With `DS_SRE_REPORTING=off` (tests and CI) a host calls
//!   nothing here, and [`finish_invocation`] touches neither store nor sink.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The environment switch. `off` disables reporting; any other value, or no
/// value, leaves it on (contract §3.4; OPEN-X §25 gates changing that default).
pub const REPORTING_SWITCH: &str = "DS_SRE_REPORTING";
/// The per-invocation event source.
pub const SOURCE_COMMAND: &str = "cli_command";
/// The daily success-count source.
pub const SOURCE_ROLLUP: &str = "cli_rollup";
/// The outbox keeps at most this many events; the oldest is dropped first.
pub const OUTBOX_MAX_EVENTS: usize = 200;
/// The outbox keeps at most this many serialized event bytes.
pub const OUTBOX_MAX_BYTES: usize = 256 * 1024;
/// Identical `(action, error_code)` endings within this window are one event.
pub const DEDUPE_WINDOW_MS: u64 = 30_000;
/// The most events one host sends in any minute.
pub const MAX_EVENTS_PER_MINUTE: usize = 30;
/// The send budget at process exit.
pub const SEND_BUDGET: Duration = Duration::from_millis(1_500);
/// The longest sanitized error message.
pub const MAX_ERROR_MESSAGE_CHARS: usize = 2_048;
/// The most flag names one event carries.
pub const MAX_FLAGS: usize = 64;
/// The most commands one rollup names (the store admits 500).
pub const MAX_ROLLUP_COMMANDS: usize = 500;
/// The longest identifier-like field (release, request id, invocation id).
pub const MAX_ID_CHARS: usize = 128;
/// The longest agent name.
pub const MAX_AGENT_CHARS: usize = 64;

const STATE_SCHEMA: &str = "ds.sre-reporter/v1";
const MINUTE_MS: u64 = 60_000;
/// Values shorter than this are not scrubbed by exact match: a one- or
/// two-character value would erase ordinary words. The token rules below
/// still drop it when it is path-, number- or value-shaped.
const MIN_SCRUBBED_VALUE_CHARS: usize = 3;

/// Whether reporting is on for this value of [`REPORTING_SWITCH`].
pub fn reporting_enabled(switch: Option<&str>) -> bool {
    !switch.is_some_and(|value| value.trim().eq_ignore_ascii_case("off"))
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Which surface ran the invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Cli,
    Mcp,
}

impl Platform {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Mcp => "mcp",
        }
    }
}

/// Which kind of host the invocation ran on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKind {
    Desktop,
    Server,
    Headless,
}

impl HostKind {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Server => "server",
            Self::Headless => "headless",
        }
    }
}

/// The terminal result class of one invocation (contract §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultClass {
    Ok,
    /// Transport error, 5xx, timeout, an unavailable owner, an internal error.
    Failed,
    /// Panic, abort, unexpected exit.
    Crashed,
    /// A declared refusal: invalid input, missing confirmation, not permitted,
    /// signed out, a stale view.
    Refused,
}

impl ResultClass {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Crashed => "crashed",
            Self::Refused => "refused",
        }
    }

    /// The store `outcome`. `ok` has none: it is never sent per call.
    pub const fn outcome(self) -> Option<&'static str> {
        match self {
            Self::Ok => None,
            Self::Failed | Self::Crashed => Some("failure"),
            Self::Refused => Some("success"),
        }
    }

    /// Whether this ending moves the availability SLI.
    pub const fn counts_against_availability(self) -> bool {
        matches!(self, Self::Failed | Self::Crashed)
    }
}

/// How one invocation ended, in the terms of the `ds` envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Termination {
    Ok,
    /// An error envelope: its class token, code, message and retryability.
    Error {
        class: String,
        code: String,
        message: String,
        retryable: bool,
    },
    /// A panic or abort; the message is whatever the hook captured.
    Crashed {
        message: String,
    },
}

/// Classify one ending. A gateway 5xx is a failure whatever class the host
/// mapped it to; otherwise the envelope's class decides:
///
/// | class | result |
/// | --- | --- |
/// | `internal`, `failed`, `unavailable` | `failed` |
/// | `invalid_input`, `unauthorized`, `conflict` | `refused` |
/// | anything else | `failed` |
///
/// `unavailable` is the class transport errors, timeouts and missing owners
/// arrive in; `conflict` carries signed-out and stale-view refusals.
pub fn classify(termination: &Termination, status: Option<u16>) -> ResultClass {
    match termination {
        Termination::Ok => ResultClass::Ok,
        Termination::Crashed { .. } => ResultClass::Crashed,
        Termination::Error { .. } if status.is_some_and(|status| status >= 500) => {
            ResultClass::Failed
        }
        Termination::Error { class, .. } => match class.as_str() {
            "invalid_input" | "unauthorized" | "conflict" => ResultClass::Refused,
            _ => ResultClass::Failed,
        },
    }
}

// ---------------------------------------------------------------------------
// The event
// ---------------------------------------------------------------------------

/// Everything a host knows about one finished invocation.
///
/// `argument_values` is the one field that never leaves the machine: it is the
/// list of values the caller supplied, used only to scrub them from the error
/// message before the event is built.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub platform: Platform,
    pub host_kind: HostKind,
    /// e.g. `ds 0.9.12 (kernel 2997236)`.
    pub release: String,
    /// `stable`, `canary` or `local` (development builds).
    pub lane: String,
    /// The dotted command id.
    pub action: String,
    /// `report`, `async`, `orchestration` or `infra` when the host knows it.
    pub action_class: Option<String>,
    /// The command's own contract version.
    pub contract: Option<u32>,
    pub latency_ms: u64,
    /// The gateway status when one exists.
    pub status: Option<u16>,
    /// The `X-Request-Id` this invocation sent; joins the ds-brain rows.
    pub request_id: Option<String>,
    /// This invocation's id (see [`new_invocation_id`]).
    pub invocation_id: String,
    /// The project, only when `--project` was passed.
    pub project: Option<String>,
    /// Flag names present; values are discarded by [`flag_names`].
    pub flags: Vec<String>,
    /// MCP `clientInfo.name` or `--agent`, when known.
    pub agent: Option<String>,
    pub termination: Termination,
    /// Every value the caller supplied. Never sent.
    pub argument_values: Vec<String>,
}

impl Invocation {
    pub fn result_class(&self) -> ResultClass {
        classify(&self.termination, self.status)
    }
}

/// A fresh invocation id, `inv_` and 32 lower-case hex characters.
pub fn new_invocation_id() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // An id is a join key, not a secret: a clock-derived one still joins.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        bytes = nanos.to_le_bytes();
    }
    let mut id = String::with_capacity(36);
    id.push_str("inv_");
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

/// The operating system token the store admits.
pub fn os_token() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        _ => "other",
    }
}

/// The `cli_command` body for one invocation, or `None` for an `ok` ending,
/// which is counted into the rollup instead.
pub fn command_event(invocation: &Invocation) -> Option<Value> {
    let result_class = invocation.result_class();
    let outcome = result_class.outcome()?;
    let (error_class, error_code, retryable, message) = match &invocation.termination {
        Termination::Ok => return None,
        Termination::Error {
            class,
            code,
            message,
            retryable,
        } => (class.as_str(), code.as_str(), *retryable, message.as_str()),
        Termination::Crashed { message } => ("internal", "panic", false, message.as_str()),
    };
    let mut event = host_fields(invocation, SOURCE_COMMAND);
    event.insert("action".into(), json!(action_id(&invocation.action)));
    if let Some(action_class) = invocation
        .action_class
        .as_deref()
        .filter(|class| matches!(*class, "report" | "async" | "orchestration" | "infra"))
    {
        event.insert("action_class".into(), json!(action_class));
    }
    if let Some(contract) = invocation.contract {
        event.insert("contract".into(), json!(contract));
    }
    event.insert("result_class".into(), json!(result_class.token()));
    event.insert("outcome".into(), json!(outcome));
    if let Some(code) = token(error_code, MAX_ID_CHARS) {
        event.insert("error_code".into(), json!(code));
    }
    if let Some(class) = token(error_class, 64) {
        event.insert("error_class".into(), json!(class));
    }
    event.insert("retryable".into(), json!(retryable));
    event.insert("latency_ms".into(), json!(invocation.latency_ms));
    if let Some(status) = invocation.status.filter(|status| *status > 0) {
        event.insert("status".into(), json!(status));
    }
    if let Some(request_id) = invocation.request_id.as_deref().and_then(id_text) {
        event.insert("request_id".into(), json!(request_id));
    }
    if let Some(invocation_id) = id_text(&invocation.invocation_id) {
        event.insert("user_action_id".into(), json!(invocation_id));
    }
    if let Some(project) = invocation
        .project
        .as_deref()
        .filter(|project| crate::validate_project_id(project).is_ok())
    {
        event.insert("ds_project".into(), json!(project));
    }
    let flags = flag_names(&invocation.flags);
    if !flags.is_empty() {
        event.insert("flags".into(), json!(flags));
    }
    if let Some(agent) = invocation.agent.as_deref().map(agent_text) {
        if !agent.is_empty() {
            event.insert("agent".into(), json!(agent));
        }
    }
    let message = sanitize_message(message, &invocation.argument_values);
    if !message.is_empty() {
        event.insert("error_message".into(), json!(message));
    }
    event.insert("occurrences".into(), json!(1));
    Some(Value::Object(event))
}

fn host_fields(invocation: &Invocation, source: &str) -> Map<String, Value> {
    let mut event = Map::new();
    event.insert("source".into(), json!(source));
    event.insert("platform".into(), json!(invocation.platform.token()));
    event.insert("host_kind".into(), json!(invocation.host_kind.token()));
    event.insert("os".into(), json!(os_token()));
    if let Some(release) = id_text(&invocation.release) {
        event.insert("release".into(), json!(release));
    }
    if let Some(lane) = token(&invocation.lane, 32) {
        event.insert("lane".into(), json!(lane));
    }
    event
}

fn rollup_event(host: &Invocation, day: &str, counts: &BTreeMap<String, u64>) -> Value {
    let mut event = host_fields(host, SOURCE_ROLLUP);
    // ds-brain's ingest requires `action` or `path` on every body; each stored
    // rollup row takes its action from `counts`, so this names the body only.
    event.insert("action".into(), json!(SOURCE_ROLLUP));
    event.insert("user_action_id".into(), json!(format!("rollup_{day}")));
    event.insert("counts".into(), json!(counts));
    Value::Object(event)
}

/// Flag names only: `--radius-m=40` keeps `--radius-m`; anything that is not a
/// flag name — a value, a path — is discarded. Deduplicated, at most
/// [`MAX_FLAGS`].
pub fn flag_names(flags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for flag in flags {
        let name = flag.split('=').next().unwrap_or_default().trim();
        let body = name.trim_start_matches('-');
        let dashes = name.len() - body.len();
        let valid = (1..=2).contains(&dashes)
            && name.len() <= 64
            && body
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && body
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if valid && !out.iter().any(|seen| seen == name) {
            out.push(name.to_string());
            if out.len() == MAX_FLAGS {
                break;
            }
        }
    }
    out
}

/// The error message as it may leave the machine (contract §3.3).
///
/// Every supplied argument value is removed first, by exact match (and, for a
/// path, by its file name too). Then whatever is still path-like or value-like
/// is replaced by a placeholder: quoted text, URLs, e-mail addresses, paths,
/// file names, `key=value` values, coordinates and long numbers, and opaque
/// tokens. Control characters are removed and the result is bounded to
/// [`MAX_ERROR_MESSAGE_CHARS`].
///
/// ds-web has no privacy sanitizer for SRE messages to share: its
/// `sanitize-message.ts` rewrites platform jargon for display and documents
/// that it must never touch wire payloads. This is the privacy sanitizer.
pub fn sanitize_message(message: &str, argument_values: &[String]) -> String {
    let mut text: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut secrets: Vec<&str> = Vec::new();
    for value in argument_values {
        let value = value.trim();
        secrets.push(value);
        if let Some(name) = value.rsplit(['/', '\\']).next() {
            secrets.push(name);
        }
    }
    secrets.retain(|value| value.chars().count() >= MIN_SCRUBBED_VALUE_CHARS);
    // Longest first, so a path is removed before the file name inside it.
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets.dedup();
    for secret in secrets {
        if text.contains(secret) {
            text = text.replace(secret, "<value>");
        }
    }
    let text = replace_quoted(&text);
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&sanitize_word(word));
    }
    bounded(&out, MAX_ERROR_MESSAGE_CHARS)
}

fn replace_quoted(text: &str) -> String {
    const PAIRS: [(char, char); 5] = [
        ('\'', '\''),
        ('"', '"'),
        ('`', '`'),
        ('\u{2018}', '\u{2019}'),
        ('\u{201c}', '\u{201d}'),
    ];
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let close = PAIRS
            .iter()
            .find(|(open, _)| *open == c)
            .map(|(_, close)| *close);
        // An apostrophe inside a word ("can't") opens nothing.
        let inside_word = c == '\''
            && index > 0
            && chars[index - 1].is_alphanumeric()
            && chars
                .get(index + 1)
                .is_some_and(|next| next.is_alphanumeric());
        if let (Some(close), false) = (close, inside_word) {
            if let Some(end) = chars[index + 1..].iter().position(|next| *next == close) {
                out.push_str("<value>");
                index += end + 2;
                continue;
            }
        }
        out.push(c);
        index += 1;
    }
    out
}

const FILE_EXTENSIONS: &[&str] = &[
    "bak", "csv", "dbf", "don", "doc", "docx", "dsgrid", "dxf", "geojson", "gpkg", "jpeg", "jpg",
    "json", "kml", "kmz", "las", "laz", "md", "ndjson", "pdf", "pmtiles", "png", "prj", "shp",
    "shx", "sqlite", "tif", "tiff", "toml", "txt", "xls", "xlsx", "xml", "xyz", "yaml", "yml",
    "zip",
];

fn sanitize_word(word: &str) -> String {
    let start = word
        .find(|c: char| !is_edge_punctuation(c))
        .unwrap_or(word.len());
    let end = word
        .rfind(|c: char| !is_edge_punctuation(c))
        .map(|index| index + word[index..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(start)
        .max(start);
    let (lead, core, trail) = (&word[..start], &word[start..end], &word[end..]);
    if core.is_empty() {
        return word.to_string();
    }
    let replaced = if core.contains("://") {
        Some("<url>".to_string())
    } else if core.contains('@') && core.rsplit('@').next().is_some_and(|d| d.contains('.')) {
        Some("<email>".to_string())
    } else if core.contains('/') || core.contains('\\') || core.starts_with('~') {
        Some("<path>".to_string())
    } else if let Some((key, _)) = core.split_once('=') {
        Some(format!("{key}=<value>"))
    } else if is_file_name(core) {
        Some("<path>".to_string())
    } else if is_number_like(core) {
        Some("<n>".to_string())
    } else if is_opaque_token(core) {
        Some("<token>".to_string())
    } else {
        None
    };
    match replaced {
        Some(core) => format!("{lead}{core}{trail}"),
        None => word.to_string(),
    }
}

fn is_edge_punctuation(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | ':' | '.' | '!' | '?'
    )
}

fn is_file_name(core: &str) -> bool {
    core.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty() && FILE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
    })
}

fn is_number_like(core: &str) -> bool {
    let digits = core.chars().filter(char::is_ascii_digit).count();
    let numeric = core
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+'));
    numeric && digits > 0 && (core.contains('.') || digits >= 5)
}

fn is_opaque_token(core: &str) -> bool {
    let shaped = core
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    shaped && core.len() >= 20 && core.chars().any(|c| c.is_ascii_digit())
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn id_text(value: &str) -> Option<String> {
    let value: String = value.chars().filter(|c| !c.is_control()).collect();
    let value = value.trim();
    (!value.is_empty()).then(|| bounded(value, MAX_ID_CHARS))
}

fn agent_text(value: &str) -> String {
    let value: String = value.chars().filter(|c| !c.is_control()).collect();
    bounded(value.trim(), MAX_AGENT_CHARS)
}

/// The command id as the store admits it; an ending before any command was
/// resolved, or an id the store would refuse, is reported as `ds` itself.
fn action_id(action: &str) -> String {
    token(action, MAX_ID_CHARS).unwrap_or_else(|| "ds".to_string())
}

/// One lower-case store token (`[a-z0-9][a-z0-9_.-]*`), or nothing.
fn token(value: &str, limit: usize) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    let valid = !value.is_empty()
        && value.len() <= limit
        && value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
    valid.then_some(value)
}

// ---------------------------------------------------------------------------
// Persistence and delivery, supplied by the host
// ---------------------------------------------------------------------------

/// Why the host could not reach the reporter's state document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReporterStoreError(pub &'static str);

/// The host's persistence for one owner-only state document.
///
/// The document is opaque to the host. `acquire` takes a bounded,
/// cross-process lease that covers one `load`/`save` pair; a host that cannot
/// get it quickly answers an error, and the invocation simply goes unreported.
pub trait ReporterStore {
    fn acquire(&mut self) -> Result<(), ReporterStoreError>;
    fn load(&mut self) -> Result<Option<Vec<u8>>, ReporterStoreError>;
    /// Replace the whole document atomically.
    fn save(&mut self, document: &[u8]) -> Result<(), ReporterStoreError>;
    fn release(&mut self);
}

/// Why one event was not delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFailure {
    /// No answer, a timeout, 408, 429 or a 5xx: keep it for a later flush.
    Unreachable,
    /// The store answered 4xx for this body and never will accept it:
    /// dropped and counted.
    Rejected,
}

/// The host's send of one body over `domains.sre.client_request_event`.
pub trait EventSink {
    fn send(&mut self, event: &Value) -> Result<(), SendFailure>;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    schema: String,
    outbox: Vec<Queued>,
    /// Dedupe key → when that key was last sent.
    sent: BTreeMap<String, u64>,
    /// Send times within the last minute.
    send_times: Vec<u64>,
    rollup_day: Option<String>,
    rollup: BTreeMap<String, u64>,
    dropped: u64,
    rejected: u64,
    last_flush_ms: Option<u64>,
    last_flush: Option<String>,
    /// The last [`JOURNAL_ENTRIES`] invocations, oldest first (contract §4).
    journal: Vec<JournalEntry>,
    /// Crash reports `ds` files by itself, waiting for a signed-in send
    /// (contract §5.3). One per crashing command; repeats raise its count.
    automatic: Vec<AutomaticReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Queued {
    /// `(action, error_code)`; a rollup has none and is never coalesced.
    key: Option<String>,
    queued_ms: u64,
    /// Not sent before this time: a repeat is held behind its key's last send.
    due_ms: u64,
    /// Identical endings before this time coalesce into this event.
    window_end_ms: u64,
    event: Value,
}

impl State {
    fn decode(document: Option<Vec<u8>>) -> Self {
        let state = document
            .and_then(|bytes| serde_json::from_slice::<State>(&bytes).ok())
            .filter(|state| state.schema == STATE_SCHEMA);
        state.unwrap_or_else(|| State {
            schema: STATE_SCHEMA.to_string(),
            ..State::default()
        })
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("the reporter state serializes")
    }

    fn outbox_bytes(&self) -> usize {
        self.outbox
            .iter()
            .map(|queued| serde_json::to_vec(&queued.event).map_or(0, |bytes| bytes.len()))
            .sum()
    }

    /// Oldest first, until both bounds hold. Every drop is counted.
    fn enforce_bounds(&mut self) {
        while !self.outbox.is_empty()
            && (self.outbox.len() > OUTBOX_MAX_EVENTS || self.outbox_bytes() > OUTBOX_MAX_BYTES)
        {
            self.outbox.remove(0);
            self.dropped += 1;
        }
    }

    fn prune(&mut self, now_ms: u64) {
        self.sent
            .retain(|_, sent_ms| now_ms.saturating_sub(*sent_ms) < DEDUPE_WINDOW_MS);
        self.send_times
            .retain(|sent_ms| now_ms.saturating_sub(*sent_ms) < MINUTE_MS);
    }
}

fn with_state<T>(
    store: &mut dyn ReporterStore,
    change: impl FnOnce(&mut State) -> T,
) -> Result<T, ReporterStoreError> {
    store.acquire()?;
    let result = (|| {
        let mut state = State::decode(store.load()?);
        let answer = change(&mut state);
        store.save(&state.encode())?;
        Ok(answer)
    })();
    store.release();
    result
}

fn utc_day(now_ms: u64) -> String {
    let seconds = i64::try_from(now_ms / 1_000).unwrap_or(i64::MAX);
    time::OffsetDateTime::from_unix_timestamp(seconds)
        .map(|moment| {
            let date = moment.date();
            format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                u8::from(date.month()),
                date.day()
            )
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Record, flush, status
// ---------------------------------------------------------------------------

/// What [`record`] did with one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub result_class: ResultClass,
    /// A new event entered the outbox.
    pub queued: bool,
    /// The ending was folded into an identical event's `occurrences`.
    pub coalesced: bool,
    /// The event's id (`user_action_id`) when one was queued or coalesced.
    pub event_id: Option<String>,
    /// What a crash did to the automatic bug report (contract §5.3).
    pub automatic: AutomaticReporting,
}

/// Record one finished invocation: an `ok` ending is counted into today's
/// rollup; any other ending becomes an outbox event, or raises the
/// `occurrences` of an identical one inside its dedupe window.
pub fn record(
    store: &mut dyn ReporterStore,
    invocation: &Invocation,
    now_ms: u64,
) -> Result<Recorded, ReporterStoreError> {
    let result_class = invocation.result_class();
    let event = command_event(invocation);
    with_state(store, |state| {
        state.prune(now_ms);
        roll_day(state, invocation, now_ms);
        journal(state, invocation, now_ms);
        let automatic = queue_automatic(state, invocation, now_ms);
        let Some(event) = event else {
            let action = action_id(&invocation.action);
            if state.rollup.contains_key(&action) || state.rollup.len() < MAX_ROLLUP_COMMANDS {
                *state.rollup.entry(action).or_insert(0) += 1;
            }
            return Recorded {
                result_class,
                queued: false,
                coalesced: false,
                event_id: None,
                automatic,
            };
        };
        let key = format!(
            "{}\u{1f}{}",
            event["action"].as_str().unwrap_or_default(),
            event["error_code"].as_str().unwrap_or_default()
        );
        if let Some(open) = state.outbox.iter_mut().find(|queued| {
            queued.key.as_deref() == Some(key.as_str()) && now_ms < queued.window_end_ms
        }) {
            let occurrences = open.event["occurrences"].as_u64().unwrap_or(1) + 1;
            open.event["occurrences"] = json!(occurrences);
            let event_id = open.event["user_action_id"].as_str().map(str::to_string);
            // The ending lives in that event now: the journal's draft and a
            // report queued by this ending name the event that will exist.
            if let Some(id) = &event_id {
                if let Some(entry) = state.journal.last_mut() {
                    entry.invocation_id = id.clone();
                }
                if automatic == AutomaticReporting::Queued {
                    if let Some(report) = state.automatic.last_mut() {
                        report.payload["context"]["sre_event"] = json!(id);
                    }
                }
            }
            return Recorded {
                result_class,
                queued: false,
                coalesced: true,
                event_id,
                automatic,
            };
        }
        let held_until = state
            .sent
            .get(&key)
            .map(|sent_ms| sent_ms + DEDUPE_WINDOW_MS)
            .filter(|until| *until > now_ms);
        let event_id = event["user_action_id"].as_str().map(str::to_string);
        state.outbox.push(Queued {
            key: Some(key),
            queued_ms: now_ms,
            due_ms: held_until.unwrap_or(now_ms),
            window_end_ms: held_until.unwrap_or(now_ms + DEDUPE_WINDOW_MS),
            event,
        });
        state.enforce_bounds();
        Recorded {
            result_class,
            queued: true,
            coalesced: false,
            event_id,
            automatic,
        }
    })
}

/// On the first invocation of a new UTC day, yesterday's counts become one
/// `cli_rollup` event.
fn roll_day(state: &mut State, host: &Invocation, now_ms: u64) {
    let today = utc_day(now_ms);
    if state.rollup_day.as_deref() == Some(today.as_str()) {
        return;
    }
    if let Some(previous) = state.rollup_day.take() {
        if !state.rollup.is_empty() {
            state.outbox.push(Queued {
                key: None,
                queued_ms: now_ms,
                due_ms: now_ms,
                window_end_ms: now_ms,
                event: rollup_event(host, &previous, &state.rollup),
            });
            state.enforce_bounds();
        }
    }
    state.rollup.clear();
    state.rollup_day = Some(today);
}

/// What one [`flush`] delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flushed {
    pub sent: usize,
    pub rejected: usize,
    /// Events still in the outbox afterwards.
    pub kept: usize,
    /// The sink was unreachable; the rest waits for the next invocation.
    pub offline: bool,
    /// The per-minute cap stopped the flush.
    pub rate_limited: bool,
}

/// Send every due outbox event, oldest first, within `budget` and the
/// per-minute cap. An unreachable sink stops the flush and keeps the events;
/// a rejected body is dropped and counted. Nothing here is ever reported.
pub fn flush(
    store: &mut dyn ReporterStore,
    sink: &mut dyn EventSink,
    now_ms: u64,
    budget: Duration,
) -> Result<Flushed, ReporterStoreError> {
    let started = Instant::now();
    with_state(store, |state| {
        state.prune(now_ms);
        let mut flushed = Flushed::default();
        let mut index = 0;
        while index < state.outbox.len() {
            if state.outbox[index].due_ms > now_ms {
                index += 1;
                continue;
            }
            if state.send_times.len() >= MAX_EVENTS_PER_MINUTE {
                flushed.rate_limited = true;
                break;
            }
            if started.elapsed() >= budget {
                break;
            }
            match sink.send(&state.outbox[index].event) {
                Ok(()) => {
                    let sent = state.outbox.remove(index);
                    if let Some(key) = sent.key {
                        state.sent.insert(key, now_ms);
                    }
                    state.send_times.push(now_ms);
                    flushed.sent += 1;
                }
                Err(SendFailure::Rejected) => {
                    state.outbox.remove(index);
                    state.rejected += 1;
                    flushed.rejected += 1;
                }
                Err(SendFailure::Unreachable) => {
                    flushed.offline = true;
                    break;
                }
            }
        }
        flushed.kept = state.outbox.len();
        state.last_flush_ms = Some(now_ms);
        state.last_flush = Some(
            if flushed.offline {
                "offline"
            } else if flushed.rate_limited {
                "rate_limited"
            } else if flushed.kept > 0 {
                "partial"
            } else {
                "sent"
            }
            .to_string(),
        );
        flushed
    })
}

/// Whether the outbox holds an event a flush would send now.
pub fn has_due(store: &mut dyn ReporterStore, now_ms: u64) -> Result<bool, ReporterStoreError> {
    with_state(store, |state| {
        state.outbox.iter().any(|queued| queued.due_ms <= now_ms)
    })
}

/// The reporting state `ds doctor` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReporterStatus {
    pub enabled: bool,
    pub outbox_events: usize,
    pub outbox_bytes: usize,
    /// Events dropped because the outbox was full.
    pub dropped: u64,
    /// Events the store refused (4xx).
    pub rejected: u64,
    pub last_flush_ms: Option<u64>,
    /// `sent`, `partial`, `offline` or `rate_limited`.
    pub last_flush: Option<String>,
    pub rollup_day: Option<String>,
    pub rollup_commands: usize,
    /// Automatic crash reports waiting for a signed-in send.
    pub automatic_reports: usize,
}

impl ReporterStatus {
    pub fn to_json(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "outbox_events": self.outbox_events,
            "outbox_bytes": self.outbox_bytes,
            "outbox_max_events": OUTBOX_MAX_EVENTS,
            "outbox_max_bytes": OUTBOX_MAX_BYTES,
            "dropped": self.dropped,
            "rejected": self.rejected,
            "last_flush_ms": self.last_flush_ms,
            "last_flush": self.last_flush,
            "rollup_day": self.rollup_day,
            "rollup_commands": self.rollup_commands,
            "automatic_reports": self.automatic_reports,
        })
    }
}

/// Read the reporting state. A disabled host answers its switch and the
/// state document as it was last left, and changes nothing.
pub fn status(
    store: &mut dyn ReporterStore,
    enabled: bool,
) -> Result<ReporterStatus, ReporterStoreError> {
    store.acquire()?;
    let state = store.load().map(State::decode);
    store.release();
    let state = state?;
    Ok(ReporterStatus {
        enabled,
        outbox_events: state.outbox.len(),
        outbox_bytes: state.outbox_bytes(),
        dropped: state.dropped,
        rejected: state.rejected,
        last_flush_ms: state.last_flush_ms,
        last_flush: state.last_flush.clone(),
        rollup_day: state.rollup_day.clone(),
        rollup_commands: state.rollup.len(),
        automatic_reports: state.automatic.len(),
    })
}

/// The whole exit path: record the invocation, then flush anything due within
/// [`SEND_BUDGET`]. Disabled, it touches neither store nor sink. Every store
/// or sink failure is swallowed here: reporting never changes a result.
pub fn finish_invocation(
    enabled: bool,
    store: &mut dyn ReporterStore,
    sink: &mut dyn EventSink,
    invocation: &Invocation,
    now_ms: u64,
) -> Option<Recorded> {
    if !enabled {
        return None;
    }
    let recorded = record(store, invocation, now_ms).ok()?;
    if has_due(store, now_ms).unwrap_or(false) {
        let _ = flush(store, sink, now_ms, SEND_BUDGET);
    }
    Some(recorded)
}

// ---------------------------------------------------------------------------
// The fixed send
// ---------------------------------------------------------------------------

/// The ingest path of `domains.sre.client_request_event`.
pub const CLIENT_EVENT_PATH: &str = "/api/v1/sre/requests";
/// The registry's response ceiling for that operation.
pub const CLIENT_EVENT_RESPONSE_LIMIT: usize = 64 * 1024;

/// One reporter body on the fixed ingest call.
///
/// The bearer is optional on purpose: ds-brain mounts the ingest with optional
/// authentication and attributes a body without one to `anonymous` (contract
/// §3.2, OPEN-X §25 default), so a signed-out host still reports. The gateway
/// key is the guard. The deadline is the exit budget, not the registry's ten
/// seconds: reporting never holds a command's exit longer than that.
pub struct SreClientEventCall<'a> {
    profile: &'a crate::ClientProfile,
    body: Vec<u8>,
    bearer_token: Option<&'a str>,
}

impl<'a> SreClientEventCall<'a> {
    pub fn new(
        profile: &'a crate::ClientProfile,
        event: &Value,
        bearer_token: Option<&'a str>,
    ) -> Self {
        Self {
            profile,
            body: serde_json::to_vec(event).expect("a reporter event serializes"),
            bearer_token: bearer_token.filter(|token| !token.is_empty()),
        }
    }
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        CLIENT_EVENT_PATH
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        CLIENT_EVENT_RESPONSE_LIMIT
    }
    pub const fn timeout(&self) -> Duration {
        SEND_BUDGET
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn bearer_token(&self) -> Option<&str> {
        self.bearer_token
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

/// How the ingest's answer maps onto [`SendFailure`]: 2xx is delivered; no
/// answer, a timeout, 408, 429 or a 5xx is kept for later; any other 4xx is a
/// body the store will never accept.
pub fn send_outcome(
    answer: Result<crate::TransportResponse, crate::TransportError>,
) -> Result<(), SendFailure> {
    match answer {
        Ok(response) if (200..300).contains(&response.status) => Ok(()),
        Ok(response) if matches!(response.status, 408 | 429) || response.status >= 500 => {
            Err(SendFailure::Unreachable)
        }
        Ok(_) => Err(SendFailure::Rejected),
        Err(_) => Err(SendFailure::Unreachable),
    }
}

/// The [`EventSink`] a host builds from its [`crate::Transport`] and profile.
pub struct TransportSink<'a, T: crate::Transport> {
    pub profile: &'a crate::ClientProfile,
    pub transport: &'a mut T,
    pub bearer_token: Option<&'a str>,
}

impl<T: crate::Transport> EventSink for TransportSink<'_, T> {
    fn send(&mut self, event: &Value) -> Result<(), SendFailure> {
        send_outcome(self.transport.sre_client_event(SreClientEventCall::new(
            self.profile,
            event,
            self.bearer_token,
        )))
    }
}

// ---------------------------------------------------------------------------
// The invocation journal and the feedback draft (contract §4)
// ---------------------------------------------------------------------------

/// The journal keeps this many invocations.
pub const JOURNAL_ENTRIES: usize = 50;
/// A draft covers at most this many invocations.
pub const MAX_DRAFTS: usize = 10;

/// One journalled invocation, under the same privacy rules as an event:
/// flag names, codes and ids, never a value or a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub at_ms: u64,
    pub action: String,
    pub flags: Vec<String>,
    pub result_class: String,
    pub error_code: Option<String>,
    pub latency_ms: u64,
    pub request_id: Option<String>,
    /// The event id an SRE row of this invocation carries (`user_action_id`).
    pub invocation_id: String,
    pub release: String,
    pub lane: String,
    pub host_kind: String,
    pub platform: String,
    pub agent: Option<String>,
}

fn journal(state: &mut State, invocation: &Invocation, now_ms: u64) {
    let error_code = match &invocation.termination {
        Termination::Ok => None,
        Termination::Error { code, .. } => token(code, MAX_ID_CHARS),
        Termination::Crashed { .. } => Some("panic".to_string()),
    };
    state.journal.push(JournalEntry {
        at_ms: now_ms,
        action: action_id(&invocation.action),
        flags: flag_names(&invocation.flags),
        result_class: invocation.result_class().token().to_string(),
        error_code,
        latency_ms: invocation.latency_ms,
        request_id: invocation.request_id.as_deref().and_then(id_text),
        invocation_id: id_text(&invocation.invocation_id).unwrap_or_default(),
        release: id_text(&invocation.release).unwrap_or_default(),
        lane: token(&invocation.lane, 32).unwrap_or_default(),
        host_kind: invocation.host_kind.token().to_string(),
        platform: invocation.platform.token().to_string(),
        agent: invocation
            .agent
            .as_deref()
            .map(agent_text)
            .filter(|agent| !agent.is_empty()),
    });
    let excess = state.journal.len().saturating_sub(JOURNAL_ENTRIES);
    state.journal.drain(..excess);
}

/// The journal, newest first.
pub fn journal_entries(
    store: &mut dyn ReporterStore,
) -> Result<Vec<JournalEntry>, ReporterStoreError> {
    store.acquire()?;
    let state = store.load().map(State::decode);
    store.release();
    let mut entries = state?.journal;
    entries.reverse();
    Ok(entries)
}

/// Ready `feedback.submit` payloads for the newest `n` failed, crashed or
/// refused invocations, newest first. Each carries the title, component, kind,
/// severity guess, evidence and context; its `detail` names the two things
/// only the caller can add: what it expected and what would show it works.
pub fn draft(entries: &[JournalEntry], n: usize) -> Vec<Value> {
    entries
        .iter()
        .filter(|entry| entry.result_class != ResultClass::Ok.token())
        .take(n.clamp(1, MAX_DRAFTS))
        .map(draft_one)
        .collect()
}

fn draft_one(entry: &JournalEntry) -> Value {
    let code = entry.error_code.as_deref().unwrap_or("no_code");
    let (kind, severity) = match entry.result_class.as_str() {
        "crashed" => ("bug", "major"),
        "failed" => ("bug", "minor"),
        _ => ("friction", "minor"),
    };
    let domain = entry.action.split('.').next().unwrap_or("ds");
    let mut evidence = vec![
        format!(
            "ds {} ended {} with {code}",
            entry.action, entry.result_class
        ),
        format!("release {}, lane {}", entry.release, entry.lane),
        format!("latency {} ms", entry.latency_ms),
    ];
    if !entry.flags.is_empty() {
        evidence.push(format!("flags {}", entry.flags.join(" ")));
    }
    if let Some(request_id) = &entry.request_id {
        evidence.push(format!("request id {request_id}"));
    }
    let mut payload = json!({
        "title": bounded(
            &format!("ds {} {}: {code}", entry.action, entry.result_class),
            200,
        ),
        "component": format!("ds-cli/{domain}"),
        "kind": kind,
        "severity": severity,
        "evidence": evidence,
        "context": {
            "sre_event": entry.invocation_id,
            "host_kind": entry.host_kind,
            "lane": entry.lane,
            "platform": entry.platform,
        },
        "detail": format!(
            "Expected: <what you expected>. Observed: ds {} ended {} with {code}. \
             Acceptance: <what would show it works>.",
            entry.action, entry.result_class
        ),
        "needs": ["detail: expected behaviour", "detail: acceptance condition"],
    });
    if let Some(agent) = &entry.agent {
        payload["agent"] = json!(agent);
    } else if let Some(needs) = payload["needs"].as_array_mut() {
        needs.push(json!("agent: the reporting agent's name"));
    }
    payload
}

// ---------------------------------------------------------------------------
// What a failure envelope points to, and what `ds` files by itself (§5.3, §5.4)
// ---------------------------------------------------------------------------

/// The command a failure envelope names so its reader can file the sighting.
pub const DRAFT_COMMAND: &str = "ds feedback draft --last";
/// The agent an automatic crash report is filed as.
pub const AUTOMATIC_AGENT: &str = "ds (automatic)";
/// Crash reports waiting for a send, at most; the oldest is dropped first.
pub const MAX_AUTOMATIC_REPORTS: usize = 20;

/// The `feedback` member a failure envelope carries: for a `failed` or
/// `crashed` ending, the draft command and this invocation's SRE event id.
/// An answer or a refusal carries none: a refusal is the contract working.
pub fn next_feedback(class: ResultClass, invocation_id: &str) -> Option<Value> {
    matches!(class, ResultClass::Failed | ResultClass::Crashed).then(|| {
        json!({
            "draft": DRAFT_COMMAND,
            "sre_event": id_text(invocation_id),
        })
    })
}

/// What one recorded ending did to the automatic crash reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomaticReporting {
    /// Not a crash or an internal error.
    None,
    /// A crash or internal error with no pending report of its command and
    /// code: one bug report waits.
    Queued,
    /// A repeat: the pending report's count rose to this.
    Raised { occurrences: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomaticReport {
    /// `(action, error_code)` of the crash.
    key: String,
    first_ms: u64,
    occurrences: u64,
    /// The `feedback.submit` payload, from the journalled invocation.
    payload: Value,
}

/// A panic or abort, or an `internal` error: the endings `ds` files by itself
/// (contract §5.3). Any other failure or refusal needs a person's expectation.
fn files_automatically(invocation: &Invocation) -> bool {
    match &invocation.termination {
        Termination::Crashed { .. } => true,
        Termination::Error { class, .. } => class == "internal",
        Termination::Ok => false,
    }
}

fn queue_automatic(state: &mut State, invocation: &Invocation, now_ms: u64) -> AutomaticReporting {
    if !files_automatically(invocation) {
        return AutomaticReporting::None;
    }
    let Some(entry) = state.journal.last() else {
        return AutomaticReporting::None;
    };
    let key = format!(
        "{}\u{1f}{}",
        entry.action,
        entry.error_code.as_deref().unwrap_or_default()
    );
    if let Some(pending) = state
        .automatic
        .iter_mut()
        .find(|pending| pending.key == key)
    {
        pending.occurrences += 1;
        return AutomaticReporting::Raised {
            occurrences: pending.occurrences,
        };
    }
    let payload = automatic_payload(entry);
    state.automatic.push(AutomaticReport {
        key,
        first_ms: now_ms,
        occurrences: 1,
        payload,
    });
    let excess = state.automatic.len().saturating_sub(MAX_AUTOMATIC_REPORTS);
    state.automatic.drain(..excess);
    AutomaticReporting::Queued
}

/// The draft of the crashing invocation, completed: `ds` itself states what
/// was expected and what would show the crash is gone.
fn automatic_payload(entry: &JournalEntry) -> Value {
    let mut payload = draft_one(entry);
    payload["agent"] = json!(AUTOMATIC_AGENT);
    payload["detail"] = json!(format!(
        "Expected: ds {action} answers or refuses with a declared code. Observed: it \
         ended {class} with {code} (the message never leaves the machine). \
         Acceptance: the same invocation no longer ends this way. Filed automatically \
         by ds.",
        action = entry.action,
        class = entry.result_class,
        code = entry.error_code.as_deref().unwrap_or("no_code"),
    ));
    if let Some(object) = payload.as_object_mut() {
        object.remove("needs");
    }
    payload
}

/// The host's submit of one `feedback.submit` payload as the signed-in user.
/// A signed-out host, no network or a 5xx is [`SendFailure::Unreachable`]: the
/// report waits for a later invocation. A body the backlog refuses is
/// [`SendFailure::Rejected`]: dropped, never retried.
pub trait ReportSink {
    fn submit(&mut self, payload: &Value) -> Result<(), SendFailure>;
}

/// What one [`file_automatic`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FiledReports {
    pub filed: usize,
    pub rejected: usize,
    /// Reports still waiting afterwards.
    pub kept: usize,
    pub offline: bool,
}

/// Submit every waiting crash report, oldest first. The state lease is held
/// only to read and to settle, never across a submit, so a slow sign-in never
/// blocks another `ds` process. A crash recorded meanwhile stays waiting: only
/// the occurrences that were submitted are settled. The feedback backlog
/// merges a repeated report into its occurrences, so a report filed again
/// after a later crash raises the same report's count.
pub fn file_automatic(
    store: &mut dyn ReporterStore,
    sink: &mut dyn ReportSink,
) -> Result<FiledReports, ReporterStoreError> {
    let pending = with_state(store, |state| state.automatic.clone())?;
    let mut settled: Vec<(String, u64)> = Vec::new();
    let mut outcome = FiledReports::default();
    for report in &pending {
        let mut payload = report.payload.clone();
        if report.occurrences > 1 {
            if let Some(evidence) = payload["evidence"].as_array_mut() {
                evidence.push(json!(format!(
                    "happened {} times since {}",
                    report.occurrences,
                    utc_day(report.first_ms)
                )));
            }
        }
        match sink.submit(&payload) {
            Ok(()) => {
                outcome.filed += 1;
                settled.push((report.key.clone(), report.occurrences));
            }
            Err(SendFailure::Rejected) => {
                outcome.rejected += 1;
                settled.push((report.key.clone(), report.occurrences));
            }
            Err(SendFailure::Unreachable) => {
                outcome.offline = true;
                break;
            }
        }
    }
    outcome.kept = with_state(store, |state| {
        for (key, submitted) in &settled {
            if let Some(index) = state.automatic.iter().position(|report| &report.key == key) {
                let report = &mut state.automatic[index];
                report.occurrences = report.occurrences.saturating_sub(*submitted);
                if report.occurrences == 0 {
                    state.automatic.remove(index);
                }
            }
        }
        state.rejected += outcome.rejected as u64;
        state.automatic.len()
    })?;
    Ok(outcome)
}

/// Whether a crash report waits for a send.
pub fn has_automatic(store: &mut dyn ReporterStore) -> Result<bool, ReporterStoreError> {
    store.acquire()?;
    let state = store.load().map(State::decode);
    store.release();
    Ok(!state?.automatic.is_empty())
}
