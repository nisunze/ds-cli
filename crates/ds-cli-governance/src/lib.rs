//! `ds governance architecture` — the governed architecture planning graph.
//!
//! Architecture is a versioned planning surface that people and agents update
//! through one authority. ds-brain owns the snapshot behind a single
//! map-independent endpoint, `POST /api/v1/architecture/planning`, whose body
//! is `{"action": …}` over the closed action set `get | list | history |
//! preview | apply`. The contract is
//! `ds-web/docs/specs/DYNAMIC_ARCHITECTURE_PLANNING_CONTRACT.md`.
//!
//! ## Why this crate holds no HTTP client and no token
//!
//! The CLI is the MCP implementation; there is no second architecture API. It
//! is also, deliberately, not a second *principal*: `ds` never receives a
//! Firebase credential. Every command here names one closed operation on the
//! paired DS GridDesign bridge, and the running application performs the
//! ds-brain call under the user it has already signed in — the same authority
//! the UI edit mode uses. That is exactly how `ds feedback`, `ds sre` and
//! `ds solar seed` reach governed ds-brain writes, and this domain does not
//! invent a third shape.
//!
//! The consequence worth stating: the arguments a handler sends **are** the
//! ds-brain request body, `action` included. One bridge operation per server
//! action keeps the closed-operation rule (two commands can never become
//! aliases for one mutation) while the payload stays byte-for-byte the
//! documented wire contract, so nothing between here and Go re-spells a key.
//!
//! ## Three properties this module refuses to give up
//!
//! * **A stale revision is never silently rebased.** A `revision_conflict`
//!   is reported with the head that moved and the instruction to re-plan. It
//!   is never retried against the new head — rebasing an edit onto work the
//!   author never saw is the one failure this whole fence exists to prevent.
//! * **An idempotency key is never random.** A retry after a network failure
//!   has to carry the same key or the work commits twice, so an absent key is
//!   derived from the command's own content ([`derive_idempotency_key`]).
//! * **A reply that is not the documented shape is not a success.** A
//!   planning error that arrives on an otherwise successful transport is
//!   reported under its own name, and anything else is a contract mismatch.

pub mod apply;
pub mod get;
pub mod history;
pub mod list;
pub mod preview;

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Domain, Refusal};
use ds_cli_desktop::ops::{self, BridgeOp};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub use ds_cli_desktop::ops::{DESCRIPTOR_ARG, INVALID_NUMBER, integer};

/// The proposal document, declared once so `preview` and `apply` cannot
/// describe the same input in two ways.
pub const COMMAND_ARG: Arg = Arg::value(
    "command",
    "<proposal.json>",
    "The proposal: one JSON object carrying the command, and optionally its fence and key.",
)
.required();

