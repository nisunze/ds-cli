//! Deterministic protocol-boundary tests.
//!
//! Every test here is a *wire situation that has actually stranded a transfer*
//! somewhere, expressed as facts a host can report. They are the reason this
//! machine exists rather than a retry loop.
use super::*;

const MIB: u64 = 1024 * 1024;

fn plan(total: u64) -> TransferPlan {
    TransferPlan {
        total_bytes: total,
        chunk_bytes: None,
    }
}

fn start(total: u64) -> Reply {
    machine::apply(None, Some(plan(total)), Event::Start, 1_000).expect("start")
}

fn step(state: TransferState, event: Event, now: u64) -> Reply {
    machine::apply(Some(state), None, event, now).expect("step")
}

fn probe_response(status: u16, range: RangeObservation) -> Event {
    Event::ProbeResponse { status, range }
}

#[test]
fn a_transfer_always_asks_the_server_before_writing_a_byte() {
    let reply = start(100 * MIB);
    assert_eq!(reply.action, Action::Probe);
    assert_eq!(reply.state.committed, 0);
    assert_eq!(reply.state.sent, 0);
    assert_eq!(reply.state.stage, Stage::Probing);
}

#[test]
fn a_resume_re_probes_instead_of_trusting_its_own_persisted_offset() {
    let mut state = start(100 * MIB).state;
    state.committed = 40 * MIB;
    state.sent = 64 * MIB; // an optimistic number from a lost attempt
    let reply = step(state, Event::Start, 2_000);
    assert_eq!(reply.action, Action::Probe);
    // The host's optimistic counter is re-anchored to the proven prefix.
    assert_eq!(reply.state.sent, 40 * MIB);
}

#[test]
fn empty_308_acknowledgement_means_zero_bytes_held_not_unknown() {
    // GCS: "If the response contains no Range header, no bytes were received."
    let state = start(24 * MIB).state;
    let reply = step(state, probe_response(308, RangeObservation::Absent), 2_000);
    assert_eq!(
        reply.action,
        Action::SendChunk {
            offset: 0,
            length: 8 * MIB
        }
    );
    assert_eq!(reply.state.committed, 0);
}

#[test]
fn an_unobservable_range_is_not_an_absent_range() {
    // The browser case. Absent would mean "resume from 0" and would re-send the
    // whole artifact; Unobservable must mean "we learned nothing".
    let state = start(24 * MIB).state;
    let reply = step(
        state,
        probe_response(308, RangeObservation::Unobservable),
        2_000,
    );
    match reply.action {
        Action::Wait { cause, .. } => assert_eq!(cause, Cause::Inconclusive),
        other => panic!("an unobservable probe must not act: {other:?}"),
    }
    assert_eq!(reply.state.committed, 0);
    assert_eq!(reply.state.inconclusive_probes, 1);
    assert_eq!(reply.state.stage, Stage::Probing);
}

#[test]
fn an_inconclusive_probe_never_restarts_the_session() {
    // The exact Stable defect: probe fails, and the old code looped forever
    // with a single opaque token. Here it is bounded AND it keeps the session.
    let mut state = start(24 * MIB).state;
    for expected in 1..MAX_INCONCLUSIVE_PROBES {
        let reply = step(
            state,
            probe_response(308, RangeObservation::Unobservable),
            2_000,
        );
        assert!(matches!(reply.action, Action::Wait { .. }));
        assert_eq!(reply.state.inconclusive_probes, expected);
        state = reply.state;
    }
    let reply = step(
        state,
        probe_response(308, RangeObservation::Unobservable),
        2_000,
    );
    match reply.action {
        // Retryable, so a later drain re-probes the SAME session. It is never
        // reopened and no byte is re-sent on the strength of not knowing.
        Action::Fail { cause, retryable } => {
            assert_eq!(cause, Cause::Inconclusive);
            assert!(retryable);
        }
        other => panic!("expected a reported, bounded failure: {other:?}"),
    }
}

