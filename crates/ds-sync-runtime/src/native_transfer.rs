//! Native storage-session byte transport shared by Desktop and headless hosts.
//!
//! A storage session URI is a credential. This module pins it before opening a
//! socket, sends no control-plane credentials, refuses redirects, and lets the
//! kernel own resumable state, retries, and terminal classification. Callers
//! supply an already receipt-sealed, seekable handle; the declared digest is
//! re-proved while bytes leave the process.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ds_command_kernel::transfer::{
    self, Action, Cause, Event, Phase, RangeObservation, Stage, TransferPlan, TransferState,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::rows::TransferReceipt;

const ALLOWED_STORAGE_SESSION_HOSTS: &[&str] = &["storage.googleapis.com"];
const MAX_SESSION_URI_BYTES: usize = 4096;
const MAX_BODY_READ_BYTES: usize = 64 * 1024;
const MAX_DISCARDED_RESPONSE_BYTES: u64 = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const ACK_TIMEOUT: Duration = Duration::from_secs(180);
const DISCARD_TIMEOUT: Duration = Duration::from_secs(30);
const MIN_PROGRESS_BYTES_PER_SECOND: u64 = 4 * 1024;
const PROGRESS_GRACE: Duration = Duration::from_secs(60);
const CANCEL_POLL: Duration = Duration::from_millis(50);
const MAX_STEPS: u32 = 200_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRefusal {
    Unparsable,
    TooLong,
    SchemeNotHttps,
    CarriesUserinfo,
    HostNotAllowed,
    NonDefaultPort,
}

/// Terminal result of one resumable native transfer. The kernel owns the
/// meaning of its state and cause; this host reports only observed facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTransferOutcome {
    Done,
    Reopen,
    Failed,
}

#[derive(Clone, Debug)]
pub struct NativeTransferResult {
    pub outcome: NativeTransferOutcome,
    pub cause: Option<Cause>,
    pub retryable: bool,
    pub committed_bytes: u64,
    pub total_bytes: u64,
    pub state: TransferState,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeTransferProgress {
    pub stage: Stage,
    pub committed_bytes: u64,
    pub sent_bytes: u64,
    pub total_bytes: u64,
}

/// Validate the DS-minted resumable-session credential before DNS or I/O.
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
    if !ALLOWED_STORAGE_SESSION_HOSTS.contains(&host) {
        return Err(SessionRefusal::HostNotAllowed);
    }
    if url.port_or_known_default() != Some(443) {
        return Err(SessionRefusal::NonDefaultPort);
    }
    Ok(url.to_string())
}

#[derive(Debug, Deserialize)]
struct KernelReply {
    state: TransferState,
    action: Action,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| {
            value.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

fn kernel_step(
    state: Option<&TransferState>,
    plan: Option<&TransferPlan>,
    event: &Event,
) -> Result<KernelReply, String> {
    let request = serde_json::json!({
        "schema": transfer::TRANSFER_SCHEMA,
        "now": now_ms(),
        "state": state,
        "plan": plan,
        "event": event,
    });
    let encoded = serde_json::to_vec(&request)
        .map_err(|error| format!("could not encode a transfer step: {error}"))?;
    let reply = transfer::evaluate(&encoded)?;
    serde_json::from_str(&reply).map_err(|error| format!("could not read a transfer step: {error}"))
}

struct DigestState {
    hasher: Sha256,
    position: u64,
    contiguous: bool,
}

impl DigestState {
    fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            position: 0,
            contiguous: true,
        }
    }
    fn absorb(&mut self, start: u64, bytes: &[u8]) {
        let end = start.saturating_add(bytes.len() as u64);
        if start > self.position {
            self.contiguous = false;
            return;
        }
        if end <= self.position {
            return;
        }
        let skip = (self.position - start) as usize;
        self.hasher.update(&bytes[skip..]);
        self.position = end;
    }
    fn complete(&self, total: u64) -> bool {
        self.contiguous && self.position == total
    }
}

