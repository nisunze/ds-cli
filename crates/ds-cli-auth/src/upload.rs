//! Native host for the shared resumable byte-transfer protocol.
//!
//! The protocol lives once, in `ds_command_kernel::transfer`, and is pure. This
//! module is the CLI's *IO half*: it speaks to a DS-minted storage session with
//! `ureq` and reports **facts** back to the machine — a status, whether a
//! `Range` header was present and what it said, and how many bytes actually
//! reached the socket. It never decides what a fact means. Every resume point,
//! every retry, every backoff and every terminal verdict comes back out of
//! `transfer::evaluate`, which is the same code the desktop shell runs
//! (`ds-web/src-tauri/src/gateway_transfer.rs`). There is no second reading of
//! an acknowledgement here.
//!
//! The contract is `ds-command-kernel/docs/contracts/ds-transport-authority.md`.
//!
//! What this replaced, and why: `upload_bytes` used to be one single-shot PUT
//! carrying `Content-Range: bytes 0-{n-1}/{n}` under one 1800-second global
//! timeout. It never probed, so it could not discover that the server already
//! held a prefix; a link that dropped at 99% of a large artifact started again
//! from zero; and every distinct transport cause collapsed into "unreachable".
//!
//! Four properties are load-bearing:
//!
//! * **Bounded memory.** Bytes reach the socket in [`MAX_BODY_READ_BYTES`]
//!   slices, and at most one protocol chunk is ever held (see [`ChunkSource`]).
//!   `ds_client_core::MAX_UPLOAD_BYTES` is 1 GiB, so a whole-file buffer was
//!   never an option.
//! * **Truthful observation.** A native client can read response headers, so it
//!   reports [`RangeObservation::Absent`] or [`RangeObservation::Value`] — never
//!   `Unobservable`, which is the browser's honest "I cannot see". Collapsing
//!   the two is the defect this replaces.
//! * **No DS credential on a storage session.** The session URI is the whole
//!   credential (contract §3). No `Authorization`, no `x-api-key`, no
//!   `X-User-Email` — pinned by a test, not by convention.
//! * **Redirects off.** A `308` on a resumable session means *resume
//!   incomplete*. Following it would turn a resume into a wrong request and
//!   could carry a credential to an unvalidated origin.

use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ds_client_core::{TransportError, TransportResponse};
use ds_command_kernel::transfer::{
    self, Action, Cause, Event, Phase, RangeObservation, TransferPlan, TransferState,
};
use serde::Deserialize;
use zeroize::Zeroizing;

/// The ONE host a DS-minted resumable storage session may address.
///
/// ds-brain mints every session by POSTing
/// `https://storage.googleapis.com/upload/storage/v1/b/{bucket}/o?uploadType=resumable`
/// and returning Google's `Location` header verbatim
/// (`ds-brain/internal/services/resumable_uploader.go`), so a genuine session
/// URI is always on this host. Under `authority: storage_session` the URI *is*
/// the whole credential, which is exactly why the origin has to be pinned
/// before a byte is written: an unpinned session URI is an arbitrary-URL door
/// with a credential already inside it.
const ALLOWED_STORAGE_SESSION_HOSTS: &[&str] = &["storage.googleapis.com"];

/// A session URI is a bounded credential, not a document.
const MAX_SESSION_URI_BYTES: usize = 4096;

/// The largest slice this host will ever hand to the socket, or read while
/// skipping forward, in one call. Asserted against the kernel's chunk bound.
const MAX_BODY_READ_BYTES: usize = 64 * 1024;

/// Response bodies are drained, bounded, and never read into a message. A
/// storage error body can echo the session URI, so it must not reach a log, an
/// error string, or a receipt.
const MAX_DISCARDED_RESPONSE_BYTES: u64 = 64 * 1024;

const TRANSFER_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// Once the last body byte has been handed to the transport, how long a commit
/// acknowledgement may take. Exceeding it is an honest `Timeout{Response}`,
/// which the kernel resolves by **probing** — never by assuming either outcome.
const TRANSFER_ACK_TIMEOUT: Duration = Duration::from_secs(180);
const TRANSFER_DISCARD_BODY_TIMEOUT: Duration = Duration::from_secs(30);
/// Progress semantics, not a deadline. The body budget is derived from how many
/// bytes the body has, so a transfer that keeps moving is never killed by a
/// wall clock; one that stops moving surfaces as `Timeout{phase}`. This is what
/// replaces the old blanket 1800-second global timeout (contract §4.2 rule 13).
const MIN_PROGRESS_BYTES_PER_SECOND: u64 = 4 * 1024;
const TRANSFER_PROGRESS_GRACE: Duration = Duration::from_secs(60);

/// How often a backoff or an in-flight body checks for cancellation.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Defence in depth only. The kernel already bounds stalls and inconclusive
/// probes, and `committed` only ever moves forward, so a real transfer needs at
/// most one step per chunk plus its bounded retries. This exists so a future
/// protocol change can never turn this loop into an invisible retry loop.
const MAX_TRANSFER_STEPS: u32 = 200_000;

// ---------------------------------------------------------------------------
// Session URI validation
// ---------------------------------------------------------------------------

/// Which origin a session URI is being validated against.
///
/// `upload_bytes` can only ever select [`SessionOrigin::Storage`]. The loopback
/// variant exists only under `cfg(test)` or the `weak-network-harness` feature,
/// which nothing but this crate's own tests enables, so no release build
/// contains a code path that could accept one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionOrigin {
    /// The DS mint host, over TLS.
    Storage,
    /// Tests only: a scripted `TcpListener` on loopback.
    #[cfg(any(test, feature = "weak-network-harness"))]
    Loopback,
}

/// Why a supplied session URI was refused. Typed, and safe to surface: it names
/// the rule that failed and never echoes the URI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionRefusal {
    Unparsable,
    TooLong,
    SchemeNotHttps,
    CarriesUserinfo,
    HostNotAllowed,
    NonDefaultPort,
}

/// The same rules `ds_client_core`'s own issuer applies when it accepts a
/// minted session, restated at the point a byte would leave this process.
fn validated_session_uri(raw: &str, origin: SessionOrigin) -> Result<String, SessionRefusal> {
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
    let host = url.host_str().ok_or(SessionRefusal::HostNotAllowed)?;
    match origin {
        SessionOrigin::Storage => {
            if url.scheme() != "https" {
                return Err(SessionRefusal::SchemeNotHttps);
            }
            if !ALLOWED_STORAGE_SESSION_HOSTS.contains(&host) {
                return Err(SessionRefusal::HostNotAllowed);
            }
            if url.port_or_known_default() != Some(443) {
                return Err(SessionRefusal::NonDefaultPort);
            }
        }
        #[cfg(any(test, feature = "weak-network-harness"))]
        SessionOrigin::Loopback => {
            if url.scheme() != "http" || !matches!(host, "127.0.0.1" | "localhost") {
                return Err(SessionRefusal::HostNotAllowed);
            }
        }
    }
    Ok(url.to_string())
}