#[test]
fn a_malformed_range_is_never_parsed_permissively() {
    let state = start(24 * MIB).state;
    let reply = step(
        state,
        probe_response(308, RangeObservation::Malformed),
        2_000,
    );
    match reply.action {
        Action::Wait { cause, .. } => assert_eq!(cause, Cause::RangeMalformed),
        other => panic!("expected an inconclusive wait: {other:?}"),
    }
}

#[test]
fn a_partial_commit_resumes_from_the_servers_number() {
    let state = start(24 * MIB).state;
    let reply = step(
        state,
        probe_response(308, RangeObservation::Value { end: 8 * MIB - 1 }),
        2_000,
    );
    assert_eq!(reply.state.committed, 8 * MIB);
    assert_eq!(
        reply.action,
        Action::SendChunk {
            offset: 8 * MIB,
            length: 8 * MIB
        }
    );
}

#[test]
fn truncation_believes_the_server_not_the_bytes_we_wrote() {
    let mut state = start(24 * MIB).state;
    state.committed = 0;
    state.stage = Stage::Sending;
    // We wrote 8 MiB; the server admits to holding only 2 MiB of it.
    let reply = step(
        state,
        Event::ChunkResponse {
            status: 308,
            range: RangeObservation::Value { end: 2 * MIB - 1 },
            bytes_written: 8 * MIB,
        },
        3_000,
    );
    assert_eq!(reply.state.committed, 2 * MIB);
    assert_eq!(reply.state.sent, 2 * MIB);
    assert_eq!(reply.state.last_cause, Some(Cause::Truncated));
    assert_eq!(
        reply.action,
        Action::SendChunk {
            offset: 2 * MIB,
            length: 8 * MIB
        }
    );
}

#[test]
fn a_server_claiming_more_bytes_than_the_artifact_has_is_refused() {
    let state = start(8 * MIB).state;
    let reply = step(
        state,
        probe_response(308, RangeObservation::Value { end: 32 * MIB }),
        2_000,
    );
    assert_eq!(
        reply.action,
        Action::Fail {
            cause: Cause::LengthMismatch,
            retryable: false
        }
    );
}

#[test]
fn an_already_complete_session_finalizes_and_never_re_sends() {
    for status in [200u16, 201] {
        let state = start(24 * MIB).state;
        let reply = step(
            state,
            probe_response(status, RangeObservation::Absent),
            2_000,
        );
        assert_eq!(
            reply.action,
            Action::Done {
                committed: 24 * MIB
            }
        );
        assert_eq!(reply.state.stage, Stage::Done);
    }
}

#[test]
fn a_lost_completion_response_probes_rather_than_guessing() {
    // The classic final-chunk hang: every byte reached the socket, the
    // acknowledgement never arrived. Assuming success loses data; assuming
    // failure re-sends bytes the server already holds.
    let mut state = start(8 * MIB).state;
    state.stage = Stage::Sending;
    let reply = step(
        state,
        Event::TransportFailure {
            cause: Cause::ConnectionClosed,
            phase: Phase::Response,
            bytes_written: 8 * MIB,
        },
        4_000,
    );
    assert_eq!(reply.state.stage, Stage::Probing);
    match reply.action {
        Action::Wait { cause, until } => {
            assert_eq!(cause, Cause::ConnectionClosed);
            assert!(until > 4_000);
        }
        other => panic!("expected a backoff then a probe: {other:?}"),
    }
    // And the probe then settles it truthfully.
    let settled = step(
        reply.state,
        probe_response(200, RangeObservation::Absent),
        5_000,
    );
    assert_eq!(settled.action, Action::Done { committed: 8 * MIB });
}

#[test]
fn a_proven_expiry_asks_the_caller_to_reopen_and_nothing_else() {
    for status in [404u16, 410] {
        let state = start(8 * MIB).state;
        let reply = step(
            state,
            probe_response(status, RangeObservation::Absent),
            2_000,
        );
        assert_eq!(
            reply.action,
            Action::Reopen {
                cause: Cause::SessionExpired
            }
        );
    }
}

