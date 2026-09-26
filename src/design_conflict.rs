//! May this operator overwrite the cloud head of a conflicted transformer?
//!
//! The browser used to answer this question TWICE. The tick and send gates
//! could disagree, and an unfinished review could reach a save. This module
//! evaluates one ordered refusal list for both actions, including the finished
//! review fence.
//!
//! This module is the ONE answer. The refusals keep their order and become
//! NAMED CODES; the tick is a declared view of the same evaluation, not a
//! second function. Every message that leaves here is an i18n KEY — the
//! sentences the browser carried belong to the host that renders them.
//!
//! ds-brain's own server-side version check does not move. This is the client
//! preflight only: it decides what to send, never what is accepted.
//!
//! Detection is here too, because the rule that MAKES a conflict lived in a
//! Svelte `$effect` — an authority in a render pass. It is arithmetic on two
//! integers in the row and belongs beside the gate that reads its result.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::design::{canonical_row_name, is_special_row};

pub const SCHEMA: &str = "ds.design-conflict/v1";

/// A whole project's rows travel in one `detect` call.
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

// ── the vocabulary ──────────────────────────────────────────────────────

/// The ordered refusals, in the order they are tested. A code is machine
/// vocabulary; `message_key` is what the host translates.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    /// 1. `pipeline.force` is required.
    ForceCapabilityMissing,
    /// 2. Overwrite was not ticked for this transformer.
    OverwriteNotSelected,
    /// 3. No typed version conflict is recorded.
    NoConflictRecorded,
    /// 4. Nothing was compared.
    ReviewMissing,
    /// 5. The local copy changed after comparison.
    LocalCopyChanged,
    /// 6. The local base version changed after comparison.
    LocalBaseVersionChanged,
    /// 7. The comparison was not pinned to a cloud version.
    ReviewNotPinned,
    /// 8. The local base version no longer matches the reviewed conflict.
    ConflictBaseVersionMismatch,
    /// 9. The cloud head moved after comparison.
    CloudHeadMoved,
    /// 10. The comparison never finished.
    ReviewNotFinished,
}

impl RefusalCode {
    /// The i18n key the host renders. No English leaves this kernel.
    pub fn message_key(self) -> &'static str {
        match self {
            Self::ForceCapabilityMissing => "dscf_force_capability_missing",
            Self::OverwriteNotSelected => "dscf_overwrite_not_selected",
            Self::NoConflictRecorded => "dscf_no_conflict_recorded",
            Self::ReviewMissing => "dscf_review_missing",
            Self::LocalCopyChanged => "dscf_local_copy_changed",
            Self::LocalBaseVersionChanged => "dscf_local_base_version_changed",
            Self::ReviewNotPinned => "dscf_review_not_pinned",
            Self::ConflictBaseVersionMismatch => "dscf_conflict_base_version_mismatch",
            Self::CloudHeadMoved => "dscf_cloud_head_moved",
            Self::ReviewNotFinished => "dscf_review_not_finished",
        }
    }

    /// Where in the ordered list this refusal sits — 1-based, as the review
    /// prose numbers them.
    pub fn ordinal(self) -> u8 {
        match self {
            Self::ForceCapabilityMissing => 1,
            Self::OverwriteNotSelected => 2,
            Self::NoConflictRecorded => 3,
            Self::ReviewMissing => 4,
            Self::LocalCopyChanged => 5,
            Self::LocalBaseVersionChanged => 6,
            Self::ReviewNotPinned => 7,
            Self::ConflictBaseVersionMismatch => 8,
            Self::CloudHeadMoved => 9,
            Self::ReviewNotFinished => 10,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub code: RefusalCode,
    pub message_key: &'static str,
    pub ordinal: u8,
}

impl From<RefusalCode> for Refusal {
    fn from(code: RefusalCode) -> Self {
        Self {
            code,
            message_key: code.message_key(),
            ordinal: code.ordinal(),
        }
    }
}

/// The wire fields one explicit force item carries. Minted only when the send
/// gate is open — never assembled by the host from its own reading.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForceFields {
    /// Always true; the field exists so the wire shape is the kernel's.
    pub force_version_conflict: bool,
    pub reviewed_current_version: i64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct CheckReply {
    pub schema: &'static str,
    pub op: &'static str,
    pub name: String,
    /// May this overwrite be SENT right now (all ordered refusals)?
    pub eligible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
    /// May the overwrite box be TICKED (the review-state refusals)?
    pub tick_admissible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick_refusal: Option<Refusal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_fields: Option<ForceFields>,
}

/// One row the register should record a typed version conflict for.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DetectedConflict {
    pub name: String,
    pub base_version: i64,
    pub current_version: i64,
    /// The code the browser's typed state carries.
    pub code: &'static str,
    /// How it was found. The register infers from the row; an explicit
    /// server refusal is the host's own evidence and is never invented here.
    pub evidence: &'static str,
}