pub static DOMAIN: Domain = Domain {
    id: "governance",
    summary: "Governed architecture graph: read, plan and apply.",
    commands: &[
        &get::COMMAND,
        &list::COMMAND,
        &history::COMMAND,
        &preview::COMMAND,
        &apply::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

/// The closed action set on ds-brain's single planning door, in the order
/// this domain exposes them. `ds` composes no sixth action.
pub const SERVER_ACTIONS: &[&str] = &["get", "list", "history", "preview", "apply"];

/// The ds-brain route the paired application calls on this domain's behalf.
/// Named here because it is the contract a reader needs, and nowhere else:
/// `ds` never composes this URL.
pub const SERVER_ROUTE: &str = "/api/v1/architecture/planning";

pub const GET: BridgeOp = BridgeOp {
    operation: "governance.architecture.get",
    arguments: &["action"],
};
pub const LIST: BridgeOp = BridgeOp {
    operation: "governance.architecture.list",
    arguments: &["action", "chapter", "state", "include_archived"],
};
pub const HISTORY: BridgeOp = BridgeOp {
    operation: "governance.architecture.history",
    arguments: &["action", "limit", "cursor"],
};
pub const PREVIEW: BridgeOp = BridgeOp {
    operation: "governance.architecture.preview",
    arguments: &["action", "expected_revision", "idempotency_key", "command"],
};
pub const APPLY: BridgeOp = BridgeOp {
    operation: "governance.architecture.apply",
    arguments: &["action", "expected_revision", "idempotency_key", "command"],
};

/// Every operation this domain may send, walked by the desktop parity suite.
pub const BRIDGE_OPS: &[&BridgeOp] = &[&GET, &LIST, &HISTORY, &PREVIEW, &APPLY];

/// Reads are one bounded ds-brain round trip through the application.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Preview runs the same validator apply runs, and apply commits one governed
/// transaction. The UI gives both the same minute.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Bounds
// ---------------------------------------------------------------------------

/// `delivery_state` is a closed set in the contract, so an unknown value is
/// refused by the argument parser before a round trip rather than after one.
pub const DELIVERY_STATES: &[&str] = &[
    "implemented",
    "in_progress",
    "user_question",
    "planned",
    "wishlist",
    "recommended_refactor",
    "rejected",
];

/// The closed mutation vocabulary a proposal's `command.kind` may name.
pub const COMMAND_KINDS: &[&str] = &[
    "add_node",
    "update_node",
    "archive_node",
    "link_edge",
    "update_edge",
    "unlink_edge",
];

/// A chapter id is an identity, not a payload.
pub const MAX_CHAPTER_CHARS: usize = 128;
/// A history cursor is opaque and server-minted; this only refuses a value
/// that cannot be one.
pub const MAX_CURSOR_CHARS: usize = 512;
/// Node, edge and command identities are identities, not documents.
pub const MAX_ID_CHARS: usize = 200;
/// The rows one history page returns. The server pages further; this is what
/// a caller pays for in context.
pub const MAX_HISTORY_LIMIT: i64 = 100;
pub const DEFAULT_HISTORY_LIMIT: &str = "20";
/// A revision is a positive counter. `1` is the seed revision.
pub const MIN_REVISION: i64 = 1;
/// One proposal is a bounded command document, never a snapshot dump.
pub const MAX_PROPOSAL_BYTES: u64 = 1024 * 1024;
/// How many violation lines a refusal renders before it says how many are
/// left. The output contract forbids a silent truncation, so the total is
/// always reported.
pub const MAX_RENDERED_VIOLATIONS: usize = 6;

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

pub const SIGNED_OUT: Refusal = Refusal {
    code: "desktop_signed_out",
    when: "DS GridDesign is running but has no signed-in user",
    remedy: "sign in to DS GridDesign; no project selection is required",
};
pub const CONTRACT_MISMATCH: Refusal = Refusal {
    code: "desktop_contract_mismatch",
    when: "the reply is not this action's documented planning shape",
    remedy: "update DS GridDesign and ds to matching releases",
};
pub const VALIDATION_FAILED: Refusal = Refusal {
    code: "architecture_validation_failed",
    when: "the proposal fails the shared validator; nothing was written",
    remedy: "fix each reported violation in the proposal and preview it again",
};
pub const REVISION_CONFLICT: Refusal = Refusal {
    code: "architecture_revision_conflict",
    when: "the head moved past the expected revision; nothing was applied",
    remedy: "read the current head, re-plan the change against it, and confirm that revision",
};
pub const NOT_PERMITTED: Refusal = Refusal {
    code: "architecture_not_permitted",
    when: "the signed-in user does not hold platform administration authority",
    remedy: "ask a platform administrator to make the change, or to grant that authority",
};
pub const NOT_FOUND: Refusal = Refusal {
    code: "architecture_not_found",
    when: "the command names a node, edge or revision the graph does not hold",
    remedy: "list the graph and take an exact id from it",
};
pub const CONFLICT: Refusal = Refusal {
    code: "architecture_conflict",
    when: "the change collides with the graph: a duplicate identity or a dangling edge",
    remedy: "read the server's message, correct the proposal, and preview it again",
};
pub const INVALID_PROPOSAL: Refusal = Refusal {
    code: "invalid_proposal",
    when: "the proposal file is not one bounded planning command document",
    remedy: "pass a JSON object carrying `command` with an id, a kind and a target_id",
};
pub const PROPOSAL_UNREADABLE: Refusal = Refusal {
    code: "proposal_unreadable",
    when: "the proposal path cannot be read, or is larger than one megabyte",
    remedy: "pass a readable path to the proposal JSON document",
};
pub const REVISION_MISMATCH: Refusal = Refusal {
    code: "proposal_revision_mismatch",
    when: "--expected-revision and the proposal's own expected_revision disagree",
    remedy: "confirm the revision the proposal was planned against, or drop it from the file",
};
pub const INVALID_TEXT: Refusal = Refusal {
    code: "invalid_text",
    when: "a filter or cursor is empty, untrimmed, or longer than its bound",
    remedy: "pass one exact trimmed value within the bound in its summary",
};

/// The pairing refusals every command in this domain shares, plus this
/// domain's own. Held once so five commands cannot document five subtly
/// different versions of the same condition.
pub const READ_REFUSALS: &[Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    SIGNED_OUT,
    CONTRACT_MISMATCH,
    NOT_PERMITTED,
    NOT_FOUND,
];

// ---------------------------------------------------------------------------
// Bounded inputs
// ---------------------------------------------------------------------------

pub fn bounded_text<'a>(value: &'a str, flag: &str, max: usize) -> Result<&'a str, Failure> {
    if value.is_empty() || value.trim() != value || value.chars().count() > max {
        return Err(Failure::invalid(
            "invalid_text",
            format!("`--{flag}` must be non-empty, trimmed, and at most {max} characters"),
        )
        .remedy(INVALID_TEXT.remedy)
        .detail(json!({ "flag": flag, "max_chars": max })));
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// The proposal document
// ---------------------------------------------------------------------------

/// One planning command, read from the operator's own file.
///
/// The command payload itself is passed through verbatim: `node` and `edge`
/// are ds-brain's vocabulary, and a CLI that subset them would silently drop
/// authored detail the way a frontend that re-declares a layer model does.
/// What is checked here is only what `ds` must be sure of before it can send
/// anything at all — that there is one command, with an identity, a kind from
/// the closed set, and a target.
#[derive(Debug, Clone)]
pub struct Proposal {
    /// The revision the proposal was planned against, when the file names one.
    pub expected_revision: Option<i64>,
    /// The key the request carries. From the file when it has one, otherwise
    /// derived from the command's content.
    pub idempotency_key: String,
    /// Whether that key was derived rather than authored.
    pub derived_key: bool,
    pub command: Value,
    pub command_id: String,
    pub kind: String,
    pub target_id: String,
}

/// The only top-level keys a proposal may carry.
///
/// Refused rather than ignored, for the same reason ds-brain decodes with
/// `DisallowUnknownFields`: a misspelled `expected_rev` that is silently
/// dropped becomes an unfenced apply.
pub const PROPOSAL_KEYS: &[&str] = &["expected_revision", "idempotency_key", "command"];

pub fn read_proposal(path: &str) -> Result<Proposal, Failure> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::unavailable(
            "proposal_unreadable",
            format!("the proposal at `{path}` cannot be read"),
        )
        .remedy(PROPOSAL_UNREADABLE.remedy)
        .detail(json!({ "reason": error.kind().to_string() }))
    })?;
    if metadata.len() > MAX_PROPOSAL_BYTES {
        return Err(Failure::unavailable(
            "proposal_unreadable",
            "the proposal is larger than one bounded command document",
        )
        .remedy(PROPOSAL_UNREADABLE.remedy)
        .detail(json!({ "bytes": metadata.len(), "max_bytes": MAX_PROPOSAL_BYTES })));
    }
    let text = std::fs::read_to_string(path).map_err(|error| {
        Failure::unavailable(
            "proposal_unreadable",
            format!("the proposal at `{path}` cannot be read"),
        )
        .remedy(PROPOSAL_UNREADABLE.remedy)
        .detail(json!({ "reason": error.kind().to_string() }))
    })?;
    parse_proposal(&text)
}