#[test]
fn a_rejected_session_credential_is_terminal() {
    for status in [401u16, 403] {
        let state = start(8 * MIB).state;
        let reply = step(
            state,
            probe_response(status, RangeObservation::Absent),
            2_000,
        );
        assert_eq!(
            reply.action,
            Action::Fail {
                cause: Cause::HttpStatus { status },
                retryable: false
            }
        );
    }
}

#[test]
fn retryable_server_failures_back_off_and_are_bounded() {
    for status in [500u16, 503, 429, 408, 425] {
        let mut state = start(8 * MIB).state;
        let mut last_until = 0;
        for attempt in 1..MAX_STALLED_ATTEMPTS {
            let reply = step(
                state,
                probe_response(status, RangeObservation::Absent),
                10_000,
            );
            match reply.action {
                Action::Wait { until, ref cause } => {
                    assert_eq!(*cause, Cause::HttpStatus { status });
                    assert!(until >= last_until, "backoff must not shrink");
                    assert!(until <= 10_000 + 30_000, "backoff is capped");
                    last_until = until;
                }
                other => panic!("attempt {attempt} for {status}: {other:?}"),
            }
            state = reply.state;
        }
        let reply = step(
            state,
            probe_response(status, RangeObservation::Absent),
            10_000,
        );
        match reply.action {
            Action::Fail { cause, retryable } => {
                assert_eq!(cause, Cause::HttpStatus { status });
                assert!(retryable, "a 5xx/429 row may be drained again later");
            }
            other => panic!("expected a bounded failure for {status}: {other:?}"),
        }
    }
}

#[test]
fn an_unexpected_client_status_is_terminal_and_not_retried() {
    let state = start(8 * MIB).state;
    let reply = step(state, probe_response(400, RangeObservation::Absent), 2_000);
    assert_eq!(
        reply.action,
        Action::Fail {
            cause: Cause::HttpStatus { status: 400 },
            retryable: false
        }
    );
}

#[test]
fn progress_clears_the_stall_budget() {
    let mut state = start(24 * MIB).state;
    // Two failures, then a real commit.
    for _ in 0..2 {
        state = step(state, probe_response(503, RangeObservation::Absent), 10_000).state;
    }
    assert_eq!(state.stalled_attempts, 2);
    let reply = step(
        state,
        probe_response(308, RangeObservation::Value { end: 8 * MIB - 1 }),
        11_000,
    );
    assert_eq!(reply.state.stalled_attempts, 0);
    assert_eq!(reply.state.inconclusive_probes, 0);
    assert_eq!(reply.state.last_cause, None);
}

#[test]
fn cancellation_is_honoured_before_dispatch_during_transfer_and_during_backoff() {
    // Before dispatch.
    let state = start(24 * MIB).state;
    let reply = step(state, Event::Cancelled, 2_000);
    assert_eq!(
        reply.action,
        Action::Fail {
            cause: Cause::Cancelled,
            retryable: true
        }
    );

    // During transfer, with bytes already on the socket. The committed prefix
    // is preserved so a resume does not start over.
    let mut state = start(24 * MIB).state;
    state.committed = 8 * MIB;
    state.stage = Stage::Sending;
    let reply = step(
        state,
        Event::TransportFailure {
            cause: Cause::Cancelled,
            phase: Phase::Request,
            bytes_written: 3 * MIB,
        },
        3_000,
    );
    assert_eq!(
        reply.action,
        Action::Fail {
            cause: Cause::Cancelled,
            retryable: true
        }
    );
    assert_eq!(reply.state.committed, 8 * MIB);

    // During backoff.
    let mut state = start(24 * MIB).state;
    state.stage = Stage::Waiting;
    state.retry_at = 99_000;
    let reply = step(state, Event::Cancelled, 4_000);
    assert_eq!(
        reply.action,
        Action::Fail {
            cause: Cause::Cancelled,
            retryable: true
        }
    );
}