#[derive(Serialize, Debug)]
struct DetectReply {
    schema: &'static str,
    op: &'static str,
    conflicts: Vec<DetectedConflict>,
}

// ── the request ─────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Check(Box<CheckRequest>),
    Detect(DetectRequest),
}

/// The recorded typed conflict — ds-brain's refusal, or the register's own
/// inference from the row.
#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub struct ConflictState {
    pub base_version: i64,
    pub current_version: i64,
}

/// The pinned comparison the operator looked at.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ReviewState {
    pub local_digest: String,
    pub base_version: i64,
    /// The exact cloud head that was fetched and compared. Absent when the
    /// comparison never pinned one.
    #[serde(default)]
    pub reviewed_current_version: Option<i64>,
    /// Did the comparison finish? Neither action may proceed before it does.
    #[serde(default)]
    pub reviewed: bool,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CheckRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    can_force: bool,
    #[serde(default)]
    explicitly_selected: bool,
    #[serde(default)]
    conflict: Option<ConflictState>,
    #[serde(default)]
    review: Option<ReviewState>,
    #[serde(default)]
    current_local_digest: Option<String>,
    #[serde(default)]
    current_base_version: Option<i64>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct DetectRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    rows: Vec<Value>,
}

// ── the rule ────────────────────────────────────────────────────────────

/// The ordered refusals, evaluated once. `Ok` is the send gate open.
fn send_refusal(request: &CheckRequest) -> Option<RefusalCode> {
    if !request.can_force {
        return Some(RefusalCode::ForceCapabilityMissing);
    }
    if !request.explicitly_selected {
        return Some(RefusalCode::OverwriteNotSelected);
    }
    let Some(conflict) = request.conflict.as_ref() else {
        return Some(RefusalCode::NoConflictRecorded);
    };
    // A conflict is only meaningful between two distinct positive persisted
    // counters. This keeps malformed host state from minting force fields.
    if conflict.base_version <= 0
        || conflict.current_version <= 0
        || conflict.base_version == conflict.current_version
    {
        return Some(RefusalCode::NoConflictRecorded);
    }
    let Some(review) = request.review.as_ref() else {
        return Some(RefusalCode::ReviewMissing);
    };
    let digest_matches = request
        .current_local_digest
        .as_deref()
        .filter(|digest| !digest.is_empty())
        .is_some_and(|digest| digest == review.local_digest);
    if !digest_matches {
        return Some(RefusalCode::LocalCopyChanged);
    }
    // The browser's `!input.currentBaseVersion` treats 0 as absent, and a
    // base version of zero is exactly "nothing saved yet" — not a base a
    // comparison can be pinned to.
    let base_matches = request
        .current_base_version
        .filter(|version| *version > 0)
        .filter(|_| review.base_version > 0)
        .is_some_and(|version| version == review.base_version);
    if !base_matches {
        return Some(RefusalCode::LocalBaseVersionChanged);
    }
    let Some(reviewed_current) = review.reviewed_current_version.filter(|value| *value > 0) else {
        return Some(RefusalCode::ReviewNotPinned);
    };
    if conflict.base_version != review.base_version {
        return Some(RefusalCode::ConflictBaseVersionMismatch);
    }
    if conflict.current_version != reviewed_current {
        return Some(RefusalCode::CloudHeadMoved);
    }
    if !review.reviewed {
        return Some(RefusalCode::ReviewNotFinished);
    }
    None
}

