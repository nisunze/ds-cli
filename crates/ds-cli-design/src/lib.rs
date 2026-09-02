//! `ds design` — headless reads, local compute, and governed collaboration.
//!
//! ## Headless reads and offline compute
//!
//! `design.lv.project-export` reads one fenced governed snapshot and asks
//! ds-network to encode its layers with explicit owner defaults into the same
//! closed request consumed by `design.lv.process`. The process command writes
//! one local result document through ds-network's native Rayon adapter.
//! It has no project id, credential, Desktop bridge, map state, browser store,
//! or generic engine operation. The Rust kernel is the same owner used below
//! ds-web's Fast WASM adapter; only host file placement differs.
//! `design.features.select` separately restores the governed native user and
//! its audience-fenced project, fetches one fixed context projection, and
//! delegates deterministic selection to `ds-geo`.
//!
//! ## Why collaboration uses the bridge
//!
//! Selections, attachments, tags, groups, and comments are governed shared
//! state behind ds-brain, which is the
//! only gateway and the only authority: it decides who may write, arbitrates
//! two people editing the same record in the same second, and refuses a write
//! authored against a version that has moved. None of that is reachable from a
//! file, and none of it may be reached with an ambient credential — so every
//! collaboration command is one named semantic operation the *paired
//! application* performs under the session it already holds.
//!
//! Collaboration commands ask the paired application for an outcome. The
//! separate `design features select` read restores the governed native user
//! and its audience-fenced project, fetches one closed context projection, and
//! delegates selection to `ds-geo`; it accepts no Desktop descriptor or
//! project override.
//!
//! ## Why this is not `ds map`
//!
//! No command here needs a map instance, an edit session, or an open design
//! room: local Fast LV consumes an explicit file; a selection is a list of
//! stable identities; an attachment is bytes with a media type; a tag is a
//! value from a project's own vocabulary. `ds map` owns local map state; this
//! domain owns none.
//!
//! ## What the family is
//!
//! ```text
//!   lv         project-export → process
//!   selection  list → read → save | archive | assign
//!   attachment list → publish | download | retire
//!   tag        list | query → define | set
//!   group      list → preview → apply | unassign; export
//!   comment    list → read → post | resolve | promote
//! ```
//!
//! ## What is deliberately absent
//!
//! **An edit.** A comment is append-only and an attachment revision is
//! immutable; there is no `comment edit` and no `attachment replace` because
//! there is no such server action. Removing comment text is a moderator's
//! audited redaction, which stays in the application where the moderator can
//! read what they are removing before they remove it.

pub mod attachment;
pub mod comment;
pub mod features;
pub mod group;
pub mod grouping;
pub mod lv;
pub mod selection;
pub mod tag;

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::{Map, Value, json};

// The paired-application primitives every bridge domain shares. They are
// declared once in `ds-cli-desktop` — the authority surface — so a caller who
// learned `--desktop-descriptor` and the pairing refusals from `ds map` has
// learned them here too.
pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, INVALID_NUMBER, NOT_PAIRED, PAIRING_REJECTED, REFUSED,
    SIGNED_OUT, UNREACHABLE, UNREADABLE, UNSUPPORTED, classify_signed_out, integer, invoke, paired,
    paired_availability, plural,
};

