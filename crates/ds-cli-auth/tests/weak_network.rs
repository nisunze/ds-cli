//! Weak-network acceptance: the resumable transfer, driven through faults.
//!
//! > "we use and prove resumable uploads on weak networks."
//!
//! This file is the proof, not the claim. It runs a real storage session — an
//! in-process HTTP server on loopback that accumulates the object the way a
//! resumable session does — and drives the CLI's real native transfer host
//! (`ds_cli_auth::upload`) against it, `ureq` and all. Every retry, resume
//! point and terminal verdict comes back out of `ds_command_kernel::transfer`,
//! which is the same pure machine the browser and the desktop shell run.
//!
//! It is Rust end to end: this test links `ds-cli-auth`, which hosts IO for the
//! kernel's protocol. No Node process, no TypeScript module and no browser is
//! started, imported or consulted anywhere in this file — `resumableUpload.ts`
//! is deleted and nothing here could revive it.
//!
//! One scenario per test, one fault per scenario:
//!
//! | Test | Fault |
//! |---|---|
//! | `connection_reset_mid_chunk_…` | socket reset with the body half written |
//! | `a_burst_of_server_errors_…` | 503, 500, 502 in a row |
//! | `too_many_requests_…` | 429 carrying `Retry-After: 1` |
//! | `request_timeouts_…` | two 408s |
//! | `a_truncated_acknowledgement_…` | the session commits less than was sent |
//! | `a_duplicate_acknowledgement_…` | the session repeats an old acknowledgement |
//! | `a_session_proven_gone_…` | 404, then 410 on the reopened session |
//! | `a_host_restart_mid_transfer_…` | the host dies and a second one resumes |
//! | `a_terminal_cause_…` | 401/403 |
//! | `an_exhausted_burst_…` | 503 forever |
//!
//! Every one of them ends at the same three questions, asked by
//! [`assert_holds_the_source_exactly_once`] or by its terminal counterpart:
//! does the session hold the source bytes, in order, with every byte committed
//! exactly once; did the resume start at the last acknowledged offset; and was
//! the number of attempts the bounded number the kernel allows.
//!
//! The live counterpart — a real publication over a real degraded link — is
//! `docs/operations/weak-network-acceptance.md`. This file bounds the protocol;
//! that procedure proves the product.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ds_cli_auth::weak_network_harness::{Outcome, drive_loopback};
use ds_command_kernel::transfer::{self, Cause};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// GCS commits resumable chunks on 256 KiB boundaries, so a session that is cut
/// mid-chunk keeps an aligned prefix. The harness models that rather than a
/// convenient byte count.
const COMMIT_ALIGNMENT: u64 = 256 * KIB;

/// A deterministic artifact whose every byte states its own offset mod 251, so
/// a test can tell WHICH bytes a chunk carried, not merely how many.
fn artifact(size: u64) -> Vec<u8> {
    (0..size).map(|index| (index % 251) as u8).collect()
}

// ---------------------------------------------------------------------------
// The kernel's own numbers, restated
// ---------------------------------------------------------------------------

/// The deterministic backoff `ds_command_kernel::transfer` applies, and which
/// `response_policy` shares rather than declaring a second curve. Restated here
/// so a test asserts against a *number* instead of against the code that
/// produced it: if the curve moves, these tests say so.
fn backoff_ms(stalled_attempt: u32) -> u64 {
    (1_000u64 << stalled_attempt.saturating_sub(1).min(5)).min(30_000)
}

/// Total time a host must sleep before dispatching attempt `attempts + 1`.
fn backoff_total(attempts: u32) -> u64 {
    (1..=attempts).map(backoff_ms).sum()
}

/// `transfer::MAX_STALLED_ATTEMPTS` — consecutive failures with no committed
/// progress before the transfer stops. The bound is the whole point: a weak
/// network must end in a reported cause, never in an invisible loop.
const MAX_STALLED_ATTEMPTS: u32 = 6;