struct ChunkBody<'a, R: Read> {
    reader: &'a mut R,
    digest: &'a mut DigestState,
    position: u64,
    remaining: u64,
    written: &'a AtomicU64,
    cancel: &'a dyn Fn() -> bool,
    cancelled: &'a AtomicBool,
    widest_read: &'a AtomicUsize,
}

impl<R: Read> Read for ChunkBody<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if (self.cancel)() {
            self.cancelled.store(true, Ordering::SeqCst);
            return Err(io::Error::other("transfer cancelled"));
        }
        if self.remaining == 0 || buffer.is_empty() {
            return Ok(0);
        }
        let want = buffer
            .len()
            .min(MAX_BODY_READ_BYTES)
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        self.widest_read.fetch_max(want, Ordering::SeqCst);
        let read = self.reader.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "sealed artifact ended before its receipt size",
            ));
        }
        self.digest.absorb(self.position, &buffer[..read]);
        self.position = self.position.saturating_add(read as u64);
        self.remaining = self.remaining.saturating_sub(read as u64);
        self.written.fetch_add(read as u64, Ordering::SeqCst);
        Ok(read)
    }
}

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
    if start != "0" || end.is_empty() || !end.bytes().all(|value| value.is_ascii_digit()) {
        return RangeObservation::Malformed;
    }
    end.parse()
        .map(|end| RangeObservation::Value { end })
        .unwrap_or(RangeObservation::Malformed)
}

fn body_phase(written: u64, length: u64) -> Phase {
    if written == 0 {
        Phase::Connect
    } else if written < length {
        Phase::Request
    } else {
        Phase::Response
    }
}

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
        ureq::Error::RedirectFailed | ureq::Error::TooManyRedirects => {
            (Cause::Inconclusive, Phase::Response)
        }
        ureq::Error::Protocol(_) => (Cause::ConnectionClosed, Phase::Response),
        ureq::Error::Io(error) => match error.kind() {
            io::ErrorKind::TimedOut => (Cause::Timeout { phase: hint }, hint),
            io::ErrorKind::ConnectionRefused
            | io::ErrorKind::AddrNotAvailable
            | io::ErrorKind::NotConnected => (Cause::Connect, Phase::Connect),
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe => (Cause::ConnectionClosed, hint),
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

fn observe(response: ureq::http::Response<ureq::Body>) -> (u16, RangeObservation) {
    let status = response.status().as_u16();
    let range = observe_range(response.headers());
    let mut body = response.into_body().into_reader();
    let _ = io::copy(
        &mut body.by_ref().take(MAX_DISCARDED_RESPONSE_BYTES),
        &mut io::sink(),
    );
    (status, range)
}

fn transfer_agent(chunk_bytes: u64) -> ureq::Agent {
    transfer_agent_for_scheme(chunk_bytes, true)
}

fn transfer_agent_for_scheme(chunk_bytes: u64, https_only: bool) -> ureq::Agent {
    let body_budget = Duration::from_secs(chunk_bytes / MIN_PROGRESS_BYTES_PER_SECOND)
        .saturating_add(PROGRESS_GRACE);
    let per_call = CONNECT_TIMEOUT
        .saturating_add(body_budget)
        .saturating_add(ACK_TIMEOUT)
        .saturating_add(DISCARD_TIMEOUT);
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .https_only(https_only)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_resolve(Some(CONNECT_TIMEOUT))
        .timeout_send_request(Some(CONNECT_TIMEOUT))
        .timeout_send_body(Some(body_budget))
        .timeout_recv_response(Some(ACK_TIMEOUT))
        .timeout_recv_body(Some(DISCARD_TIMEOUT))
        .timeout_per_call(Some(per_call))
        .build()
        .new_agent()
}

struct Driver<'a, R: Read + Seek> {
    agent: ureq::Agent,
    session: String,
    reader: R,
    total: u64,
    expected_sha256: &'a str,
    chunk_bytes: Option<u64>,
    digest: DigestState,
    cancel: &'a dyn Fn() -> bool,
    progress: &'a mut dyn FnMut(NativeTransferProgress),
    widest_read: AtomicUsize,
}