pub static DOMAIN: Domain = Domain {
    id: "design",
    summary: "Headless reads, offline LV compute, and governed collaboration.",
    commands: &[
        &features::COMMAND,
        &selection::list::COMMAND,
        &selection::read::COMMAND,
        &selection::save::COMMAND,
        &selection::archive::COMMAND,
        &selection::assign::COMMAND,
        &attachment::list::COMMAND,
        &attachment::publish::COMMAND,
        &attachment::download::COMMAND,
        &attachment::retire::COMMAND,
        &tag::list::COMMAND,
        &tag::query::COMMAND,
        &tag::define::COMMAND,
        &tag::set::COMMAND,
        &group::list::COMMAND,
        &group::preview::COMMAND,
        &group::apply::COMMAND,
        &group::unassign::COMMAND,
        &group::export::COMMAND,
        &grouping::preview::COMMAND,
        &grouping::apply::COMMAND,
        &comment::list::COMMAND,
        &comment::read::COMMAND,
        &comment::post::COMMAND,
        &comment::resolve::COMMAND,
        &comment::promote::COMMAND,
        &lv::project_export::COMMAND,
        &lv::process::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

pub const SELECTION_LIST: BridgeOp = BridgeOp {
    operation: "design.selection.list",
    arguments: &["archived", "limit"],
};
pub const SELECTION_READ: BridgeOp = BridgeOp {
    operation: "design.selection.read",
    arguments: &["selection"],
};
pub const SELECTION_SAVE: BridgeOp = BridgeOp {
    operation: "design.selection.save",
    arguments: &["name", "transformers", "selection", "description"],
};
pub const SELECTION_ARCHIVE: BridgeOp = BridgeOp {
    operation: "design.selection.archive",
    arguments: &["selection", "restore"],
};
pub const SELECTION_ASSIGN: BridgeOp = BridgeOp {
    operation: "design.selection.assign",
    arguments: &["selection", "title", "owner", "purpose"],
};
pub const ATTACHMENT_LIST: BridgeOp = BridgeOp {
    operation: "design.attachment.list",
    arguments: &["kind", "object", "version", "archived"],
};
pub const ATTACHMENT_PUBLISH: BridgeOp = BridgeOp {
    operation: "design.attachment.publish",
    arguments: &[
        "kind",
        "object",
        "path",
        "attachment",
        "version",
        "label",
        "purpose",
    ],
};
pub const ATTACHMENT_DOWNLOAD: BridgeOp = BridgeOp {
    operation: "design.attachment.download",
    arguments: &["attachment", "revision"],
};
pub const ATTACHMENT_RETIRE: BridgeOp = BridgeOp {
    operation: "design.attachment.retire",
    arguments: &["attachment", "revision", "restore"],
};
pub const TAG_LIST: BridgeOp = BridgeOp {
    operation: "design.tag.list",
    arguments: &["kind", "object", "version"],
};
pub const TAG_DEFINE: BridgeOp = BridgeOp {
    operation: "design.tag.define",
    arguments: &[
        "definition",
        "name",
        "cardinality",
        "values",
        "description",
        "value_type",
        "input_control",
        "constraints",
    ],
};
pub const TAG_SET: BridgeOp = BridgeOp {
    operation: "design.tag.set",
    arguments: &[
        "kind",
        "object",
        "version",
        "definition",
        "values",
        "typed_values",
    ],
};
pub const TAG_QUERY: BridgeOp = BridgeOp {
    operation: "design.tag.query",
    arguments: &["kind", "match", "filters", "limit"],
};
pub const GROUP_LIST: BridgeOp = BridgeOp {
    operation: "design.group.list",
    arguments: &["transformers"],
};
pub const GROUP_PREVIEW: BridgeOp = BridgeOp {
    operation: "design.group.preview",
    arguments: &["group", "transformers", "value"],
};
pub const GROUP_APPLY: BridgeOp = BridgeOp {
    operation: "design.group.apply",
    arguments: &["group", "transformers", "value", "digest"],
};
pub const GROUP_UNASSIGN: BridgeOp = BridgeOp {
    operation: "design.group.unassign",
    arguments: &["group", "transformers", "digest"],
};
pub const GROUP_EXPORT: BridgeOp = BridgeOp {
    operation: "design.group.export",
    arguments: &["transformers", "definition-ids"],
};
pub const CONSUMER_GROUPING_PREVIEW: BridgeOp = BridgeOp {
    operation: "design.consumer-grouping.preview",
    arguments: &["transformers", "definition-ids", "bindings"],
};
pub const CONSUMER_GROUPING_APPLY: BridgeOp = BridgeOp {
    operation: "design.consumer-grouping.apply",
    arguments: &["transformers", "definition-ids", "bindings", "digest"],
};
pub const COMMENT_LIST: BridgeOp = BridgeOp {
    operation: "design.comment.list",
    arguments: &["kind", "object", "version", "resolved"],
};
pub const COMMENT_READ: BridgeOp = BridgeOp {
    operation: "design.comment.read",
    arguments: &["thread"],
};
pub const COMMENT_POST: BridgeOp = BridgeOp {
    operation: "design.comment.post",
    arguments: &["kind", "object", "version", "thread", "title", "body"],
};
pub const COMMENT_RESOLVE: BridgeOp = BridgeOp {
    operation: "design.comment.resolve",
    arguments: &["thread", "reopen"],
};
pub const COMMENT_PROMOTE: BridgeOp = BridgeOp {
    operation: "design.comment.promote",
    arguments: &["thread", "title"],
};

/// Every operation this domain can send, for the parity test to walk. A new
/// operation absent from this list cannot be sent: [`invoke`] takes a
/// [`BridgeOp`], and the test requires each one to be an operation the
/// application actually implements.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &SELECTION_LIST,
    &SELECTION_READ,
    &SELECTION_SAVE,
    &SELECTION_ARCHIVE,
    &SELECTION_ASSIGN,
    &ATTACHMENT_LIST,
    &ATTACHMENT_PUBLISH,
    &ATTACHMENT_DOWNLOAD,
    &ATTACHMENT_RETIRE,
    &TAG_LIST,
    &TAG_QUERY,
    &TAG_DEFINE,
    &TAG_SET,
    &GROUP_LIST,
    &GROUP_PREVIEW,
    &GROUP_APPLY,
    &GROUP_UNASSIGN,
    &GROUP_EXPORT,
    &CONSUMER_GROUPING_PREVIEW,
    &CONSUMER_GROUPING_APPLY,
    &COMMENT_LIST,
    &COMMENT_READ,
    &COMMENT_POST,
    &COMMENT_RESOLVE,
    &COMMENT_PROMOTE,
];

/// The largest page any design projection returns. The application bounds its
/// own projections to the same number; the total is always reported, so a
/// truncated page is never silent.
pub const MAX_PAGE_SIZE: i64 = 200;

/// The largest number of transformers one saved selection may name. A hand copy
/// of ds-brain's own bound, here only so an over-large list is refused locally
/// with a remedy rather than by a rejected write.
pub const MAX_SELECTION_MEMBERS: usize = 500;

/// The largest number of values one tag definition may declare.
pub const MAX_TAG_VALUES: usize = 100;

/// One backend tag query evaluates at most this many predicates.
pub const MAX_TAG_QUERY_FILTERS: usize = 20;

/// The backend deliberately refuses projects and result sets beyond this
/// bound instead of silently truncating Transformer Status membership.
pub const MAX_TAG_QUERY_ROWS: i64 = 2_000;

// ---------------------------------------------------------------------------
// Timeouts
// ---------------------------------------------------------------------------

/// A read is one governed round trip to ds-brain through the application.
pub const READ_TIMEOUT: Duration = Duration::from_secs(2 * 60);
/// A write is the same round trip; publishing an attachment additionally
/// streams bytes and waits for the server to hash and verify them.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(3 * 60);
/// Publishing carries the file. A native workspace over a field connection is
/// the slow case this budget exists for.
pub const PUBLISH_TIMEOUT: Duration = Duration::from_secs(20 * 60);

// ---------------------------------------------------------------------------
// Shared inputs
// ---------------------------------------------------------------------------

pub const KIND_ARG: Arg = Arg {
    name: "kind",
    kind: ArgKind::Value,
    value: "<object-kind>",
    required: true,
    default: None,
    choices: &["lv_transformer", "mv_model"],
    summary: "Which design object family the anchor names.",
};

pub const OBJECT_ARG: Arg = Arg {
    name: "object",
    kind: ArgKind::Value,
    value: "<id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The transformer name or the project DS Grid model id.",
};

pub const VERSION_ARG: Arg = Arg {
    name: "version",
    kind: ArgKind::Value,
    value: "<version-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Pin one exact object version. Omit for the object as a whole.",
};

pub const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("50"),
    choices: &[],
    summary: "Rows in one page (1-200). The total is always reported.",
};

