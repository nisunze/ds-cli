//! `ds design selection assign` — scope one project work task to a selection.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{DesignSelectionAnswer, DesignSelectionPromotion, DesignSelectionRequest};
use serde_json::{Value, json};

use super::LANE;
use crate::selection::read::SELECTION_ARG;

const TITLE_ARG: Arg = Arg {
    name: "title",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "What the work is, e.g. \"Nixon: review these transformers\".",
};

const OWNER_ARG: Arg = Arg {
    name: "owner",
    kind: ArgKind::Value,
    value: "<email>",
    required: false,
    default: None,
    choices: &[],
    summary: "The person responsible. Omit to leave the task unassigned.",
};

const PURPOSE_ARG: Arg = Arg {
    name: "purpose",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Why the work was assigned. Recorded on the receipt.",
};

const ASSIGNMENT_ARG: Arg = Arg::value(
    "assignment",
    "<assignment-id>",
    "The receipt id to assign under. Omit and one is minted from the title.",
);

pub static COMMAND: Command = Command {
    id: "design.selection.assign",
    path: &["design", "selection", "assign"],
    contract: 1,
    summary: "Create one project work task scoped to a saved selection.",
    purpose: "\
Creates an ordinary Project Work task carrying a link to the selection, and \
writes an immutable receipt pinning the selection's version, its member digest \
and the exact transformer ids that resolved at that moment. The task never \
holds a copy of the transformer data, and a later edit of the selection cannot \
change what was assigned. The application re-evaluates membership first and \
refuses if it moved since the read — a promotion assigns a set somebody \
approved, or it does not happen. Members that no longer resolve are reported on \
the receipt rather than silently included.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        SELECTION_ARG,
        TITLE_ARG,
        OWNER_ARG,
        PURPOSE_ARG,
        ASSIGNMENT_ARG,
        LANE,
    ],
    output: "\
The project, the `selection`, the minted `assignment` and `task` ids, the \
pinned `memberDigest`, the assigned `members`, any `missing` members the \
selection could not resolve, and the `committedRevision` the plan moved to.",
    examples: &[Example {
        command: "ds design selection assign --selection sel-week-32 --title \"Review LV designs\" --owner nixon@example.com --yes",
        note: "Read .data.memberDigest on the receipt to see exactly what was assigned.",
        runnable: false,
    }],
    refusals: super::REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let selection = inputs.require("selection")?;
    let title = inputs.require("title")?;
    // Membership is evaluated first, and the digest that read returned is what
    // travels: echoed, never derived. A selection that moved in between is
    // refused by ds-brain rather than quietly assigning a different set.
    let (_, read) = super::read_selection(lane, selection)?;
    let assignment_id = match inputs.value("assignment") {
        Some(pinned) => pinned.to_owned(),
        None => super::mint_id("assign", title),
    };
    let (project, answer) = super::ask(
        lane,
        selection,
        &DesignSelectionRequest::Promote(DesignSelectionPromotion {
            selection_id: selection.to_owned(),
            assignment_id,
            expected_version: read.selection.version,
            expected_member_digest: read.member_digest,
            // Project Work requires at least 8 characters; this is the fence
            // for the command, not a name anybody reads.
            command_id: super::mint_id("dscmd", ""),
            title: title.to_owned(),
            purpose: inputs.value("purpose").map(str::to_owned),
            responsible_email: inputs.value("owner").map(str::to_owned),
            review_required: true,
        }),
    )?;
    let DesignSelectionAnswer::Assigned(receipt) = answer else {
        return Err(Failure::unavailable(
            "auth_response_unreadable",
            "the promotion receipt did not match its closed contract",
        ));
    };
    Ok(json!({
        "project": project,
        "selection": selection,
        "assignment": receipt.assignment_id,
        "task": receipt.task_id,
        "memberDigest": receipt.member_digest,
        "members": receipt.member_ids,
        "missing": receipt.missing_member_ids,
        "committedRevision": receipt.committed_revision,
    }))
}

pub fn render(data: &Value) -> String {
    let members = data["members"].as_array().map_or(0, Vec::len) as u64;
    let mut out = format!(
        "assigned {} as {} in {} · revision {}\n",
        crate::plural(members, "transformer"),
        data["task"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
    );
    let missing = data["missing"].as_array().map_or(0, Vec::len) as u64;
    if missing > 0 {
        out.push_str(&format!(
            "  ! {} could not be resolved and was not assigned\n",
            crate::plural(missing, "member"),
        ));
    }
    if let Some(digest) = data["memberDigest"].as_str() {
        out.push_str(&format!(
            "  pinned digest {}\n",
            &digest[..digest.len().min(16)]
        ));
    }
    out
}