#[test]
fn deliberate_isolation_stops_without_burning_the_retry_budget() {
    let state = start(24 * MIB).state;
    let reply = step(
        state,
        Event::TransportFailure {
            cause: Cause::Offline,
            phase: Phase::Connect,
            bytes_written: 0,
        },
        2_000,
    );
    assert_eq!(
        reply.action,
        Action::Fail {
            cause: Cause::Offline,
            retryable: true
        }
    );
    assert_eq!(reply.state.stalled_attempts, 0);
}

#[test]
fn a_connect_failure_with_no_bytes_written_backs_off_without_probing() {
    let state = start(24 * MIB).state;
    let reply = step(
        state,
        Event::TransportFailure {
            cause: Cause::Dns,
            phase: Phase::Connect,
            bytes_written: 0,
        },
        2_000,
    );
    assert!(matches!(reply.action, Action::Wait { .. }));
    assert_eq!(reply.state.stage, Stage::Waiting);
}

#[test]
fn digest_and_length_mismatches_are_terminal_conflicts() {
    for cause in [Cause::DigestMismatch, Cause::LengthMismatch] {
        let state = start(8 * MIB).state;
        let reply = step(
            state,
            Event::IntegrityFailure {
                cause: cause.clone(),
            },
            2_000,
        );
        assert_eq!(
            reply.action,
            Action::Fail {
                cause,
                retryable: false
            }
        );
    }
}

#[test]
fn a_completed_transfer_is_idempotent_under_duplicate_acknowledgements() {
    let mut state = start(8 * MIB).state;
    state = step(state, probe_response(200, RangeObservation::Absent), 2_000).state;
    for event in [
        probe_response(308, RangeObservation::Absent),
        Event::Cancelled,
        Event::TransportFailure {
            cause: Cause::ConnectionClosed,
            phase: Phase::Response,
            bytes_written: 8 * MIB,
        },
    ] {
        let reply = step(state.clone(), event, 3_000);
        assert_eq!(
            reply.action,
            Action::Done { committed: 8 * MIB },
            "a committed transfer must never be reopened",
        );
    }
}