// ---------------------------------------------------------------------------
// Refusals this domain adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const DESIGN_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "no such record, or ds-brain declined the request",
    remedy: "check the id with the matching `list` command; read detail.detail for its message",
};
pub const NOT_PERMITTED: Refusal = Refusal {
    code: "design_not_permitted",
    when: "the signed-in user may read this project's design records but not change them",
    remedy: "ask a project admin for the matching design capability",
};
pub const CONFLICT: Refusal = Refusal {
    code: "design_version_conflict",
    when: "the record moved while the command was in flight",
    remedy: "re-read with the matching `read` or `list` command and issue the command again",
};
pub const READ_ONLY: Refusal = Refusal {
    code: "design_project_read_only",
    when: "the project is archived or expired, so it accepts no design changes",
    remedy: "ask a project admin to unarchive it, or extend its expiry",
};
pub const INVALID_ANCHOR: Refusal = Refusal {
    code: "invalid_design_anchor",
    when: "the object anchor names a reserved document or a kind that does not exist",
    remedy: "anchor to an ordinary LV transformer or a project DS Grid model",
};
/// The paired desktop reads a named path through a BOUNDED reader. Publishing a
/// larger file from `ds` would need a streaming reader the shell does not have,
/// so the refusal names the surface that does rather than truncating the file to
/// a preview and registering a revision against the wrong bytes.
pub const TOO_LARGE: Refusal = Refusal {
    code: "attachment_too_large",
    when: "the file is larger than the paired desktop's bounded path reader",
    remedy: "publish it from the application's Attachments dialog, which streams from the file picker",
};
pub const TOO_MANY: Refusal = Refusal {
    code: "too_many_values",
    when: "a list flag carries more entries than the record accepts",
    remedy: "split the work, or pass fewer values",
};
pub const INVALID_VALUE_LIST: Refusal = Refusal {
    code: "invalid_value_list",
    when: "a comma-separated list is empty after whitespace and separators are removed",
    remedy: "pass at least one non-empty comma-separated value, e.g. --values ready,review",
};
pub const INVALID_TAG_INPUT: Refusal = Refusal {
    code: "invalid_tag_input",
    when: "typed tag flags conflict, or a typed query predicate is malformed",
    remedy: "read `ds design tag <command> --help` and pass one compatible value shape",
};
pub const TAG_VALUE_CASE_MISMATCH: Refusal = Refusal {
    code: "tag_value_case_mismatch",
    when: "a choice value differs from one stored vocabulary token only by case",
    remedy: "read the vocabulary and repeat its authored spelling exactly",
};
pub const TOO_MANY_TAG_FILTERS: Refusal = Refusal {
    code: "too_many_tag_filters",
    when: "a project tag query carries more than 20 predicates",
    remedy: "narrow or split the query so one call carries at most 20 predicates",
};
/// A third governed group does not exist. Refused locally because the set is
/// closed and declared on the descriptor: a round trip would only say the same
/// thing more slowly.
pub const UNKNOWN_TAG_GROUP: Refusal = Refusal {
    code: "unknown_tag_group",
    when: "--group named something other than the two governed groups",
    remedy: "pass --group city or --group phasing",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that changes governed project state",
    remedy: "re-run with --yes once you intend the change",
};

