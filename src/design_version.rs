//! Deliberate transformer versions — has the saved state moved, and may the
//! pen open?
//!
//! `metadata.version` is the optimistic-concurrency SAVE COUNTER, not a
//! version. The two meet at exactly one point: `lead_source_version`, the save
//! generation present when the deliberate version began. Comparing those two
//! integers is the whole of "has this transformer moved since its lead" — and
//! a backend that reports neither cannot prove the document is unchanged, so
//! it reports changed and the prompt is offered rather than silently skipped.
//!
//! Five ordered clauses decide ask-vs-open, and the ORDER is the authority: a
//! gesture that closes a session, or one the entry gates already refused, must
//! never raise a dialog, because in neither case is an edit about to start.
//!
//! `design_versions` is a different concept with a near-identical name: it
//! governs OFFLINE LOCAL version rooms and explicitly disclaims server vN
//! identity. This module answers the server-assigned ordinal's prompt. The
//! ordinal itself stays ds-brain's — the kernel only says which one is next.
//!
//! Pure: `now` is never read, no IO, no clock. Every label that leaves here is
//! a token or a `v<n>` identifier, never a sentence.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::design::row_allows;
/// The browser's `finiteInteger`. One spelling, in `design_status_row`, which
/// reads the same ordinals off the same rows for the register.
use crate::design_status_row::finite_ordinal;

pub const SCHEMA: &str = "ds.design-version/v1";

/// A whole project's rows travel in one `facts` call.
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// The honest internal "never versioned" sentinel, never a fabricated v0.
pub const UNVERSIONED: i64 = 0;

/// A reason is what makes the history readable a month later, so it is
/// required — and bounded here so every entry point submits the same shape.
pub const VERSION_REASON_MAX: usize = 280;

// ── the answers ─────────────────────────────────────────────────────────

/// Everything a version surface reads about one row, decided once.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VersionFacts {
    pub name: String,
    /// The version currently in force; `UNVERSIONED` when there is none.
    pub version: i64,
    /// The highest ordinal ever assigned — never below the lead.
    pub latest: i64,
    /// One past the highest, so restoring to v7 and cutting again yields v11,
    /// never a second v8.
    pub next: i64,
    /// Deliberate metadata rows retained.
    pub count: i64,
    /// `v<n>` — an identifier, not a label to translate.
    pub label: String,
    /// The lead is not the newest snapshot: something was restored.
    pub restored: bool,
    pub unversioned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Has the SAVED state moved since the version in force was cut? There is
    /// no such comparison before the first version, so unversioned rows carry
    /// `null` rather than a fabricated movement signal.
    pub changed_since_version: Option<bool>,
    /// May this row carry server-assigned vN versions at all
    /// (`design::row_allows`)?
    pub versionable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<VersionLead>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VersionLead {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_ms: Option<i64>,
}

/// Why the pen opened without a stop, in the order the clauses are tested.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OpenReason {
    /// This click ENDS an existing session — never a version moment.
    Exiting,
    /// The entry gates already refused; do not stack a second dialog.
    Blocked,
    /// The version moment for this work has passed; beginning one now would
    /// misorder operator intent.
    LocallyDirty,
    Unversionable,
    Unchanged,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PromptDecision {
    /// Open the edit session straight away — no dialog.
    Open { reason: OpenReason },
    /// Offer to begin the next deliberate version before editing begins.
    Ask {
        current_version: i64,
        next_version: i64,
        first_ever: bool,
    },
}

#[derive(Serialize, Debug)]
struct FactsReply {
    schema: &'static str,
    op: &'static str,
    versions: Vec<VersionFacts>,
}

#[derive(Serialize, Debug)]
struct PromptReply {
    schema: &'static str,
    op: &'static str,
    #[serde(flatten)]
    decision: PromptDecision,
    facts: VersionFacts,
}

#[derive(Serialize, Debug)]
struct PartitionReply {
    schema: &'static str,
    op: &'static str,
    versionable: Vec<String>,
    skipped: Vec<String>,
}

#[derive(Serialize, Debug)]
struct ReasonReply {
    schema: &'static str,
    op: &'static str,
    normalized: String,
    valid: bool,
    max: usize,
}

/// Why an explicit upload option may (not) cut a version before it overwrites.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadOverwriteReason {
    /// Append adds; it destroys nothing.
    AppendMode,
    Unversionable,
    /// Nothing saved server-side = nothing an overwrite can destroy.
    NothingSaved,
    Eligible,
}

#[derive(Serialize, Debug)]
struct UploadOverwriteReply {
    schema: &'static str,
    op: &'static str,
    eligible: bool,
    reason: UploadOverwriteReason,
}