/// The tick gate: the same list, restricted to what a tick can know. It never
/// reads the capability, the tick itself, or the live digest/base — those move
/// under the operator's hand between ticking and sending.
fn tick_refusal(request: &CheckRequest) -> Option<RefusalCode> {
    let Some(conflict) = request.conflict.as_ref() else {
        return Some(RefusalCode::NoConflictRecorded);
    };
    if conflict.base_version <= 0
        || conflict.current_version <= 0
        || conflict.base_version == conflict.current_version
    {
        return Some(RefusalCode::NoConflictRecorded);
    }
    let Some(review) = request.review.as_ref() else {
        return Some(RefusalCode::ReviewMissing);
    };
    if !review.reviewed {
        return Some(RefusalCode::ReviewNotFinished);
    }
    if review.base_version <= 0 || conflict.base_version != review.base_version {
        return Some(RefusalCode::ConflictBaseVersionMismatch);
    }
    let Some(reviewed_current) = review.reviewed_current_version.filter(|value| *value > 0) else {
        return Some(RefusalCode::ReviewNotPinned);
    };
    if conflict.current_version != reviewed_current {
        return Some(RefusalCode::CloudHeadMoved);
    }
    None
}

fn check(request: &CheckRequest) -> CheckReply {
    let refusal = send_refusal(request);
    let tick = tick_refusal(request);
    let force_fields = match (refusal, request.review.as_ref()) {
        (None, Some(review)) => review
            .reviewed_current_version
            .filter(|value| *value > 0)
            .map(|reviewed_current_version| ForceFields {
                force_version_conflict: true,
                reviewed_current_version,
            }),
        _ => None,
    };
    CheckReply {
        schema: SCHEMA,
        op: "check",
        name: request.name.clone(),
        eligible: refusal.is_none(),
        refusal: refusal.map(Refusal::from),
        tick_admissible: tick.is_none(),
        tick_refusal: tick.map(Refusal::from),
        force_fields,
    }
}

/// Which rows are conflicted. A room this browser holds, dirty and known to
/// the server, whose positive save counter differs from the positive base it
/// was taken from, is a conflict — and the register recorded it from a render
/// pass until now. Version counters have no direction in this rule: a local
/// copy can carry a base from a different branch that is ahead of the reviewed
/// cloud head. The reviewed overwrite still pins that actual cloud head later.
pub fn detect(rows: &[Value]) -> Vec<DetectedConflict> {
    let mut conflicts = Vec::new();
    for row in rows {
        let Some(map) = row.as_object() else { continue };
        let name = map
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if name.is_empty() || is_special_row(name) {
            continue;
        }
        if map.get("client_dirty").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        if map.get("client_server_known").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let base_version = number(map.get("client_base_server_version"));
        let current_version = number(
            map.get("metadata")
                .and_then(Value::as_object)
                .and_then(|metadata| metadata.get("version")),
        );
        if base_version > 0 && current_version > 0 && current_version != base_version {
            conflicts.push(DetectedConflict {
                name: name.to_string(),
                base_version,
                current_version,
                code: "TRANSFORMER_VERSION_MISMATCH",
                evidence: "inferred",
            });
        }
    }
    conflicts
}

fn number(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().map(|raw| raw.trunc() as i64).unwrap_or(0),
        Some(Value::String(raw)) => raw
            .trim()
            .parse::<f64>()
            .map(|raw| raw.trunc() as i64)
            .unwrap_or(0),
        _ => 0,
    }
}