/// What the application says when the signed-in user may read but not write.
/// Hand copies of its prose, held to the application's source by
/// `tests/bridge_parity.rs`.
pub const NOT_PERMITTED_MARKERS: &[&str] = &["capability", "permission denied"];

/// What the application says when a record moved under a command in flight.
pub const CONFLICT_MARKERS: &[&str] = &["not ", "changed since", "already exists"];

/// What the application says when the project accepts no changes at all.
pub const READ_ONLY_MARKERS: &[&str] = &["archived", "expired", "read-only"];
pub const TAG_VALUE_CASE_MISMATCH_MARKERS: &[&str] =
    &["authored spelling exactly", "differ only by case"];

/// What the application says when a file exceeds its bounded path reader.
pub const TOO_LARGE_MARKERS: &[&str] = &["path reader is bounded"];

/// Give this domain's three named conditions their own codes.
///
/// All three arrive as ordinary operation refusals — the application answered,
/// and what it answered was "you may not", "you were too late", or "this
/// project is closed". Letting them through as `desktop_refused` would send a
/// caller to read `detail` for three conditions that have a name, a remedy and
/// a *different next step*: one needs an admin, one needs a re-read and retry,
/// and one is not going to succeed today at all. Telling them apart is the
/// whole reason an unattended caller can act on a refusal.
pub fn classify_design_failure(failure: Failure) -> Failure {
    let failure = classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    // Size is checked FIRST and on its own: it is a fact about the file, not
    // about authority, and it is the one refusal here whose remedy is another
    // surface rather than another permission.
    if TOO_LARGE_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::invalid(
            "attachment_too_large",
            "the file is larger than the paired desktop's bounded path reader",
        )
        .remedy(TOO_LARGE.remedy)
        .next("open the Attachments dialog in DS GridDesign");
    }
    // Among the authority answers, read-only is checked first: an archived
    // project also refuses for want of
    // a capability, and "unarchive it" is the actionable half of that answer.
    if READ_ONLY_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::unauthorized(
            "design_project_read_only",
            "the project is archived or expired, so it accepts no design changes",
        )
        .remedy(READ_ONLY.remedy)
        .next("ds project status");
    }
    if TAG_VALUE_CASE_MISMATCH_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::invalid(
            "tag_value_case_mismatch",
            "the choice value does not use the vocabulary's authored case",
        )
        .remedy(TAG_VALUE_CASE_MISMATCH.remedy)
        .next("ds design tag list --kind <kind> --object <object>");
    }
    if NOT_PERMITTED_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::unauthorized(
            "design_not_permitted",
            "the signed-in user may read this project's design records but not change them",
        )
        .remedy(NOT_PERMITTED.remedy)
        .next("ds design selection list");
    }
    if detail.contains("version") && CONFLICT_MARKERS.iter().any(|m| detail.contains(m)) {
        return Failure::conflict(
            "design_version_conflict",
            "the record moved while the command was in flight",
        )
        .remedy(CONFLICT.remedy)
        .next("ds design selection read --selection <selection-id>");
    }
    failure
}