// ── the request ─────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Facts(FactsRequest),
    Prompt(Box<PromptRequest>),
    Partition(PartitionRequest),
    Reason(ReasonRequest),
    UploadOverwrite(Box<UploadOverwriteRequest>),
    Label(LabelRequest),
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct LabelRequest {
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    value: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct FactsRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    rows: Vec<Value>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PromptRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    /// The row as the register holds it. Absent for a transformer with no
    /// document yet — which is `UNVERSIONED`, and therefore changed.
    #[serde(default)]
    row: Option<Value>,
    /// The transformer the gesture targets. Falls back to `row.name`.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    exiting_session: bool,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    locally_dirty: bool,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PartitionRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    names: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ReasonRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct UploadOverwriteRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    #[serde(default)]
    row: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    merge_mode: String,
}

// ── the rules ───────────────────────────────────────────────────────────

fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn metadata_string(metadata: &Map<String, Value>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn version_metadata(row: Option<&Value>) -> Option<&Map<String, Value>> {
    row.and_then(object)?
        .get("version_metadata")
        .and_then(object)
}

fn row_name(row: Option<&Value>) -> String {
    row.and_then(object)
        .and_then(|map| map.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Has this transformer's SAVED state moved since the lead was cut?
///
/// This predicate is evaluated only after a version exists. The public facts
/// return `null` for an unversioned row. Otherwise the save counter is compared
/// with the one the lead froze; a backend that reports neither number cannot
/// prove the document is unchanged, so it reports changed.
pub fn changed_since_version(row: Option<&Value>) -> bool {
    let metadata = version_metadata(row);
    let lead =
        finite_ordinal(metadata.and_then(|map| map.get("lead_version"))).unwrap_or(UNVERSIONED);
    if lead == UNVERSIONED {
        return true;
    }
    let snapshotted = finite_ordinal(metadata.and_then(|map| map.get("lead_source_version")));
    let current = finite_ordinal(
        row.and_then(object)
            .and_then(|map| map.get("metadata"))
            .and_then(object)
            .and_then(|map| map.get("version")),
    );
    match (snapshotted, current) {
        (Some(snapshotted), Some(current)) => current != snapshotted,
        _ => true,
    }
}

/// Every version fact about one row, decided once.
///
/// The lead, the highest ordinal, `restored`, `unversioned` and the reason are
/// NOT decided here: `design_status_row::version_view` already decides them for
/// every row the register renders, and a second spelling would be free to
/// drift. This folds the ordinal arithmetic, the label, the row's versionable
/// kind and the movement test around that one answer.
pub fn facts(row: Option<&Value>, name_override: Option<&str>) -> VersionFacts {
    let empty = Map::new();
    let map = row.and_then(object).unwrap_or(&empty);
    let view = crate::design_status_row::version_view(map);
    let metadata = version_metadata(row);
    let count = finite_ordinal(metadata.and_then(|map| map.get("count"))).unwrap_or(view.latest);
    let name = name_override
        .map(str::to_string)
        .unwrap_or_else(|| row_name(row));
    let lead = (!view.unversioned)
        .then(|| VersionLead {
            reason: metadata.and_then(|map| metadata_string(map, "lead_reason")),
            user: metadata.and_then(|map| metadata_string(map, "lead_created_by")),
            when_ms: metadata.and_then(|map| {
                let value = crate::design_status_row::timestamp_ms(map.get("lead_created_at"));
                (value > 0).then_some(value)
            }),
        })
        .filter(|lead| lead.reason.is_some() || lead.user.is_some() || lead.when_ms.is_some());
    VersionFacts {
        versionable: row_allows(&name).versionable,
        name,
        version: view.version,
        latest: view.latest,
        next: view.latest + 1,
        count,
        label: format!("v{}", view.version),
        restored: view.restored,
        unversioned: view.unversioned,
        reason: view.reason,
        changed_since_version: (!view.unversioned).then(|| changed_since_version(row)),
        lead,
    }
}

/// The complete predicate for "ask vs just open". The order is the authority.
pub fn prompt(
    row: Option<&Value>,
    name: Option<&str>,
    exiting_session: bool,
    blocked: bool,
    locally_dirty: bool,
) -> (PromptDecision, VersionFacts) {
    let facts = facts(row, name);
    let decision = if exiting_session {
        PromptDecision::Open {
            reason: OpenReason::Exiting,
        }
    } else if blocked {
        PromptDecision::Open {
            reason: OpenReason::Blocked,
        }
    } else if locally_dirty {
        PromptDecision::Open {
            reason: OpenReason::LocallyDirty,
        }
    } else if !facts.versionable {
        PromptDecision::Open {
            reason: OpenReason::Unversionable,
        }
    } else if facts.changed_since_version == Some(false) {
        PromptDecision::Open {
            reason: OpenReason::Unchanged,
        }
    } else {
        PromptDecision::Ask {
            current_version: facts.version,
            next_version: facts.next,
            first_ever: facts.version == UNVERSIONED,
        }
    };
    (decision, facts)
}

/// Which of the operator's selected rows may actually be versioned. Reserved
/// and synthetic rows are dropped here rather than sent and rejected one by
/// one, so the confirmation can state the real count before anything is
/// written. Order and first-wins de-duplication are the browser's.
pub fn partition_versionable<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> (Vec<String>, Vec<String>) {
    let mut versionable = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for raw in names {
        let name = raw.trim();
        if name.is_empty() || !seen.insert(name.to_string()) {
            continue;
        }
        if row_allows(name).versionable {
            versionable.push(name.to_string());
        } else {
            skipped.push(name.to_string());
        }
    }
    (versionable, skipped)
}

/// Trimmed and bounded so every entry point submits the same shape.
pub fn normalize_reason(reason: &str) -> String {
    reason.trim().chars().take(VERSION_REASON_MAX).collect()
}

/// Whether an explicit upload option may create a version before overwrite.
pub fn upload_overwrite_reason(
    name: &str,
    merge_mode: &str,
    row: Option<&Value>,
) -> UploadOverwriteReason {
    if merge_mode == "append" {
        return UploadOverwriteReason::AppendMode;
    }
    if !row_allows(name).versionable {
        return UploadOverwriteReason::Unversionable;
    }
    let saved = finite_ordinal(
        row.and_then(object)
            .and_then(|map| map.get("metadata"))
            .and_then(object)
            .and_then(|map| map.get("version")),
    );
    match saved {
        Some(saved) if saved >= 1 => UploadOverwriteReason::Eligible,
        _ => UploadOverwriteReason::NothingSaved,
    }
}

// ── the entry point ─────────────────────────────────────────────────────

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Design version request exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design version request: {error}"))?;
    let reply = match request {
        Request::Facts(request) => serde_json::to_string(&FactsReply {
            schema: SCHEMA,
            op: "facts",
            versions: request
                .rows
                .iter()
                .map(|row| facts(Some(row), None))
                .collect(),
        }),
        Request::Prompt(request) => {
            let (decision, facts) = prompt(
                request.row.as_ref(),
                request.name.as_deref(),
                request.exiting_session,
                request.blocked,
                request.locally_dirty,
            );
            serde_json::to_string(&PromptReply {
                schema: SCHEMA,
                op: "prompt",
                decision,
                facts,
            })
        }
        Request::Partition(request) => {
            let (versionable, skipped) =
                partition_versionable(request.names.iter().map(String::as_str));
            serde_json::to_string(&PartitionReply {
                schema: SCHEMA,
                op: "partition",
                versionable,
                skipped,
            })
        }
        Request::Reason(request) => {
            let normalized = normalize_reason(&request.reason);
            serde_json::to_string(&ReasonReply {
                schema: SCHEMA,
                op: "reason",
                valid: !normalized.is_empty(),
                normalized,
                max: VERSION_REASON_MAX,
            })
        }
        Request::UploadOverwrite(request) => {
            let name = request
                .name
                .clone()
                .unwrap_or_else(|| row_name(request.row.as_ref()));
            let reason = upload_overwrite_reason(&name, &request.merge_mode, request.row.as_ref());
            serde_json::to_string(&UploadOverwriteReply {
                schema: SCHEMA,
                op: "upload_overwrite",
                eligible: reason == UploadOverwriteReason::Eligible,
                reason,
            })
        }
        Request::Label(request) => serde_json::to_string(&serde_json::json!({
            "schema": SCHEMA,
            "op": "label",
            "valid": request.value.strip_prefix('v').is_some_and(|ordinal| {
                !ordinal.is_empty()
                    && ordinal.bytes().all(|byte| byte.is_ascii_digit())
                    && (ordinal == "0" || !ordinal.starts_with('0'))
                    && ordinal.parse::<i64>().is_ok()
            }),
        })),
    };
    reply.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ask(row: Value) -> PromptDecision {
        prompt(Some(&row), None, false, false, false).0
    }

    #[test]
    fn an_unversioned_transformer_is_always_offered_a_first_version() {
        let decision = ask(json!({"name": "tx_1", "metadata": {"version": 9}}));
        assert_eq!(
            decision,
            PromptDecision::Ask {
                current_version: 0,
                next_version: 1,
                first_ever: true
            }
        );
    }

    #[test]
    fn an_unmoved_save_counter_opens_the_pen_without_a_dialog() {
        let decision = ask(json!({
            "name": "tx_1",
            "metadata": {"version": 4},
            "version_metadata": {"lead_version": 2, "lead_source_version": 4},
        }));
        assert_eq!(
            decision,
            PromptDecision::Open {
                reason: OpenReason::Unchanged
            }
        );
    }

    #[test]
    fn a_backend_that_proves_nothing_reports_changed() {
        let decision = ask(json!({
            "name": "tx_1",
            "version_metadata": {"lead_version": 3},
        }));
        assert_eq!(
            decision,
            PromptDecision::Ask {
                current_version: 3,
                next_version: 4,
                first_ever: false
            }
        );
    }

    #[test]
    fn the_five_clauses_keep_their_order() {
        let row = json!({"name": "tx_1", "version_metadata": {"lead_version": 1}});
        // Exiting beats everything, including a row that would otherwise ask.
        assert_eq!(
            prompt(Some(&row), None, true, true, true).0,
            PromptDecision::Open {
                reason: OpenReason::Exiting
            }
        );
        assert_eq!(
            prompt(Some(&row), None, false, true, true).0,
            PromptDecision::Open {
                reason: OpenReason::Blocked
            }
        );
        assert_eq!(
            prompt(Some(&row), None, false, false, true).0,
            PromptDecision::Open {
                reason: OpenReason::LocallyDirty
            }
        );
        // Unversionable is tested after the three gesture clauses.
        let collisions = json!({"name": "collisions"});
        assert_eq!(
            prompt(Some(&collisions), None, false, false, false).0,
            PromptDecision::Open {
                reason: OpenReason::Unversionable
            }
        );
    }

    #[test]
    fn restore_then_cut_yields_one_past_the_highest() {
        let restored = json!({
            "name": "tx_1",
            "version_metadata": {"lead_version": 7, "latest_version": 10},
        });
        let facts = facts(Some(&restored), None);
        assert_eq!((facts.version, facts.latest, facts.next), (7, 10, 11));
        assert!(facts.restored);
        assert_eq!(facts.count, 10);
    }

    #[test]
    fn the_version_lead_carries_its_human_reason_and_stamp() {
        let row = json!({
            "name": "tx_1",
            "version_metadata": {
                "lead_version": 2,
                "lead_reason": "before site visit",
                "lead_created_by": "operator@example.com",
                "lead_created_at": "2026-09-15T10:00:00Z"
            }
        });
        assert_eq!(
            facts(Some(&row), None).lead,
            Some(VersionLead {
                reason: Some("before site visit".into()),
                user: Some("operator@example.com".into()),
                when_ms: Some(1_789_466_400_000),
            })
        );
    }

    #[test]
    fn a_latest_below_the_lead_never_lowers_it() {
        let row =
            json!({"name": "tx_1", "version_metadata": {"lead_version": 9, "latest_version": 4}});
        assert_eq!(facts(Some(&row), None).latest, 9);
    }

    #[test]
    fn partition_drops_reserved_and_synthetic_rows_once() {
        let (versionable, skipped) = partition_versionable([
            "tx_1",
            "mv_data",
            "collisions",
            " tx_1 ",
            "",
            "combined_transformer",
        ]);
        assert_eq!(versionable, vec!["tx_1", "mv_data"]);
        assert_eq!(skipped, vec!["collisions", "combined_transformer"]);
    }

    #[test]
    fn a_reason_is_trimmed_and_bounded() {
        assert!(normalize_reason("   ").is_empty());
        assert_eq!(
            normalize_reason("  before site visit  "),
            "before site visit"
        );
        assert_eq!(
            normalize_reason(&"x".repeat(400)).chars().count(),
            VERSION_REASON_MAX
        );
    }

    #[test]
    fn append_destroys_nothing_and_an_unsaved_document_has_nothing_to_lose() {
        let saved = json!({"name": "tx_1", "metadata": {"version": 3}});
        assert_eq!(
            upload_overwrite_reason("tx_1", "append", Some(&saved)),
            UploadOverwriteReason::AppendMode
        );
        assert_eq!(
            upload_overwrite_reason("tx_1", "replace_all", Some(&saved)),
            UploadOverwriteReason::Eligible
        );
        assert_eq!(
            upload_overwrite_reason("tx_1", "replace_key", Some(&json!({"name": "tx_1"}))),
            UploadOverwriteReason::NothingSaved
        );
        assert_eq!(
            upload_overwrite_reason("collisions", "replace_all", Some(&saved)),
            UploadOverwriteReason::Unversionable
        );
    }

    #[test]
    fn evaluate_answers_every_op() {
        for request in [
            json!({"op": "facts", "rows": [{"name": "tx_1"}]}),
            json!({"op": "prompt", "name": "tx_1", "row": {"name": "tx_1"}}),
            json!({"op": "partition", "names": ["tx_1"]}),
            json!({"op": "reason", "reason": "baseline"}),
            json!({"op": "upload_overwrite", "name": "tx_1", "merge_mode": "append"}),
            json!({"op": "label", "value": "v12"}),
        ] {
            let reply = evaluate(&serde_json::to_vec(&request).unwrap()).expect("op answers");
            assert!(reply.contains(SCHEMA), "{reply}");
        }
    }
}