// ---------------------------------------------------------------------------
// Kernel bridge
// ---------------------------------------------------------------------------

/// `transfer::Reply` is serialize-only in the kernel, so the host mirrors it to
/// read one back. The state and action types themselves are the kernel's.
#[derive(Debug, Deserialize)]
struct KernelReply {
    state: TransferState,
    action: Action,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            since.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

/// Ask the kernel what to do next. Every step goes through here: this host
/// holds no retry policy, no resume arithmetic, and no notion of what a status
/// means.
fn kernel_step(
    state: Option<&TransferState>,
    plan: Option<&TransferPlan>,
    event: &Event,
    now: u64,
) -> Result<KernelReply, String> {
    let request = serde_json::json!({
        "schema": transfer::TRANSFER_SCHEMA,
        "now": now,
        "state": state,
        "plan": plan,
        "event": event,
    });
    let encoded = serde_json::to_vec(&request)
        .map_err(|error| format!("could not encode a transfer step: {error}"))?;
    let reply = transfer::evaluate(&encoded)?;
    serde_json::from_str(&reply).map_err(|error| format!("could not read a transfer step: {error}"))
}

// ---------------------------------------------------------------------------
// The byte source
// ---------------------------------------------------------------------------

/// A forward-only artifact reader with a bounded re-send window.
///
/// `ds_client_core::UploadBytesCall` hands this host a `&mut dyn Read` and
/// nothing else: no path, no file handle, no `Seek`. The desktop shell holds
/// the sealed file itself and simply seeks to the resume offset; the CLI
/// cannot, so it keeps the ONE protocol chunk the server has not yet
/// acknowledged. That is enough for every move the machine can ask for:
///
/// * a resume offset past the reader is a bounded forward skip;
/// * a re-send of the in-flight chunk is served from `staged`;
/// * a truncated commit resumes mid-`staged`, which is already held.
///
/// The invariant is `staged_start + staged.len() == reader_position`, and
/// `staged` never exceeds one chunk, so peak memory is a chunk — never the
/// artifact, which may be 1 GiB.
struct ChunkSource<'a> {
    reader: &'a mut dyn Read,
    /// Absolute offset of the next byte `reader` will yield.
    reader_position: u64,
    /// The chunk the server has not yet acknowledged.
    staged: Vec<u8>,
    /// Absolute offset of `staged[0]`.
    staged_start: u64,
    /// Instrumentation: the largest `staged` ever grew to.
    widest_stage: usize,
}

/// Why a chunk could not be served. Kept separate from a transport cause: none
/// of these is something the server did.
#[derive(Debug)]
enum SourceError {
    /// The machine asked for bytes behind this host's re-send window. The
    /// kernel never moves `committed` backwards, so this is unreachable; it is
    /// reported rather than papered over with a re-read from zero.
    Rewind,
    Io(io::Error),
}

impl<'a> ChunkSource<'a> {
    fn new(reader: &'a mut dyn Read) -> Self {
        ChunkSource {
            reader,
            reader_position: 0,
            staged: Vec::new(),
            staged_start: 0,
            widest_stage: 0,
        }
    }

    /// Read and discard `count` bytes, in bounded slices.
    fn skip(&mut self, count: u64) -> Result<(), SourceError> {
        let mut remaining = count;
        let mut scratch = [0_u8; MAX_BODY_READ_BYTES];
        while remaining > 0 {
            let want = usize::try_from(remaining)
                .unwrap_or(MAX_BODY_READ_BYTES)
                .min(MAX_BODY_READ_BYTES);
            let read = self
                .reader
                .read(&mut scratch[..want])
                .map_err(SourceError::Io)?;
            if read == 0 {
                return Err(SourceError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the artifact ended before the prefix the server already holds",
                )));
            }
            remaining -= read as u64;
            self.reader_position = self.reader_position.saturating_add(read as u64);
        }
        Ok(())
    }

    /// The exact bytes `[offset, offset + length)` of the artifact.
    fn chunk(&mut self, offset: u64, length: u64) -> Result<&[u8], SourceError> {
        if offset < self.staged_start {
            return Err(SourceError::Rewind);
        }
        if offset > self.reader_position {
            let skip = offset - self.reader_position;
            self.staged.clear();
            self.skip(skip)?;
            self.staged_start = offset;
        } else {
            let drop = (offset - self.staged_start) as usize;
            self.staged.drain(..drop);
            self.staged_start = offset;
        }
        let want = usize::try_from(length).map_err(|_| {
            SourceError::Io(io::Error::other(
                "a chunk exceeded this host's address space",
            ))
        })?;
        let mut scratch = [0_u8; MAX_BODY_READ_BYTES];
        while self.staged.len() < want {
            let take = (want - self.staged.len()).min(MAX_BODY_READ_BYTES);
            let read = self
                .reader
                .read(&mut scratch[..take])
                .map_err(SourceError::Io)?;
            if read == 0 {
                return Err(SourceError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the artifact ended before its declared size",
                )));
            }
            self.staged.extend_from_slice(&scratch[..read]);
            self.reader_position = self.reader_position.saturating_add(read as u64);
        }
        self.widest_stage = self.widest_stage.max(self.staged.len());
        Ok(&self.staged[..want])
    }
}

/// Feeds one already-staged chunk into the socket.
///
/// It also *is* the byte counter and the cancellation point for an in-flight
/// body: `ureq` pulls from here, so this is the only place that knows how much
/// reached the transport and the only place that can stop mid-body.
struct ChunkBody<'a> {
    bytes: &'a [u8],
    position: usize,
    written: &'a AtomicU64,
    cancel: &'a dyn Fn() -> bool,
    cancelled: &'a AtomicBool,
    widest_read: &'a AtomicUsize,
}

impl Read for ChunkBody<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if (self.cancel)() {
            self.cancelled.store(true, Ordering::SeqCst);
            return Err(io::Error::other("transfer cancelled"));
        }
        let remaining = self.bytes.len() - self.position;
        if remaining == 0 || buf.is_empty() {
            return Ok(0);
        }
        let take = buf.len().min(MAX_BODY_READ_BYTES).min(remaining);
        self.widest_read.fetch_max(take, Ordering::SeqCst);
        buf[..take].copy_from_slice(&self.bytes[self.position..self.position + take]);
        self.position += take;
        self.written.fetch_add(take as u64, Ordering::SeqCst);
        Ok(take)
    }
}

