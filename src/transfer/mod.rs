//! Resumable byte-transfer protocol, owned once and shared by every host.
//!
//! This module is **pure**: no IO, no HTTP client, no file handle, no clock of
//! its own, no randomness. Hosts perform IO and report *facts*; this machine
//! decides what those facts mean and what to do next. Native and browser hosts
//! therefore cannot disagree about an acknowledgement, a committed prefix, a
//! resume point, or a retry.
//!
//! The contract is `ds-command-kernel/docs/contracts/ds-transport-authority.md`.
//!
//! Two properties are load-bearing and are the reason this exists:
//!
//! * **Bytes written to a socket are not bytes committed by the server.** They
//!   are tracked separately and only the server's number ever moves `committed`.
//! * **"I could not tell" is a value, not a default.** A host that cannot read
//!   response headers says [`RangeObservation::Unobservable`]; it does not
//!   report "no Range header", which is a different, stronger fact.
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod browser;
mod machine;
#[cfg(test)]
mod tests;

pub use machine::evaluate;

pub const TRANSFER_SCHEMA: &str = "ds.transfer/v1";

/// GCS requires every non-final resumable chunk to be a multiple of 256 KiB.
pub const CHUNK_ALIGNMENT_BYTES: u64 = 256 * 1024;
/// Default chunk. Bounded so peak host memory is a chunk, never the artifact.
pub const DEFAULT_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
/// A transfer is not allowed to buffer more than this at once, on any host.
pub const MAX_CHUNK_BYTES: u64 = 32 * 1024 * 1024;
/// Consecutive failures that produced no committed progress.
pub const MAX_STALLED_ATTEMPTS: u32 = 6;
/// Consecutive probes that established nothing. Bounded so an unreachable
/// session surfaces as a *reported* cause instead of an invisible retry loop.
pub const MAX_INCONCLUSIVE_PROBES: u32 = 4;

/// The only host a DS-minted resumable storage session may point at.
pub const STORAGE_SESSION_HOSTS: &[&str] = &["storage.googleapis.com"];
const MAX_SESSION_URI_BYTES: usize = 4096;

/// Why a session URI is not a storage session this client may write to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRefusal {
    Unparsable,
    TooLong,
    SchemeNotHttps,
    CarriesUserinfo,
    HostNotAllowed,
    NonDefaultPort,
}

/// What a send that got no answer means. A browser reports a dropped link and
/// a platform refusal the same way, without a status, so both read as the
/// platform's refusal and [`Cause::retryable`] lets a later attempt try again.
/// Every executor maps an unanswered send through this one rule.
#[cfg(any(test, all(feature = "wasm", target_arch = "wasm32")))]
pub(crate) fn unanswered() -> Cause {
    Cause::BlockedByHostPolicy
}

/// How one single-request PUT ended: the status storage committed with, or
/// the cause and whether a later attempt could fix it, by the same
/// classification the resumable machine acts on. `None` is a send that ran
/// out of time.
#[cfg(any(test, all(feature = "wasm", target_arch = "wasm32")))]
pub(crate) fn single_put_end(answer: Option<Result<u16, Cause>>) -> Result<u16, (Cause, bool)> {
    let cause = match answer {
        Some(Ok(status @ (200 | 201))) => return Ok(status),
        Some(Ok(status)) => Cause::HttpStatus { status },
        Some(Err(cause)) => cause,
        None => Cause::Timeout {
            phase: Phase::Response,
        },
    };
    let retryable = cause.retryable();
    Err((cause, retryable))
}

/// Validate the DS-minted resumable-session credential before DNS or any
/// request: HTTPS on the default port, a storage host, no user info. Every
/// host's executor asks this one rule.
pub fn validated_storage_session(raw: &str) -> Result<String, SessionRefusal> {
    if raw.len() > MAX_SESSION_URI_BYTES {
        return Err(SessionRefusal::TooLong);
    }
    if raw.is_empty() || raw.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(SessionRefusal::Unparsable);
    }
    let url = url::Url::parse(raw).map_err(|_| SessionRefusal::Unparsable)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(SessionRefusal::CarriesUserinfo);
    }
    if url.scheme() != "https" {
        return Err(SessionRefusal::SchemeNotHttps);
    }
    let host = url.host_str().ok_or(SessionRefusal::HostNotAllowed)?;
    if !STORAGE_SESSION_HOSTS.contains(&host) {
        return Err(SessionRefusal::HostNotAllowed);
    }
    if url.port_or_known_default() != Some(443) {
        return Err(SessionRefusal::NonDefaultPort);
    }
    Ok(url.to_string())
}

