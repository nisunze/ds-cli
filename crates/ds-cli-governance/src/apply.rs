//! `ds governance architecture apply` — commit one proposal, revision-fenced.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{
    APPLY, COMMAND_ARG, DESCRIPTOR_ARG, MIN_REVISION, REVISION_MISMATCH, WRITE_TIMEOUT,
    command_request, integer, invoke, read_proposal, require_fields,
};

const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a write to the governed architecture graph",
    remedy: "preview the proposal, then re-run with --yes once you intend that exact revision",
};

static REFUSALS: &[Refusal] = &[
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
    REVISION_MISMATCH,
    crate::INVALID_NUMBER,
    CONFIRMATION_REQUIRED,
];

pub static COMMAND: Command = Command {
    id: "governance.architecture.apply",
    path: &["governance", "architecture", "apply"],
    contract: 1,
    summary: "Commit one previewed proposal, fenced to an exact revision.",
    purpose: "\
Applies one bounded command to the governed graph as a new immutable \
revision. --expected-revision is required and is the fence: if the head has \
moved past it the write is refused with the head it moved to, and `ds` never \
retries against that head — silently rebasing an edit onto work its author \
never saw is what the fence exists to prevent. The idempotency key makes a \
retry safe: a replay of work already committed answers `applied: false`, \
which is success, not failure.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        COMMAND_ARG,
        Arg::value(
            "expected-revision",
            "<revision>",
            "The revision this change was planned against; refuse if the head moved.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "\
The committed `snapshot` and its new `revision`, the `actor` and `at` of the \
commit, the `command_id` receipt, and `applied` — true for a commit, false \
for an idempotent replay of work already committed under the same key.",
    examples: &[
        Example {
            command: "ds governance architecture apply --command proposal.json --expected-revision 12 --yes --output json",
            note: "A hypothetical request: Mark the transformer selection-set work implemented with exact evidence.",
            runnable: false,
        },
        Example {
            command: "ds governance architecture apply --command question.json --expected-revision 12 --yes --output json",
            note: "A hypothetical request: Record this form-factory question in the Survey lifecycle chapter. A question is not a delivery claim.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/governance.md"),
    availability: ds_cli_desktop::ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The flag is validated before the file is opened, so a mistyped fence is
    // refused without reading anything.
    let revision = integer(
        inputs.require("expected-revision")?,
        "expected-revision",
        MIN_REVISION,
        i64::MAX,
    )?;
    let proposal = read_proposal(inputs.require("command")?)?;
    // Two fences that disagree are not a preference to resolve. One of them
    // is wrong, and applying under either would commit against a revision
    // somebody did not choose.
    if let Some(authored) = proposal.expected_revision
        && authored != revision
    {
        return Err(Failure::invalid(
            "proposal_revision_mismatch",
            format!(
                "--expected-revision {revision} disagrees with the proposal's own expected_revision {authored}"
            ),
        )
        .remedy(REVISION_MISMATCH.remedy)
        .detail(json!({ "flag": revision, "proposal": authored }))
        .next("ds governance architecture preview --command <proposal.json> --output json"));
    }

    let body = command_request("apply", revision, &proposal);
    let reply = invoke(
        inputs.value("desktop-descriptor"),
        &APPLY,
        body,
        WRITE_TIMEOUT,
    )?;
    require_fields(
        &reply,
        APPLY.operation,
        &["snapshot", "revision", "command_id", "applied"],
    )?;
    crate::require_receipt(&reply, APPLY.operation, &proposal)?;
    Ok(reply)
}

/// An idempotent replay is a success, and says so.
///
/// `applied: false` means the server already holds this exact command under
/// this key — the retry after a dropped connection did the right thing. A
/// caller told that as an error would re-plan work that is already committed.
pub fn render(data: &Value) -> String {
    let applied = data["applied"].as_bool().unwrap_or(false);
    format!(
        "{}  revision {}  command {}\nactor     {}  {}\n",
        if applied { "applied  " } else { "idempotent" },
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
    fn a_write_is_confirmed_and_fenced() {
        assert_eq!(COMMAND.effect, Effect::GlobalWrite);
        assert!(
            COMMAND.effect.needs_confirmation(),
            "dispatch refuses this without --yes before the handler runs"
        );
        let fence = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "expected-revision")
            .expect("expected-revision flag");
        assert!(fence.required, "an unfenced apply is not offered at all");
        assert!(
            REFUSALS.iter().any(|r| r.code == "confirmation_required"),
            "the confirmation gate is documented where a caller reads it"
        );
    }

    #[test]
    fn an_idempotent_replay_reads_as_success_not_as_a_failure() {
        let replay = render(&serde_json::json!({
            "applied": false, "revision": 12, "command_id": "c1",
            "actor": "ops@example.test", "at": "2026-08-29T09:00:00Z",
        }));
        assert!(replay.starts_with("idempotent"), "{replay}");
        let committed = render(&serde_json::json!({
            "applied": true, "revision": 13, "command_id": "c1",
            "actor": "ops@example.test", "at": "2026-08-29T09:00:00Z",
        }));
        assert!(committed.starts_with("applied"), "{committed}");
        assert!(committed.contains("revision 13"), "{committed}");
    }
}