impl<R: Read + Seek> Driver<'_, R> {
    fn gate(&self) -> Option<Cause> {
        (self.cancel)().then_some(Cause::Cancelled)
    }
    fn probe(&mut self) -> Event {
        if let Some(cause) = self.gate() {
            return Event::TransportFailure {
                cause,
                phase: Phase::Connect,
                bytes_written: 0,
            };
        }
        match self
            .agent
            .put(&self.session)
            .header("Content-Range", format!("bytes */{}", self.total))
            .send_empty()
        {
            Ok(response) => {
                let (status, range) = observe(response);
                Event::ProbeResponse { status, range }
            }
            Err(error) => {
                let (cause, phase) = classify(&error, Phase::Response);
                Event::TransportFailure {
                    cause,
                    phase,
                    bytes_written: 0,
                }
            }
        }
    }
    fn send_chunk(&mut self, offset: u64, length: u64) -> Event {
        if let Some(cause) = self.gate() {
            return Event::TransportFailure {
                cause,
                phase: Phase::Connect,
                bytes_written: 0,
            };
        }
        if let Err(error) = self.reader.seek(SeekFrom::Start(offset)) {
            return Event::TransportFailure {
                cause: classify_io(&error),
                phase: Phase::Connect,
                bytes_written: 0,
            };
        }
        let written = AtomicU64::new(0);
        let cancelled = AtomicBool::new(false);
        let last = offset.saturating_add(length).saturating_sub(1);
        let response = {
            let mut body = ChunkBody {
                reader: &mut self.reader,
                digest: &mut self.digest,
                position: offset,
                remaining: length,
                written: &written,
                cancel: self.cancel,
                cancelled: &cancelled,
                widest_read: &self.widest_read,
            };
            self.agent
                .put(&self.session)
                .header(
                    "Content-Range",
                    format!("bytes {offset}-{last}/{}", self.total),
                )
                .header("Content-Length", length.to_string())
                .send(ureq::SendBody::from_reader(&mut body))
        };
        let bytes_written = written.load(Ordering::SeqCst);
        match response {
            Ok(response) => {
                let (status, range) = observe(response);
                Event::ChunkResponse {
                    status,
                    range,
                    bytes_written,
                }
            }
            Err(_error) if cancelled.load(Ordering::SeqCst) => Event::TransportFailure {
                cause: Cause::Cancelled,
                phase: body_phase(bytes_written, length),
                bytes_written,
            },
            Err(error) => {
                let (cause, phase) = classify(&error, body_phase(bytes_written, length));
                Event::TransportFailure {
                    cause,
                    phase,
                    bytes_written,
                }
            }
        }
    }
    fn digest_matches(&mut self) -> Result<Option<bool>, String> {
        if self.digest.complete(self.total) {
            return Ok(Some(
                format!("{:x}", self.digest.hasher.clone().finalize()) == self.expected_sha256,
            ));
        }
        self.reader
            .seek(SeekFrom::Start(0))
            .map_err(|error| format!("could not rewind sealed artifact: {error}"))?;
        let mut hash = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = vec![0_u8; MAX_BODY_READ_BYTES];
        loop {
            if (self.cancel)() {
                return Ok(None);
            }
            let read = self
                .reader
                .read(&mut buffer)
                .map_err(|error| format!("could not read sealed artifact: {error}"))?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as u64);
            if total > self.total {
                return Ok(Some(false));
            }
            hash.update(&buffer[..read]);
        }
        Ok(Some(
            total == self.total && format!("{:x}", hash.finalize()) == self.expected_sha256,
        ))
    }
    fn advance(&mut self, before: &TransferState, event: Event) -> Result<KernelReply, String> {
        let after = kernel_step(Some(before), None, &event)?;
        if !matches!(after.action, Action::Done { .. }) {
            return Ok(after);
        }
        match self.digest_matches()? {
            Some(true) => Ok(after),
            Some(false) => kernel_step(
                Some(before),
                None,
                &Event::IntegrityFailure {
                    cause: Cause::DigestMismatch,
                },
            ),
            None => kernel_step(Some(before), None, &Event::Cancelled),
        }
    }
    fn wait_until(&self, until: u64) -> bool {
        loop {
            if (self.cancel)() {
                return false;
            }
            let now = now_ms();
            if now >= until {
                return true;
            }
            std::thread::sleep(Duration::from_millis(until - now).min(CANCEL_POLL));
        }
    }
    fn run(&mut self, state: Option<TransferState>) -> Result<NativeTransferResult, String> {
        let plan = TransferPlan {
            total_bytes: self.total,
            chunk_bytes: self.chunk_bytes,
        };
        let mut reply = kernel_step(state.as_ref(), Some(&plan), &Event::Start)?;
        for _ in 0..MAX_STEPS {
            (self.progress)(NativeTransferProgress {
                stage: reply.state.stage,
                committed_bytes: reply.state.committed,
                sent_bytes: reply.state.sent,
                total_bytes: reply.state.total_bytes,
            });
            match reply.action.clone() {
                Action::Done { committed } => {
                    return Ok(NativeTransferResult {
                        outcome: NativeTransferOutcome::Done,
                        cause: None,
                        retryable: false,
                        committed_bytes: committed,
                        total_bytes: reply.state.total_bytes,
                        state: reply.state,
                    })
                }
                Action::Reopen { cause } => {
                    return Ok(NativeTransferResult {
                        outcome: NativeTransferOutcome::Reopen,
                        cause: Some(cause),
                        retryable: true,
                        committed_bytes: reply.state.committed,
                        total_bytes: reply.state.total_bytes,
                        state: reply.state,
                    })
                }
                Action::Fail { cause, retryable } => {
                    return Ok(NativeTransferResult {
                        outcome: NativeTransferOutcome::Failed,
                        cause: Some(cause),
                        retryable,
                        committed_bytes: reply.state.committed,
                        total_bytes: reply.state.total_bytes,
                        state: reply.state,
                    })
                }
                Action::Wait { until, .. } => {
                    if !self.wait_until(until) {
                        reply = kernel_step(Some(&reply.state), None, &Event::Cancelled)?;
                        continue;
                    }
                    reply = kernel_step(Some(&reply.state), None, &Event::Start)?;
                }
                Action::Probe => {
                    let before = reply.state.clone();
                    let event = self.probe();
                    reply = self.advance(&before, event)?;
                }
                Action::SendChunk { offset, length } => {
                    let before = reply.state.clone();
                    let event = self.send_chunk(offset, length);
                    reply = self.advance(&before, event)?;
                }
            }
        }
        Err("transfer exceeded its bounded step count".into())
    }
}

