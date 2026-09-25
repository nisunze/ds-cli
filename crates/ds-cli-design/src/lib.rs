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
//! ds-web's WASM adapter; only host file placement differs.
//! `design.features.select` separately restores the governed native user and
//! its audience-fenced project, fetches one fixed context projection, and
//! delegates deterministic selection to `ds-geo`.
//!
//! ## How collaboration reaches ds-brain
//!
//! Selections, attachments, tags, groups, and comments are governed shared
//! state behind ds-brain, which is the only gateway and the only authority: it
//! decides who may write, arbitrates two people editing the same record in
//! the same second, and refuses a write authored against a version that has
//! moved. None of that is reachable from a file, and none of it may be
//! reached with an ambient credential — so every collaboration command is one
//! closed kernel door (`ds_client_core::design_annotations`,
//! `known_columns`, the material-propagation report action) run under the
//! restored native user and its audience-fenced project by `ds auth`. Since
//! 2026-09-20 no command here needs a window: the Server and the desktop
//! answer the same (contract `dsgrid-authority/01-server-required.md`).
//!
//! The separate `design features select` read restores the same governed
//! native user, fetches one closed context projection, and delegates
//! selection to `ds-geo`; it accepts no project override.
//!
//! ## Why this is not `ds map`
//!
//! No command here needs a map instance, an edit session, or an open design
//! room: local LV processing consumes an explicit file; a selection is a list of
//! stable identities; an attachment is bytes with a media type; a tag is a
//! value from a project's own vocabulary. `ds map` owns local map state; this
//! domain owns none.
//!
//! ## What the family is
//!
//! ```text
//!   status     the project's transformer status rows (headless, unreshaped)
//!   lv         project-export → process
//!   transformer inventory → retire | restore; status; dashboard
//!   selection  list → read → save | archive | assign
//!   attachment list | list-project | show | versions → publish | download | set-latest | retire
//!   tag        list | query → define | set; enrich-preview → enrich-apply
//!   group      list → preview → apply | unassign; export
//!   consumer-grouping preview → apply; read | archive
//!   comment    list → read → post | resolve | promote | redact
//! ```
//!
//! ## What is deliberately absent
//!
//! **An edit.** A comment is append-only and an attachment revision is
//! immutable; there is no `comment edit` and no `attachment replace` because
//! there is no such server action. Removing comment text is a moderator's
//! audited redaction, `comment redact`, fenced on the thread version the
//! moderator read so what is removed is what was read.

pub mod activities;
pub mod attachment;
pub mod autoprocess;
pub mod category_catalog;
pub mod collisions;
pub mod comment;
pub mod config;
pub mod data_lane;
pub mod features;
pub mod feeder_limits;
pub mod force_gate;
pub mod group;
pub mod grouping;
pub mod headless;
pub mod intake_upload;
pub mod known_columns;
pub mod lv;
pub mod materials;
pub mod migrate;
pub mod native_tags;
pub mod pinned;
pub mod preview;
pub mod process_settings;
pub mod project;
pub mod selection;
pub mod tag;
pub mod transformer;
pub mod versions;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::{Map, Value, json};

// Neutral argument helpers: a numeric bound and an English count say
// nothing about a paired window, so they come from the contract crate.
pub use ds_cli_contract::args::{INVALID_NUMBER, integer, plural};