pub fn parse_proposal(text: &str) -> Result<Proposal, Failure> {
    let document: Value = serde_json::from_str(text).map_err(|error| {
        Failure::invalid("invalid_proposal", "the proposal is not valid JSON")
            .remedy(INVALID_PROPOSAL.remedy)
            .detail(json!({ "reason": error.to_string() }))
    })?;
    let Some(fields) = document.as_object() else {
        return Err(invalid_proposal("the proposal must be one JSON object"));
    };
    if let Some(unknown) = fields
        .keys()
        .find(|key| !PROPOSAL_KEYS.contains(&key.as_str()))
    {
        return Err(
            invalid_proposal(&format!("`{unknown}` is not a proposal key")).detail(json!({
                "unknown": unknown,
                "accepted": PROPOSAL_KEYS,
            })),
        );
    }

    let expected_revision = match fields.get("expected_revision") {
        None | Some(Value::Null) => None,
        Some(value) => Some(revision_field(value)?),
    };

    let command = fields
        .get("command")
        .cloned()
        .ok_or_else(|| invalid_proposal("the proposal carries no `command`"))?;
    let Some(body) = command.as_object() else {
        return Err(invalid_proposal("`command` must be one JSON object"));
    };
    let command_id = identity(body.get("id"), "command.id")?;
    let kind = identity(body.get("kind"), "command.kind")?;
    if !COMMAND_KINDS.contains(&kind.as_str()) {
        return Err(
            invalid_proposal(&format!("`{kind}` is not a planning command kind")).detail(json!({
                "given": kind,
                "accepted": COMMAND_KINDS,
            })),
        );
    }
    let target_id = identity(body.get("target_id"), "command.target_id")?;

    let (idempotency_key, derived_key) = match fields.get("idempotency_key") {
        None | Some(Value::Null) => (derive_idempotency_key(&command), true),
        Some(value) => (identity(Some(value), "idempotency_key")?, false),
    };

    Ok(Proposal {
        expected_revision,
        idempotency_key,
        derived_key,
        command,
        command_id,
        kind,
        target_id,
    })
}

fn invalid_proposal(detail: &str) -> Failure {
    Failure::invalid("invalid_proposal", detail.to_string()).remedy(INVALID_PROPOSAL.remedy)
}

fn identity(value: Option<&Value>, field: &str) -> Result<String, Failure> {
    let text = value.and_then(Value::as_str).unwrap_or_default();
    if text.is_empty() || text.trim() != text || text.chars().count() > MAX_ID_CHARS {
        return Err(invalid_proposal(&format!(
            "`{field}` must be a trimmed identity of at most {MAX_ID_CHARS} characters"
        )));
    }
    Ok(text.to_string())
}

fn revision_field(value: &Value) -> Result<i64, Failure> {
    match value.as_i64() {
        Some(revision) if revision >= MIN_REVISION => Ok(revision),
        _ => Err(invalid_proposal(
            "`expected_revision` must be a whole number of at least 1",
        )),
    }
}

// ---------------------------------------------------------------------------
// The idempotency key
// ---------------------------------------------------------------------------