/// Drive one receipt-sealed file through a DS-minted storage session. This is
/// the sole native transfer driver used by Desktop Sync Center and Server.
#[allow(clippy::too_many_arguments)]
pub fn drive_verified_output<R: Read + Seek>(
    session_uri: &str,
    total_bytes: u64,
    expected_sha256: &str,
    reader: R,
    state: Option<TransferState>,
    chunk_bytes: Option<u64>,
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(NativeTransferProgress),
) -> Result<NativeTransferResult, String> {
    if total_bytes == 0
        || expected_sha256.len() != 64
        || !expected_sha256
            .bytes()
            .all(|value| value.is_ascii_digit() || matches!(value, b'a'..=b'f'))
    {
        return Err("sealed Sync Center output declaration is invalid".into());
    }
    if let Some(existing) = state.as_ref() {
        if existing.total_bytes != total_bytes {
            return Err("durable transfer state does not describe this artifact".into());
        }
    }
    let session = validated_storage_session(session_uri)
        .map_err(|_| "native Sync Center storage session is invalid")?;
    let budget = chunk_bytes
        .unwrap_or(transfer::DEFAULT_CHUNK_BYTES)
        .clamp(transfer::CHUNK_ALIGNMENT_BYTES, transfer::MAX_CHUNK_BYTES);
    Driver {
        agent: transfer_agent(budget),
        session,
        reader,
        total: total_bytes,
        expected_sha256,
        chunk_bytes,
        digest: DigestState::new(),
        cancel,
        progress,
        widest_read: AtomicUsize::new(0),
    }
    .run(state)
}