pub static DOMAIN: Domain = Domain {
    id: "design",
    summary: "Headless reads, offline LV compute, and governed collaboration.",
    commands: &[
        &native_tags::DEFINITIONS,
        &native_tags::PROJECTION,
        &native_tags::PREVIEW,
        &native_tags::APPLY,
        &config::SHEETS,
        &config::READ,
        &config::DIFF,
        &config::SET,
        &config::SAVE,
        &config::DUPLICATE,
        &project::SOURCES,
        &project::INIT,
        &project::WRITE,
        &project::EDIT,
        &project::READ,
        &project::RESTORE,
        &project::REVISIONS,
        &project::COMPARE,
        &project::STATUS,
        &project::PROCESS,
        &project::CANCEL,
        &project::RESULT,
        &project::OUTBOX,
        &project::REPORT,
        &features::COMMAND,
        &selection::list::COMMAND,
        &selection::read::COMMAND,
        &selection::save::COMMAND,
        &selection::archive::COMMAND,
        &selection::assign::COMMAND,
        &attachment::list::COMMAND,
        &attachment::list_project::COMMAND,
        &attachment::show::COMMAND,
        &attachment::versions::COMMAND,
        &attachment::publish::COMMAND,
        &attachment::download::COMMAND,
        &attachment::set_latest::COMMAND,
        &attachment::retire::COMMAND,
        &tag::list::COMMAND,
        &tag::query::COMMAND,
        &tag::define::COMMAND,
        &tag::set::COMMAND,
        &tag::enrich::PREVIEW_COMMAND,
        &tag::enrich::APPLY_COMMAND,
        &materials::PREVIEW,
        &feeder_limits::READ,
        &feeder_limits::SET,
        &category_catalog::METER,
        &category_catalog::METER_DEFAULT,
        &category_catalog::READ,
        &category_catalog::ALIAS_SET,
        &category_catalog::CUSTOMER_UNBIND,
        &category_catalog::CUSTOMER_RENAME,
        &category_catalog::CUSTOMER_RETIRE,
        &category_catalog::CUSTOMER_RETIRE_UNNAMED,
        &materials::APPLY,
        &known_columns::list::COMMAND,
        &known_columns::set::COMMAND,
        &group::list::COMMAND,
        &group::preview::COMMAND,
        &group::apply::COMMAND,
        &group::unassign::COMMAND,
        &group::export::COMMAND,
        &grouping::preview::COMMAND,
        &grouping::apply::COMMAND,
        &grouping::read::READ_COMMAND,
        &grouping::read::ARCHIVE_COMMAND,
        &comment::list::COMMAND,
        &comment::read::COMMAND,
        &comment::post::COMMAND,
        &comment::resolve::COMMAND,
        &comment::promote::COMMAND,
        &comment::redact::COMMAND,
        &lv::project_export::COMMAND,
        &lv::project_save::COMMAND,
        &lv::process::COMMAND,
        &process_settings::COMMAND,
        &collisions::COMMAND,
        &migrate::plan::COMMAND,
        &migrate::apply::COMMAND,
        &pinned::COMMAND,
        &autoprocess::COMMAND,
        &force_gate::COMMAND,
        &data_lane::COMMAND,
        &transformer::status::COMMAND,
        &activities::sweep::COMMAND,
        &activities::read::COMMAND,
        &preview::BULK_PLAN,
        &preview::DOWNLOAD_PLAN,
        &versions::STATUS,
        &versions::LIST,
        &versions::COMPARE,
        &versions::BEGIN,
        &versions::RESTORE,
        &preview::CONFLICT_LIST,
        &preview::CONFLICT_CHECK,
        &preview::PRESENCE_STATUS,
        &transformer::dashboard::COMMAND,
        &transformer::inventory::COMMAND,
        &transformer::retire::COMMAND,
        &transformer::restore::COMMAND,
    ],
};

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

/// Which native credential lane a headless design command authenticates on.
pub const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Native credential lane.")
    .choices(&["stable", "canary"])
    .default("stable");
/// The project a headless design command is about, always named: the saved
/// selection is never read, so one host serves several projects at once.
pub const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact ds_project this call is about; the saved selection is never read.",
)
.required();

/// The refusals the headless project client can answer with, for every
/// headless command of this domain. Declared once in `ds auth`.
pub const HEADLESS_REFUSALS: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
/// The route's own three: a malformed request, a missing record, a fault.
const ROUTE_REFUSALS: [Refusal; 3] = [
    INVALID_DESIGN_REQUEST,
    DESIGN_RECORD_NOT_FOUND,
    DESIGN_SERVICE_FAILED,
];
/// The base every headless collaboration command declares.
pub const HEADLESS_BASE: usize = 15 + 3;
const _: () = assert!(HEADLESS_REFUSALS.len() + ROUTE_REFUSALS.len() == HEADLESS_BASE);

/// The headless set, the route's own, then the command's own. `TOTAL` is
/// `HEADLESS_BASE + own.len()`, checked at compile time; the
/// [`headless_refusals!`] macro states it from the list.
pub const fn headless_refusal_table<const TOTAL: usize>(own: &[Refusal]) -> [Refusal; TOTAL] {
    assert!(TOTAL == HEADLESS_BASE + own.len());
    let mut out = [INVALID_DESIGN_REQUEST; TOTAL];
    let mut i = 0;
    while i < HEADLESS_REFUSALS.len() {
        out[i] = HEADLESS_REFUSALS[i];
        i += 1;
    }
    let mut k = 0;
    while k < ROUTE_REFUSALS.len() {
        out[i + k] = ROUTE_REFUSALS[k];
        k += 1;
    }
    i += ROUTE_REFUSALS.len();
    let mut j = 0;
    while j < own.len() {
        out[i + j] = own[j];
        j += 1;
    }
    out
}

