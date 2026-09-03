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