/// Test-only real TCP driver for host integration proofs. This is compiled
/// only through the existing `fakes` feature and accepts no production storage
/// authority: plain HTTP is restricted to a numeric loopback endpoint.
#[cfg(feature = "fakes")]
#[allow(clippy::too_many_arguments)]
pub fn drive_loopback_output<R: Read + Seek>(
    session_uri: &str,
    total_bytes: u64,
    expected_sha256: &str,
    reader: R,
    state: Option<TransferState>,
    chunk_bytes: Option<u64>,
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(NativeTransferProgress),
) -> Result<NativeTransferResult, String> {
    let url = url::Url::parse(session_uri).map_err(|_| "test loopback session is invalid")?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
    {
        return Err("test loopback session is invalid".into());
    }
    let budget = chunk_bytes
        .unwrap_or(transfer::DEFAULT_CHUNK_BYTES)
        .clamp(transfer::CHUNK_ALIGNMENT_BYTES, transfer::MAX_CHUNK_BYTES);
    Driver {
        agent: transfer_agent_for_scheme(budget, false),
        session: session_uri.into(),
        reader,
        total: total_bytes,
        expected_sha256,
        chunk_bytes,
        digest: DigestState::new(),
        cancel,
        progress,
        widest_read: AtomicUsize::new(0),
    }
    .run(state)
}