// ---------------------------------------------------------------------------
// Observation and classification — facts, never conclusions
// ---------------------------------------------------------------------------

/// Read the `Range` header the way a resumable session means it.
///
/// A native host CAN see response headers, so it never answers `Unobservable`.
/// Absent means the server stated it holds zero bytes; that is a real answer
/// and must not be softened into "unknown". Anything present that is not
/// exactly `bytes=0-N` is `Malformed` — never parsed permissively.
fn observe_range(headers: &ureq::http::HeaderMap) -> RangeObservation {
    let mut values = headers.get_all("range").iter();
    let Some(value) = values.next() else {
        return RangeObservation::Absent;
    };
    if values.next().is_some() {
        return RangeObservation::Malformed;
    }
    let Ok(text) = value.to_str() else {
        return RangeObservation::Malformed;
    };
    let Some(span) = text.trim().strip_prefix("bytes=") else {
        return RangeObservation::Malformed;
    };
    let Some((start, end)) = span.split_once('-') else {
        return RangeObservation::Malformed;
    };
    if start != "0" || end.is_empty() || !end.bytes().all(|byte| byte.is_ascii_digit()) {
        return RangeObservation::Malformed;
    }
    match end.parse::<u64>() {
        Ok(end) => RangeObservation::Value { end },
        Err(_) => RangeObservation::Malformed,
    }
}

/// Which phase a body failure happened in, derived from the only fact that
/// distinguishes them: how many bytes actually reached the transport.
fn body_phase(bytes_written: u64, length: u64) -> Phase {
    if bytes_written == 0 {
        Phase::Connect
    } else if bytes_written < length {
        Phase::Request
    } else {
        // Every byte was written and no usable answer came back. This is the
        // ambiguous acknowledgement: the kernel resolves it by probing.
        Phase::Response
    }
}

/// Map a `ureq` outcome to a typed cause. `BlockedByHostPolicy` is deliberately
/// never produced: it is the browser's opaque platform refusal, and a native
/// client that emitted it would be inventing a diagnosis. `Offline` is likewise
/// never produced — `ds` has no offline switch to observe.
fn classify(error: &ureq::Error, hint: Phase) -> (Cause, Phase) {
    match error {
        ureq::Error::Timeout(timeout) => {
            let phase = match timeout {
                ureq::Timeout::Connect | ureq::Timeout::Resolve => Phase::Connect,
                ureq::Timeout::SendRequest | ureq::Timeout::SendBody => Phase::Request,
                ureq::Timeout::RecvResponse | ureq::Timeout::RecvBody => Phase::Response,
                _ => hint,
            };
            (Cause::Timeout { phase }, phase)
        }
        ureq::Error::HostNotFound => (Cause::Dns, Phase::Connect),
        ureq::Error::ConnectionFailed => (Cause::Connect, Phase::Connect),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => (Cause::Tls, Phase::Connect),
        // A resumable session with redirects disabled should never see these;
        // if one does, the session URI is not what it claimed to be.
        ureq::Error::RedirectFailed | ureq::Error::TooManyRedirects => {
            (Cause::Inconclusive, Phase::Response)
        }
        ureq::Error::Protocol(_) => (Cause::ConnectionClosed, Phase::Response),
        ureq::Error::Io(io_error) => match io_error.kind() {
            io::ErrorKind::TimedOut => (Cause::Timeout { phase: hint }, hint),
            io::ErrorKind::ConnectionRefused
            | io::ErrorKind::AddrNotAvailable
            | io::ErrorKind::NotConnected => (Cause::Connect, Phase::Connect),
            _ => (Cause::ConnectionClosed, hint),
        },
        _ => (Cause::Inconclusive, hint),
    }
}

fn classify_io(error: &io::Error) -> Cause {
    match error.kind() {
        io::ErrorKind::TimedOut => Cause::Timeout {
            phase: Phase::Connect,
        },
        _ => Cause::Inconclusive,
    }
}

/// Read the two facts a response carries, then drain a bounded amount of the
/// body so the connection can be reused. The body is never inspected: a storage
/// error body can echo the session URI.
fn observe(response: ureq::http::Response<ureq::Body>) -> (u16, RangeObservation) {
    let status = response.status().as_u16();
    let range = observe_range(response.headers());
    let reader = response.into_body().into_reader();
    let _ = io::copy(
        &mut reader.take(MAX_DISCARDED_RESPONSE_BYTES),
        &mut io::sink(),
    );
    (status, range)
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// Instrumentation the tests assert on. The memory bound is an invariant, so it
/// is measured rather than argued. Never reaches a caller or a receipt.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(not(any(test, feature = "weak-network-harness")), allow(dead_code))]
struct TransferStats {
    /// Largest slice ever handed to the socket, in bytes.
    widest_read: usize,
    /// Largest re-send window ever held, in bytes.
    widest_stage: usize,
    /// Requests this host dispatched.
    requests: u32,
}

/// What one transfer established. `status` is the last status the server
/// actually returned; nothing here is inferred.
///
/// `committed` and `total` are unread outside tests, and that is the honest
/// shape of the ceiling: `Transport::upload_bytes` returns a
/// `TransportResponse` and nothing else, so a partially committed prefix has
/// nowhere to go. Resume works fully *within* one call — the transfer probes
/// and picks up from the server's number — but the durable
/// `transfer::TransferState` cannot cross the call boundary, so a resume
/// across two `ds` invocations would need a `ds-client-core` change.
#[derive(Clone, Debug)]
#[cfg_attr(not(any(test, feature = "weak-network-harness")), allow(dead_code))]
struct UploadReport {
    done: bool,
    status: Option<u16>,
    cause: Option<Cause>,
    committed: u64,
    total: u64,
}

impl UploadReport {
    /// Translate into the `UploadBytesCall` contract, which is a status the
    /// core's `uploaded()` decoder reads. `Done` is reported as `200` because
    /// the kernel only reaches it when the server holds every byte; every other
    /// terminal answer surfaces the observed status, so a proven expiry stays a
    /// 404/410 and a rejected credential stays a 401/403.
    fn into_response(self) -> Result<TransportResponse, TransportError> {
        if self.done {
            return Ok(TransportResponse::new(200, Vec::new()));
        }
        match self.cause {
            Some(Cause::HttpStatus { status }) => Ok(TransportResponse::new(status, Vec::new())),
            // Only an observed 404/410 can produce this.
            Some(Cause::SessionExpired) => Ok(TransportResponse::new(
                self.status.unwrap_or(410),
                Vec::new(),
            )),
            Some(Cause::Timeout { .. }) => Err(TransportError::TimedOut),
            _ => Err(TransportError::Unreachable),
        }
    }
}

struct NativeUpload<'a> {
    agent: ureq::Agent,
    /// A credential. Never logged, never formatted into an error, never
    /// returned to a caller.
    session_uri: Zeroizing<String>,
    total_bytes: u64,
    source: ChunkSource<'a>,
    cancel: &'a dyn Fn() -> bool,
    /// The last status the server actually returned, if any.
    last_status: Option<u16>,
    widest_read: AtomicUsize,
    requests: u32,
}