#[test]
fn chunks_are_aligned_bounded_and_the_last_one_is_the_remainder() {
    // Non-final chunks must be a multiple of 256 KiB or GCS rejects them.
    let plan = TransferPlan {
        total_bytes: 20 * MIB + 7,
        chunk_bytes: Some(8 * MIB + 3),
    };
    let mut reply = machine::apply(None, Some(plan), Event::Start, 0).expect("start");
    assert_eq!(reply.state.chunk_bytes % CHUNK_ALIGNMENT_BYTES, 0);
    let mut offset = 0u64;
    loop {
        reply = step(
            reply.state,
            probe_response(
                308,
                if offset == 0 {
                    RangeObservation::Absent
                } else {
                    RangeObservation::Value { end: offset - 1 }
                },
            ),
            0,
        );
        match reply.action {
            Action::SendChunk { offset: at, length } => {
                assert_eq!(at, offset);
                assert!(length <= MAX_CHUNK_BYTES, "peak host memory stays bounded");
                let last = at + length == 20 * MIB + 7;
                assert!(
                    last || length % CHUNK_ALIGNMENT_BYTES == 0,
                    "only the final chunk may be unaligned",
                );
                offset = at + length;
            }
            Action::Done { committed } => {
                assert_eq!(committed, 20 * MIB + 7);
                break;
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}

#[test]
fn a_transfer_far_above_the_old_ceilings_streams_in_bounded_chunks() {
    // 512 MiB — well past both the retired 32 MiB and 64 MiB limits. The point
    // is that no single step ever asks a host to hold more than one chunk.
    let total = 512 * MIB;
    let mut reply = machine::apply(None, Some(plan(total)), Event::Start, 0).expect("start");
    let mut offset = 0u64;
    let mut steps = 0u32;
    loop {
        steps += 1;
        assert!(steps < 10_000, "must terminate");
        reply = step(
            reply.state,
            probe_response(
                308,
                if offset == 0 {
                    RangeObservation::Absent
                } else {
                    RangeObservation::Value { end: offset - 1 }
                },
            ),
            0,
        );
        match reply.action {
            Action::SendChunk { offset: at, length } => {
                assert!(length <= MAX_CHUNK_BYTES);
                offset = at + length;
            }
            Action::Done { committed } => {
                assert_eq!(committed, total);
                break;
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert_eq!(offset, total);
}

#[test]
fn causes_never_carry_anything_that_could_be_a_credential() {
    // A cause is surfaced to operators and telemetry, so its serialization must
    // be a closed vocabulary plus a status number — never a URI or a body.
    for cause in [
        Cause::HttpStatus { status: 403 },
        Cause::Dns,
        Cause::Connect,
        Cause::Tls,
        Cause::Timeout { phase: Phase::Idle },
        Cause::ConnectionClosed,
        Cause::Offline,
        Cause::Cancelled,
        Cause::Truncated,
        Cause::DigestMismatch,
        Cause::LengthMismatch,
        Cause::RangeMalformed,
        Cause::SessionExpired,
        Cause::BlockedByHostPolicy,
        Cause::Inconclusive,
    ] {
        let rendered = serde_json::to_string(&cause).expect("serialize");
        assert!(!rendered.contains("://"), "no URL may appear: {rendered}");
        assert!(!rendered.contains("Bearer"), "{rendered}");
        assert!(!rendered.contains('?'), "no query string: {rendered}");
        // A closed vocabulary plus a three-digit status cannot be long.
        assert!(rendered.len() < 80, "{rendered}");
    }
}

#[test]
fn the_command_envelope_is_bounded_and_schema_fenced() {
    assert!(evaluate(b"").is_err());
    assert!(evaluate(&vec![b'x'; 65 * 1024]).is_err());
    assert!(evaluate(br#"{"schema":"other/v1","now":0,"event":{"kind":"start"}}"#).is_err());
    // Unknown fields are refused rather than silently ignored.
    assert!(evaluate(
        br#"{"schema":"ds.transfer/v1","now":0,"event":{"kind":"start"},"plan":{"total_bytes":1},"extra":true}"#
    )
    .is_err());
    // A start with no plan cannot invent one.
    assert!(evaluate(br#"{"schema":"ds.transfer/v1","now":0,"event":{"kind":"start"}}"#).is_err());
    // An event with neither state nor start is refused.
    assert!(evaluate(
        br#"{"schema":"ds.transfer/v1","now":0,"event":{"kind":"probe_response","status":200,"range":{"kind":"absent"}}}"#
    )
    .is_err());
    let ok = evaluate(
        br#"{"schema":"ds.transfer/v1","now":0,"event":{"kind":"start"},"plan":{"total_bytes":1048576}}"#,
    )
    .expect("valid");
    assert!(ok.contains("\"probe\""));
}

#[test]
fn a_zero_byte_transfer_is_refused_rather_than_silently_succeeding() {
    assert!(machine::apply(None, Some(plan(0)), Event::Start, 0).is_err());
}

#[test]
fn a_storage_session_is_validated_once_for_every_host() {
    let session = "https://storage.googleapis.com/upload/storage/v1/b/bucket/o?uploadType=resumable&upload_id=abc";
    assert_eq!(validated_storage_session(session).as_deref(), Ok(session));
    for (raw, refusal) in [
        ("", SessionRefusal::Unparsable),
        (
            "https://storage.googleapis.com/a\nb",
            SessionRefusal::Unparsable,
        ),
        (
            "http://storage.googleapis.com/upload/session",
            SessionRefusal::SchemeNotHttps,
        ),
        (
            "https://storage.googleapis.com.attacker.example/upload",
            SessionRefusal::HostNotAllowed,
        ),
        (
            "https://user:secret@storage.googleapis.com/upload",
            SessionRefusal::CarriesUserinfo,
        ),
        (
            "https://storage.googleapis.com:8443/upload",
            SessionRefusal::NonDefaultPort,
        ),
    ] {
        assert_eq!(validated_storage_session(raw), Err(refusal), "{raw:?}");
    }
    let long = format!("https://storage.googleapis.com/{}", "x".repeat(4096));
    assert_eq!(
        validated_storage_session(&long),
        Err(SessionRefusal::TooLong)
    );
}
