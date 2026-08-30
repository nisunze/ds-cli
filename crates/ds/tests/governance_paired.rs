//! `ds governance architecture` against a mocked bridge transport.
//!
//! The transport is a loopback socket this test owns, not a network and not a
//! real desktop: what is being proven is the closed wire contract. A suite
//! that only checked help would not catch a renamed action, a filter sent as
//! an empty string, a conflict quietly retried against the head that moved, or
//! an idempotency key minted fresh on every run.
//!
//! The five properties worth naming, because each is a way this family could
//! be wrong while compiling and helping correctly:
//!
//! 1. Every subcommand sends the ds-brain body for the action it names.
//! 2. A filter the caller did not pass is omitted, never sent empty.
//! 3. `apply` without `--yes` reaches no socket at all.
//! 4. A `revision_conflict` is reported once and never re-sent.
//! 5. `applied: false` is an idempotent replay, and therefore a success.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// The contract's own example proposal, minus whatever a test omits.
fn proposal(body: Value) -> Fixture {
    let unique = unique();
    let path = std::env::temp_dir().join(format!(
        "ds-cli-governance-proposal-{}-{unique}.json",
        std::process::id()
    ));
    std::fs::write(
        &path,
        serde_json::to_vec(&body).expect("serialize proposal"),
    )
    .expect("write proposal");
    Fixture { path }
}

struct Fixture {
    path: PathBuf,
}

impl Fixture {
    fn arg(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn update_node_command() -> Value {
    json!({
        "id": "record-form-factory-question",
        "kind": "update_node",
        "target_id": "survey-form-factory",
        "node": { "delivery_state": "user_question", "chapter": "survey-lifecycle" },
    })
}

fn snapshot_reply(revision: u64, applied: bool, preview: bool) -> Value {
    json!({
        "snapshot": { "nodes": [], "edges": [] },
        "revision": revision,
        "actor": "ops@example.test",
        "at": "2026-08-29T09:00:00Z",
        "command_id": "record-form-factory-question",
        "applied": applied,
        "preview": preview,
    })
}

/// A name no other fixture in this process can take.
///
/// The clock alone is not enough. Tests here run in parallel threads of one
/// process, and on a host whose clock granularity is coarse two of them can
/// read the same instant — at which point both write the same descriptor path
/// and each `ds` is pointed at the other's socket. That failure looks like a
/// wrong operation name, which is the one thing this suite exists to detect,
/// so the counter is what keeps a real regression distinguishable from a
/// collision.
fn unique() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )
}

struct Bridge {
    descriptor: PathBuf,
    server: JoinHandle<Vec<Value>>,
}

impl Bridge {
    fn arg(&self) -> String {
        self.descriptor.to_string_lossy().into_owned()
    }
}

fn ds(args: &[&str]) -> (Value, i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let envelope = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("ds did not return JSON ({error}): {stdout}{stderr}"));
    (envelope, output.status.code().unwrap_or(-1), stdout, stderr)
}

fn write_descriptor(address: std::net::SocketAddr, label: &str) -> PathBuf {
    let descriptor = std::env::temp_dir().join(format!(
        "ds-cli-governance-{label}-{}-{}.json",
        std::process::id(),
        unique()
    ));
    let body = json!({
        "version": 1,
        "url": format!("http://{address}"),
        "token": "test-pairing-secret",
        "pid": 1,
    });
    std::fs::write(
        &descriptor,
        serde_json::to_vec(&body).expect("serialize descriptor"),
    )
    .expect("write descriptor");
    descriptor
}

