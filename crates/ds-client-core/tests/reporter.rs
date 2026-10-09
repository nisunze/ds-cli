//! `ds` reliability reporting, proved offline against the contract's own
//! acceptance scenarios (`docs/contracts/ds-cli-reliability-and-feedback.md`
//! §7.1–7.5): a transport failure waits in the outbox and the next online call
//! flushes it with the same request id; a refusal is recorded without moving
//! availability; a crash is one failure; sentinel argument values appear in no
//! event and no stored byte; and with reporting off nothing is touched.

use ds_client_core::reporter::{
    self, classify, command_event, finish_invocation, flag_names, flush, sanitize_message, status,
    EventSink, HostKind, Invocation, Platform, ReporterStore, ReporterStoreError, ResultClass,
    SendFailure, Termination, DEDUPE_WINDOW_MS, MAX_EVENTS_PER_MINUTE, OUTBOX_MAX_BYTES,
    OUTBOX_MAX_EVENTS, SEND_BUDGET,
};
use serde_json::Value;

/// 2026-10-09T05:00:00Z.
const T0: u64 = 1_791_522_000_000;
const DAY_MS: u64 = 86_400_000;

#[derive(Default)]
struct MemoryStore {
    document: Option<Vec<u8>>,
    acquired: usize,
    held: bool,
}