/// Stream a receipt-sealed output to its DS-minted storage session.
///
/// `reader` must be the single handle from the producer's closed read door;
/// callers cannot supply a path or a URL. The output is only marked completed
/// after the storage session proves every byte and the local stream matches the
/// sealed size and digest.
pub fn transfer_verified_output<R: Read + Seek>(
    output_id: &str,
    session_uri: &str,
    total_bytes: u64,
    expected_sha256: &str,
    reader: R,
) -> Result<TransferReceipt, String> {
    if output_id.is_empty() || output_id.len() > 256 || output_id.chars().any(char::is_control) {
        return Err("native Sync Center output identity is invalid".into());
    }
    let result = drive_verified_output(
        session_uri,
        total_bytes,
        expected_sha256,
        reader,
        None,
        None,
        &|| false,
        &mut |_| {},
    )?;
    Ok(TransferReceipt {
        output_id: output_id.to_owned(),
        outcome: if result.outcome == NativeTransferOutcome::Done {
            TransferReceipt::COMPLETED
        } else {
            "not_committed"
        }
        .into(),
        committed_bytes: result.committed_bytes,
        total_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn storage_sessions_refuse_redirect_targets_before_any_byte_can_leave() {
        for raw in [
            "http://storage.googleapis.com/upload/session",
            "https://storage.googleapis.com.attacker.example/upload/session",
            "https://attacker.example/redirected-session",
            "https://user:secret@storage.googleapis.com/upload/session",
            "https://storage.googleapis.com:8443/upload/session",
        ] {
            assert!(
                validated_storage_session(raw).is_err(),
                "{raw} must be refused"
            );
        }
    }

    #[test]
    fn corrupted_sealed_bytes_fail_the_shared_digest_gate() {
        let bytes = b"corrupted bytes".to_vec();
        let mut progress = |_| {};
        let mut driver = Driver {
            agent: transfer_agent(transfer::DEFAULT_CHUNK_BYTES),
            session: "https://storage.googleapis.com/upload/session".into(),
            reader: Cursor::new(bytes),
            total: 15,
            expected_sha256: &"0".repeat(64),
            chunk_bytes: None,
            digest: DigestState::new(),
            cancel: &|| false,
            progress: &mut progress,
            widest_read: AtomicUsize::new(0),
        };
        assert_eq!(driver.digest_matches().unwrap(), Some(false));
    }

    #[test]
    fn redirect_responses_are_observed_and_never_followed() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            assert!(request.starts_with("GET /session "));
            stream
                .write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/escaped\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
        });
        let response = transfer_agent_for_scheme(transfer::DEFAULT_CHUNK_BYTES, false)
            .get(format!("http://{address}/session"))
            .call()
            .unwrap();
        assert_eq!(response.status().as_u16(), 302);
        worker.join().unwrap();
    }

    #[test]
    fn acknowledgement_ranges_are_strict_facts_not_guesses() {
        let mut headers = ureq::http::HeaderMap::new();
        assert_eq!(observe_range(&headers), RangeObservation::Absent);
        headers.insert("range", "bytes=0-262143".parse().unwrap());
        assert_eq!(
            observe_range(&headers),
            RangeObservation::Value { end: 262_143 }
        );
        headers.insert("range", "bytes=1-262143".parse().unwrap());
        assert_eq!(observe_range(&headers), RangeObservation::Malformed);
    }

    #[test]
    fn resumed_prefix_gets_a_full_digest_pass_before_success() {
        let bytes = b"resume source bytes".to_vec();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let mut progress = |_| {};
        let mut driver = Driver {
            agent: transfer_agent(transfer::DEFAULT_CHUNK_BYTES),
            session: "https://storage.googleapis.com/upload/session".into(),
            reader: Cursor::new(bytes.clone()),
            total: bytes.len() as u64,
            expected_sha256: &digest,
            chunk_bytes: None,
            digest: DigestState::new(),
            cancel: &|| false,
            progress: &mut progress,
            widest_read: AtomicUsize::new(0),
        };
        driver.digest.absorb(8, &bytes[8..]);
        assert!(!driver.digest.complete(bytes.len() as u64));
        assert_eq!(driver.digest_matches().unwrap(), Some(true));
    }

    #[test]
    fn cancellation_stops_the_body_before_it_reads_source_bytes() {
        let cancelled = AtomicBool::new(true);
        let mut source = Cursor::new(b"source".to_vec());
        let mut digest = DigestState::new();
        let written = AtomicU64::new(0);
        let body_cancelled = AtomicBool::new(false);
        let widest = AtomicUsize::new(0);
        {
            let mut body = ChunkBody {
                reader: &mut source,
                digest: &mut digest,
                position: 0,
                remaining: 6,
                written: &written,
                cancel: &|| cancelled.load(Ordering::SeqCst),
                cancelled: &body_cancelled,
                widest_read: &widest,
            };
            assert!(body.read(&mut [0_u8; 64]).is_err());
        }
        assert!(body_cancelled.load(Ordering::SeqCst));
        assert_eq!(source.position(), 0);
    }

    #[test]
    fn body_reads_never_exceed_the_shared_memory_slice() {
        let mut source = Cursor::new(vec![7_u8; MAX_BODY_READ_BYTES * 2]);
        let mut digest = DigestState::new();
        let written = AtomicU64::new(0);
        let cancelled = AtomicBool::new(false);
        let widest = AtomicUsize::new(0);
        let mut body = ChunkBody {
            reader: &mut source,
            digest: &mut digest,
            position: 0,
            remaining: (MAX_BODY_READ_BYTES * 2) as u64,
            written: &written,
            cancel: &|| false,
            cancelled: &cancelled,
            widest_read: &widest,
        };
        let mut request_buffer = vec![0_u8; MAX_BODY_READ_BYTES * 2];
        assert_eq!(body.read(&mut request_buffer).unwrap(), MAX_BODY_READ_BYTES);
        assert_eq!(widest.load(Ordering::SeqCst), MAX_BODY_READ_BYTES);
    }

    #[test]
    fn restart_state_for_another_artifact_is_refused_before_session_io() {
        let state = TransferState {
            schema: transfer::TRANSFER_SCHEMA.into(),
            total_bytes: 9,
            chunk_bytes: transfer::DEFAULT_CHUNK_BYTES,
            committed: 0,
            sent: 0,
            stage: Stage::Probing,
            stalled_attempts: 0,
            inconclusive_probes: 0,
            last_cause: None,
            retry_at: 0,
        };
        let reader = Cursor::new(b"other".to_vec());
        let error = drive_verified_output(
            "https://attacker.example/session",
            5,
            &format!("{:x}", Sha256::digest(b"other")),
            reader,
            Some(state),
            None,
            &|| false,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.contains("durable transfer state does not describe this artifact"));
    }
}