/// Derive the idempotency key for a proposal that does not author one.
///
/// **This must never be a random UUID, and that is the whole reason it is a
/// hash.** The key exists so a retry after a network failure — a timeout, a
/// dropped loopback socket, a desktop restart — reaches ds-brain as the same
/// command it already committed and is answered `applied: false` instead of
/// committing a second revision. A key minted per invocation would make every
/// retry a new command, which is the exact double-write the fence is for. So
/// the key is a pure function of the command's content: the same proposal
/// derives the same key on any machine, at any hour, in any process; a
/// different command derives a different one.
///
/// The content is canonicalized ([`canonical_json`]) rather than hashed as
/// authored, so re-indenting or reordering the file does not mint a new key
/// for a command that has not changed.
pub fn derive_idempotency_key(command: &Value) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(canonical_json(command).as_bytes())
    )
}

/// JSON with every object key sorted, recursively.
///
/// `serde_json`'s map happens to be ordered today, but the derivation above
/// is a promise about bytes that must survive a dependency feature flip, so
/// the ordering is performed here rather than assumed.
pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(fields) => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        Value::String(key.clone()),
                        canonical_json(&fields[key])
                    )
                })
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => {
            let body: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", body.join(","))
        }
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// The exact ds-brain body for one action, as the bridge arguments.
///
/// Every builder starts here so no action can be sent without naming itself,
/// and so the payload a test asserts on is the payload the server decodes.
fn action(name: &'static str) -> Map<String, Value> {
    Map::from_iter([("action".to_string(), json!(name))])
}

pub fn get_request() -> Value {
    Value::Object(action("get"))
}

/// An absent filter is omitted, never sent as `""` or `false`.
///
/// ds-brain reads an absent `chapter` as every chapter and an absent `state`
/// as every state, so an empty string would be a different request — one
/// asking for the chapter whose id is the empty string.
pub fn list_request(
    chapter: Option<&str>,
    state: Option<&str>,
    include_archived: bool,
) -> Result<Value, Failure> {
    let mut body = action("list");
    if let Some(chapter) = chapter {
        body.insert(
            "chapter".into(),
            json!(bounded_text(chapter, "chapter", MAX_CHAPTER_CHARS)?),
        );
    }
    if let Some(state) = state {
        body.insert("state".into(), json!(state));
    }
    if include_archived {
        body.insert("include_archived".into(), json!(true));
    }
    Ok(Value::Object(body))
}

pub fn history_request(limit: Option<&str>, cursor: Option<&str>) -> Result<Value, Failure> {
    let mut body = action("history");
    if let Some(limit) = limit {
        body.insert(
            "limit".into(),
            json!(integer(limit, "limit", 1, MAX_HISTORY_LIMIT)?),
        );
    }
    if let Some(cursor) = cursor {
        body.insert(
            "cursor".into(),
            json!(bounded_text(cursor, "cursor", MAX_CURSOR_CHARS)?),
        );
    }
    Ok(Value::Object(body))
}

pub fn command_request(name: &'static str, revision: i64, proposal: &Proposal) -> Value {
    let mut body = action(name);
    body.insert("expected_revision".into(), json!(revision));
    body.insert("idempotency_key".into(), json!(proposal.idempotency_key));
    body.insert("command".into(), proposal.command.clone());
    Value::Object(body)
}

// ---------------------------------------------------------------------------
// The paired call
// ---------------------------------------------------------------------------

pub fn invoke(
    descriptor_path: Option<&str>,
    op: &BridgeOp,
    body: Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    let descriptor = ops::paired(descriptor_path)?;
    let reply = ops::invoke(&descriptor, op, body, timeout).map_err(classify_planning_failure)?;
    // A planning error that rode back on a successful transport is still a
    // planning error. Reporting it under its own name costs nothing and is
    // strictly better for a caller than `desktop_contract_mismatch`.
    if let Some(failure) = planning_error(&reply) {
        return Err(failure);
    }
    Ok(reply)
}