/// Where a host observed a transport failure. Kept coarse on purpose: it is a
/// diagnostic, never an authorization or routing input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Name resolution, connection, or TLS setup — before any byte was written.
    Connect,
    /// While writing the request body.
    Request,
    /// After the body was written, while waiting for or reading the response.
    Response,
    /// No progress for the host's idle bound.
    Idle,
}

/// A safe, typed transport cause.
///
/// Every variant is derived from something the host actually observed. None of
/// them may carry a session URI, an `Authorization` value, a query string, or a
/// response body: a cause is surfaced to operators and telemetry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cause {
    /// The server answered with this status. Safe: a status is not a secret.
    HttpStatus {
        status: u16,
    },
    Dns,
    Connect,
    Tls,
    Timeout {
        phase: Phase,
    },
    ConnectionClosed,
    /// The device is deliberately isolated. Not a defect and not retried here.
    Offline,
    Cancelled,
    /// The server's committed prefix is shorter than the bytes this host wrote.
    Truncated,
    DigestMismatch,
    LengthMismatch,
    /// A `Range` header was present but could not be parsed. Never guessed at.
    RangeMalformed,
    /// Proven gone: the server said 404/410 for this session.
    SessionExpired,
    /// The platform refused the request without surfacing a status — the
    /// browser's opaque CORS/permission failure. An honest classification of an
    /// observed refusal, never a diagnosis of the server.
    BlockedByHostPolicy,
    /// Nothing was established. The session is preserved and never restarted.
    Inconclusive,
}

impl Cause {
    /// What an operator is told. Never a URL, a host, a header value or a
    /// body; a status code is not a secret and is the most useful fact.
    pub fn describe(&self) -> String {
        match self {
            Cause::HttpStatus { status } => format!("storage answered HTTP {status}"),
            Cause::Dns => "the storage host could not be resolved".into(),
            Cause::Connect => "the connection to storage could not be opened".into(),
            Cause::Tls => "the secure connection to storage failed".into(),
            Cause::Timeout { phase: Phase::Idle } => "the transfer stopped making progress".into(),
            Cause::Timeout { phase } => format!(
                "the transfer timed out while {}",
                match phase {
                    Phase::Connect => "connecting",
                    Phase::Request => "sending",
                    _ => "waiting for the acknowledgement",
                }
            ),
            Cause::ConnectionClosed => "the connection closed before storage acknowledged".into(),
            Cause::Offline => crate::connectivity::OFFLINE_MESSAGE.into(),
            Cause::Cancelled => "the transfer was cancelled".into(),
            Cause::Truncated => "storage committed fewer bytes than were sent".into(),
            Cause::DigestMismatch => "the transferred bytes do not match the sealed digest".into(),
            Cause::LengthMismatch => "the transferred length does not match the declaration".into(),
            Cause::RangeMalformed => {
                "storage returned a resume range this client cannot parse".into()
            }
            Cause::SessionExpired => "the resumable session is gone".into(),
            Cause::BlockedByHostPolicy => {
                "this browser refused the request without a status".into()
            }
            Cause::Inconclusive => "the resumable session could not be observed".into(),
        }
    }

    /// Whether a *later* attempt could plausibly succeed. This is about the
    /// cause alone; the machine additionally bounds how often it acts on it.
    pub fn retryable(&self) -> bool {
        match self {
            Cause::HttpStatus { status } => *status >= 500 || matches!(status, 408 | 425 | 429),
            Cause::Dns
            | Cause::Connect
            | Cause::Tls
            | Cause::Timeout { .. }
            | Cause::ConnectionClosed
            | Cause::RangeMalformed
            | Cause::BlockedByHostPolicy
            | Cause::Inconclusive
            | Cause::Truncated => true,
            Cause::Offline | Cause::Cancelled => true,
            Cause::DigestMismatch | Cause::LengthMismatch | Cause::SessionExpired => false,
        }
    }
}

