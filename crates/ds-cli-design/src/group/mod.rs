//! `ds design group` — the two governed tag groups, `city` and `phasing`.
//!
//! ```text
//!   list → preview → apply | unassign
//!   export
//! ```
//!
//! A group is an ORDINARY tag definition held to a fixed shape (single-valued,
//! LV transformers only). What the two reserved names buy is a BATCH: assigning
//! a city to sixty transformers is one operation with one outcome per entry,
//! not sixty `tag set` calls. The generic `ds design tag set` refuses a
//! governed group and points here, so a group has exactly one write door.
//!
//! Three properties a caller must not work around:
//!
//! * **Preview first.** `apply` and `unassign` require the digest `preview`
//!   returned. The server recomputes it, so a batch approved against one state
//!   cannot land against another. `ds` carries the digest; it never mints one.
//! * **A value is matched, not corrected.** Values are checked against the
//!   project's own vocabulary by exact bytes. `Phase 1` is not `phase 1`; read
//!   `allowed` from `ds design group list` rather than guessing a spelling.
//! * **`phasing` needs two authorities.** Its canonical home is the DS Grid
//!   model's alignment. `ds` holds no model session, so it reports no receipt
//!   and a phasing batch comes back `partial` with the transformers still
//!   outstanding. That is the honest state, not a degraded one — nobody has
//!   written the model. Finish those in the application.
//!
//! `export` is the separate read a REPORT needs: the digest-pinned document
//! ds-network-reporter groups a per-city sheet by.

pub mod apply;
pub mod export;
pub mod list;
pub mod preview;
pub mod unassign;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Refusal};
use serde_json::Value;

/// The closed set of governed groups. Declared here so a caller learns it from
/// the descriptor rather than from a refusal.
pub const GROUPS: &[&str] = &["city", "phasing"];

/// ds-brain's own batch bound, held locally so an over-large selection is
/// refused with a remedy instead of by a rejected write.
pub const MAX_GROUP_BATCH: usize = 200;

/// ds-brain's bound for the report projection, which is a READ and much
/// larger.
///
/// A batch of 200 is one Firestore transaction's write budget. A projection is
/// a whole project's export, and a live project already carries 202
/// transformers — splitting one produces two documents with two digests, which
/// a report pins separately and will not join.
pub const MAX_PROJECTION_TRANSFORMERS: usize = 2_000;

pub const GROUP_ARG: Arg = Arg {
    name: "group",
    kind: ArgKind::Value,
    value: "<group>",
    required: true,
    default: None,
    choices: GROUPS,
    summary: "Which governed group to act on.",
};

pub const TRANSFORMERS_ARG: Arg = Arg {
    name: "transformers",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names (1-200). Repeats are reported, not dropped.",
};

/// The same flag on `export`, which is a read over a whole project rather than
/// one transaction's worth of writes.
pub const PROJECTION_TRANSFORMERS_ARG: Arg = Arg {
    name: "transformers",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names the export covers (1-2000).",
};

pub const PROJECTION_DEFINITION_IDS_ARG: Arg = Arg {
    name: "definition-ids",
    kind: ArgKind::Value,
    value: "<ids>",
    required: false,
    default: None,
    choices: &[],
    summary: "Ordered comma-separated typed tag definition IDs used for grouping; omit for one untagged group.",
};

pub fn projection_definition_ids(inputs: &ds_cli_contract::Inputs) -> Result<Vec<String>, Failure> {
    match inputs.value("definition-ids") {
        Some(value) => crate::list_values(value, "definition-ids", 16),
        None => Ok(Vec::new()),
    }
}

pub const DIGEST_ARG: Arg = Arg {
    name: "digest",
    kind: ArgKind::Value,
    value: "<plan-digest>",
    required: true,
    default: None,
    choices: &[],
    summary: "The digest `ds design group preview` returned for this exact plan.",
};

/// A plan whose digest no longer describes the project.
pub const PLAN_STALE: Refusal = Refusal {
    code: "design_plan_stale",
    when: "the project moved after the plan was previewed, so the digest no longer holds",
    remedy: "run `ds design group preview` again and apply the digest it returns",
};

/// Split and bound the transformer flag for a batch.
pub fn transformers(inputs: &ds_cli_contract::Inputs) -> Result<Vec<String>, Failure> {
    bounded_transformers(inputs, MAX_GROUP_BATCH)
}

/// The same flag under the projection's larger read bound.
pub fn projection_transformers(inputs: &ds_cli_contract::Inputs) -> Result<Vec<String>, Failure> {
    bounded_transformers(inputs, MAX_PROJECTION_TRANSFORMERS)
}

fn bounded_transformers(
    inputs: &ds_cli_contract::Inputs,
    max: usize,
) -> Result<Vec<String>, Failure> {
    crate::list_values(inputs.require("transformers")?, "transformers", max)
}

/// Read the group flag against the closed set.
///
/// Refused locally because the set is closed and declared: a third name is not
/// a group this platform has, and a round trip would only say so more slowly.
pub fn group(inputs: &ds_cli_contract::Inputs) -> Result<String, Failure> {
    let value = inputs.require("group")?.trim().to_string();
    if !GROUPS.contains(&value.as_str()) {
        return Err(Failure::invalid(
            "unknown_tag_group",
            "only `city` and `phasing` are governed groups",
        )
        .remedy("pass --group city or --group phasing")
        .next("ds design tag list --kind lv_transformer --object <name>"));
    }
    Ok(value)
}

/// One plan, rendered the same for preview, apply and unassign.
///
/// `finished` is the SERVER's answer and is printed as it came. Re-deriving it
/// from the outcomes would quietly disagree with the authority that decided it
/// — a phasing batch whose tags all landed is still not finished.
pub fn render_plan(data: &Value) -> String {
    let mut out = format!(
        "{} {} · {} · {} changing, {} unchanged, {} refused\n",
        data["group"].as_str().unwrap_or("?"),
        data["operation"].as_str().unwrap_or("?"),
        data["state"].as_str().unwrap_or("?"),
        data["changed"].as_u64().unwrap_or(0),
        data["unchanged"].as_u64().unwrap_or(0),
        data["refused"].as_u64().unwrap_or(0),
    );
    if let Some(digest) = data["digest"].as_str() {
        out.push_str(&format!("  plan {digest}\n"));
    }
    let outstanding: Vec<&str> = data["outstanding"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !outstanding.is_empty() {
        out.push_str(&format!(
            "  awaiting the DS Grid model: {}\n",
            outstanding.join(", ")
        ));
    }
    for outcome in data["outcomes"].as_array().into_iter().flatten() {
        let action = outcome["action"].as_str().unwrap_or("?");
        let mut line = format!(
            "  {} {}",
            outcome["transformer"].as_str().unwrap_or("?"),
            action
        );
        if let Some(to) = outcome["to"].as_str() {
            line.push_str(&format!(" → {to}"));
        }
        if let Some(reason) = outcome["reason"].as_str() {
            line.push_str(&format!(" ({reason})"));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}
