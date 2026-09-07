//! `ds design consumer-grouping` — the project's persisted grouping plans.
//!
//! ```text
//!   preview → apply; read | archive
//! ```
//!
//! A plan is one consumer's answer to "which transformers belong in which
//! group, and by what authority?". The PURPOSE says which consumer, and it is a
//! closed vocabulary — a consumer with a private grouping rule is exactly what
//! this contract removes:
//!
//! * `solar_report` additionally binds each group to a governed Solar city id.
//!   A tag tuple is not a Solar city, so the binding is explicit and refused
//!   when missing.
//! * `report_archive` binds nothing. It is the folder and section authority for
//!   combined and compounded reports, which used to group privately on a Rwanda
//!   administrative column.
//!
//! The ORDER of `--definition-ids` is identity: `city,phase` and `phase,city`
//! are different plans with different digests.

pub mod apply;
pub mod preview;
pub mod read;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind};

pub const PURPOSE_ARG: Arg = Arg {
    name: "purpose",
    kind: ArgKind::Value,
    value: "<consumer>",
    required: false,
    default: Some("solar_report"),
    choices: &["solar_report", "report_archive"],
    summary: "Which consumer this plan groups for. report_archive binds no external source.",
};

/// Read the purpose.
///
/// The closed set lives in [`PURPOSE_ARG`]'s `choices`, which the parser
/// enforces before a handler runs — so there is deliberately no second
/// validation here to drift from it. The default keeps every caller written
/// before archive grouping existed working, and the value is never inferred
/// from what a caller happens to have selected.
pub fn purpose(inputs: &ds_cli_contract::Inputs) -> Result<String, Failure> {
    Ok(inputs
        .value("purpose")
        .unwrap_or("solar_report")
        .trim()
        .to_string())
}

/// The plan digest, which only `consumer-grouping preview` can mint.
///
/// `crate::group::DIGEST_ARG` is the same flag for a different family: it
/// fences an assignment batch and names `ds design group preview` as its
/// producer. Borrowing it here printed that producer in `--help`, sending the
/// operator to a command whose digest this one cannot accept.
pub const PLAN_DIGEST_ARG: Arg = Arg {
    name: "digest",
    kind: ArgKind::Value,
    value: "<plan-digest>",
    required: true,
    default: None,
    choices: &[],
    summary: "The plan_digest `ds design consumer-grouping preview` returned for this exact plan.",
};

/// The grouping dimensions, which a consumer plan must actually carry.
///
/// `crate::group::PROJECTION_DEFINITION_IDS_ARG` is optional and reads an
/// omitted flag as one untagged group. That is TRUE of the projection it was
/// authored for — `design group export` deliberately exports a single untagged
/// group — and FALSE here: a consumer grouping is 1-16 ordered definitions and
/// the server rejects an empty selection. So this family declares its own
/// required flag rather than inheriting a promise it cannot keep.
pub const DEFINITION_IDS_ARG: Arg = Arg {
    name: "definition-ids",
    kind: ArgKind::Value,
    value: "<ids>",
    required: true,
    default: None,
    choices: &[],
    summary: "Ordered comma-separated typed tag definition IDs (1-16). Order is identity; there is no untagged-only plan.",
};

/// Read the grouping dimensions.
///
/// Deliberately not [`crate::group::projection_definition_ids`]: that reader
/// turns an omitted flag into an empty selection because the projection
/// genuinely accepts one. A consumer grouping does not, so the omission is a
/// `missing_input` from the parser here, with a remedy naming the flag,
/// instead of a server string arriving after the round trip.
pub fn definition_ids(inputs: &ds_cli_contract::Inputs) -> Result<Vec<String>, Failure> {
    crate::list_values(inputs.require("definition-ids")?, "definition-ids", 16)
}