/// `headless_refusals!(A, B, C)` — the table above with `TOTAL` counted
/// from the list, so a command adds a refusal without restating a number.
#[macro_export]
macro_rules! headless_refusals {
    ($($refusal:expr),* $(,)?) => {
        $crate::headless_refusal_table::<{ $crate::HEADLESS_BASE + [$(stringify!($refusal)),*].len() }>(&[$($refusal),*])
    };
}

// ---------------------------------------------------------------------------
// Refusals this domain adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const DESIGN_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "no such record, or ds-brain declined the request",
    remedy: "check the id with the matching `list` command; read detail.detail for its message",
};
/// What `desktop_refused` used to hide. ds-brain answers a rejected bound, an
/// absent record and a fault of its own with three different HTTP statuses,
/// and all three arrived as one code of class `failed` — which tells a caller
/// to retry the first two, though the identical call can never succeed.
///
/// The class is the point of separating them: the first two are
/// `invalid_input` and want a different request, the third is `unavailable`
/// and wants the same one later.
pub const INVALID_DESIGN_REQUEST: Refusal = ds_cli_auth::DESIGN_REQUEST_INVALID_REFUSAL;
pub const DESIGN_RECORD_NOT_FOUND: Refusal = ds_cli_auth::DESIGN_RECORD_NOT_FOUND_REFUSAL;
pub const DESIGN_SERVICE_FAILED: Refusal = ds_cli_auth::DESIGN_SERVICE_FAILED_REFUSAL;
pub const NOT_PERMITTED: Refusal = ds_cli_auth::DESIGN_NOT_PERMITTED_REFUSAL;
pub const CONFLICT: Refusal = ds_cli_auth::DESIGN_VERSION_CONFLICT_REFUSAL;
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
/// The read path's half of the same rule: a predicate value the project never
/// authored is refused rather than answered with an empty — or, for
/// `not_equals`, a complete — row set the caller would read as a fact.
pub const TAG_VALUE_NOT_IN_VOCABULARY: Refusal = Refusal {
    code: "tag_value_not_in_vocabulary",
    when: "a choice predicate names a value the definition's stored vocabulary does not contain",
    remedy: "read the vocabulary with `ds design tag list` and pass one of its values",
};
pub const TOO_MANY_TAG_FILTERS: Refusal = Refusal {
    code: "too_many_tag_filters",
    when: "a project tag query carries more than 20 predicates",
    remedy: "narrow or split the query so one call carries at most 20 predicates",
};
/// The selected definition is absent or not eligible for single-value LV
/// transformer batching.
pub const UNKNOWN_TAG_GROUP: Refusal = Refusal {
    code: "unknown_tag_group",
    when: "--group does not identify a batchable project tag definition",
    remedy: "read the available definition ids with `ds design group list`",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that changes governed project state",
    remedy: "re-run with --yes once you intend the change",
};

/// What ds-brain says when the signed-in user may read but not write. Hand
/// copies of the route's prose (`ds-brain/app/routes/design_annotations.py`
/// and its neighbours), read case-insensitively off the refusal message.
pub const NOT_PERMITTED_MARKERS: &[&str] = &["capability", "permission denied"];

/// What ds-brain says when a record moved under a command in flight.
pub const CONFLICT_MARKERS: &[&str] = &["not ", "changed since", "already exists"];

/// What ds-brain says when the project accepts no changes at all.
pub const READ_ONLY_MARKERS: &[&str] = &["archived", "expired", "read-only"];
pub const TAG_VALUE_CASE_MISMATCH_MARKERS: &[&str] =
    &["authored spelling exactly", "differ only by case"];

/// The identities still open to refinement by [`classify_design_failure`].
///
/// These are what `ds auth` mints from an HTTP status alone, and a status is
/// a starting point rather than an answer: an archived project and a missing
/// capability are the same 403, and a case-mismatched tag value and an
/// exceeded bound are the same 400. So the branches below refine those — the
/// narrower code, its remedy and its next step are what an unattended caller
/// acts on.
///
/// Nothing else is touched. An `unavailable` identity in particular is never
/// refined into an authority answer: the service did not speak, so its message
/// carries no condition to read.
const REFINABLE_CODES: &[&str] = &[
    "design_request_invalid",
    "design_record_not_found",
    "design_not_permitted",
    "design_version_conflict",
];

