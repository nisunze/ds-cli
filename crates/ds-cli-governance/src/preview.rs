//! `ds governance architecture preview` — the same validator, writing nothing.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{
    COMMAND_ARG, DESCRIPTOR_ARG, PREVIEW, WRITE_TIMEOUT, command_request, invoke, mismatch,
    read_proposal, require_fields,
};

pub static REFUSALS: &[Refusal] = &[
    ds_cli_desktop::ops::NOT_PAIRED,
    ds_cli_desktop::ops::AMBIGUOUS,
    ds_cli_desktop::ops::UNREACHABLE,
    ds_cli_desktop::ops::PAIRING_REJECTED,
    ds_cli_desktop::ops::REFUSED,
    ds_cli_desktop::ops::UNSUPPORTED,
    ds_cli_desktop::ops::UNREADABLE,
    crate::SIGNED_OUT,
    crate::CONTRACT_MISMATCH,
    crate::NOT_PERMITTED,
    crate::NOT_FOUND,
    crate::CONFLICT,
    crate::VALIDATION_FAILED,
    crate::REVISION_CONFLICT,
    crate::INVALID_PROPOSAL,
    crate::PROPOSAL_UNREADABLE,
];

pub static COMMAND: Command = Command {
    id: "governance.architecture.preview",
    path: &["governance", "architecture", "preview"],
    contract: 1,
    summary: "Validate one proposal against a revision; write nothing.",
    purpose: "\
Runs the exact validator `apply` runs, over the exact command `apply` would \
send, and returns the snapshot that would result — with `applied: false`. \
The revision it is fenced against comes from the proposal's own \
`expected_revision` when it names one, and otherwise from the head read \
immediately before. A proposal with no `idempotency_key` gets one derived \
from its command's content, never a fresh random value, so previewing and \
then applying the same file carries the same key.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[COMMAND_ARG, DESCRIPTOR_ARG],
    output: "\
The resulting `snapshot` and `revision`, the `actor` and `at` of the \
evaluation, the `command_id` receipt, `applied: false` and `preview: true`. \
A refusal instead carries each validation violation on its own line.",
    examples: &[
        Example {
            command: "ds governance architecture preview --command proposal.json --output json",
            note: "A hypothetical request: Link the CLI project-settings command to its ds-brain authority as inferred.",
            runnable: false,
        },
        Example {
            command: "ds governance architecture preview --command refactor.json --output json",
            note: "A hypothetical request: Propose splitting this container as a recommended refactor. A refactor is a recommendation, never a promised feature.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/governance.md"),
    availability: ds_cli_desktop::ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = inputs.value("desktop-descriptor");
    let proposal = read_proposal(inputs.require("command")?)?;
    // The proposal's own fence wins when it has one: previewing against a
    // revision the author never chose would validate a different question
    // from the one they asked.
    let revision = match proposal.expected_revision {
        Some(revision) => revision,
        None => crate::get::head(descriptor)?,
    };
    let body = command_request("preview", revision, &proposal);
    let reply = invoke(descriptor, &PREVIEW, body, WRITE_TIMEOUT)?;
    require_fields(
        &reply,
        PREVIEW.operation,
        &["snapshot", "revision", "command_id", "applied"],
    )?;
    // `applied` is on the wire precisely so no client infers "this was safe"
    // from which action it called.
    if reply["applied"] != Value::Bool(false) {
        return Err(mismatch(
            PREVIEW.operation,
            "a preview reported that it applied",
        ));
    }
    crate::require_receipt(&reply, PREVIEW.operation, &proposal)?;
    Ok(reply)
}

pub fn render(data: &Value) -> String {
    format!(
        "preview   revision {}  command {}\nactor     {}  {}\napplied   false (nothing was written)\n",
        data["revision"].as_u64().unwrap_or(0),
        data["command_id"].as_str().unwrap_or("?"),
        data["actor"].as_str().unwrap_or("?"),
        data["at"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_writes_nothing_and_therefore_needs_no_confirmation() {
        assert_eq!(COMMAND.effect, Effect::ReadOnly);
        assert!(!COMMAND.effect.needs_confirmation());
        assert!(
            COMMAND
                .args
                .iter()
                .all(|arg| arg.name != "expected-revision"),
            "preview takes its fence from the proposal or from the head, never from a flag"
        );
    }

    #[test]
    fn the_proposal_is_required() {
        let command = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "command")
            .expect("command flag");
        assert!(command.required);
    }
}