/// Refuse a reply that is not this action's documented shape.
pub fn require_fields(
    reply: &Value,
    operation: &'static str,
    fields: &[&str],
) -> Result<(), Failure> {
    for field in fields {
        let present = match *field {
            "revision" => reply["revision"].as_u64().is_some(),
            "snapshot" => reply["snapshot"].is_object(),
            "nodes" | "edges" | "revisions" => reply[*field].is_array(),
            "command_id" => reply["command_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty()),
            "applied" => reply["applied"].is_boolean(),
            _ => !reply[*field].is_null(),
        };
        if !present {
            return Err(mismatch(
                operation,
                &format!("the reply carries no `{field}`"),
            ));
        }
    }
    Ok(())
}

/// The receipt must name the command that was sent.
///
/// Same reasoning as a confirmed digest on a governed seed: a reply that
/// receipts some other command describes work the caller never authorized.
/// Either identity the request carried is accepted, because the contract's
/// own example gives a command the same value for both and a server is free
/// to receipt under either.
pub fn require_receipt(
    reply: &Value,
    operation: &'static str,
    proposal: &Proposal,
) -> Result<(), Failure> {
    let receipted = reply["command_id"].as_str().unwrap_or_default();
    if receipted == proposal.command_id || receipted == proposal.idempotency_key {
        return Ok(());
    }
    Err(mismatch(
        operation,
        "the receipt names a different command from the one that was sent",
    )
    .detail(json!({ "sent": proposal.command_id, "receipted": receipted })))
}

pub fn mismatch(operation: &'static str, detail: &str) -> Failure {
    Failure::unavailable(
        "desktop_contract_mismatch",
        format!("the paired session returned an invalid reply for `{operation}`: {detail}"),
    )
    .remedy(CONTRACT_MISMATCH.remedy)
}

// ---------------------------------------------------------------------------
// Refusal classification
// ---------------------------------------------------------------------------

/// ds-brain's own `error` tokens, verbatim. Matching on the token rather than
/// on prose is deliberate: the message is what a UI localizes, and the token
/// is the stable half of the contract.
///
/// Every error body is one uniform envelope — `{"error": "<code>", "message":
/// "…"}` — and the two structured cases add fields beside it:
/// `validation_failed` adds `violations[]`, and `revision_conflict` adds
/// `expected`, `current` and `snapshot`. Order matters here: `revision_conflict`
/// must be tested before `conflict`, which it contains.
pub const SERVER_ERRORS: &[&str] = &[
    "validation_failed",
    "revision_conflict",
    "forbidden",
    "not_found",
    "conflict",
];

/// Turn the paired application's refusal into this domain's named one.
///
/// Two sources are read, in order of how much they prove. The HTTP status the
/// application reports is exact; the message is its own prose and carries
/// ds-brain's token. A structured payload — violations, or the revisions of a
/// conflict — is used whenever the application forwards one, which is why the
/// caller of a build that does not yet forward it still gets the right code
/// and the right remedy, just without the per-violation lines.
pub fn classify_planning_failure(failure: Failure) -> Failure {
    let failure = ops::classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let Some(detail) = failure.detail_value() else {
        return failure;
    };
    let status = detail["http_status"].as_u64().unwrap_or(0);
    let message = detail["detail"].as_str().unwrap_or_default();

    let token = SERVER_ERRORS
        .iter()
        .copied()
        .find(|token| message.contains(token))
        // The status is the fallback, and it is exact: an adapter that reports
        // only prose still lands on the right named refusal.
        .or(match status {
            403 => Some("forbidden"),
            404 => Some("not_found"),
            409 => Some("conflict"),
            _ => None,
        });
    let Some(token) = token else {
        return failure;
    };
    named_failure(token, detail, message)
}

/// The same naming, for a planning error that arrived on a 200 reply.
///
/// `message` is read where the envelope carries one and is never required:
/// what identifies the condition is the `error` token, and a build that omits
/// the prose must still be classified rather than reported as a success.
fn planning_error(reply: &Value) -> Option<Failure> {
    let token = reply["error"].as_str()?;
    if !SERVER_ERRORS.contains(&token) {
        return None;
    }
    Some(named_failure(
        token,
        reply,
        reply["message"].as_str().unwrap_or_default(),
    ))
}

fn named_failure(token: &str, payload: &Value, message: &str) -> Failure {
    match token {
        "validation_failed" => validation_failure(payload),
        "revision_conflict" => revision_conflict(payload),
        // `forbidden` is the platform-administration gate. The server's own
        // message names the capability; the remedy names the person who can
        // grant it, because that is the caller's actual next move.
        "forbidden" => Failure::unauthorized(
            "architecture_not_permitted",
            bounded_message(
                message,
                "this needs platform administration authority over the architecture graph",
            ),
        )
        .remedy(NOT_PERMITTED.remedy),
        "not_found" => Failure::invalid(
            "architecture_not_found",
            bounded_message(message, "the graph does not hold what the command named"),
        )
        .remedy(NOT_FOUND.remedy)
        .next("ds governance architecture list --output json"),
        _ => Failure::conflict(
            "architecture_conflict",
            bounded_message(message, "the change collides with the graph"),
        )
        .remedy(CONFLICT.remedy),
    }
}

fn bounded_message(message: &str, fallback: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    trimmed.chars().take(200).collect()
}

/// A stale expected revision, reported as the thing it is: the head moved,
/// nothing was applied, and the caller must re-plan.
///
/// There is deliberately no retry here and no `next` that would look like
/// one. Re-sending against the head the server just reported would apply an
/// edit on top of work its author never saw, which is exactly what the
/// contract forbids.
fn revision_conflict(payload: &Value) -> Failure {
    let expected = payload["expected"].as_i64();
    let current = payload["current"].as_i64();
    let message = match (expected, current) {
        (Some(expected), Some(current)) => format!(
            "the architecture head moved to revision {current}; nothing was applied against {expected}"
        ),
        _ => "the architecture head moved past the expected revision; nothing was applied"
            .to_string(),
    };
    let mut detail = Map::new();
    if let Some(expected) = expected {
        detail.insert("expected".into(), json!(expected));
    }
    if let Some(current) = current {
        detail.insert("current".into(), json!(current));
    }
    let failure = Failure::conflict("architecture_revision_conflict", message)
        .remedy(REVISION_CONFLICT.remedy)
        .next("ds governance architecture get --output json");
    if detail.is_empty() {
        failure
    } else {
        failure.detail(Value::Object(detail))
    }
}

/// Every violation on its own line.
///
/// The refusal detail is an object with one key per violation rather than one
/// array, because the human tier renders an object as one line per key and an
/// array as a single joined line. A validator that found six problems must
/// not report them as one run-on sentence, and it must never hide the
/// seventh: the total is always carried, so a bounded render still says how
/// many were not printed.
fn validation_failure(payload: &Value) -> Failure {
    let violations = payload["violations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut detail = Map::new();
    for (index, violation) in violations.iter().take(MAX_RENDERED_VIOLATIONS).enumerate() {
        detail.insert(
            format!("violation_{}", index + 1),
            json!(violation_line(violation)),
        );
    }
    let hidden = violations.len().saturating_sub(MAX_RENDERED_VIOLATIONS);
    if hidden > 0 {
        detail.insert(
            "violation_omitted".into(),
            json!(format!(
                "{hidden} more; read the JSON envelope for all of them"
            )),
        );
    }
    detail.insert("violations_total".into(), json!(violations.len()));

    let message = if violations.is_empty() {
        "the proposal failed validation; nothing was written".to_string()
    } else {
        format!(
            "the proposal failed validation with {} violation{}; nothing was written",
            violations.len(),
            if violations.len() == 1 { "" } else { "s" }
        )
    };
    Failure::invalid("architecture_validation_failed", message)
        .remedy(VALIDATION_FAILED.remedy)
        .detail(Value::Object(detail))
}

/// One violation as one readable line: what is wrong, where, and why.
pub fn violation_line(violation: &Value) -> String {
    let field = violation["field"].as_str().unwrap_or("");
    let target = violation["target_id"].as_str().unwrap_or("");
    let message = violation["message"].as_str().unwrap_or("").trim();
    let mut line = String::new();
    if !field.is_empty() {
        line.push_str(field);
    }
    if !target.is_empty() {
        if !line.is_empty() {
            line.push_str(" · ");
        }
        line.push_str(target);
    }
    if message.is_empty() {
        if line.is_empty() {
            return violation.to_string();
        }
        return line;
    }
    if line.is_empty() {
        return message.to_string();
    }
    format!("{line} — {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_operations_are_closed_and_carry_the_wire_body() {
        assert_eq!(BRIDGE_OPS.len(), DOMAIN.commands.len());
        assert_eq!(BRIDGE_OPS.len(), SERVER_ACTIONS.len());
        for (op, name) in BRIDGE_OPS.iter().zip(SERVER_ACTIONS) {
            assert_eq!(op.operation, format!("governance.architecture.{name}"));
            assert_eq!(
                op.arguments.first(),
                Some(&"action"),
                "every operation sends the ds-brain body, `action` included"
            );
        }
        // A read can never carry a command, and a mutation can never carry a
        // filter. The declared-key guard makes that structural.
        assert!(!LIST.arguments.contains(&"command"));
        assert!(!APPLY.arguments.contains(&"chapter"));
    }

    #[test]
    fn a_derived_key_is_stable_across_runs_and_unique_to_the_command() {
        // The property the whole retry story rests on. Two runs of the same
        // proposal must produce the same key or a retry commits twice.
        let command = json!({
            "id": "record-form-factory-question",
            "kind": "update_node",
            "target_id": "survey-form-factory",
            "node": { "delivery_state": "user_question", "tags": ["survey"] },
        });
        let first = derive_idempotency_key(&command);
        let second = derive_idempotency_key(&command);
        assert_eq!(first, second, "the same command must derive the same key");
        assert!(first.starts_with("sha256:") && first.len() == 71);

        // Key order and whitespace are not content: the same command written
        // differently is the same command.
        let reordered: Value = serde_json::from_str(
            "{\n  \"target_id\": \"survey-form-factory\",\n  \"kind\": \"update_node\",\n  \
             \"id\": \"record-form-factory-question\",\n  \
             \"node\": {\"tags\": [\"survey\"], \"delivery_state\": \"user_question\"}\n}",
        )
        .expect("fixture parses");
        assert_eq!(derive_idempotency_key(&reordered), first);

        // Different content is a different command, and must never replay
        // over the first one.
        let mut changed = command.clone();
        changed["node"]["delivery_state"] = json!("planned");
        assert_ne!(derive_idempotency_key(&changed), first);
        // Array order IS content — two edges in the other order are a
        // different graph, so the key must move.
        let mut reordered_array = command.clone();
        reordered_array["node"]["tags"] = json!(["design", "survey"]);
        assert_ne!(derive_idempotency_key(&reordered_array), first);
    }

    #[test]
    fn a_proposal_is_read_but_its_command_payload_is_never_subset() {
        let proposal = parse_proposal(
            r#"{"expected_revision":12,"idempotency_key":"stable-command-id",
                "command":{"id":"stable-command-id","kind":"update_node",
                "target_id":"survey-form-factory","node":{"delivery_state":"user_question"}}}"#,
        )
        .expect("the contract's own example parses");
        assert_eq!(proposal.expected_revision, Some(12));
        assert_eq!(proposal.idempotency_key, "stable-command-id");
        assert!(!proposal.derived_key);
        assert_eq!(proposal.command_id, "stable-command-id");
        assert_eq!(proposal.kind, "update_node");
        assert_eq!(proposal.target_id, "survey-form-factory");
        assert_eq!(
            proposal.command["node"]["delivery_state"],
            json!("user_question"),
            "the command payload is ds-brain's vocabulary and travels verbatim"
        );
    }

    #[test]
    fn an_absent_key_is_derived_and_an_absent_revision_stays_absent() {
        let proposal = parse_proposal(
            r#"{"command":{"id":"c1","kind":"link_edge","target_id":"cli-project-settings"}}"#,
        )
        .expect("a minimal proposal parses");
        assert_eq!(proposal.expected_revision, None);
        assert!(proposal.derived_key);
        assert_eq!(
            proposal.idempotency_key,
            derive_idempotency_key(&proposal.command)
        );
    }

    #[test]
    fn a_malformed_proposal_is_refused_before_anything_is_sent() {
        for (text, why) in [
            ("[]", "not an object"),
            ("{", "not JSON"),
            (r#"{"command":{}}"#, "no identity"),
            (
                r#"{"command":{"id":"c","kind":"rename","target_id":"t"}}"#,
                "kind outside the set",
            ),
            (
                r#"{"command":{"id":"c","kind":"add_node","target_id":" t"}}"#,
                "untrimmed target",
            ),
            (
                r#"{"expected_rev":3,"command":{"id":"c","kind":"add_node","target_id":"t"}}"#,
                "a misspelled fence",
            ),
            (
                r#"{"expected_revision":0,"command":{"id":"c","kind":"add_node","target_id":"t"}}"#,
                "revision below the seed",
            ),
        ] {
            let failure = parse_proposal(text).expect_err(why);
            assert_eq!(failure.code(), "invalid_proposal", "{why}");
        }
    }

    #[test]
    fn a_list_omits_what_the_caller_did_not_ask_for() {
        assert_eq!(
            list_request(None, None, false).expect("no filters is valid"),
            json!({ "action": "list" }),
            "an absent filter must not become an empty string"
        );
        assert_eq!(
            list_request(Some("survey-lifecycle"), Some("wishlist"), true).expect("filters"),
            json!({
                "action": "list",
                "chapter": "survey-lifecycle",
                "state": "wishlist",
                "include_archived": true,
            })
        );
        assert_eq!(
            bounded_text(" survey", "chapter", MAX_CHAPTER_CHARS)
                .expect_err("untrimmed")
                .code(),
            "invalid_text"
        );
    }

    #[test]
    fn history_is_held_to_its_documented_maximum() {
        assert_eq!(
            history_request(None, None).expect("defaults"),
            json!({ "action": "history" })
        );
        assert_eq!(
            history_request(Some("100"), Some("c-42")).expect("bounded"),
            json!({ "action": "history", "limit": 100, "cursor": "c-42" })
        );
        for over in ["101", "0", "-1", "many"] {
            assert_eq!(
                history_request(Some(over), None)
                    .expect_err("outside the bound")
                    .code(),
                "invalid_number",
                "`--limit {over}` must be refused here, before a round trip"
            );
        }
    }

    #[test]
    fn a_mutation_request_is_the_documented_body() {
        let proposal = parse_proposal(
            r#"{"idempotency_key":"k","command":{"id":"k","kind":"update_node","target_id":"t"}}"#,
        )
        .expect("proposal");
        assert_eq!(
            command_request("preview", 12, &proposal),
            json!({
                "action": "preview",
                "expected_revision": 12,
                "idempotency_key": "k",
                "command": { "id": "k", "kind": "update_node", "target_id": "t" },
            })
        );
        assert_eq!(command_request("apply", 12, &proposal)["action"], "apply");
    }

    fn refused(status: u64, message: &str) -> Failure {
        Failure::failed(
            "desktop_refused",
            "the paired session refused the operation",
        )
        .detail(json!({ "http_status": status, "detail": message }))
    }

    #[test]
    fn a_revision_conflict_names_the_head_and_offers_no_retry() {
        let failure = planning_error(&json!({
            "error": "revision_conflict",
            "message": "the head moved",
            "expected": 12,
            "current": 15,
            "snapshot": {},
        }))
        .expect("a planning error on a 200 reply is still a planning error");
        assert_eq!(failure.code(), "architecture_revision_conflict");
        assert_eq!(failure.class().token(), "conflict");
        assert!(
            failure.message().contains("15") && failure.message().contains("nothing was applied")
        );
        let detail = failure.detail_value().expect("the revisions");
        assert_eq!(detail["expected"], 12);
        assert_eq!(detail["current"], 15);
        // Never a re-send: the only next step reads the head for a human to
        // re-plan against.
        assert_eq!(
            failure.next_commands(),
            ["ds governance architecture get --output json"]
        );
        assert!(
            !failure
                .next_commands()
                .iter()
                .any(|next| next.contains("apply")),
            "a conflict must never suggest re-applying against the head that moved"
        );
    }

    #[test]
    fn every_violation_reaches_the_caller_on_its_own_line() {
        let failure = planning_error(&json!({
            "error": "validation_failed",
            "message": "3 violations",
            "violations": [
                { "field": "delivery_state", "target_id": "survey-form-factory", "message": "implemented needs one citation" },
                { "field": "evidence", "target_id": "survey-form-factory", "message": "evidence[] is empty" },
                { "field": "chapter_ref", "target_id": "cli-project-settings", "message": "a cross-chapter edge needs a doorway" },
            ],
        }))
        .expect("a validation failure");
        assert_eq!(failure.code(), "architecture_validation_failed");
        assert_eq!(failure.class().token(), "invalid_input");
        let detail = failure.detail_value().expect("violations");
        assert_eq!(detail["violations_total"], 3);
        assert_eq!(
            detail["violation_1"],
            "delivery_state · survey-form-factory — implemented needs one citation"
        );
        assert_eq!(
            detail["violation_2"],
            "evidence · survey-form-factory — evidence[] is empty"
        );
        assert_eq!(
            detail["violation_3"],
            "chapter_ref · cli-project-settings — a cross-chapter edge needs a doorway"
        );
    }

    #[test]
    fn a_long_violation_list_reports_what_it_did_not_print() {
        let violations: Vec<Value> = (0..9)
            .map(|index| json!({ "field": format!("f{index}"), "target_id": "t", "message": "no" }))
            .collect();
        let failure = planning_error(&json!({
            "error": "validation_failed",
            "message": "9 violations",
            "violations": violations,
        }))
        .expect("a validation failure");
        let detail = failure.detail_value().expect("violations");
        assert_eq!(detail["violations_total"], 9);
        assert_eq!(detail["violation_6"], "f5 · t — no");
        assert!(detail["violation_7"].is_null(), "the render is bounded");
        assert!(
            detail["violation_omitted"]
                .as_str()
                .is_some_and(|text| text.starts_with("3 more")),
            "a bounded render must say how many it did not print"
        );
    }

    #[test]
    fn the_server_names_the_condition_and_the_status_is_the_fallback() {
        assert_eq!(
            classify_planning_failure(refused(400, "validation_failed")).code(),
            "architecture_validation_failed"
        );
        assert_eq!(
            classify_planning_failure(refused(409, "revision_conflict")).code(),
            "architecture_revision_conflict"
        );
        assert_eq!(
            classify_planning_failure(refused(403, "platform administration is required")).code(),
            "architecture_not_permitted"
        );
        assert_eq!(
            classify_planning_failure(refused(404, "no such node")).code(),
            "architecture_not_found"
        );
        assert_eq!(
            classify_planning_failure(refused(409, "duplicate identity")).code(),
            "architecture_conflict"
        );
        // Anything the server did not name keeps the application's own
        // refusal rather than becoming a wrong named one.
        assert_eq!(
            classify_planning_failure(refused(500, "the backlog is unavailable")).code(),
            "desktop_refused"
        );
        // And a signed-out session is still the signed-out refusal.
        assert_eq!(
            classify_planning_failure(refused(401, "Sign in to DS GridDesign first.")).code(),
            "desktop_signed_out"
        );
    }

    #[test]
    fn a_reply_that_is_not_the_documented_shape_is_never_a_success() {
        assert!(
            require_fields(
                &json!({ "revision": 4, "snapshot": {} }),
                "x",
                &["revision", "snapshot"]
            )
            .is_ok()
        );
        for (reply, fields) in [
            (json!({ "snapshot": {} }), &["revision", "snapshot"][..]),
            (json!({ "revision": 4 }), &["revision", "snapshot"][..]),
            (json!({ "revision": 4, "nodes": {} }), &["nodes"][..]),
            (json!({ "applied": "yes" }), &["applied"][..]),
            (json!({ "command_id": "" }), &["command_id"][..]),
        ] {
            assert_eq!(
                require_fields(&reply, "governance.architecture.get", fields)
                    .expect_err("an undocumented reply")
                    .code(),
                "desktop_contract_mismatch"
            );
        }
    }

    #[test]
    fn the_hypothetical_requests_from_the_contract_are_discoverable_verbatim() {
        // Discovery metadata is what an agent reads before it knows what it
        // wants. These four are the contract's own wording and must survive
        // an edit to any one command's examples.
        const REQUESTS: &[&str] = &[
            "Record this form-factory question in the Survey lifecycle chapter.",
            "Link the CLI project-settings command to its ds-brain authority as inferred.",
            "Mark the transformer selection-set work implemented with exact evidence.",
            "Propose splitting this container as a recommended refactor.",
        ];
        let notes: Vec<&str> = DOMAIN
            .commands
            .iter()
            .flat_map(|command| command.examples.iter())
            .map(|example| example.note)
            .collect();
        for request in REQUESTS {
            assert!(
                notes.iter().any(|note| note.contains(request)),
                "`{request}` is not discoverable from any ds governance example"
            );
        }
    }
}