fn transfer_agent(chunk_bytes: u64, origin: SessionOrigin) -> ureq::Agent {
    // A body budget derived from its own size: `chunk_bytes` at the slowest
    // link we will keep serving, plus a fixed grace. This is a no-progress
    // bound, not a deadline on the operation.
    let body_budget = Duration::from_secs(chunk_bytes / MIN_PROGRESS_BYTES_PER_SECOND)
        .saturating_add(TRANSFER_PROGRESS_GRACE);
    let per_call = TRANSFER_CONNECT_TIMEOUT
        .saturating_add(body_budget)
        .saturating_add(TRANSFER_ACK_TIMEOUT)
        .saturating_add(TRANSFER_DISCARD_BODY_TIMEOUT);
    ureq::Agent::config_builder()
        // MANDATORY. A 308 on a resumable session means "resume incomplete",
        // not "permanent redirect": following it would send the next chunk to
        // the wrong place and could move a credential to an unvalidated origin.
        .max_redirects(0)
        // MANDATORY. 4xx/5xx are protocol answers here — 404 is a proven
        // expiry, 401 is terminal, 500 is a bounded retry. The kernel decides
        // which; an Err would erase the status before it could.
        .http_status_as_error(false)
        .https_only(matches!(origin, SessionOrigin::Storage))
        .timeout_connect(Some(TRANSFER_CONNECT_TIMEOUT))
        .timeout_resolve(Some(TRANSFER_CONNECT_TIMEOUT))
        .timeout_send_request(Some(TRANSFER_CONNECT_TIMEOUT))
        .timeout_send_body(Some(body_budget))
        .timeout_recv_response(Some(TRANSFER_ACK_TIMEOUT))
        .timeout_recv_body(Some(TRANSFER_DISCARD_BODY_TIMEOUT))
        .timeout_per_call(Some(per_call))
        .build()
        .new_agent()
}

impl NativeUpload<'_> {
    /// Facts that make dispatch impossible, checked before a socket is opened.
    fn gate(&self) -> Option<Cause> {
        (self.cancel)().then_some(Cause::Cancelled)
    }

    /// Empty-body PUT with `Content-Range: bytes */total` — ask the session what
    /// it holds. The session URI is the entire credential, so nothing else is
    /// attached to this request.
    fn probe(&mut self) -> Event {
        if let Some(cause) = self.gate() {
            return Event::TransportFailure {
                cause,
                phase: Phase::Connect,
                bytes_written: 0,
            };
        }
        self.requests = self.requests.saturating_add(1);
        let response = self
            .agent
            .put(self.session_uri.as_str())
            .header("Content-Range", &format!("bytes */{}", self.total_bytes))
            .send_empty();
        match response {
            Ok(response) => {
                let (status, range) = observe(response);
                self.last_status = Some(status);
                Event::ProbeResponse { status, range }
            }
            Err(error) => {
                // The empty request was fully written, so anything that is not
                // a connect-class failure happened while awaiting the answer.
                let (cause, phase) = classify(&error, Phase::Response);
                Event::TransportFailure {
                    cause,
                    phase,
                    bytes_written: 0,
                }
            }
        }
    }

    fn send_chunk(&mut self, offset: u64, length: u64) -> Result<Event, String> {
        if let Some(cause) = self.gate() {
            return Ok(Event::TransportFailure {
                cause,
                phase: Phase::Connect,
                bytes_written: 0,
            });
        }
        let bytes = match self.source.chunk(offset, length) {
            Ok(bytes) => bytes,
            Err(SourceError::Rewind) => {
                return Err("the transfer asked for bytes behind this host's resend window".into());
            }
            Err(SourceError::Io(error)) => {
                return Ok(Event::TransportFailure {
                    cause: classify_io(&error),
                    phase: Phase::Connect,
                    bytes_written: 0,
                });
            }
        };
        let written = AtomicU64::new(0);
        let cancelled = AtomicBool::new(false);
        let last = offset.saturating_add(length).saturating_sub(1);
        let content_range = format!("bytes {offset}-{last}/{}", self.total_bytes);
        let content_length = length.to_string();
        self.requests = self.requests.saturating_add(1);
        let response = {
            let mut body = ChunkBody {
                bytes,
                position: 0,
                written: &written,
                cancel: self.cancel,
                cancelled: &cancelled,
                widest_read: &self.widest_read,
            };
            self.agent
                .put(self.session_uri.as_str())
                .header("Content-Range", &content_range)
                // An explicit content-length keeps the body length-delimited: a
                // resumable session must not receive a chunked body.
                .header("Content-Length", &content_length)
                .send(ureq::SendBody::from_reader(&mut body))
        };
        let bytes_written = written.load(Ordering::SeqCst);
        Ok(match response {
            Ok(response) => {
                let (status, range) = observe(response);
                self.last_status = Some(status);
                Event::ChunkResponse {
                    status,
                    range,
                    bytes_written,
                }
            }
            Err(error) => {
                if cancelled.load(Ordering::SeqCst) {
                    return Ok(Event::TransportFailure {
                        cause: Cause::Cancelled,
                        phase: body_phase(bytes_written, length),
                        bytes_written,
                    });
                }
                let (cause, phase) = classify(&error, body_phase(bytes_written, length));
                Event::TransportFailure {
                    cause,
                    phase,
                    bytes_written,
                }
            }
        })
    }

    /// Sleep out a bounded backoff, staying cancellable. `false` means the
    /// caller asked to stop.
    fn wait_until(&self, until_ms: u64) -> bool {
        loop {
            if (self.cancel)() {
                return false;
            }
            let now = now_ms();
            if now >= until_ms {
                return true;
            }
            let remaining = Duration::from_millis(until_ms - now);
            std::thread::sleep(remaining.min(CANCEL_POLL_INTERVAL));
        }
    }

    fn run(&mut self) -> Result<UploadReport, String> {
        let plan = TransferPlan {
            total_bytes: self.total_bytes,
            // The kernel owns the chunk size: it clamps and 256 KiB-aligns it.
            chunk_bytes: None,
        };
        let mut reply = kernel_step(None, Some(&plan), &Event::Start, now_ms())?;
        for _ in 0..MAX_TRANSFER_STEPS {
            match reply.action.clone() {
                Action::Done { committed } => {
                    return Ok(UploadReport {
                        done: true,
                        status: self.last_status,
                        cause: None,
                        committed,
                        total: reply.state.total_bytes,
                    });
                }
                // The CLI cannot mint a second session inside one `upload_bytes`
                // call: the session was issued by the core's `upload_start` and
                // the reopen budget lives with that caller. A proven expiry is
                // therefore reported as the status the server gave, which the
                // core's decoder turns into a transient failure.
                Action::Reopen { cause } | Action::Fail { cause, .. } => {
                    return Ok(UploadReport {
                        done: false,
                        status: self.last_status,
                        cause: Some(cause),
                        committed: reply.state.committed,
                        total: reply.state.total_bytes,
                    });
                }
                Action::Wait { until, .. } => {
                    if !self.wait_until(until) {
                        let stopped =
                            kernel_step(Some(&reply.state), None, &Event::Cancelled, now_ms())?;
                        reply = stopped;
                        continue;
                    }
                    // Re-entering is a restart of the step, not of the session:
                    // `Start` always asks the server what it holds first.
                    reply = kernel_step(Some(&reply.state), None, &Event::Start, now_ms())?;
                }
                Action::Probe => {
                    let event = self.probe();
                    reply = kernel_step(Some(&reply.state), None, &event, now_ms())?;
                }
                Action::SendChunk { offset, length } => {
                    let event = self.send_chunk(offset, length)?;
                    reply = kernel_step(Some(&reply.state), None, &event, now_ms())?;
                }
            }
        }
        Err("transfer exceeded its bounded step count".to_string())
    }

    fn stats(&self) -> TransferStats {
        TransferStats {
            widest_read: self.widest_read.load(Ordering::SeqCst),
            widest_stage: self.source.widest_stage,
            requests: self.requests,
        }
    }
}