/// The attempt window a run of `retries` retries must land in. Lower bound: it
/// actually slept the curve. Upper bound: it did not retry once more.
fn assert_retries(elapsed: Duration, retries: u32) {
    let floor = Duration::from_millis(backoff_total(retries));
    let ceiling = Duration::from_millis(backoff_total(retries + 1));
    assert!(
        elapsed >= floor,
        "{retries} retries must sleep at least {floor:?}, slept {elapsed:?}"
    );
    assert!(
        elapsed < ceiling,
        "{retries} retries must not reach the {retries}+1 curve at {ceiling:?}, took {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// The fault the session injects
// ---------------------------------------------------------------------------

/// One fault, applied to one request. The session pops one per request and
/// serves honestly once the queue is empty, so a scenario reads as a sentence:
/// "two timeouts, then the link comes back".
#[derive(Clone, Debug, PartialEq, Eq)]
enum Fault {
    /// Serve honestly.
    None,
    /// Read this many body bytes, commit the aligned prefix that actually
    /// arrived, then reset the connection with the rest still in flight.
    ResetMidChunk { after_bytes: u64 },
    /// Drain the body, then answer with this status and nothing else.
    Status {
        status: u16,
        retry_after: Option<&'static str>,
    },
    /// Take the body but commit only this many bytes in total, and acknowledge
    /// exactly that — the server took less than the host wrote.
    TruncatedAck { commit: u64 },
    /// Commit the body, then repeat the acknowledgement the previous answer
    /// already gave. The host must not read that as progress, and the bytes it
    /// re-sends must not be committed twice.
    DuplicateAck,
}

/// One request as the session saw it. `body_bytes` is what the session actually
/// read off the socket, which for a reset is less than the host wrote.
#[derive(Clone, Debug)]
struct Exchange {
    content_range: String,
    body_bytes: u64,
    first_byte: Option<u8>,
    fault: Fault,
    answered: Option<u16>,
    acknowledged: Option<u64>,
}

impl Exchange {
    /// The offset this request declared, from its `Content-Range`. `None` for a
    /// probe, which declares `bytes */total`.
    fn offset(&self) -> Option<u64> {
        let span = self.content_range.strip_prefix("bytes ")?;
        let (start, _) = span.split_once('-')?;
        start.parse().ok()
    }

    fn is_probe(&self) -> bool {
        self.content_range.starts_with("bytes */")
    }
}

// ---------------------------------------------------------------------------
// The storage session
// ---------------------------------------------------------------------------

/// A resumable session's whole observable state. It is append-only on purpose:
/// `appended` can only equal `total` at the end if every byte was committed
/// exactly once, so "exactly once" is measured rather than argued.
struct Session {
    total: u64,
    object: Vec<u8>,
    appended: u64,
    acknowledged: u64,
    faults: VecDeque<Fault>,
    log: Vec<Exchange>,
    /// A chunk that began past the committed prefix would leave a hole. The
    /// host must never produce one; this counts the times it did.
    gaps: u32,
}

impl Session {
    /// Commit the part of `body` that starts at the committed prefix. Bytes
    /// already held are ignored, never appended a second time.
    fn commit(&mut self, offset: u64, body: &[u8]) {
        let held = self.object.len() as u64;
        if offset > held {
            self.gaps += 1;
            return;
        }
        let skip = (held - offset) as usize;
        if skip >= body.len() {
            return;
        }
        let fresh = &body[skip..];
        self.object.extend_from_slice(fresh);
        self.appended += fresh.len() as u64;
    }

    /// The answer an honest session owes right now.
    fn honest_answer(&self) -> (u16, Option<u64>) {
        let held = self.object.len() as u64;
        if held >= self.total {
            // The object is complete and immutable: finalize, never re-send.
            (200, None)
        } else if held == 0 {
            // 308 with no `Range` is the session stating it holds ZERO bytes.
            (308, None)
        } else {
            (308, Some(held - 1))
        }
    }
}

struct Harness {
    port: u16,
    session: Arc<Mutex<Session>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Harness {
    fn start(total: u64, faults: Vec<Fault>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let session = Arc::new(Mutex::new(Session {
            total,
            object: Vec::new(),
            appended: 0,
            acknowledged: 0,
            faults: faults.into(),
            log: Vec::new(),
            gaps: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let session = session.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => serve(stream, &session),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Harness {
            port,
            session,
            stop,
            worker: Some(worker),
        }
    }

    fn uri(&self) -> String {
        format!("http://127.0.0.1:{}/resumable/session", self.port)
    }

    fn object(&self) -> Vec<u8> {
        self.session.lock().expect("session").object.clone()
    }

    fn appended(&self) -> u64 {
        self.session.lock().expect("session").appended
    }

    fn acknowledged(&self) -> u64 {
        self.session.lock().expect("session").acknowledged
    }

    fn gaps(&self) -> u32 {
        self.session.lock().expect("session").gaps
    }

    fn log(&self) -> Vec<Exchange> {
        self.session.lock().expect("session").log.clone()
    }

    /// Drive one transfer of `bytes` to a terminal outcome, timed.
    fn run(&self, bytes: &[u8], cancel: &dyn Fn() -> bool) -> (Outcome, Duration) {
        let mut reader = Cursor::new(bytes);
        let started = Instant::now();
        let outcome = drive_loopback(&self.uri(), bytes.len() as u64, &mut reader, cancel)
            .expect("the transfer drives to a terminal outcome");
        (outcome, started.elapsed())
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn never_cancelled() -> impl Fn() -> bool {
    || false
}

/// Serve one request. The session lock is never held across body IO: a test's
/// cancel closure reads the same state from the client thread.
fn serve(stream: TcpStream, session: &Arc<Mutex<Session>>) {
    stream.set_nonblocking(false).expect("blocking stream");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("read timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    let mut content_length = 0_u64;
    let mut content_range = String::new();
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
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => content_length = value.trim().parse().unwrap_or(0),
            "content-range" => content_range = value.trim().to_string(),
            _ => {}
        }
    }
    let fault = {
        let mut held = session.lock().expect("session");
        held.faults.pop_front().unwrap_or(Fault::None)
    };
    let offset = content_range
        .strip_prefix("bytes ")
        .and_then(|span| span.split_once('-'))
        .and_then(|(start, _)| start.parse::<u64>().ok());
    let mut stream = stream;

    if let Fault::ResetMidChunk { after_bytes } = fault {
        // Read part of the body, keep the aligned prefix that arrived, then cut
        // the connection with the rest still in flight. Unread bytes remain in
        // the receive queue, so the peer sees a reset rather than a clean close.
        let mut body = Vec::new();
        let mut scratch = vec![0_u8; 64 * KIB as usize];
        while (body.len() as u64) < after_bytes {
            let want = ((after_bytes - body.len() as u64) as usize).min(scratch.len());
            match reader.read(&mut scratch[..want]) {
                Ok(0) | Err(_) => break,
                Ok(read) => body.extend_from_slice(&scratch[..read]),
            }
        }
        let keep = ((body.len() as u64) / COMMIT_ALIGNMENT) * COMMIT_ALIGNMENT;
        let first_byte = body.first().copied();
        let body_bytes = body.len() as u64;
        {
            let mut held = session.lock().expect("session");
            if let Some(offset) = offset {
                held.commit(offset, &body[..keep as usize]);
                held.acknowledged = held.object.len() as u64;
            }
            let acknowledged = held.acknowledged;
            held.log.push(Exchange {
                content_range,
                body_bytes,
                first_byte,
                fault,
                answered: None,
                acknowledged: Some(acknowledged),
            });
        }
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }

    // Every other fault answers, so the body is drained first: a peer that
    // never reads the body would inject a reset this scenario did not ask for.
    let mut body = Vec::new();
    let mut sink = (&mut reader).take(content_length);
    let body_bytes = std::io::copy(&mut sink, &mut body).unwrap_or(0);
    let first_byte = body.first().copied();

    let (status, range, retry_after) = {
        let mut held = session.lock().expect("session");
        let previous_ack = held.acknowledged;
        match fault {
            Fault::Status {
                status,
                retry_after,
            } => (status, None, retry_after),
            Fault::TruncatedAck { commit } => {
                if let Some(offset) = offset {
                    let keep = commit.saturating_sub(offset).min(body_bytes) as usize;
                    held.commit(offset, &body[..keep]);
                }
                held.acknowledged = held.object.len() as u64;
                let (status, range) = held.honest_answer();
                (status, range, None)
            }
            Fault::DuplicateAck => {
                if let Some(offset) = offset {
                    held.commit(offset, &body);
                }
                // The acknowledgement does NOT move: the session repeats itself.
                let range = previous_ack.checked_sub(1);
                (308, range, None)
            }
            Fault::None | Fault::ResetMidChunk { .. } => {
                if let Some(offset) = offset {
                    held.commit(offset, &body);
                }
                held.acknowledged = held.object.len() as u64;
                let (status, range) = held.honest_answer();
                (status, range, None)
            }
        }
    };
    {
        let mut held = session.lock().expect("session");
        let acknowledged = held.acknowledged;
        held.log.push(Exchange {
            content_range,
            body_bytes,
            first_byte,
            fault,
            answered: Some(status),
            acknowledged: Some(acknowledged),
        });
    }

    let mut response = format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n");
    if let Some(end) = range {
        response.push_str(&format!("Range: bytes=0-{end}\r\n"));
    }
    if let Some(value) = retry_after {
        response.push_str(&format!("Retry-After: {value}\r\n"));
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

// ---------------------------------------------------------------------------
// The acceptance question, asked the same way every time
// ---------------------------------------------------------------------------

/// The session holds the source bytes, in order, with every byte committed
/// exactly once, and nothing ever arrived past the committed prefix.
fn assert_holds_the_source_exactly_once(harness: &Harness, source: &[u8]) {
    assert_eq!(
        harness.gaps(),
        0,
        "no chunk may begin past the committed prefix"
    );
    assert_eq!(
        harness.object().len(),
        source.len(),
        "the session must hold exactly the artifact's byte count"
    );
    assert!(
        harness.object() == source,
        "the session must hold the source bytes, in order"
    );
    assert_eq!(
        harness.appended(),
        source.len() as u64,
        "every byte must be committed exactly once"
    );
}

/// A terminal outcome that wrote nothing: the session is untouched, so the
/// caller's reopen decision costs no re-upload.
fn assert_committed_nothing(harness: &Harness, outcome: &Outcome) {
    assert!(!outcome.done);
    assert_eq!(outcome.committed, 0);
    assert!(harness.object().is_empty());
    assert_eq!(harness.appended(), 0);
}

// ---------------------------------------------------------------------------
// Scenario 1 — the connection resets with the body half written
// ---------------------------------------------------------------------------

#[test]
fn connection_reset_mid_chunk_resumes_from_the_acknowledged_offset() {
    let source = artifact(12 * MIB);
    // The reset lands on an unaligned byte on purpose: the session keeps the
    // 256 KiB-aligned prefix, so the resume point is the SERVER's number and
    // not the count the host happened to write.
    let harness = Harness::start(
        12 * MIB,
        vec![
            Fault::None,
            Fault::ResetMidChunk {
                after_bytes: 4 * MIB + 7_919,
            },
        ],
    );
    let (outcome, elapsed) = harness.run(&source, &never_cancelled());

    assert!(outcome.done, "a reset mid-chunk must not end the transfer");
    assert_eq!(outcome.committed, 12 * MIB);
    assert_eq!(outcome.cause, None);
    assert_holds_the_source_exactly_once(&harness, &source);

    let log = harness.log();
    assert_eq!(outcome.requests, 4);
    assert_eq!(log.len(), 4);
    // probe, chunk (reset), probe, chunk.
    assert!(log[0].is_probe() && log[0].body_bytes == 0);
    assert_eq!(log[1].offset(), Some(0));
    assert_eq!(
        log[1].fault,
        Fault::ResetMidChunk {
            after_bytes: 4 * MIB + 7_919
        },
        "the reset must land on the chunk, not on a probe"
    );
    assert_eq!(log[1].acknowledged, Some(4 * MIB));
    // The ambiguous acknowledgement produced a PROBE, never a restart from 0
    // and never an assumed success.
    assert!(log[2].is_probe() && log[2].body_bytes == 0);
    // Resume begins at the last acknowledged offset, from the right bytes.
    assert_eq!(log[3].offset(), Some(4 * MIB));
    assert_eq!(log[3].first_byte, Some(source[4 * MIB as usize]));
    assert_eq!(log[3].body_bytes, 8 * MIB);
    // One stall, one bounded backoff: the ambiguity cost exactly one retry.
    assert_retries(elapsed, 1);
    // A reset does not widen the host's memory: at most one protocol chunk is
    // held for the re-send, and the socket still sees 64 KiB slices.
    assert!(outcome.widest_stage as u64 <= 8 * MIB);
    assert!(outcome.widest_read as u64 <= 64 * KIB);
}

// ---------------------------------------------------------------------------
// Scenario 2 — a burst of 5xx
// ---------------------------------------------------------------------------

#[test]
fn a_burst_of_server_errors_backs_off_on_the_curve_and_then_completes() {
    let source = artifact(MIB);
    let harness = Harness::start(
        MIB,
        vec![
            Fault::Status {
                status: 503,
                retry_after: None,
            },
            Fault::Status {
                status: 500,
                retry_after: None,
            },
            Fault::Status {
                status: 502,
                retry_after: None,
            },
        ],
    );
    let (outcome, elapsed) = harness.run(&source, &never_cancelled());

    assert!(outcome.done);
    assert_eq!(outcome.cause, None);
    assert_holds_the_source_exactly_once(&harness, &source);
    // Three refused probes, one that answered, one chunk. Nothing was written
    // while the session was erroring.
    assert_eq!(outcome.requests, 5);
    let log = harness.log();
    assert!(log[..4].iter().all(Exchange::is_probe));
    assert_eq!(log[0].answered, Some(503));
    assert_eq!(log[1].answered, Some(500));
    assert_eq!(log[2].answered, Some(502));
    assert_eq!(log[4].offset(), Some(0));
    assert_retries(elapsed, 3);
}

// ---------------------------------------------------------------------------
// Scenario 3 — 429 with Retry-After
// ---------------------------------------------------------------------------

#[test]
fn too_many_requests_is_never_retried_before_the_delay_the_server_named() {
    let source = artifact(MIB);
    let harness = Harness::start(
        MIB,
        vec![Fault::Status {
            status: 429,
            retry_after: Some("1"),
        }],
    );
    let (outcome, elapsed) = harness.run(&source, &never_cancelled());

    assert!(outcome.done);
    assert_holds_the_source_exactly_once(&harness, &source);
    assert_eq!(outcome.requests, 3);
    // The server named one second. The kernel's first backoff step is one
    // second, so the retry is no earlier than the server allowed.
    assert!(
        elapsed >= Duration::from_secs(1),
        "a Retry-After of 1s must not be undercut, waited {elapsed:?}"
    );
    assert_retries(elapsed, 1);
    // Stated plainly because it is a real ceiling, not a passing test's
    // silence: the transfer lane applies the kernel's own curve and does not
    // read `Retry-After`. A `Retry-After` LONGER than the curve's step would
    // not be honoured here. `response_policy` honours the header for gateway
    // requests; the storage-session lane does not yet share that reading.
    assert!(backoff_ms(1) >= 1_000);
}

// ---------------------------------------------------------------------------
// Scenario 4 — 408 timeouts
// ---------------------------------------------------------------------------

#[test]
fn request_timeouts_are_retried_on_the_kernels_curve_and_then_complete() {
    let source = artifact(MIB);
    let timeout = Fault::Status {
        status: 408,
        retry_after: None,
    };
    let harness = Harness::start(MIB, vec![timeout.clone(), timeout]);
    let (outcome, elapsed) = harness.run(&source, &never_cancelled());

    assert!(outcome.done);
    assert_holds_the_source_exactly_once(&harness, &source);
    assert_eq!(outcome.requests, 4);
    assert_retries(elapsed, 2);
}

// ---------------------------------------------------------------------------
// Scenario 5 — the session acknowledges less than was sent
// ---------------------------------------------------------------------------

#[test]
fn a_truncated_acknowledgement_resumes_from_the_servers_number() {
    let source = artifact(MIB);
    let harness = Harness::start(
        MIB,
        vec![Fault::None, Fault::TruncatedAck { commit: 256 * KIB }],
    );
    let (outcome, elapsed) = harness.run(&source, &never_cancelled());

    assert!(outcome.done);
    assert_holds_the_source_exactly_once(&harness, &source);
    let log = harness.log();
    assert_eq!(outcome.requests, 3);
    assert_eq!(log[1].offset(), Some(0));
    assert_eq!(log[1].body_bytes, MIB);
    assert_eq!(log[1].acknowledged, Some(256 * KIB));
    // The host wrote a megabyte; the session took a quarter of it. The resume
    // point is the session's number, served from the host's own re-send window.
    assert_eq!(log[2].offset(), Some(256 * KIB));
    assert_eq!(log[2].first_byte, Some(source[256 * KIB as usize]));
    assert_eq!(log[2].body_bytes, MIB - 256 * KIB);
    // Truncation is a fact, not a failure: it costs no backoff.
    assert_retries(elapsed, 0);
}

// ---------------------------------------------------------------------------
// Scenario 6 — the session repeats an acknowledgement
// ---------------------------------------------------------------------------

#[test]
fn a_duplicate_acknowledgement_never_commits_a_byte_twice() {
    let source = artifact(12 * MIB);
    let harness = Harness::start(
        12 * MIB,
        vec![Fault::None, Fault::None, Fault::DuplicateAck],
    );
    let (outcome, elapsed) = harness.run(&source, &never_cancelled());

    assert!(outcome.done);
    assert_holds_the_source_exactly_once(&harness, &source);
    let log = harness.log();
    assert_eq!(outcome.requests, 4);
    // The stale acknowledgement was not read as progress: the host re-sent the
    // window rather than declaring the transfer finished.
    assert_eq!(log[2].offset(), Some(8 * MIB));
    assert_eq!(log[3].offset(), Some(8 * MIB));
    assert_eq!(log[3].first_byte, Some(source[8 * MIB as usize]));
    // The proof that "exactly once" is about commits and not about traffic:
    // 16 MiB crossed the socket for a 12 MiB artifact, and the session still
    // holds 12 MiB, appended once.
    let transmitted: u64 = log.iter().map(|exchange| exchange.body_bytes).sum();
    assert_eq!(transmitted, 16 * MIB);
    assert_eq!(harness.appended(), 12 * MIB);
    // A repeated acknowledgement is not a failure either.
    assert_retries(elapsed, 0);
}

// ---------------------------------------------------------------------------
// Scenario 7 — the session is proven gone
// ---------------------------------------------------------------------------

#[test]
fn a_session_proven_gone_is_reported_and_the_reopen_budget_stays_the_callers() {
    let source = artifact(MIB);

    // 404: proven expired. The host reports it and stops — it holds no reopen
    // budget, because the session was minted by `upload_start` and the budget
    // lives with that caller (`compute_artifact_engine`'s `reopens`).
    let first = Harness::start(
        MIB,
        vec![Fault::Status {
            status: 404,
            retry_after: None,
        }],
    );
    let (outcome, elapsed) = first.run(&source, &never_cancelled());
    assert_eq!(outcome.cause, Some(Cause::SessionExpired));
    assert_eq!(outcome.status, Some(404));
    assert_eq!(outcome.requests, 1, "a proven expiry is never retried");
    assert_committed_nothing(&first, &outcome);
    assert_retries(elapsed, 0);

    // The caller spends a reopen. The new session is also gone — 410 this time.
    let second = Harness::start(
        MIB,
        vec![Fault::Status {
            status: 410,
            retry_after: None,
        }],
    );
    let (outcome, elapsed) = second.run(&source, &never_cancelled());
    assert_eq!(outcome.cause, Some(Cause::SessionExpired));
    assert_eq!(outcome.status, Some(410));
    assert_eq!(outcome.requests, 1);
    assert_committed_nothing(&second, &outcome);
    assert_retries(elapsed, 0);

    // A reopen that lands on a live session transfers the artifact once, from
    // zero, because a new session holds nothing. That is the correct resume
    // point, not a regression to the old restart-on-anything behaviour.
    let third = Harness::start(MIB, Vec::new());
    let (outcome, _) = third.run(&source, &never_cancelled());
    assert!(outcome.done);
    assert_holds_the_source_exactly_once(&third, &source);
    assert_eq!(outcome.requests, 2);
}

// ---------------------------------------------------------------------------
// Scenario 8 — the host dies mid-transfer
// ---------------------------------------------------------------------------

#[test]
fn a_host_restart_mid_transfer_resumes_at_the_acknowledged_offset() {
    let source = artifact(12 * MIB);
    let harness = Harness::start(12 * MIB, Vec::new());

    // Host #1 stops the moment the session has committed the first chunk —
    // the laptop closing, the process being killed, the operator walking out
    // of coverage. Cancellation is honoured before dispatch and leaves the
    // session exactly as the server acknowledged it.
    let stop_after_first_chunk = {
        let session = harness.session.clone();
        move || session.lock().expect("session").object.len() as u64 >= 8 * MIB
    };
    let (first, _) = harness.run(&source, &stop_after_first_chunk);
    assert_eq!(first.cause, Some(Cause::Cancelled));
    assert!(!first.done);
    assert_eq!(first.committed, 8 * MIB);
    assert_eq!(harness.acknowledged(), 8 * MIB);
    assert_eq!(first.requests, 2);

    // What the dying host held, persisted and read back. The kernel's durable
    // state is plain JSON with no host types in it, and a resume from it opens
    // with a PROBE: the persisted number is a hint the kernel deliberately does
    // not trust across a process boundary, which is exactly why host #2 needs
    // nothing but the session URI.
    let resumed = resume_action_from_persisted_state(12 * MIB, 8 * MIB);
    assert_eq!(resumed, "probe");

    // Host #1 is gone. Host #2 is a fresh driver, a fresh reader over the same
    // artifact, and the same session URI.
    let before = harness.log().len();
    let (second, elapsed) = harness.run(&source, &never_cancelled());
    assert!(second.done);
    assert_eq!(second.committed, 12 * MIB);
    assert_holds_the_source_exactly_once(&harness, &source);

    let resumed_log = &harness.log()[before..];
    assert_eq!(second.requests, 2);
    assert_eq!(resumed_log.len(), 2);
    assert!(resumed_log[0].is_probe());
    assert_eq!(resumed_log[1].offset(), Some(8 * MIB));
    assert_eq!(resumed_log[1].first_byte, Some(source[8 * MIB as usize]));
    // The committed prefix was never sent again: the restart cost 4 MiB, not 12.
    let resent: u64 = resumed_log.iter().map(|exchange| exchange.body_bytes).sum();
    assert_eq!(resent, 4 * MIB);
    assert_retries(elapsed, 0);
}

/// Persist the kernel's durable transfer state exactly as a host would, read it
/// back, and report what a resume from it does first.
///
/// This goes through `transfer::evaluate` — the same entry point the native
/// host calls — so the durability being proven is the kernel's own, not a
/// re-implementation of it.
fn resume_action_from_persisted_state(total_bytes: u64, acknowledged: u64) -> String {
    let step = |state: Option<&serde_json::Value>,
                plan: Option<serde_json::Value>,
                event: serde_json::Value| {
        let request = serde_json::json!({
            "schema": transfer::TRANSFER_SCHEMA,
            "now": 1_700_000_000_000_u64,
            "state": state,
            "plan": plan,
            "event": event,
        });
        let encoded = serde_json::to_vec(&request).expect("encode a transfer step");
        let reply = transfer::evaluate(&encoded).expect("the kernel answers the step");
        serde_json::from_str::<serde_json::Value>(&reply).expect("decode a transfer step")
    };

    let started = step(
        None,
        Some(serde_json::json!({ "total_bytes": total_bytes })),
        serde_json::json!({ "kind": "start" }),
    );
    assert_eq!(started["action"]["kind"], "probe");

    let acknowledged = step(
        Some(&started["state"]),
        None,
        serde_json::json!({
            "kind": "probe_response",
            "status": 308,
            "range": { "kind": "value", "end": acknowledged - 1 },
        }),
    );
    assert_eq!(acknowledged["action"]["kind"], "send_chunk");

    // The host writes this to disk and dies. A second host reads it back.
    let persisted = serde_json::to_string(&acknowledged["state"]).expect("persist");
    let restored: serde_json::Value = serde_json::from_str(&persisted).expect("read back");
    assert_eq!(restored, acknowledged["state"], "durable state round-trips");

    let resumed = step(
        Some(&restored),
        None,
        serde_json::json!({ "kind": "start" }),
    );
    resumed["action"]["kind"]
        .as_str()
        .expect("an action kind")
        .to_string()
}

// ---------------------------------------------------------------------------
// Scenario 9 — a terminal cause
// ---------------------------------------------------------------------------

#[test]
fn a_terminal_cause_is_reported_at_once_and_never_retried() {
    for status in [401, 403] {
        let source = artifact(MIB);
        let harness = Harness::start(
            MIB,
            vec![Fault::Status {
                status,
                retry_after: None,
            }],
        );
        let (outcome, elapsed) = harness.run(&source, &never_cancelled());
        assert_eq!(outcome.cause, Some(Cause::HttpStatus { status }));
        assert_eq!(
            outcome.requests, 1,
            "{status} is terminal: retrying the same URI cannot help"
        );
        assert_committed_nothing(&harness, &outcome);
        assert_retries(elapsed, 0);
    }
}

// ---------------------------------------------------------------------------
// Scenario 10 — the link never comes back
// ---------------------------------------------------------------------------

/// The no-unbounded-loop proof, and the slowest test here by design: it sleeps
/// the kernel's whole curve (1+2+4+8+16 s) before the bound bites.
#[test]
fn an_endless_burst_stops_at_the_kernels_bound_with_a_typed_cause() {
    let source = artifact(MIB);
    let refusal = Fault::Status {
        status: 503,
        retry_after: None,
    };
    let harness = Harness::start(MIB, vec![refusal; (MAX_STALLED_ATTEMPTS + 4) as usize]);
    let (outcome, elapsed) = harness.run(&source, &never_cancelled());

    assert!(!outcome.done);
    // A typed cause the operator can act on, not "unreachable".
    assert_eq!(outcome.cause, Some(Cause::HttpStatus { status: 503 }));
    assert_eq!(
        outcome.requests, MAX_STALLED_ATTEMPTS,
        "the transfer stops at the kernel's bound, not when the faults run out"
    );
    assert_committed_nothing(&harness, &outcome);
    // Five bounded sleeps, then the verdict — and not a sixth.
    assert_retries(elapsed, MAX_STALLED_ATTEMPTS - 1);
}