impl ReporterStore for MemoryStore {
    fn acquire(&mut self) -> Result<(), ReporterStoreError> {
        assert!(!self.held, "the lease is never taken twice");
        self.held = true;
        self.acquired += 1;
        Ok(())
    }
    fn load(&mut self) -> Result<Option<Vec<u8>>, ReporterStoreError> {
        assert!(self.held, "load happens under the lease");
        Ok(self.document.clone())
    }
    fn save(&mut self, document: &[u8]) -> Result<(), ReporterStoreError> {
        assert!(self.held, "save happens under the lease");
        self.document = Some(document.to_vec());
        Ok(())
    }
    fn release(&mut self) {
        self.held = false;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Gateway {
    Online,
    Unreachable,
    Refusing,
}

struct Sink {
    gateway: Gateway,
    sent: Vec<Value>,
    attempts: usize,
}

impl Sink {
    fn new(gateway: Gateway) -> Self {
        Self {
            gateway,
            sent: Vec::new(),
            attempts: 0,
        }
    }
}

impl EventSink for Sink {
    fn send(&mut self, event: &Value) -> Result<(), SendFailure> {
        self.attempts += 1;
        match self.gateway {
            Gateway::Online => {
                self.sent.push(event.clone());
                Ok(())
            }
            Gateway::Unreachable => Err(SendFailure::Unreachable),
            Gateway::Refusing => Err(SendFailure::Rejected),
        }
    }
}

fn invocation(action: &str, termination: Termination) -> Invocation {
    Invocation {
        platform: Platform::Cli,
        host_kind: HostKind::Server,
        release: "ds 0.9.12 (kernel 2997236)".into(),
        lane: "canary".into(),
        action: action.into(),
        action_class: None,
        contract: Some(2),
        latency_ms: 412,
        status: None,
        request_id: Some("req-0001".into()),
        invocation_id: reporter::new_invocation_id(),
        project: None,
        flags: Vec::new(),
        agent: Some("claude-code".into()),
        termination,
        argument_values: Vec::new(),
    }
}

fn failure(action: &str, code: &str) -> Invocation {
    invocation(
        action,
        Termination::Error {
            class: "unavailable".into(),
            code: code.into(),
            message: "the gateway did not answer".into(),
            retryable: true,
        },
    )
}

fn ok(action: &str) -> Invocation {
    invocation(action, Termination::Ok)
}

fn outbox_len(store: &mut MemoryStore) -> usize {
    status(store, true).expect("status").outbox_events
}

/// §7.1: the command fails exactly as before, the event sits in the outbox,
/// and the next online call flushes it with the same `request_id`.
#[test]
fn a_transport_failure_waits_in_the_outbox_and_the_next_online_call_flushes_it() {
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    let mut failed = failure("data.vector.buffer", "gateway_unreachable");
    failed.request_id = Some("req-7c1f".into());
    failed.status = None;

    let recorded =
        finish_invocation(true, &mut store, &mut offline, &failed, T0).expect("reporting is on");
    assert_eq!(recorded.result_class, ResultClass::Failed);
    assert!(recorded.queued);
    assert_eq!(offline.attempts, 1, "the exit path tries the send once");
    let held = status(&mut store, true).expect("status");
    assert_eq!(held.outbox_events, 1);
    assert_eq!(held.last_flush.as_deref(), Some("offline"));

    let mut online = Sink::new(Gateway::Online);
    finish_invocation(
        true,
        &mut store,
        &mut online,
        &ok("project.list"),
        T0 + 1_000,
    );
    assert_eq!(
        online.sent.len(),
        1,
        "the next online invocation flushes it"
    );
    let event = &online.sent[0];
    assert_eq!(event["request_id"], "req-7c1f");
    assert_eq!(event["source"], "cli_command");
    assert_eq!(event["result_class"], "failed");
    assert_eq!(event["outcome"], "failure");
    assert_eq!(event["action"], "data.vector.buffer");
    assert_eq!(event["user_action_id"], failed.invocation_id.as_str());
    assert_eq!(outbox_len(&mut store), 0);
    assert_eq!(
        status(&mut store, true).unwrap().last_flush.as_deref(),
        Some("sent")
    );
}

/// No recursion: the failed send is never itself an event.
#[test]
fn a_failed_send_is_never_reported() {
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    for at in [T0, T0 + 40_000, T0 + 80_000] {
        finish_invocation(
            true,
            &mut store,
            &mut offline,
            &failure("map.capture", "capture_timeout"),
            at,
        );
    }
    let document: Value = serde_json::from_slice(store.document.as_ref().unwrap()).unwrap();
    let outbox = document["outbox"].as_array().unwrap();
    assert_eq!(outbox.len(), 3, "three windows, three events, nothing else");
    assert!(outbox
        .iter()
        .all(|queued| queued["event"]["action"] == "map.capture"));
}

/// §7.2: a declared refusal is `refused` / `success` and never counts against
/// availability; a gateway 5xx is a failure whatever its class.
#[test]
fn a_refusal_is_recorded_as_success_and_never_moves_availability() {
    for (class, code) in [
        ("invalid_input", "confirmation_required"),
        ("unauthorized", "feedback_not_permitted"),
        ("conflict", "headless_signed_out"),
    ] {
        let refused = invocation(
            "feedback.submit",
            Termination::Error {
                class: class.into(),
                code: code.into(),
                message: "re-run with --yes".into(),
                retryable: false,
            },
        );
        assert_eq!(refused.result_class(), ResultClass::Refused, "{code}");
        assert!(!refused.result_class().counts_against_availability());
        let event = command_event(&refused).expect("a refusal is recorded");
        assert_eq!(event["result_class"], "refused");
        assert_eq!(event["outcome"], "success");
        assert_eq!(event["error_code"], code);
        assert_eq!(event["error_class"], class);
    }
    for class in ["internal", "failed", "unavailable", "something_new"] {
        let failed = Termination::Error {
            class: class.into(),
            code: "x".into(),
            message: String::new(),
            retryable: false,
        };
        assert_eq!(classify(&failed, None), ResultClass::Failed, "{class}");
    }
    let refused_by_class = Termination::Error {
        class: "invalid_input".into(),
        code: "x".into(),
        message: String::new(),
        retryable: false,
    };
    assert_eq!(classify(&refused_by_class, Some(502)), ResultClass::Failed);
    assert_eq!(classify(&refused_by_class, Some(400)), ResultClass::Refused);
    assert_eq!(classify(&Termination::Ok, Some(200)), ResultClass::Ok);
    assert!(
        command_event(&ok("project.list")).is_none(),
        "ok is never sent per call"
    );
}

/// §7.3, the classification half: a panic is one `crashed` failure.
#[test]
fn a_crash_is_one_crashed_failure() {
    let crashed = invocation(
        "dsgrid.inspect",
        Termination::Crashed {
            message: "panicked at src/lib.rs:12:5: index out of bounds".into(),
        },
    );
    assert_eq!(crashed.result_class(), ResultClass::Crashed);
    assert!(crashed.result_class().counts_against_availability());
    let event = command_event(&crashed).unwrap();
    assert_eq!(event["result_class"], "crashed");
    assert_eq!(event["outcome"], "failure");
    assert_eq!(event["error_class"], "internal");
    assert!(!event["error_message"]
        .as_str()
        .unwrap()
        .contains("src/lib.rs"));
}

/// §7.4: sentinel values in every argument of a failing command appear in no
/// event and in no byte of the stored outbox.
#[test]
fn sentinel_argument_values_never_leave_the_machine() {
    let sentinels = [
        "/home/sentinel-user/Projects/SENTINEL_DIR/parcels-SENTINEL.geojson",
        "C:\\Users\\SentinelOwner\\Desktop\\SENTINEL_MODEL.dsgrid",
        "SENTINEL-VALUE-7731",
        "sentinel.person@example.org",
        "30.0619,-1.9441",
        "sk_SENTINEL0123456789abcdefghij",
        "Customer Sentinel Mukamana",
    ];
    let message = format!(
        "could not read '{}' (parcels-SENTINEL.geojson) or {}; value {} for --radius-m rejected; \
         owner {} at {} with token={} for \"{}\"; also https://storage.example.org/SENTINEL_BUCKET/x \
         and ~/SENTINEL_HOME/notes.txt and 123456789 and SENTINEL_MODEL.dsgrid",
        sentinels[0], sentinels[1], sentinels[2], sentinels[3], sentinels[4], sentinels[5], sentinels[6],
    );
    let mut failed = invocation(
        "data.vector.buffer",
        Termination::Error {
            class: "failed".into(),
            code: "vector_engine_empty".into(),
            message,
            retryable: false,
        },
    );
    failed.flags = vec![
        format!("--source={}", sentinels[0]),
        "--radius-m".into(),
        sentinels[2].into(),
        sentinels[1].into(),
        "--output".into(),
        "--radius-m".into(),
    ];
    failed.argument_values = sentinels.iter().map(|value| value.to_string()).collect();

    let event = command_event(&failed).unwrap();
    assert_eq!(
        event["flags"],
        serde_json::json!(["--source", "--radius-m", "--output"]),
        "flag names only, deduplicated"
    );
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    finish_invocation(true, &mut store, &mut offline, &failed, T0);
    let stored = String::from_utf8(store.document.clone().unwrap()).unwrap();
    let sent = event.to_string();
    for haystack in [sent.as_str(), stored.as_str()] {
        for needle in [
            "SENTINEL",
            "Sentinel",
            "sentinel",
            "7731",
            "30.0619",
            "1.9441",
            "example.org",
            "Mukamana",
            "notes.txt",
            "123456789",
        ] {
            assert!(
                !haystack.contains(needle),
                "`{needle}` left the machine: {haystack}"
            );
        }
    }
    // The diagnosis survives the scrubbing.
    let kept = event["error_message"].as_str().unwrap();
    assert!(kept.contains("could not read"), "{kept}");
    assert!(kept.contains("--radius-m rejected"), "{kept}");
}

#[test]
fn the_sanitizer_drops_path_and_value_shapes_without_argument_values() {
    let cleaned = sanitize_message(
        "open /var/lib/ds/state.json failed: C:\\Data\\a.dsgrid; email a.b@c.rw; \
         lat=-1.95 key ABCDEFGHIJ0123456789xyz; 'quoted secret'; can't proceed; 502 Bad Gateway",
        &[],
    );
    for gone in [
        "/var/lib",
        "state.json",
        "a.dsgrid",
        "a.b@c.rw",
        "-1.95",
        "ABCDEFGHIJ0123456789xyz",
        "quoted secret",
    ] {
        assert!(!cleaned.contains(gone), "`{gone}` survived: {cleaned}");
    }
    for kept in ["can't proceed", "502 Bad Gateway", "lat=<value>", "<path>"] {
        assert!(cleaned.contains(kept), "`{kept}` was lost: {cleaned}");
    }
    let long = "x ".repeat(5_000);
    assert!(sanitize_message(&long, &[]).chars().count() <= reporter::MAX_ERROR_MESSAGE_CHARS);
    assert_eq!(
        flag_names(&["-v".into(), "--Bad".into(), "---x".into(), "value".into()]),
        vec!["-v".to_string()]
    );
}

/// §7.5, the reporter's half: off touches neither the store nor the sink, and
/// nothing the reporter returns is output. (The CLI proves byte-identical
/// stdout, stderr and exit codes on top of this.)
#[test]
fn reporting_off_touches_neither_store_nor_sink() {
    assert!(!reporter::reporting_enabled(Some("off")));
    assert!(!reporter::reporting_enabled(Some(" OFF ")));
    assert!(reporter::reporting_enabled(None));
    assert!(reporter::reporting_enabled(Some("on")));
    let mut store = MemoryStore::default();
    let mut sink = Sink::new(Gateway::Online);
    let answer = finish_invocation(
        false,
        &mut store,
        &mut sink,
        &failure("data.vector.buffer", "gateway_unreachable"),
        T0,
    );
    assert!(answer.is_none());
    assert_eq!(store.acquired, 0);
    assert!(store.document.is_none());
    assert_eq!(sink.attempts, 0);
}

/// Identical `(action, error_code)` within 30 s is one event with
/// `occurrences`: the first is sent at once, the repeats are held and sent
/// as one event when the window closes. Nothing is lost.
#[test]
fn identical_endings_within_the_window_are_one_event_with_occurrences() {
    let mut store = MemoryStore::default();
    let mut sink = Sink::new(Gateway::Online);
    for offset in [0, 5_000, 10_000] {
        finish_invocation(
            true,
            &mut store,
            &mut sink,
            &failure("map.capture", "capture_timeout"),
            T0 + offset,
        );
    }
    assert_eq!(sink.sent.len(), 1, "the first ending is sent at once");
    assert_eq!(outbox_len(&mut store), 1, "the repeats are one held event");
    finish_invocation(
        true,
        &mut store,
        &mut sink,
        &ok("project.list"),
        T0 + DEDUPE_WINDOW_MS + 1,
    );
    assert_eq!(sink.sent.len(), 2);
    assert_eq!(sink.sent[0]["occurrences"], 1);
    assert_eq!(sink.sent[1]["occurrences"], 2);

    // Offline, the repeats fold into the waiting event.
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    for offset in [0, 1_000, 2_000] {
        let recorded = finish_invocation(
            true,
            &mut store,
            &mut offline,
            &failure("map.capture", "capture_timeout"),
            T0 + offset,
        )
        .unwrap();
        assert_eq!(recorded.coalesced, offset > 0);
    }
    let document: Value = serde_json::from_slice(store.document.as_ref().unwrap()).unwrap();
    assert_eq!(document["outbox"].as_array().unwrap().len(), 1);
    assert_eq!(document["outbox"][0]["event"]["occurrences"], 3);
}

#[test]
fn at_most_thirty_events_leave_a_host_in_a_minute() {
    let mut store = MemoryStore::default();
    let mut sink = Sink::new(Gateway::Online);
    for index in 0..(MAX_EVENTS_PER_MINUTE + 5) {
        finish_invocation(
            true,
            &mut store,
            &mut sink,
            &failure("data.vector.buffer", &format!("code_{index}")),
            T0 + index as u64,
        );
    }
    assert_eq!(sink.sent.len(), MAX_EVENTS_PER_MINUTE);
    let held = status(&mut store, true).unwrap();
    assert_eq!(held.outbox_events, 5);
    assert_eq!(held.last_flush.as_deref(), Some("rate_limited"));
    flush(&mut store, &mut sink, T0 + 61_000, SEND_BUDGET).unwrap();
    assert_eq!(sink.sent.len(), MAX_EVENTS_PER_MINUTE + 5);
}

#[test]
fn the_outbox_is_bounded_and_counts_what_it_drops() {
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    for index in 0..(OUTBOX_MAX_EVENTS + 5) {
        finish_invocation(
            true,
            &mut store,
            &mut offline,
            &failure("data.vector.buffer", &format!("code_{index}")),
            T0 + index as u64,
        );
    }
    let held = status(&mut store, true).unwrap();
    assert_eq!(held.outbox_events, OUTBOX_MAX_EVENTS);
    assert_eq!(held.dropped, 5, "the oldest are dropped first, and counted");
    let document: Value = serde_json::from_slice(store.document.as_ref().unwrap()).unwrap();
    assert_eq!(
        document["outbox"][0]["event"]["error_code"], "code_5",
        "the oldest five went"
    );

    let mut store = MemoryStore::default();
    for index in 0..150 {
        let mut large = failure("data.vector.buffer", &format!("code_{index}"));
        large.termination = Termination::Error {
            class: "failed".into(),
            code: format!("code_{index}"),
            message: "word ".repeat(400),
            retryable: false,
        };
        finish_invocation(true, &mut store, &mut offline, &large, T0 + index as u64);
    }
    let held = status(&mut store, true).unwrap();
    assert!(
        held.outbox_bytes <= OUTBOX_MAX_BYTES,
        "{}",
        held.outbox_bytes
    );
    assert!(held.dropped > 0);
}

/// Successes are counted locally and sent as one `cli_rollup` per host per
/// day, never one event per call.
#[test]
fn successes_become_one_rollup_event_per_day() {
    let mut store = MemoryStore::default();
    let mut sink = Sink::new(Gateway::Online);
    for (offset, action) in [
        (0, "project.list"),
        (1, "project.list"),
        (2, "data.vector.buffer"),
    ] {
        finish_invocation(true, &mut store, &mut sink, &ok(action), T0 + offset);
    }
    assert!(sink.sent.is_empty(), "no per-call success events");
    let today = status(&mut store, true).unwrap();
    assert_eq!(today.rollup_commands, 2);
    assert_eq!(today.rollup_day.as_deref(), Some("2026-10-09"));

    finish_invocation(
        true,
        &mut store,
        &mut sink,
        &ok("project.list"),
        T0 + DAY_MS,
    );
    assert_eq!(sink.sent.len(), 1);
    let rollup = &sink.sent[0];
    assert_eq!(rollup["source"], "cli_rollup");
    assert_eq!(rollup["user_action_id"], "rollup_2026-10-09");
    assert_eq!(rollup["counts"]["project.list"], 2);
    assert_eq!(rollup["counts"]["data.vector.buffer"], 1);
    assert_eq!(rollup["host_kind"], "server");
    let next = status(&mut store, true).unwrap();
    assert_eq!(next.rollup_day.as_deref(), Some("2026-10-10"));
    assert_eq!(next.rollup_commands, 1);
}

#[test]
fn a_body_the_store_refuses_is_dropped_and_counted_never_retried() {
    let mut store = MemoryStore::default();
    let mut refusing = Sink::new(Gateway::Refusing);
    finish_invocation(
        true,
        &mut store,
        &mut refusing,
        &failure("data.vector.buffer", "gateway_unreachable"),
        T0,
    );
    let after = status(&mut store, true).unwrap();
    assert_eq!(after.outbox_events, 0);
    assert_eq!(after.rejected, 1);
    finish_invocation(true, &mut store, &mut refusing, &ok("project.list"), T0 + 1);
    assert_eq!(refusing.attempts, 1, "a refused body is never resent");
}

/// Every field sent is a field of the store's `ds` client body (ds-sre
/// `REQUEST_EVENT_STORE_CONTRACT.md` §2.3) — nothing else rides along.
#[test]
fn every_field_sent_is_a_store_field() {
    const STORE_FIELDS: &[&str] = &[
        "source",
        "platform",
        "host_kind",
        "os",
        "release",
        "lane",
        "action",
        "action_class",
        "contract",
        "result_class",
        "outcome",
        "error_code",
        "error_class",
        "retryable",
        "latency_ms",
        "status",
        "request_id",
        "user_action_id",
        "ds_project",
        "flags",
        "agent",
        "error_message",
        "occurrences",
        "counts",
    ];
    let mut full = failure("data.vector.buffer", "gateway_unreachable");
    full.action_class = Some("report".into());
    full.status = Some(502);
    full.project = Some("kigali_north".into());
    full.flags = vec!["--source".into()];
    let event = command_event(&full).unwrap();
    for key in event.as_object().unwrap().keys() {
        assert!(
            STORE_FIELDS.contains(&key.as_str()),
            "`{key}` is not a store field"
        );
    }
    assert_eq!(event["ds_project"], "kigali_north");
    assert_eq!(event["status"], 502);
    assert_eq!(event["action_class"], "report");
    assert_eq!(event["platform"], "cli");
    assert_eq!(event["lane"], "canary");
    assert_eq!(event["contract"], 2);

    let mut store = MemoryStore::default();
    let mut sink = Sink::new(Gateway::Online);
    finish_invocation(true, &mut store, &mut sink, &ok("project.list"), T0);
    finish_invocation(
        true,
        &mut store,
        &mut sink,
        &ok("project.list"),
        T0 + DAY_MS,
    );
    for key in sink.sent[0].as_object().unwrap().keys() {
        assert!(STORE_FIELDS.contains(&key.as_str()), "rollup `{key}`");
    }
    let id = reporter::new_invocation_id();
    assert!(id.starts_with("inv_") && id.len() == 36, "{id}");
}

/// §4 and the kernel half of §7.6: the journal keeps the last 50 invocations
/// and no value; a draft names everything except what only the caller knows —
/// what it expected and what would show it works.
#[test]
fn the_journal_keeps_fifty_and_a_draft_needs_only_expectation_and_acceptance() {
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    for index in 0..60 {
        finish_invocation(
            true,
            &mut store,
            &mut offline,
            &ok("project.list"),
            T0 + index,
        );
    }
    let mut failed = invocation(
        "data.vector.buffer",
        Termination::Error {
            class: "failed".into(),
            code: "vector_engine_empty".into(),
            message: "SENTINEL-DRAFT could not be read".into(),
            retryable: false,
        },
    );
    failed.request_id = Some("req-draft".into());
    failed.flags = vec!["--source".into(), "--radius-m".into()];
    failed.argument_values = vec!["SENTINEL-DRAFT".into()];
    finish_invocation(true, &mut store, &mut offline, &failed, T0 + 100);

    let entries = reporter::journal_entries(&mut store).unwrap();
    assert_eq!(entries.len(), reporter::JOURNAL_ENTRIES);
    assert_eq!(entries[0].action, "data.vector.buffer", "newest first");
    assert_eq!(entries[0].result_class, "failed");
    assert_eq!(
        entries[0].error_code.as_deref(),
        Some("vector_engine_empty")
    );

    let drafts = reporter::draft(&entries, 3);
    assert_eq!(
        drafts.len(),
        1,
        "only failed, crashed and refused endings are drafted"
    );
    let draft = &drafts[0];
    assert_eq!(draft["kind"], "bug");
    assert_eq!(draft["severity"], "minor");
    assert_eq!(draft["component"], "ds-cli/data");
    assert_eq!(draft["agent"], "claude-code");
    assert_eq!(draft["context"]["sre_event"], failed.invocation_id.as_str());
    assert_eq!(draft["context"]["lane"], "canary");
    let evidence: Vec<&str> = draft["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect();
    assert!(evidence.contains(&"request id req-draft"), "{evidence:?}");
    assert!(
        evidence.contains(&"flags --source --radius-m"),
        "{evidence:?}"
    );
    // Inside feedback.submit's bounds: title, evidence count and length, context.
    assert!(draft["title"].as_str().unwrap().chars().count() <= 200);
    assert!(evidence.len() <= 20 && evidence.iter().all(|line| line.len() <= 500));
    assert!(draft["context"].as_object().unwrap().len() <= 24);
    assert!(draft["detail"]
        .as_str()
        .unwrap()
        .contains("Expected: <what you expected>"));
    let text = draft.to_string();
    assert!(!text.contains("SENTINEL"), "{text}");

    let refused = invocation(
        "feedback.submit",
        Termination::Error {
            class: "invalid_input".into(),
            code: "confirmation_required".into(),
            message: String::new(),
            retryable: false,
        },
    );
    finish_invocation(true, &mut store, &mut offline, &refused, T0 + 200);
    let entries = reporter::journal_entries(&mut store).unwrap();
    let drafts = reporter::draft(&entries, 3);
    assert_eq!(drafts.len(), 2);
    assert_eq!(
        drafts[0]["kind"], "friction",
        "a refusal drafts as friction"
    );
}

#[test]
fn a_state_document_from_before_the_journal_still_reads() {
    let mut store = MemoryStore {
        document: Some(br#"{"schema":"ds.sre-reporter/v1","outbox":[],"dropped":3}"#.to_vec()),
        ..MemoryStore::default()
    };
    let status = status(&mut store, true).unwrap();
    assert_eq!(status.dropped, 3);
    assert!(reporter::journal_entries(&mut store).unwrap().is_empty());
}

struct Backlog {
    online: bool,
    submitted: Vec<Value>,
}

impl reporter::ReportSink for Backlog {
    fn submit(&mut self, payload: &Value) -> Result<(), SendFailure> {
        if !self.online {
            return Err(SendFailure::Unreachable);
        }
        self.submitted.push(payload.clone());
        Ok(())
    }
}

fn crash(action: &str) -> Invocation {
    let mut crashed = invocation(
        action,
        Termination::Crashed {
            message: "SENTINEL-PANIC at /home/sentinel/private.geojson".into(),
        },
    );
    crashed.argument_values = vec!["SENTINEL-VALUE".into()];
    crashed
}

/// §5.3 and §7.3: a crash queues one automatic `bug` report as `ds
/// (automatic)` with the SRE event id; a repeat raises its count and queues
/// nothing new; a signed-out or offline host keeps it; the next signed-in send
/// files it once and nothing waits afterwards. No value or message is in it.
#[test]
fn a_crash_files_one_automatic_bug_and_a_repeat_only_raises_its_count() {
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    let first = crash("data.vector.buffer");
    let recorded = finish_invocation(true, &mut store, &mut offline, &first, T0).unwrap();
    assert_eq!(recorded.result_class, ResultClass::Crashed);
    assert_eq!(recorded.automatic, reporter::AutomaticReporting::Queued);
    let again = finish_invocation(
        true,
        &mut store,
        &mut offline,
        &crash("data.vector.buffer"),
        T0 + 1_000,
    )
    .unwrap();
    assert_eq!(
        again.automatic,
        reporter::AutomaticReporting::Raised { occurrences: 2 }
    );
    let other = finish_invocation(
        true,
        &mut store,
        &mut offline,
        &failure("data.vector.buffer", "gateway_unreachable"),
        T0 + 2_000,
    )
    .unwrap();
    assert_eq!(
        other.automatic,
        reporter::AutomaticReporting::None,
        "only a crash is filed by ds itself"
    );
    assert_eq!(status(&mut store, true).unwrap().automatic_reports, 1);

    let mut signed_out = Backlog {
        online: false,
        submitted: Vec::new(),
    };
    let kept = reporter::file_automatic(&mut store, &mut signed_out).unwrap();
    assert!(kept.offline);
    assert_eq!((kept.filed, kept.kept), (0, 1));
    assert!(reporter::has_automatic(&mut store).unwrap());

    let mut backlog = Backlog {
        online: true,
        submitted: Vec::new(),
    };
    let filed = reporter::file_automatic(&mut store, &mut backlog).unwrap();
    assert_eq!((filed.filed, filed.kept), (1, 0));
    assert!(!reporter::has_automatic(&mut store).unwrap());
    let report = &backlog.submitted[0];
    assert_eq!(report["agent"], reporter::AUTOMATIC_AGENT);
    assert_eq!(report["kind"], "bug");
    assert_eq!(report["severity"], "major");
    assert_eq!(report["title"], "ds data.vector.buffer crashed: panic");
    assert_eq!(report["context"]["sre_event"], first.invocation_id.as_str());
    assert!(
        report.get("needs").is_none(),
        "nothing is left to the caller"
    );
    assert!(report["detail"]
        .as_str()
        .unwrap()
        .starts_with("Expected: ds data.vector.buffer answers"));
    assert!(report["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line
            .as_str()
            .unwrap()
            .starts_with("happened 2 times since ")));
    let text = report.to_string();
    for sentinel in ["SENTINEL", "sentinel", "private.geojson"] {
        assert!(!text.contains(sentinel), "`{sentinel}` in {text}");
    }
    // The event keeps the sanitized message (§3.3); no argument value or path
    // reaches the stored document.
    let stored = String::from_utf8(store.document.clone().unwrap()).unwrap();
    for sentinel in ["SENTINEL-VALUE", "private.geojson", "/home/"] {
        assert!(!stored.contains(sentinel), "`{sentinel}` in {stored}");
    }

    // A crash after the report was filed queues it again; the backlog merges
    // the same title into that report's occurrences.
    let later = finish_invocation(
        true,
        &mut store,
        &mut offline,
        &crash("data.vector.buffer"),
        T0 + 60_000,
    )
    .unwrap();
    assert_eq!(later.automatic, reporter::AutomaticReporting::Queued);
    reporter::file_automatic(&mut store, &mut backlog).unwrap();
    assert_eq!(backlog.submitted[1]["title"], backlog.submitted[0]["title"]);
}

/// §5.4: a failed or crashed envelope names the draft command and its SRE
/// event id; an answer or a refusal names nothing.
#[test]
fn only_a_failure_or_crash_envelope_points_to_a_feedback_draft() {
    let pointer = reporter::next_feedback(ResultClass::Failed, "inv_0123").unwrap();
    assert_eq!(pointer["draft"], "ds feedback draft --last");
    assert_eq!(pointer["sre_event"], "inv_0123");
    assert!(reporter::next_feedback(ResultClass::Crashed, "inv_0123").is_some());
    assert!(reporter::next_feedback(ResultClass::Refused, "inv_0123").is_none());
    assert!(reporter::next_feedback(ResultClass::Ok, "inv_0123").is_none());
}

/// §5.3: an `internal` error is filed by `ds` itself as well; a transport
/// failure or a refusal is not.
#[test]
fn an_internal_error_is_filed_automatically_and_a_transport_failure_is_not() {
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    let internal = invocation(
        "dsgrid.describe",
        Termination::Error {
            class: "internal".into(),
            code: "descriptor_unreadable".into(),
            message: "SENTINEL could not be described".into(),
            retryable: false,
        },
    );
    let recorded = finish_invocation(true, &mut store, &mut offline, &internal, T0).unwrap();
    assert_eq!(recorded.result_class, ResultClass::Failed);
    assert_eq!(recorded.automatic, reporter::AutomaticReporting::Queued);
    let transport = finish_invocation(
        true,
        &mut store,
        &mut offline,
        &failure("dsgrid.describe", "gateway_unreachable"),
        T0 + 1,
    )
    .unwrap();
    assert_eq!(transport.automatic, reporter::AutomaticReporting::None);

    let mut backlog = Backlog {
        online: true,
        submitted: Vec::new(),
    };
    reporter::file_automatic(&mut store, &mut backlog).unwrap();
    assert_eq!(backlog.submitted.len(), 1);
    assert_eq!(
        backlog.submitted[0]["title"],
        "ds dsgrid.describe failed: descriptor_unreadable"
    );
    assert!(!backlog.submitted[0].to_string().contains("SENTINEL"));
}

/// A repeat folded into an earlier event's occurrences is journalled under
/// that event's id, so its draft names an SRE event that exists.
#[test]
fn a_coalesced_ending_is_drafted_under_the_event_that_holds_it() {
    let mut store = MemoryStore::default();
    let mut offline = Sink::new(Gateway::Unreachable);
    let first = failure("data.vector.buffer", "gateway_unreachable");
    let second = failure("data.vector.buffer", "gateway_unreachable");
    finish_invocation(true, &mut store, &mut offline, &first, T0).unwrap();
    let repeat = finish_invocation(true, &mut store, &mut offline, &second, T0 + 1_000).unwrap();
    assert!(repeat.coalesced);
    assert_eq!(
        repeat.event_id.as_deref(),
        Some(first.invocation_id.as_str())
    );
    let entries = reporter::journal_entries(&mut store).unwrap();
    assert_eq!(entries[0].invocation_id, first.invocation_id);
    let drafts = reporter::draft(&entries, 1);
    assert_eq!(
        drafts[0]["context"]["sre_event"],
        first.invocation_id.as_str()
    );
}