/// Run one artifact's transfer to a terminal outcome.
fn drive(
    session_uri: String,
    origin: SessionOrigin,
    total_bytes: u64,
    reader: &mut dyn Read,
    cancel: &dyn Fn() -> bool,
) -> Result<(UploadReport, TransferStats), String> {
    if total_bytes == 0 {
        // The kernel refuses a zero-byte plan, and a resumable session has no
        // `Content-Range` that can express one. Refused here so the refusal is
        // this host's, stated once, rather than a kernel decode error.
        return Err("a resumable transfer needs at least one byte".to_string());
    }
    let mut native = NativeUpload {
        agent: transfer_agent(transfer::DEFAULT_CHUNK_BYTES, origin),
        session_uri: Zeroizing::new(session_uri),
        total_bytes,
        source: ChunkSource::new(reader),
        cancel,
        last_status: None,
        widest_read: AtomicUsize::new(0),
        requests: 0,
    };
    let report = native.run()?;
    Ok((report, native.stats()))
}

/// Transfer `total_bytes` from `reader` to a DS-minted storage session.
///
/// Nothing is dispatched until the session URI is proven to be one: a URI that
/// is not costs no DNS lookup and no byte.
pub(crate) fn transfer(
    session_uri: &str,
    origin: SessionOrigin,
    total_bytes: u64,
    reader: &mut dyn Read,
    cancel: &dyn Fn() -> bool,
) -> Result<TransportResponse, TransportError> {
    let session_uri =
        validated_session_uri(session_uri, origin).map_err(|_| TransportError::Unreachable)?;
    let (report, _) = drive(session_uri, origin, total_bytes, reader, cancel)
        .map_err(|_| TransportError::Unreachable)?;
    report.into_response()
}

/// Drive one already-declared Sync Center output through a DS-minted storage
/// session. The server producer owns the reader; this is the only native HTTP
/// path it may use for bytes, and the session URI never crosses back out.
pub fn transfer_sync_output(
    output_id: &str,
    session_uri: &str,
    total_bytes: u64,
    reader: &mut dyn Read,
) -> Result<ds_sync_runtime::TransferReceipt, String> {
    if output_id.is_empty() || output_id.len() > 256 || output_id.chars().any(char::is_control) {
        return Err("native Sync Center output identity is invalid".into());
    }
    let session_uri = validated_session_uri(session_uri, SessionOrigin::Storage)
        .map_err(|_| "native Sync Center storage session is invalid")?;
    let (report, _) = drive(
        session_uri,
        SessionOrigin::Storage,
        total_bytes,
        reader,
        &|| false,
    )
    .map_err(|_| "native Sync Center transfer did not reach a terminal outcome")?;
    Ok(ds_sync_runtime::TransferReceipt {
        output_id: output_id.to_owned(),
        outcome: if report.done {
            "completed"
        } else {
            "not_committed"
        }
        .into(),
        committed_bytes: report.committed,
        total_bytes: report.total,
    })
}

// ---------------------------------------------------------------------------
// Weak-network harness seam
// ---------------------------------------------------------------------------

/// The one seam an out-of-crate harness gets: it drives *this* host, unchanged,
/// against a scripted in-process session on loopback.
///
/// It adds no behaviour. [`drive_loopback`] validates the session URI through
/// the same [`validated_session_uri`] every transfer goes through, then calls
/// the same [`drive`] that `upload_bytes` calls; the only thing it does that
/// production cannot is name [`SessionOrigin::Loopback`], which is why the
/// whole module — and that variant — is behind the `weak-network-harness`
/// feature. Nothing but `crates/ds-cli-auth/tests/weak_network.rs` enables it,
/// dev-dependencies are not built for a release profile, and `cargo build -p
/// ds` therefore contains neither this module nor a loopback-accepting origin.
///
/// Why a feature rather than `cfg(test)`: an integration test links this crate
/// as an external one, so `cfg(test)` items are invisible to it. The
/// alternative — making `drive` public — would put a loopback-capable entry
/// point in every release build, which is the property the origin check exists
/// to hold.
#[cfg(feature = "weak-network-harness")]
pub mod weak_network_harness {
    use super::*;

    /// Everything one transfer established, flattened so a harness can assert
    /// on it without reaching further into the crate.
    #[derive(Clone, Debug)]
    pub struct Outcome {
        /// The server holds every byte.
        pub done: bool,
        /// The last status the server actually returned, if any.
        pub status: Option<u16>,
        /// The kernel's typed terminal cause, when the transfer did not finish.
        pub cause: Option<Cause>,
        /// Bytes the SERVER proved it holds.
        pub committed: u64,
        pub total: u64,
        /// Requests this host dispatched, probes included.
        pub requests: u32,
        /// Largest slice ever handed to the socket.
        pub widest_read: usize,
        /// Largest re-send window ever held.
        pub widest_stage: usize,
    }

