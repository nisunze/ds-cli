//! `ds feedback close` — retire one backlog report the session has addressed.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{MAX_RESOLUTION_CHARS, bounded_text};

/// One governed ds-brain transaction, read-modify-write over one document.
const MAX_ID_CHARS: usize = 200;

const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a triage decision on the shared backlog",
    remedy: "re-run with --yes once the resolution states what changed",
};

pub static COMMAND: Command = Command {
    id: "feedback.close",
    path: &["feedback", "close"],
    contract: 2,
    summary: "Mark one backlog report addressed, with the resolution.",
    purpose: "\
Closes the loop a coding session opens: a gap reported through `ds feedback \
submit` and then actually fixed is retired here, with the resolution a reader \
of the `fb` tab will see. This is that tab's own triage mutation — same status \
vocabulary, same optimistic version, same platform capability — so the backlog \
does not keep counting work that is already done. Close only what the session \
can show is addressed; the resolution is the record, not a formality.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "id",
            "<report-id>",
            "The report to close, from `ds feedback list`.",
        )
        .required(),
        Arg::value(
            "resolution",
            "<text>",
            "What changed, or why no action will be taken; at most 1000 characters.",
        )
        .required(),
        Arg::value("status", "<status>", "How the report was addressed.")
            .choices(&["resolved", "wont_fix"])
            .default("resolved"),
        Arg::value(
            "expect-version",
            "<version>",
            "Refuse if the report is no longer at this version; defaults to the version read now.",
        ),
        crate::LANE_ARG,
    ],
    output: "\
`report` in its closed state, with `previous_status` and the new `version`. \
The report stays in the backlog and remains readable with \
`ds feedback list --view addressed`.",
    examples: &[Example {
        command: "ds feedback close --id fb_01J2X --resolution 'ds now exposes feedback.close; verified against the acceptance condition.' --yes --output json",
        note: "Take --id from `ds feedback list`; the close is refused if the report moved meanwhile.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<
        6,
        { 6 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() },
    >([
        ds_cli_contract::args::INVALID_NUMBER,
        crate::INVALID_TEXT,
        crate::NOT_FOUND,
        crate::CONFLICT,
        crate::NOT_PERMITTED,
        CONFIRMATION_REQUIRED,
    ]),
    reference: Some("docs/reference/feedback.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = bounded_text(inputs.require("id")?, "id", MAX_ID_CHARS)?;
    let resolution = bounded_text(
        inputs.require("resolution")?,
        "resolution",
        MAX_RESOLUTION_CHARS,
    )?;
    let status = inputs.require("status")?;
    let mut arguments = Map::from_iter([
        ("id".to_string(), json!(id)),
        ("status".to_string(), json!(status)),
        ("resolution".to_string(), json!(resolution)),
    ]);
    if let Some(version) = inputs.value("expect-version") {
        arguments.insert(
            "expected_version".into(),
            json!(ds_cli_contract::args::integer(
                version,
                "expect-version",
                1,
                i64::MAX
            )?),
        );
    }
    crate::invoke_native(inputs, "close", arguments)
}

pub fn render(data: &Value) -> String {
    let report = &data["report"];
    format!(
        "feedback {}  {} (v{}, was {})\n",
        report["status"].as_str().unwrap_or("closed"),
        report["id"].as_str().unwrap_or("?"),
        report["version"].as_u64().unwrap_or(0),
        data["previous_status"].as_str().unwrap_or("open"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_closed_statuses_are_the_only_choices() {
        let status = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "status")
            .expect("status flag");
        assert_eq!(status.choices, crate::CLOSED_STATUSES);
        assert_eq!(status.default, Some("resolved"));
    }

    #[test]
    fn a_resolution_is_required_and_bounded() {
        let resolution = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "resolution")
            .expect("resolution flag");
        assert!(resolution.required);
        assert!(
            bounded_text(
                &"x".repeat(MAX_RESOLUTION_CHARS),
                "resolution",
                MAX_RESOLUTION_CHARS
            )
            .is_ok()
        );
        assert!(
            bounded_text(
                &"x".repeat(MAX_RESOLUTION_CHARS + 1),
                "resolution",
                MAX_RESOLUTION_CHARS
            )
            .is_err()
        );
    }
}