/// Give this domain's named conditions their own codes.
///
/// They arrive as ordinary route refusals — ds-brain answered, and what it
/// answered was "you may not", "you were too late", or "this project is
/// closed". Letting them through as the coarse code they arrive with would send
/// a caller to read a message for conditions that have a name, a remedy and a
/// *different next step*: one needs an admin, one needs a re-read and retry, and
/// one is not going to succeed today at all. Telling them apart is the whole
/// reason an unattended caller can act on a refusal.
pub fn classify_design_failure(failure: Failure) -> Failure {
    if !REFINABLE_CODES.contains(&failure.code()) {
        return failure;
    }
    // A structured refusal carries `http_status` in `detail` and the route's
    // own sentence in `message`; an older shape carried that sentence in
    // `detail.detail`. Read whichever is present, because an unmatched branch
    // below is indistinguishable from "no condition applies" — and reading an
    // empty string would silently retire every named code this domain
    // declares.
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_else(|| failure.message())
        .to_ascii_lowercase();

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
        let failure = Failure::invalid(
            "design_request_invalid",
            "phase allows \"Phase II\", not \"phase ii\"; use the authored spelling exactly",
        )
        .detail(json!({ "http_status": 400 }));

        let classified = classify_design_failure(failure);
        assert_eq!(classified.code(), "tag_value_case_mismatch");
        assert_eq!(
            classified.remedy_text(),
            Some(TAG_VALUE_CASE_MISMATCH.remedy)
        );
    }

    #[test]
    fn unrelated_design_refusal_stays_generic() {
        let failure = Failure::invalid("design_request_invalid", "the transformer does not exist")
            .detail(json!({ "http_status": 400 }));

        assert_eq!(
            classify_design_failure(failure).code(),
            "design_request_invalid"
        );
    }

    /// A signed-out or paired-window identity is not this domain's to refine:
    /// the headless client answers `headless_signed_out` itself, and nothing
    /// here renames a code it does not own.
    #[test]
    fn foreign_identities_pass_through_untouched() {
        let failure = Failure::unauthorized("headless_signed_out", "no native user is signed in")
            .detail(json!({ "detail": "permission denied" }));
        assert_eq!(
            classify_design_failure(failure).code(),
            "headless_signed_out"
        );
    }

    /// `ds auth` types a refused envelope from its HTTP status before this
    /// domain sees it, so its conditions arrive with a coarse code and the
    /// route's prose in `message`. They are still the same conditions, and
    /// their remedies are not interchangeable: no admin can grant a capability
    /// on an archived project.
    #[test]
    fn a_status_typed_refusal_is_still_refined_to_its_own_condition() {
        let archived = Failure::unauthorized(
            "design_not_permitted",
            "this project is archived and accepts no design changes (403)",
        )
        .detail(json!({ "http_status": 422 }));
        let archived = classify_design_failure(archived);
        assert_eq!(archived.code(), "design_project_read_only");
        assert_eq!(archived.remedy_text(), Some(READ_ONLY.remedy));

        let case_only = Failure::invalid(
            "design_request_invalid",
            "city allows \"Kigali\", not \"kigali\"; use the authored spelling exactly (400)",
        )
        .detail(json!({ "http_status": 422 }));
        let case_only = classify_design_failure(case_only);
        assert_eq!(case_only.code(), "tag_value_case_mismatch");
        assert_eq!(
            case_only.remedy_text(),
            Some(TAG_VALUE_CASE_MISMATCH.remedy)
        );

        // An API that never answered says nothing about authority, so it is
        // never refined into one — even when its message happens to carry a
        // word one of the branches reads.
        let unreachable = Failure::unavailable(
            "auth_transient",
            "the Data Solutions API did not answer; the project may be archived",
        );
        assert_eq!(
            classify_design_failure(unreachable).code(),
            "auth_transient"
        );

        // A status-typed refusal with no condition in it keeps the identity the
        // application gave it, rather than being renamed for want of a match.
        let plain = Failure::invalid(
            "design_request_invalid",
            "tag query matched more than limit 200; raise limit explicitly (400)",
        )
        .detail(json!({ "http_status": 422 }));
        assert_eq!(
            classify_design_failure(plain).code(),
            "design_request_invalid"
        );
    }
}