fn paired(replies: Vec<(&'static str, Value)>) -> Bridge {
    bridge_with_status(
        replies
            .into_iter()
            .map(|(operation, reply)| (operation, 200, reply))
            .collect(),
    )
}

fn bridge_with_status(replies: Vec<(&'static str, u16, Value)>) -> Bridge {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback bridge");
    let address = listener.local_addr().expect("bridge address");
    listener
        .set_nonblocking(true)
        .expect("make loopback bridge nonblocking");
    let server = thread::spawn(move || {
        let mut received = Vec::new();
        for (expected_operation, status, reply) in replies {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "bridge timed out waiting for `{expected_operation}`; ds refused before it sent one"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("bridge accepts `{expected_operation}`: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("set bridge read timeout");
            let request = read_json_request(&mut stream);
            assert_eq!(
                request["operation"], expected_operation,
                "the CLI must call the closed operation it declares"
            );
            received.push(request);

            let reply = serde_json::to_vec(&reply).expect("serialize bridge reply");
            let headers = format!(
                "HTTP/1.1 {status} REPLY\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            );
            stream.write_all(headers.as_bytes()).expect("write headers");
            stream.write_all(&reply).expect("write body");
            stream.flush().expect("flush bridge reply");
        }
        received
    });

    Bridge {
        descriptor: write_descriptor(address, "bridge"),
        server,
    }
}

/// A paired session that must never be contacted.
///
/// The listener stays bound in this thread, so a connection `ds` made anyway
/// sits in the accept backlog and is found — which a bridge whose server
/// thread had already exited would silently miss.
struct QuietBridge {
    descriptor: PathBuf,
    listener: TcpListener,
}

fn quiet_bridge() -> QuietBridge {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback bridge");
    let address = listener.local_addr().expect("bridge address");
    listener
        .set_nonblocking(true)
        .expect("make loopback bridge nonblocking");
    QuietBridge {
        descriptor: write_descriptor(address, "quiet"),
        listener,
    }
}

impl QuietBridge {
    fn arg(&self) -> String {
        self.descriptor.to_string_lossy().into_owned()
    }

    fn assert_untouched(self, why: &str) {
        let contacted = self.listener.accept().is_ok();
        let _ = std::fs::remove_file(&self.descriptor);
        assert!(!contacted, "{why}");
    }
}

fn finish(bridge: Bridge) -> Vec<Value> {
    let Bridge { descriptor, server } = bridge;
    let _ = std::fs::remove_file(descriptor);
    server.join().expect("bridge server did not panic")
}

fn read_json_request(stream: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).expect("read bridge request");
        assert!(count > 0, "bridge client closed before completing request");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break at;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).expect("request headers UTF-8");
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
        })
        .expect("bridge requests declare a content length");
    let mut body = bytes[header_end + 4..].to_vec();
    while body.len() < content_length {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).expect("read bridge body");
        assert!(count > 0, "bridge client closed mid-body");
        body.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&body[..content_length]).expect("bridge request body is JSON")
}

// ---------------------------------------------------------------------------
// 1. Every subcommand sends the body for the action it names
// ---------------------------------------------------------------------------