    /// Run one transfer against a loopback session to a terminal outcome.
    pub fn drive_loopback(
        session_uri: &str,
        total_bytes: u64,
        reader: &mut dyn Read,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Outcome, String> {
        let session_uri = validated_session_uri(session_uri, SessionOrigin::Loopback)
            .map_err(|refusal| format!("harness session URI refused: {refusal:?}"))?;
        let (report, stats) = drive(
            session_uri,
            SessionOrigin::Loopback,
            total_bytes,
            reader,
            cancel,
        )?;
        Ok(Outcome {
            done: report.done,
            status: report.status,
            cause: report.cause,
            committed: report.committed,
            total: report.total,
            requests: stats.requests,
            widest_read: stats.widest_read,
            widest_stage: stats.widest_stage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Cursor, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    // -- scripted storage session ------------------------------------------

    #[derive(Clone, Debug)]
    enum Scripted {
        /// Answer with this status, and this exact `Range` header if given.
        Reply {
            status: u16,
            range: Option<String>,
            location: Option<String>,
        },
        /// Read the whole body, then drop the socket without answering — the
        /// ambiguous acknowledgement.
        DropAfterBody,
    }

    fn reply(status: u16) -> Scripted {
        Scripted::Reply {
            status,
            range: None,
            location: None,
        }
    }

    fn reply_range(status: u16, end: u64) -> Scripted {
        Scripted::Reply {
            status,
            range: Some(format!("bytes=0-{end}")),
            location: None,
        }
    }

    #[derive(Clone, Debug)]
    struct Recorded {
        method: String,
        content_range: String,
        body_len: u64,
        body_sha_prefix: u8,
        header_names: Vec<String>,
    }

    struct ScriptedSession {
        port: u16,
        recorded: Arc<Mutex<Vec<Recorded>>>,
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    impl ScriptedSession {
        fn start(script: Vec<Scripted>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
            let port = listener.local_addr().expect("local addr").port();
            listener.set_nonblocking(true).expect("nonblocking");
            let recorded = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let worker = {
                let recorded = recorded.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let mut remaining = script.into_iter();
                    while !stop.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let step = remaining.next();
                                serve(stream, step, &recorded);
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err(_) => break,
                        }
                    }
                })
            };
            ScriptedSession {
                port,
                recorded,
                stop,
                worker: Some(worker),
            }
        }

        fn uri(&self) -> String {
            format!("http://127.0.0.1:{}/resumable/session", self.port)
        }

        fn requests(&self) -> Vec<Recorded> {
            self.recorded.lock().expect("recorded").clone()
        }
    }

    impl Drop for ScriptedSession {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn serve(stream: TcpStream, step: Option<Scripted>, recorded: &Arc<Mutex<Vec<Recorded>>>) {
        stream.set_nonblocking(false).expect("blocking stream");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("read timeout");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
            return;
        }
        let method = request_line
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        let mut content_length = 0_u64;
        let mut content_range = String::new();
        let mut header_names = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                return;
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            let Some((name, value)) = trimmed.split_once(':') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            if name == "content-range" {
                content_range = value.clone();
            }
            header_names.push(name);
        }
        let mut body = Vec::new();
        let mut sink = reader.take(content_length);
        let body_len = io::copy(&mut sink, &mut body).unwrap_or(0);
        recorded.lock().expect("recorded").push(Recorded {
            method,
            content_range,
            body_len,
            body_sha_prefix: body.first().copied().unwrap_or(0),
            header_names,
        });
        let Some(step) = step else { return };
        let mut stream = stream;
        match step {
            Scripted::DropAfterBody => {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            Scripted::Reply {
                status,
                range,
                location,
            } => {
                let mut response =
                    format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n");
                if let Some(range) = range {
                    response.push_str(&format!("Range: {range}\r\n"));
                }
                if let Some(location) = location {
                    response.push_str(&format!("Location: {location}\r\n"));
                }
                response.push_str("\r\n");
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    // -- fixtures -----------------------------------------------------------

    const KIB: u64 = 1024;

    /// A deterministic artifact whose every byte states its own offset mod 251,
    /// so a test can tell WHICH bytes a chunk carried, not just how many.
    fn artifact(size: u64) -> Vec<u8> {
        (0..size).map(|index| (index % 251) as u8).collect()
    }

    struct Run {
        report: UploadReport,
        stats: TransferStats,
    }

    fn run_transfer(session: &ScriptedSession, bytes: &[u8], cancel: &dyn Fn() -> bool) -> Run {
        let uri = validated_session_uri(&session.uri(), SessionOrigin::Loopback)
            .expect("loopback session is a valid test origin");
        let mut reader = Cursor::new(bytes);
        let (report, stats) = drive(
            uri,
            SessionOrigin::Loopback,
            bytes.len() as u64,
            &mut reader,
            cancel,
        )
        .expect("transfer drives to a terminal outcome");
        Run { report, stats }
    }

    fn never_cancelled() -> impl Fn() -> bool {
        || false
    }

    // -- session URI validation --------------------------------------------

    #[test]
    fn only_a_ds_minted_storage_session_is_accepted() {
        let good = "https://storage.googleapis.com/upload/storage/v1/b/ds/o?uploadType=resumable&upload_id=abc";
        assert!(validated_session_uri(good, SessionOrigin::Storage).is_ok());
        assert!(
            validated_session_uri(
                "https://storage.googleapis.com:443/upload?upload_id=abc",
                SessionOrigin::Storage
            )
            .is_ok()
        );

        for (raw, expected) in [
            (
                "http://storage.googleapis.com/upload",
                SessionRefusal::SchemeNotHttps,
            ),
            (
                "https://user:secret@storage.googleapis.com/upload",
                SessionRefusal::CarriesUserinfo,
            ),
            (
                "https://storage.googleapis.com.attacker.example/upload",
                SessionRefusal::HostNotAllowed,
            ),
            (
                "https://attacker.example/storage.googleapis.com",
                SessionRefusal::HostNotAllowed,
            ),
            (
                "https://storage.googleapis.com:8443/upload",
                SessionRefusal::NonDefaultPort,
            ),
            ("not a uri", SessionRefusal::Unparsable),
            ("", SessionRefusal::Unparsable),
        ] {
            assert_eq!(
                validated_session_uri(raw, SessionOrigin::Storage),
                Err(expected),
                "{raw} must be refused"
            );
        }
        // The loopback origin the tests use is itself refused in production.
        assert_eq!(
            validated_session_uri("http://127.0.0.1:9/session", SessionOrigin::Storage),
            Err(SessionRefusal::SchemeNotHttps)
        );
        assert_eq!(
            validated_session_uri(
                &"https://storage.googleapis.com/".repeat(200),
                SessionOrigin::Storage
            ),
            Err(SessionRefusal::TooLong)
        );
    }

    #[test]
    fn an_unvalidated_session_costs_no_byte_and_no_lookup() {
        let bytes = artifact(4 * KIB);
        let mut reader = Cursor::new(&bytes[..]);
        let outcome = transfer(
            "https://attacker.example/upload",
            SessionOrigin::Storage,
            bytes.len() as u64,
            &mut reader,
            &never_cancelled(),
        );
        assert!(matches!(outcome, Err(TransportError::Unreachable)));
        // Nothing was read from the artifact: the refusal precedes every step.
        assert_eq!(reader.position(), 0);
    }

    // -- range observation --------------------------------------------------

    #[test]
    fn a_range_header_is_parsed_strictly_or_reported_malformed() {
        fn observed(raw: Option<&str>) -> RangeObservation {
            let mut headers = ureq::http::HeaderMap::new();
            if let Some(raw) = raw {
                headers.insert("range", raw.parse().expect("header value"));
            }
            observe_range(&headers)
        }
        assert_eq!(observed(None), RangeObservation::Absent);
        assert_eq!(
            observed(Some("bytes=0-262143")),
            RangeObservation::Value { end: 262_143 }
        );
        for malformed in [
            "bytes=1-99",
            "bytes 0-99",
            "bytes=0-",
            "bytes=0-abc",
            "0-99",
            "",
            "bytes=0-99, bytes=0-100",
        ] {
            assert_eq!(
                observed(Some(malformed)),
                RangeObservation::Malformed,
                "{malformed} must be malformed, never guessed"
            );
        }
        // Never `Unobservable`: this host can always read headers.
        assert_ne!(observed(None), RangeObservation::Unobservable);
    }

    // -- protocol boundaries ------------------------------------------------

    #[test]
    fn a_transfer_probes_before_it_writes_a_byte() {
        let bytes = artifact(512 * KIB);
        let session = ScriptedSession::start(vec![reply(308), reply(200)]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        let requests = session.requests();
        assert_eq!(requests.len(), 2);
        // The very first request is the empty-body status query the old
        // single-shot PUT never made.
        assert_eq!(requests[0].method, "PUT");
        assert_eq!(requests[0].content_range, "bytes */524288");
        assert_eq!(requests[0].body_len, 0);
        assert_eq!(requests[1].content_range, "bytes 0-524287/524288");
        assert_eq!(requests[1].body_len, 512 * KIB);
    }

    #[test]
    fn a_308_with_a_range_header_resumes_from_the_next_byte() {
        let bytes = artifact(768 * KIB);
        let session = ScriptedSession::start(vec![
            // The session already holds the first 256 KiB from an earlier run.
            reply_range(308, 256 * KIB - 1),
            reply_range(308, 512 * KIB - 1),
            reply(200),
        ]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        assert_eq!(run.report.committed, 768 * KIB);
        let requests = session.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].content_range, "bytes */786432");
        assert_eq!(requests[1].content_range, "bytes 262144-786431/786432");
        // The forward skip landed on the right byte, not merely the right count.
        assert_eq!(requests[1].body_sha_prefix, bytes[262_144]);
        assert_eq!(requests[2].content_range, "bytes 524288-786431/786432");
        assert_eq!(requests[2].body_sha_prefix, bytes[524_288]);
    }

    #[test]
    fn a_308_without_a_range_header_means_zero_committed() {
        let bytes = artifact(512 * KIB);
        let session = ScriptedSession::start(vec![reply(308), reply(200)]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        let requests = session.requests();
        assert_eq!(requests.len(), 2);
        // ZERO committed is a fact, not an ambiguity: send from byte 0.
        assert_eq!(requests[1].content_range, "bytes 0-524287/524288");
        assert_eq!(requests[1].body_sha_prefix, bytes[0]);
    }

    #[test]
    fn an_already_complete_object_is_finalized_without_resending() {
        for status in [200, 201] {
            let bytes = artifact(256 * KIB);
            let session = ScriptedSession::start(vec![reply(status)]);
            let run = run_transfer(&session, &bytes, &never_cancelled());
            assert!(run.report.done);
            assert_eq!(run.report.committed, 256 * KIB);
            assert_eq!(session.requests().len(), 1, "{status} must not re-send");
            assert_eq!(session.requests()[0].body_len, 0);
            assert_eq!(run.report.into_response().unwrap().status, 200);
        }
    }

    #[test]
    fn a_proven_expired_session_surfaces_its_status_and_never_restarts() {
        for status in [404, 410] {
            let bytes = artifact(256 * KIB);
            let session = ScriptedSession::start(vec![reply(status)]);
            let run = run_transfer(&session, &bytes, &never_cancelled());
            assert!(!run.report.done);
            assert_eq!(run.report.cause, Some(Cause::SessionExpired));
            // Nothing was committed and nothing was re-sent: the session is
            // reported gone, and the caller owns the reopen decision.
            assert_eq!(run.report.committed, 0);
            assert_eq!(run.report.total, 256 * KIB);
            assert_eq!(session.requests().len(), 1);
            assert_eq!(run.report.into_response().unwrap().status, status);
        }
    }

    #[test]
    fn a_rejected_session_credential_is_terminal_and_not_retried() {
        for status in [401, 403] {
            let bytes = artifact(256 * KIB);
            let session = ScriptedSession::start(vec![reply(status)]);
            let run = run_transfer(&session, &bytes, &never_cancelled());
            assert_eq!(run.report.cause, Some(Cause::HttpStatus { status }));
            assert_eq!(session.requests().len(), 1);
            assert_eq!(run.report.into_response().unwrap().status, status);
        }
    }

    #[test]
    fn a_server_error_backs_off_and_then_completes() {
        let bytes = artifact(256 * KIB);
        let session = ScriptedSession::start(vec![reply(500), reply(308), reply(201)]);
        let started = std::time::Instant::now();
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        // Bounded backoff, actually slept — not a hot loop.
        assert!(started.elapsed() >= Duration::from_millis(900));
        let requests = session.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1].content_range, "bytes */262144");
        assert_eq!(requests[2].content_range, "bytes 0-262143/262144");
    }

    #[test]
    fn a_malformed_range_is_inconclusive_and_never_restarts_the_session() {
        let bytes = artifact(256 * KIB);
        let session = ScriptedSession::start(vec![
            Scripted::Reply {
                status: 308,
                range: Some("bytes 12-97".to_string()),
                location: None,
            },
            reply_range(308, 128 * KIB - 1),
            reply(200),
        ]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        let requests = session.requests();
        // The malformed answer produced another PROBE, never a re-send from 0.
        assert_eq!(requests[0].content_range, "bytes */262144");
        assert_eq!(requests[1].content_range, "bytes */262144");
        assert_eq!(requests[1].body_len, 0);
        assert_eq!(requests[2].content_range, "bytes 131072-262143/262144");
    }

    #[test]
    fn an_ambiguous_acknowledgement_probes_instead_of_restarting() {
        let bytes = artifact(512 * KIB);
        let session = ScriptedSession::start(vec![
            reply(308),
            Scripted::DropAfterBody,
            reply_range(308, 512 * KIB - 1),
        ]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        let requests = session.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1].content_range, "bytes 0-524287/524288");
        assert_eq!(requests[1].body_len, 512 * KIB);
        // The bytes were written but never acknowledged: the next move is a
        // status query, not a re-send and not a failure.
        assert_eq!(requests[2].content_range, "bytes */524288");
        assert_eq!(requests[2].body_len, 0);
    }

    #[test]
    fn a_shorter_committed_prefix_resumes_from_the_servers_number() {
        let bytes = artifact(768 * KIB);
        let session = ScriptedSession::start(vec![
            reply(308),
            // The host wrote the whole first chunk; the server took half of it.
            reply_range(308, 128 * KIB - 1),
            reply(200),
        ]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        let requests = session.requests();
        assert_eq!(requests[1].content_range, "bytes 0-786431/786432");
        // Resumed from the SERVER's number, from bytes this host still held —
        // the reader is forward-only and was never re-read.
        assert_eq!(requests[2].content_range, "bytes 131072-786431/786432");
        assert_eq!(requests[2].body_sha_prefix, bytes[131_072]);
    }

    #[test]
    fn a_redirect_is_read_as_resume_incomplete_and_never_followed() {
        let bytes = artifact(256 * KIB);
        let session = ScriptedSession::start(vec![
            Scripted::Reply {
                status: 308,
                range: None,
                location: Some("http://127.0.0.1:9/steal".to_string()),
            },
            reply(200),
        ]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        let requests = session.requests();
        // Two requests, both to the scripted session: the Location was data,
        // not an instruction, and the bytes went nowhere else.
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].content_range, "bytes 0-262143/262144");
    }

    #[test]
    fn a_storage_session_carries_no_ds_credential() {
        let bytes = artifact(256 * KIB);
        let session = ScriptedSession::start(vec![reply(308), reply(200)]);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        for request in session.requests() {
            assert_eq!(request.method, "PUT");
            for forbidden in [
                "authorization",
                "x-forwarded-authorization",
                "x-api-key",
                "x-user-email",
                "x-app-id",
                "x-ds-processing-lane",
                "cookie",
            ] {
                assert!(
                    !request.header_names.iter().any(|name| name == forbidden),
                    "{forbidden} must never reach a storage session"
                );
            }
        }
    }

    #[test]
    fn a_transfer_past_the_old_whole_file_read_stays_in_bounded_memory() {
        let total = 24 * 1024 * KIB;
        let bytes = artifact(total);
        let chunk = transfer::DEFAULT_CHUNK_BYTES;
        let mut script = vec![reply(308)];
        let mut sent = chunk;
        while sent < total {
            script.push(reply_range(308, sent - 1));
            sent += chunk;
        }
        script.push(reply(200));
        let session = ScriptedSession::start(script);
        let run = run_transfer(&session, &bytes, &never_cancelled());
        assert!(run.report.done);
        assert_eq!(run.report.committed, total);
        assert_eq!(run.report.total, total);
        // Peak resident transfer memory is one chunk plus one socket slice —
        // independent of the artifact, which the old single-shot PUT streamed
        // under one 1800-second deadline with no way to resume.
        assert!(run.stats.widest_read <= MAX_BODY_READ_BYTES);
        assert!((run.stats.widest_stage as u64) <= chunk);
        assert!((run.stats.widest_stage as u64) < total);
        assert!((chunk) <= transfer::MAX_CHUNK_BYTES);
        assert_eq!(run.stats.requests, 1 + (total / chunk) as u32);
    }

    #[test]
    fn cancellation_during_backoff_is_honoured() {
        let bytes = artifact(256 * KIB);
        let session = ScriptedSession::start(vec![reply(503)]);
        let armed = Arc::new(AtomicBool::new(false));
        let flag = armed.clone();
        let cancel = move || {
            let already = flag.load(Ordering::SeqCst);
            flag.store(true, Ordering::SeqCst);
            already
        };
        let started = std::time::Instant::now();
        let run = run_transfer(&session, &bytes, &cancel);
        assert_eq!(run.report.cause, Some(Cause::Cancelled));
        // Stopped inside the backoff rather than sleeping it out.
        assert!(started.elapsed() < Duration::from_millis(900));
        assert_eq!(session.requests().len(), 1);
        assert!(matches!(
            run.report.into_response(),
            Err(TransportError::Unreachable)
        ));
    }

    #[test]
    fn cancellation_before_dispatch_writes_nothing() {
        let bytes = artifact(256 * KIB);
        let session = ScriptedSession::start(vec![reply(308), reply(200)]);
        let run = run_transfer(&session, &bytes, &|| true);
        assert_eq!(run.report.cause, Some(Cause::Cancelled));
        assert!(session.requests().is_empty());
    }

    // -- host bounds ---------------------------------------------------------

    #[test]
    fn a_zero_byte_transfer_is_refused_by_this_host() {
        let mut reader = Cursor::new(Vec::new());
        assert!(
            drive(
                "http://127.0.0.1:9/session".to_string(),
                SessionOrigin::Loopback,
                0,
                &mut reader,
                &never_cancelled(),
            )
            .is_err()
        );
    }

    #[test]
    fn the_socket_slice_never_exceeds_the_kernels_chunk_bound() {
        assert!((MAX_BODY_READ_BYTES as u64) <= transfer::MAX_CHUNK_BYTES);
        assert!((MAX_BODY_READ_BYTES as u64) <= transfer::CHUNK_ALIGNMENT_BYTES);
    }

    #[test]
    fn a_forward_only_source_serves_skips_resends_and_truncations() {
        let bytes = artifact(1024);
        let mut reader = Cursor::new(&bytes[..]);
        let mut source = ChunkSource::new(&mut reader);
        // Resume: skip forward to the server's committed prefix.
        assert_eq!(source.chunk(256, 256).unwrap(), &bytes[256..512]);
        // Re-send the same chunk: served from the window, reader untouched.
        assert_eq!(source.chunk(256, 256).unwrap(), &bytes[256..512]);
        // Truncated commit: resume mid-window.
        assert_eq!(source.chunk(384, 256).unwrap(), &bytes[384..640]);
        // Forward again.
        assert_eq!(source.chunk(640, 128).unwrap(), &bytes[640..768]);
        // Behind the window is a reported refusal, never a silent re-read.
        assert!(matches!(source.chunk(0, 16), Err(SourceError::Rewind)));
    }
}