/// What the host was able to observe about a response's `Range` header.
///
/// The distinction between [`Absent`](RangeObservation::Absent) and
/// [`Unobservable`](RangeObservation::Unobservable) is the whole point: for a
/// resumable session, "no Range header" is the server stating that it committed
/// **zero** bytes, while "I cannot read response headers" states nothing at
/// all. Collapsing them is how a client convinces itself to re-send a file the
/// server already holds — or to give up on one it does not.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RangeObservation {
    /// The host read the response headers and there was no `Range`.
    Absent,
    /// `Range: bytes=0-END` was read. `end` is that END.
    Value { end: u64 },
    /// A `Range` header was present but not `bytes=0-N`.
    Malformed,
    /// The host cannot see response headers for this request.
    Unobservable,
}

/// Immutable parameters of one transfer. Supplied once at `start`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransferPlan {
    /// Exact byte count of the artifact being transferred.
    pub total_bytes: u64,
    /// Requested chunk size. Clamped and aligned by the machine.
    #[serde(default)]
    pub chunk_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Ask the server what it holds before writing anything.
    Probing,
    /// A chunk is in flight or about to be.
    Sending,
    /// Waiting out a bounded backoff.
    Waiting,
    Done,
    Failed,
}

/// Durable transfer state. The host persists this verbatim and hands it back;
/// it is the only thing a resume needs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransferState {
    pub schema: String,
    pub total_bytes: u64,
    pub chunk_bytes: u64,
    /// Bytes the SERVER has proven it holds. Only a server answer moves this.
    pub committed: u64,
    /// Bytes this host wrote to the socket since the last proven commit.
    /// Never reported as progress to a user and never treated as committed.
    pub sent: u64,
    pub stage: Stage,
    /// Consecutive failures that produced no committed progress.
    pub stalled_attempts: u32,
    /// Consecutive probes that established nothing.
    pub inconclusive_probes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_cause: Option<Cause>,
    /// Milliseconds since epoch after which the next action may run.
    #[serde(default)]
    pub retry_at: u64,
}

/// The single IO step the host must now perform.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Empty-body request asking the session what it holds.
    Probe,
    /// Write exactly these bytes of the artifact, at this offset.
    SendChunk { offset: u64, length: u64 },
    /// Sleep until `until`, then re-enter with the same state. `cause` is why.
    Wait { until: u64, cause: Cause },
    /// Every byte is committed. The caller finalizes through its own authority.
    Done { committed: u64 },
    /// The session is proven gone. The caller may mint a new one within its own
    /// reopen budget; the machine does not decide that.
    Reopen { cause: Cause },
    /// Stop. `retryable` says whether a later drain may try this row again.
    Fail { cause: Cause, retryable: bool },
}

/// What the host observed. Facts only — never a conclusion.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// Begin, or resume from persisted state.
    Start,
    /// Answer to [`Action::Probe`].
    ProbeResponse {
        status: u16,
        range: RangeObservation,
    },
    /// Answer to [`Action::SendChunk`].
    ChunkResponse {
        status: u16,
        range: RangeObservation,
        /// Bytes this host actually wrote to the socket for that chunk.
        bytes_written: u64,
    },
    /// No usable response. `bytes_written` is what reached the socket, which is
    /// exactly the situation where an acknowledgement is ambiguous.
    TransportFailure {
        cause: Cause,
        phase: Phase,
        #[serde(default)]
        bytes_written: u64,
    },
    /// The caller cancelled. Durable state stays resumable.
    Cancelled,
    /// The transfer completed but an integrity check failed afterwards.
    IntegrityFailure { cause: Cause },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub now: u64,
    /// Absent on the very first call; the plan is then required.
    #[serde(default)]
    pub state: Option<TransferState>,
    #[serde(default)]
    pub plan: Option<TransferPlan>,
    pub event: Event,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Reply {
    pub state: TransferState,
    pub action: Action,
}