/// The anchor flags, as the one map every anchored command sends.
///
/// Built once so `kind`, `object` and `version` cannot be spelled differently
/// by two commands that mean the same object.
pub fn anchor(inputs: &ds_cli_contract::Inputs) -> Result<Map<String, Value>, Failure> {
    let mut arguments = Map::new();
    arguments.insert("kind".into(), json!(inputs.require("kind")?));
    arguments.insert("object".into(), json!(inputs.require("object")?));
    if let Some(version) = inputs.value("version") {
        arguments.insert("version".into(), json!(version));
    }
    Ok(arguments)
}

/// Split a comma-separated list flag into values, refusing an over-long one.
///
/// Comma is the separator because these are ids and tag values, neither of
/// which may contain one. Splitting silently on whitespace would mangle a tag
/// label; refusing an over-long list locally saves a round trip that would be
/// rejected anyway.
pub fn list_values(raw: &str, flag: &str, max: usize) -> Result<Vec<String>, Failure> {
    let values: Vec<String> = raw
        .split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty() {
        return Err(
            Failure::invalid("invalid_value_list", format!("--{flag} carries no values"))
                .remedy(format!("pass a comma-separated list, e.g. --{flag} a,b")),
        );
    }
    if values.len() > max {
        return Err(Failure::invalid(
            "too_many_values",
            format!(
                "--{flag} carries {} values; the bound is {max}",
                values.len()
            ),
        )
        .remedy(TOO_MANY.remedy));
    }
    Ok(values)
}

#[cfg(test)]
mod tag_value_case_tests {
    use super::*;

    #[test]
    fn case_only_tag_refusal_has_one_actionable_public_code() {
        let failure = Failure::failed("desktop_refused", "the paired session refused the write")
            .detail(json!({
                "detail": "phase allows \"Phase II\", not \"phase ii\"; use the authored spelling exactly"
            }));

        let classified = classify_design_failure(failure);
        assert_eq!(classified.code(), "tag_value_case_mismatch");
        assert_eq!(
            classified.remedy_text(),
            Some(TAG_VALUE_CASE_MISMATCH.remedy)
        );
    }

    #[test]
    fn unrelated_design_refusal_stays_generic() {
        let failure = Failure::failed("desktop_refused", "the paired session refused the write")
            .detail(json!({ "detail": "the transformer does not exist" }));

        assert_eq!(classify_design_failure(failure).code(), "desktop_refused");
    }
}