#[test]
fn get_sends_the_get_action_and_nothing_else() {
    let bridge = paired(vec![(
        "governance.architecture.get",
        json!({ "snapshot": { "nodes": [], "edges": [] }, "revision": 12 }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "get",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["data"]["revision"], 12);

    let requests = finish(bridge);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["arguments"], json!({ "action": "get" }));
}

#[test]
fn list_sends_every_filter_the_caller_passed() {
    let bridge = paired(vec![(
        "governance.architecture.list",
        json!({ "revision": 12, "nodes": [], "edges": [] }),
    )]);
    let descriptor = bridge.arg();
    let (_, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "list",
        "--chapter",
        "survey-lifecycle",
        "--state",
        "wishlist",
        "--include-archived",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");

    let requests = finish(bridge);
    assert_eq!(
        requests[0]["arguments"],
        json!({
            "action": "list",
            "chapter": "survey-lifecycle",
            "state": "wishlist",
            "include_archived": true,
        })
    );
}

#[test]
fn list_omits_the_filters_the_caller_did_not_pass() {
    // The negative control for the whole payload. ds-brain reads an absent
    // `chapter` as every chapter; `""` would ask for the chapter whose id is
    // the empty string, which is a different — and always empty — question.
    let bridge = paired(vec![(
        "governance.architecture.list",
        json!({ "revision": 12, "nodes": [], "edges": [] }),
    )]);
    let descriptor = bridge.arg();
    let (_, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "list",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");

    let requests = finish(bridge);
    assert_eq!(requests[0]["arguments"], json!({ "action": "list" }));
    for absent in ["chapter", "state", "include_archived"] {
        assert!(
            requests[0]["arguments"].get(absent).is_none(),
            "`{absent}` was sent although the caller never passed it"
        );
    }
}

#[test]
fn history_sends_its_bound_and_its_cursor() {
    let bridge = paired(vec![(
        "governance.architecture.history",
        json!({ "revisions": [], "next_cursor": "c-2" }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "history",
        "--limit",
        "20",
        "--cursor",
        "c-1",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["data"]["next_cursor"], "c-2");

    let requests = finish(bridge);
    assert_eq!(
        requests[0]["arguments"],
        json!({ "action": "history", "limit": 20, "cursor": "c-1" })
    );
}

#[test]
fn a_history_page_with_no_next_cursor_is_the_end_and_not_a_failure() {
    // `next_cursor` is omitted when the history is exhausted. Requiring it
    // would turn the last page of every history into a contract mismatch.
    let bridge = paired(vec![(
        "governance.architecture.history",
        json!({ "revisions": [{
            "revision": 12, "actor": "ops@example.test", "at": "2026-08-29T09:00:00Z",
            "command_id": "c1", "kind": "update_node", "target_id": "survey-form-factory",
        }] }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "history",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(envelope["data"].get("next_cursor").is_none());
    let requests = finish(bridge);
    // No `--limit` was passed, but the flag declares a default, so the
    // documented page size is what goes on the wire.
    assert_eq!(
        requests[0]["arguments"],
        json!({ "action": "history", "limit": 20 })
    );
}

#[test]
fn preview_sends_the_proposals_own_fence_and_writes_nothing() {
    let file = proposal(json!({
        "expected_revision": 12,
        "idempotency_key": "stable-command-id",
        "command": update_node_command(),
    }));
    let bridge = paired(vec![(
        "governance.architecture.preview",
        snapshot_reply(12, false, true),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "preview",
        "--command",
        &file.arg(),
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["data"]["applied"], false);

    let requests = finish(bridge);
    assert_eq!(requests.len(), 1, "a fenced proposal needs no head read");
    assert_eq!(
        requests[0]["arguments"],
        json!({
            "action": "preview",
            "expected_revision": 12,
            "idempotency_key": "stable-command-id",
            "command": update_node_command(),
        })
    );
}

#[test]
fn preview_reads_the_head_when_the_proposal_names_no_revision() {
    let file = proposal(json!({ "command": update_node_command() }));
    let bridge = paired(vec![
        (
            "governance.architecture.get",
            json!({ "snapshot": { "nodes": [] }, "revision": 31 }),
        ),
        (
            "governance.architecture.preview",
            snapshot_reply(31, false, true),
        ),
    ]);
    let descriptor = bridge.arg();
    let (_, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "preview",
        "--command",
        &file.arg(),
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");

    let requests = finish(bridge);
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["arguments"], json!({ "action": "get" }));
    assert_eq!(
        requests[1]["arguments"]["expected_revision"], 31,
        "the fence must be the head that was just read"
    );
}

#[test]
fn apply_sends_the_apply_action_fenced_to_the_flag() {
    let file = proposal(json!({
        "idempotency_key": "stable-command-id",
        "command": update_node_command(),
    }));
    let bridge = paired(vec![(
        "governance.architecture.apply",
        snapshot_reply(13, true, false),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["data"]["applied"], true);
    assert_eq!(envelope["data"]["revision"], 13);

    let requests = finish(bridge);
    assert_eq!(
        requests[0]["arguments"],
        json!({
            "action": "apply",
            "expected_revision": 12,
            "idempotency_key": "stable-command-id",
            "command": update_node_command(),
        })
    );
}

// ---------------------------------------------------------------------------
// 2. The idempotency key
// ---------------------------------------------------------------------------

#[test]
fn a_derived_key_is_the_same_on_a_second_run_and_different_for_other_content() {
    // The property a retry after a dropped connection depends on: the same
    // proposal must carry the same key, or the retry commits a second
    // revision. A random UUID would pass every other test in this file.
    let file = proposal(json!({ "command": update_node_command() }));
    let mut keys = Vec::new();
    for _ in 0..2 {
        let bridge = paired(vec![(
            "governance.architecture.apply",
            snapshot_reply(13, true, false),
        )]);
        let descriptor = bridge.arg();
        let (_, code, stdout, stderr) = ds(&[
            "governance",
            "architecture",
            "apply",
            "--command",
            &file.arg(),
            "--expected-revision",
            "12",
            "--yes",
            "--desktop-descriptor",
            &descriptor,
            "--output",
            "json",
        ]);
        assert_eq!(code, 0, "{stdout}{stderr}");
        let requests = finish(bridge);
        keys.push(
            requests[0]["arguments"]["idempotency_key"]
                .as_str()
                .expect("a key is always sent")
                .to_string(),
        );
    }
    assert_eq!(
        keys[0], keys[1],
        "two runs of the same proposal must carry the same key"
    );
    assert!(
        keys[0].starts_with("sha256:"),
        "the key must be a content digest, not a fresh identifier: {}",
        keys[0]
    );

    let mut other = update_node_command();
    other["node"]["delivery_state"] = json!("planned");
    let changed = proposal(json!({ "command": other }));
    let bridge = paired(vec![(
        "governance.architecture.apply",
        snapshot_reply(13, true, false),
    )]);
    let descriptor = bridge.arg();
    let (_, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &changed.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let requests = finish(bridge);
    assert_ne!(
        requests[0]["arguments"]["idempotency_key"].as_str(),
        Some(keys[0].as_str()),
        "a different command must never replay over an earlier one's key"
    );
}

#[test]
fn an_idempotent_replay_is_reported_as_success() {
    // `applied: false` means the server already holds this exact command
    // under this key. Reporting that as a failure would send a caller to
    // re-plan work that is already committed.
    let file = proposal(json!({
        "idempotency_key": "stable-command-id",
        "command": update_node_command(),
    }));
    let bridge = paired(vec![(
        "governance.architecture.apply",
        snapshot_reply(13, false, false),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "an idempotent replay is success: {stdout}{stderr}");
    assert_eq!(envelope["status"], "ok");
    assert_eq!(envelope["data"]["applied"], false);
    finish(bridge);

    // And the human tier says so in a word, rather than looking like a
    // silently ignored write.
    let bridge = paired(vec![(
        "governance.architecture.apply",
        snapshot_reply(13, false, false),
    )]);
    let descriptor = bridge.arg();
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "governance",
            "architecture",
            "apply",
            "--command",
            &file.arg(),
            "--expected-revision",
            "12",
            "--yes",
            "--desktop-descriptor",
            &descriptor,
        ])
        .env("NO_COLOR", "1")
        .output()
        .expect("ds runs");
    let human = String::from_utf8_lossy(&output.stdout).into_owned();
    finish(bridge);
    assert!(human.starts_with("idempotent"), "{human}");
}

// ---------------------------------------------------------------------------
// 3. Confirmation
// ---------------------------------------------------------------------------

#[test]
fn apply_without_confirmation_never_reaches_the_bridge() {
    let file = proposal(json!({ "command": update_node_command() }));
    let quiet = quiet_bridge();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--desktop-descriptor",
        &quiet.arg(),
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "confirmation_required",
        "{stdout}"
    );
    assert_eq!(code, 2, "{stdout}{stderr}");
    quiet.assert_untouched("apply reached the governed graph without --yes");
}

#[test]
fn a_bad_fence_is_refused_before_any_socket_is_opened() {
    let file = proposal(json!({
        "expected_revision": 9,
        "command": update_node_command(),
    }));

    // Two fences that disagree are not a preference to resolve.
    let quiet = quiet_bridge();
    let (envelope, code, stdout, _) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &quiet.arg(),
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "proposal_revision_mismatch",
        "{stdout}"
    );
    assert_eq!(code, 2);
    quiet.assert_untouched("a proposal fenced to another revision was still sent");

    // And a revision below the seed is an input error, not a round trip.
    let quiet = quiet_bridge();
    let (envelope, _, stdout, _) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "0",
        "--yes",
        "--desktop-descriptor",
        &quiet.arg(),
        "--output",
        "json",
    ]);
    assert_eq!(envelope["error"]["code"], "invalid_number", "{stdout}");
    quiet.assert_untouched("revision 0 was sent to the governed graph");
}

#[test]
fn a_history_limit_outside_its_bound_is_refused_locally() {
    let quiet = quiet_bridge();
    let (envelope, code, stdout, _) = ds(&[
        "governance",
        "architecture",
        "history",
        "--limit",
        "101",
        "--desktop-descriptor",
        &quiet.arg(),
        "--output",
        "json",
    ]);
    assert_eq!(envelope["error"]["code"], "invalid_number", "{stdout}");
    assert_eq!(code, 2);
    assert!(
        envelope["error"]["remedy"]
            .as_str()
            .is_some_and(|remedy| remedy.contains("1..100")),
        "the refusal must carry the accepted range: {stdout}"
    );
    quiet.assert_untouched("an over-large page was sent rather than refused");
}

#[test]
fn an_unknown_delivery_state_never_leaves_the_process() {
    // The delivery states are a closed set, so the parser refuses one the
    // graph does not have before a socket is opened.
    let quiet = quiet_bridge();
    let (envelope, _, stdout, _) = ds(&[
        "governance",
        "architecture",
        "list",
        "--state",
        "shipped",
        "--desktop-descriptor",
        &quiet.arg(),
        "--output",
        "json",
    ]);
    assert_eq!(envelope["error"]["code"], "invalid_choice", "{stdout}");
    quiet.assert_untouched("an unknown delivery state was sent");
}

#[test]
fn a_malformed_proposal_is_refused_before_pairing() {
    let file = proposal(json!({ "command": { "id": "c", "kind": "rename", "target_id": "t" } }));
    let quiet = quiet_bridge();
    let (envelope, code, stdout, _) = ds(&[
        "governance",
        "architecture",
        "preview",
        "--command",
        &file.arg(),
        "--desktop-descriptor",
        &quiet.arg(),
        "--output",
        "json",
    ]);
    assert_eq!(envelope["error"]["code"], "invalid_proposal", "{stdout}");
    assert_eq!(code, 2);
    quiet.assert_untouched("a proposal with an unknown command kind was still sent");
}

// ---------------------------------------------------------------------------
// 4. Refusals from the authority
// ---------------------------------------------------------------------------

#[test]
fn a_revision_conflict_is_reported_once_and_never_retried() {
    // The single most important behaviour in this family. Re-sending against
    // the head the server just reported would apply an edit on top of work
    // its author never saw, which is exactly what the fence exists to stop.
    let file = proposal(json!({ "command": update_node_command() }));
    let bridge = bridge_with_status(vec![(
        "governance.architecture.apply",
        409,
        json!({
            "error": "revision_conflict",
            "message": "the architecture head has moved",
            "expected": 12,
            "current": 15,
            "snapshot": { "nodes": [] },
        }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "architecture_revision_conflict",
        "{stdout}{stderr}"
    );
    assert_eq!(envelope["error"]["class"], "conflict");
    assert_eq!(code, 5, "a conflict has its own exit class");
    assert!(
        !envelope["error"]["next"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|next| next.as_str().is_some_and(|next| next.contains("apply"))),
        "a conflict must never propose re-applying: {stdout}"
    );

    let requests = finish(bridge);
    assert_eq!(
        requests.len(),
        1,
        "the conflict was retried; the head must never be silently rebased onto"
    );
}

#[test]
fn a_conflict_that_carries_the_head_names_it() {
    // The same condition, delivered as the server's own error envelope on an
    // otherwise successful transport. The revisions survive, so the caller is
    // told which head to re-plan against.
    let file = proposal(json!({ "command": update_node_command() }));
    let bridge = paired(vec![(
        "governance.architecture.apply",
        json!({
            "error": "revision_conflict",
            "message": "the architecture head has moved",
            "expected": 12,
            "current": 15,
            "snapshot": { "nodes": [] },
        }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, _) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 5, "{stdout}");
    assert_eq!(envelope["error"]["detail"]["current"], 15);
    assert_eq!(envelope["error"]["detail"]["expected"], 12);
    assert!(
        envelope["error"]["message"].as_str().is_some_and(
            |message| message.contains("15") && message.contains("nothing was applied")
        ),
        "{stdout}"
    );
    assert_eq!(finish(bridge).len(), 1);
}

#[test]
fn every_validation_violation_reaches_the_caller_on_its_own_line() {
    // Fenced in the file, so the only round trip is the preview itself.
    let file = proposal(json!({
        "expected_revision": 12,
        "command": update_node_command(),
    }));
    let bridge = paired(vec![(
        "governance.architecture.preview",
        json!({
            "error": "validation_failed",
            "message": "3 violations",
            "violations": [
                { "field": "delivery_state", "target_id": "survey-form-factory",
                  "message": "implemented requires at least one citation" },
                { "field": "evidence", "target_id": "survey-form-factory",
                  "message": "evidence[] is empty" },
                // A violation with no target omits the field entirely; it must
                // still render, and must not be dropped for lacking one.
                { "field": "chapter", "message": "cross-chapter edges need a doorway" },
            ],
        }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "preview",
        "--command",
        &file.arg(),
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "architecture_validation_failed",
        "{stdout}{stderr}"
    );
    assert_eq!(code, 2, "a validation failure exits nonzero");
    let detail = &envelope["error"]["detail"];
    assert_eq!(detail["violations_total"], 3);
    assert_eq!(
        detail["violation_1"],
        "delivery_state · survey-form-factory — implemented requires at least one citation"
    );
    assert_eq!(
        detail["violation_2"],
        "evidence · survey-form-factory — evidence[] is empty"
    );
    assert_eq!(
        detail["violation_3"], "chapter — cross-chapter edges need a doorway",
        "a violation with no target_id must still be reported"
    );
    finish(bridge);

    // The human tier renders one line per violation rather than one joined
    // sentence — the reason the detail is an object and not an array.
    let bridge = paired(vec![(
        "governance.architecture.preview",
        json!({
            "error": "validation_failed",
            "message": "2 violations",
            "violations": [
                { "field": "delivery_state", "target_id": "a", "message": "needs a citation" },
                { "field": "evidence", "target_id": "b", "message": "is empty" },
            ],
        }),
    )]);
    let descriptor = bridge.arg();
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "governance",
            "architecture",
            "preview",
            "--command",
            &file.arg(),
            "--desktop-descriptor",
            &descriptor,
        ])
        .env("NO_COLOR", "1")
        .output()
        .expect("ds runs");
    let human = String::from_utf8_lossy(&output.stderr).into_owned();
    finish(bridge);
    let lines: Vec<&str> = human.lines().map(str::trim).collect();
    assert!(
        lines.contains(&"violation_1: delivery_state · a — needs a citation"),
        "{human}"
    );
    assert!(
        lines.contains(&"violation_2: evidence · b — is empty"),
        "{human}"
    );
}

#[test]
fn the_platform_administration_gate_is_named_as_authority_not_as_a_bad_request() {
    let file = proposal(json!({ "command": update_node_command() }));
    let bridge = bridge_with_status(vec![(
        "governance.architecture.apply",
        403,
        json!({ "error": "forbidden", "message": "platform.admin capability required" }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "architecture_not_permitted",
        "{stdout}{stderr}"
    );
    assert_eq!(envelope["error"]["class"], "unauthorized");
    assert_eq!(code, 4);
    finish(bridge);
}

#[test]
fn an_unknown_chapter_is_a_refusal_and_never_an_empty_answer() {
    // ds-brain refuses an unknown `chapter` with 400 validation_failed rather
    // than returning nothing. Rendering that as "no results" would tell a
    // caller their chapter is empty when it does not exist.
    let bridge = bridge_with_status(vec![(
        "governance.architecture.list",
        400,
        json!({ "error": "validation_failed", "message": "unknown chapter" }),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "list",
        "--chapter",
        "not-a-chapter",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "architecture_validation_failed",
        "{stdout}{stderr}"
    );
    assert_ne!(code, 0, "an unknown chapter must not exit 0 with no rows");
    finish(bridge);
}

#[test]
fn a_reply_that_is_not_the_documented_shape_is_never_reported_as_success() {
    let file = proposal(json!({
        "expected_revision": 12,
        "idempotency_key": "stable-command-id",
        "command": update_node_command(),
    }));
    // A preview that claims it applied is a contract break, not a state this
    // command can render: `applied` is on the wire so no client has to infer
    // safety from which action it called.
    let bridge = paired(vec![(
        "governance.architecture.preview",
        snapshot_reply(12, true, true),
    )]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, _) = ds(&[
        "governance",
        "architecture",
        "preview",
        "--command",
        &file.arg(),
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "desktop_contract_mismatch",
        "{stdout}"
    );
    assert_eq!(code, 3);
    finish(bridge);

    // And a receipt for some other command describes work nobody authorized.
    let mut wrong = snapshot_reply(13, true, false);
    wrong["command_id"] = json!("some-other-command");
    let bridge = paired(vec![("governance.architecture.apply", wrong)]);
    let descriptor = bridge.arg();
    let (envelope, _, stdout, _) = ds(&[
        "governance",
        "architecture",
        "apply",
        "--command",
        &file.arg(),
        "--expected-revision",
        "12",
        "--yes",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(
        envelope["error"]["code"], "desktop_contract_mismatch",
        "{stdout}"
    );
    finish(bridge);
}

#[test]
fn a_newer_server_field_never_breaks_an_older_cli() {
    // Responses are read permissively: the reply is checked for the fields the
    // contract promises and is otherwise passed through, so a field this build
    // has never heard of arrives intact instead of failing to parse.
    let mut reply = snapshot_reply(12, false, true);
    reply["provenance"] = json!({ "seeded_from": "manifest", "engine": "v2" });
    reply["snapshot"]["doorways"] = json!([{ "from": "survey", "to": "design" }]);
    let file = proposal(json!({
        "expected_revision": 12,
        "idempotency_key": "stable-command-id",
        "command": update_node_command(),
    }));
    let bridge = paired(vec![("governance.architecture.preview", reply)]);
    let descriptor = bridge.arg();
    let (envelope, code, stdout, stderr) = ds(&[
        "governance",
        "architecture",
        "preview",
        "--command",
        &file.arg(),
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["data"]["provenance"]["engine"], "v2");
    assert_eq!(
        envelope["data"]["snapshot"]["doorways"][0]["to"], "design",
        "an unknown nested field must reach the caller untouched"
    );
    finish(bridge);
}
