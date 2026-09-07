//! `ds design group` — bounded batch editing for eligible project tag definitions.
//!
//! ```text
//!   list → preview → apply | unassign
//!   export
//! ```
//!
//! A group is an ordinary active, single-valued choice definition applicable to
//! LV transformers. The definition id is discovered from project metadata and
//! never interpreted as a business category by the CLI.
//!
//! Three properties a caller must not work around:
//!
//! * **Preview first.** `apply` and `unassign` require the digest `preview`
//!   returned. The server recomputes it, so a batch approved against one state
//!   cannot land against another. `ds` carries the digest; it never mints one.
//! * **A value is matched, not corrected.** Values are checked against the
//!   project's own vocabulary by exact bytes. `Phase 1` is not `phase 1`; read
//!   `allowed` from `ds design group list` rather than guessing a spelling.
//! * **Model evidence is explicit.** A returned model state is reported as-is;
//!   the CLI never infers a second authority from the selected definition id.
//!
//! `export` is the separate read a REPORT needs: the digest-pinned document
//! a report consumer groups by.

pub mod apply;
pub mod export;
pub mod list;
pub mod preview;
pub mod unassign;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Refusal};
use serde_json::Value;

/// ds-brain's own batch bound, held locally so an over-large selection is
/// refused with a remedy instead of by a rejected write.
pub const MAX_GROUP_BATCH: usize = 200;

/// What one group LISTING covers: the batch's number, held for a different
/// reason.
///
/// `ListTagGroups` writes nothing, and ds-brain no longer charges it a write
/// budget: `designTagGroupListingLimit` is 2,000 against the batch's 200,
/// separated there for the same reason it is separated here — borrowing the
/// transaction's write budget as a read bound made a whole-project listing
/// impossible for exactly the projects the feature exists for. A live project
/// already carries 202 transformers.
///
/// This tracks that constant, not the batch. It was equal to the batch while
/// the server bounded both the same way; when the server's listing bound moved
/// this moved with it, and the transaction's write budget did not.
pub const MAX_GROUP_LISTING: usize = 2_000;

/// ds-brain's bound for the report projection, which is a READ and much
/// larger.
///
/// A batch of 200 is one Firestore transaction's write budget. A projection is
/// a whole project's export, and a live project already carries 202
/// transformers — splitting one produces two documents with two digests, which
/// a report pins separately and will not join.
///
/// It is the projection's bound, not every read's: `list` is a read the server
/// still holds at `MAX_GROUP_LISTING`, so widening `list` to this number would
/// trade a local refusal for the same refusal one round trip later.
pub const MAX_PROJECTION_TRANSFORMERS: usize = 2_000;

pub const GROUP_ARG: Arg = Arg {
    name: "group",
    kind: ArgKind::Value,
    value: "<group>",
    required: true,
    default: None,
    choices: &[],
    summary: "Tag definition ID to batch-edit.",
};

pub const TRANSFORMERS_ARG: Arg = Arg {
    name: "transformers",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names (1-200, one transaction's batch). Repeats are reported, not dropped.",
};

/// The same flag on `list`, bounded by `MAX_GROUP_LISTING`: the server's own
/// listing bound, not a transaction budget a read has no writes to spend.
///
/// What the listing owes a caller over it is where the whole-project read
/// actually is, which the summary and `LISTING_TOO_MANY` both name.
pub const LISTING_TRANSFORMERS_ARG: Arg = Arg {
    name: "transformers",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names (1-2000, one listing's bound).",
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

/// The listing's own over-long selection.
///
/// The shared `TOO_MANY` remedy — split the work, or pass fewer values — is a
/// dead end for the caller who asked the question `list` looks like it answers:
/// what does this whole project carry. That read exists, under two other
/// commands, so the refusal names them rather than sending the caller back to
/// count.
pub const LISTING_TOO_MANY: Refusal = Refusal {
    code: "too_many_values",
    when: "--transformers names more than one group listing covers",
    remedy: "list them in parts, or read a whole project with `ds design group export` or `ds design tag query`",
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

/// The same flag on the listing: the server's listing bound, the read's remedy.
///
/// The bound is `MAX_GROUP_LISTING` rather than the batch constant it equals
/// today, so a listing can never inherit a write budget it does not spend —
/// refusing here saves the round trip the server would reject anyway. What
/// changes is the way out: a caller over the bound is pointed at the reads
/// whose unit is a whole project instead of at "pass fewer values".
pub fn listing_transformers(inputs: &ds_cli_contract::Inputs) -> Result<Vec<String>, Failure> {
    bounded_transformers(inputs, MAX_GROUP_LISTING).map_err(|failure| {
        if failure.code() == LISTING_TOO_MANY.code {
            failure
                .remedy(LISTING_TOO_MANY.remedy)
                .next("ds design group export --transformers <names> --output json")
        } else {
            failure
        }
    })
}

fn bounded_transformers(
    inputs: &ds_cli_contract::Inputs,
    max: usize,
) -> Result<Vec<String>, Failure> {
    crate::list_values(inputs.require("transformers")?, "transformers", max)
}

/// Read the authored definition id. Eligibility is discovered and validated
/// by ds-brain from the project's tag metadata.
pub fn group(inputs: &ds_cli_contract::Inputs) -> Result<String, Failure> {
    let value = inputs.require("group")?.trim().to_string();
    if value.is_empty() {
        return Err(
            Failure::invalid("unknown_tag_group", "group must name a tag definition id")
                .remedy("read an id from `ds design group list`")
                .next("ds design tag list --kind lv_transformer --object <name>"),
        );
    }
    Ok(value)
}

/// One plan, rendered the same for preview, apply and unassign.
///
/// `finished` is the SERVER's answer and is printed as it came. Re-deriving it
/// from the outcomes would quietly disagree with the authority that decided it
/// — a batch with model evidence may remain unfinished after tag writes land.
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

#[cfg(test)]
mod bound_tests {
    use super::{MAX_GROUP_BATCH, MAX_GROUP_LISTING};

    /// The two bounds answer to two different server constants and must not be
    /// collapsed back into one.
    ///
    /// `MAX_GROUP_BATCH` tracks ds-brain's `designTagGroupBatchLimit` (200) — one
    /// Firestore transaction's WRITE budget. `MAX_GROUP_LISTING` tracks
    /// `designTagGroupListingLimit` (2,000) — a read that writes nothing.
    ///
    /// They were equal while ds-brain bounded both by the batch, and a stress
    /// session found that made a whole-project listing impossible for a project
    /// already carrying 202 transformers. If a future edit sets them equal again,
    /// that regression is back.
    #[test]
    fn a_listing_is_not_charged_a_transaction_s_write_budget() {
        assert_eq!(MAX_GROUP_BATCH, 200, "ds-brain designTagGroupBatchLimit");
        assert_eq!(
            MAX_GROUP_LISTING, 2_000,
            "ds-brain designTagGroupListingLimit"
        );
        assert!(
            MAX_GROUP_LISTING > MAX_GROUP_BATCH,
            "a read must not be bounded by a write budget"
        );
    }
}