// ── the entry point ─────────────────────────────────────────────────────

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Design conflict request exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design conflict request: {error}"))?;
    match request {
        Request::Check(mut request) => {
            request.name = canonical_row_name(&request.name);
            serde_json::to_string(&check(&request))
        }
        Request::Detect(request) => serde_json::to_string(&DetectReply {
            schema: SCHEMA,
            op: "detect",
            conflicts: detect(&request.rows),
        }),
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eligible_request() -> CheckRequest {
        CheckRequest {
            schema: None,
            name: "tx_1".into(),
            can_force: true,
            explicitly_selected: true,
            conflict: Some(ConflictState {
                base_version: 4,
                current_version: 7,
            }),
            review: Some(ReviewState {
                local_digest: "abc".into(),
                base_version: 4,
                reviewed_current_version: Some(7),
                reviewed: true,
            }),
            current_local_digest: Some("abc".into()),
            current_base_version: Some(4),
        }
    }

    #[test]
    fn the_eligible_case_mints_the_wire_fields() {
        let reply = check(&eligible_request());
        assert!(reply.eligible && reply.tick_admissible);
        assert_eq!(
            reply.force_fields,
            Some(ForceFields {
                force_version_conflict: true,
                reviewed_current_version: 7
            })
        );
    }

    #[test]
    fn every_refusal_keeps_its_place_in_the_order() {
        type Mutation = Box<dyn Fn(&mut CheckRequest)>;
        let cases: Vec<(RefusalCode, Mutation)> = vec![
            (
                RefusalCode::ForceCapabilityMissing,
                Box::new(|r: &mut CheckRequest| r.can_force = false),
            ),
            (
                RefusalCode::OverwriteNotSelected,
                Box::new(|r: &mut CheckRequest| r.explicitly_selected = false),
            ),
            (
                RefusalCode::NoConflictRecorded,
                Box::new(|r: &mut CheckRequest| r.conflict = None),
            ),
            (
                RefusalCode::ReviewMissing,
                Box::new(|r: &mut CheckRequest| r.review = None),
            ),
            (
                RefusalCode::LocalCopyChanged,
                Box::new(|r: &mut CheckRequest| r.current_local_digest = Some("moved".into())),
            ),
            (
                RefusalCode::LocalBaseVersionChanged,
                Box::new(|r: &mut CheckRequest| r.current_base_version = Some(5)),
            ),
            (
                RefusalCode::ReviewNotPinned,
                Box::new(|r: &mut CheckRequest| {
                    r.review.as_mut().unwrap().reviewed_current_version = None
                }),
            ),
            (
                RefusalCode::ConflictBaseVersionMismatch,
                Box::new(|r: &mut CheckRequest| {
                    r.conflict.as_mut().unwrap().base_version = 3;
                }),
            ),
            (
                RefusalCode::CloudHeadMoved,
                Box::new(|r: &mut CheckRequest| r.conflict.as_mut().unwrap().current_version = 8),
            ),
        ];
        for (expected, mutate) in cases {
            let mut request = eligible_request();
            mutate(&mut request);
            let reply = check(&request);
            assert!(!reply.eligible, "{expected:?} stayed eligible");
            assert_eq!(reply.refusal.unwrap().code, expected);
            assert!(reply.force_fields.is_none());
        }
    }

    #[test]
    fn the_tick_gate_reads_the_review_state_from_the_send_gate() {
        // A tick is admissible without the capability and without the tick
        // itself — the two facts the send gate adds.
        let mut request = eligible_request();
        request.can_force = false;
        request.explicitly_selected = false;
        let reply = check(&request);
        assert!(!reply.eligible);
        assert!(reply.tick_admissible);

        // A comparison must finish before either action is allowed.
        let mut request = eligible_request();
        request.review.as_mut().unwrap().reviewed = false;
        let reply = check(&request);
        assert!(!reply.eligible);
        assert_eq!(reply.refusal.unwrap().code, RefusalCode::ReviewNotFinished);
        assert!(!reply.tick_admissible);
        assert_eq!(
            reply.tick_refusal.unwrap().code,
            RefusalCode::ReviewNotFinished
        );
    }

    #[test]
    fn a_zero_base_version_is_not_a_typed_conflict() {
        let mut request = eligible_request();
        request.current_base_version = Some(0);
        request.review.as_mut().unwrap().base_version = 0;
        request.conflict.as_mut().unwrap().base_version = 0;
        assert_eq!(
            check(&request).refusal.unwrap().code,
            RefusalCode::NoConflictRecorded
        );
    }

    #[test]
    fn malformed_or_equal_versions_never_mint_force_fields() {
        type Mutation = Box<dyn Fn(&mut CheckRequest)>;
        let cases: Vec<(&str, Mutation)> = vec![
            (
                "equal conflict",
                Box::new(|r| r.conflict.as_mut().unwrap().current_version = 4),
            ),
            (
                "negative local base",
                Box::new(|r| r.current_base_version = Some(-4)),
            ),
            (
                "negative review base",
                Box::new(|r| r.review.as_mut().unwrap().base_version = -4),
            ),
            (
                "negative reviewed head",
                Box::new(|r| r.review.as_mut().unwrap().reviewed_current_version = Some(-7)),
            ),
            (
                "negative conflict head",
                Box::new(|r| r.conflict.as_mut().unwrap().current_version = -7),
            ),
        ];
        for (name, mutate) in cases {
            let mut request = eligible_request();
            mutate(&mut request);
            let reply = check(&request);
            assert!(!reply.eligible, "{name} stayed eligible");
            assert!(reply.force_fields.is_none(), "{name} minted force fields");
        }
    }

    #[test]
    fn detection_reads_the_row_and_skips_special_rows() {
        let rows = vec![
            json!({"name": "tx_1", "client_dirty": true, "client_server_known": true,
                   "client_base_server_version": 4, "metadata": {"version": 7}}),
            // Not dirty.
            json!({"name": "tx_2", "client_base_server_version": 4, "metadata": {"version": 7}}),
            // Server does not know it.
            json!({"name": "tx_3", "client_dirty": true, "client_server_known": false,
                   "client_base_server_version": 4, "metadata": {"version": 7}}),
            // The head has not changed.
            json!({"name": "tx_4", "client_dirty": true, "client_base_server_version": 7,
                   "metadata": {"version": 7}}),
            // A local base may be from a different branch and ahead of the
            // current cloud head. It remains a conflict to review and resolve.
            json!({"name": "tx_5", "client_dirty": true, "client_base_server_version": 7,
                   "metadata": {"version": 4}}),
            // No base version: nothing to conflict with.
            json!({"name": "tx_6", "client_dirty": true, "metadata": {"version": 7}}),
            // Stored and local version counters must both be positive.
            json!({"name": "tx_7", "client_dirty": true, "client_base_server_version": -1,
                   "metadata": {"version": 4}}),
            json!({"name": "tx_8", "client_dirty": true, "client_base_server_version": 4,
                   "metadata": {"version": -1}}),
            json!({"name": "collisions", "client_dirty": true, "client_base_server_version": 4,
                   "metadata": {"version": 7}}),
        ];
        let conflicts = detect(&rows);
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].name, "tx_1");
        assert_eq!(
            (conflicts[0].base_version, conflicts[0].current_version),
            (4, 7)
        );
        assert_eq!(conflicts[0].evidence, "inferred");
        assert_eq!(
            (
                conflicts[1].name.as_str(),
                conflicts[1].base_version,
                conflicts[1].current_version
            ),
            ("tx_5", 7, 4)
        );
    }

    #[test]
    fn evaluate_answers_both_ops() {
        let check = evaluate(
            &serde_json::to_vec(&json!({"op": "check", "name": "TX 1", "can_force": true}))
                .unwrap(),
        )
        .expect("check answers");
        assert!(check.contains("\"name\":\"tx_1\""), "{check}");
        let detect = evaluate(&serde_json::to_vec(&json!({"op": "detect", "rows": []})).unwrap())
            .expect("detect answers");
        assert!(detect.contains("\"conflicts\":[]"), "{detect}");
    }
}
