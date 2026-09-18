//! `ds feedback note` — say what an open report is waiting on, without closing it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{MAX_BLOCKED_ON_CHARS, MAX_NOTE_CHARS, bounded_text};

const MAX_ID_CHARS: usize = 200;

pub static COMMAND: Command = Command {
    id: "feedback.note",
    path: &["feedback", "note"],
    contract: 2,
    summary: "Record what an open report is waiting on, without closing it.",
    purpose: "\
Leaves one later observation on a report this session touched but could not \
close — the deploy, the ruling, the terraform apply or the other report it \
waits on. Without it the only way to say anything about a report was to close \
it, so a known blocker left no trace and the next reader re-read the whole \
report to rediscover it. A named blocker shows on every listing row from then \
on. The note never edits the submitter's own sighting: that stays exactly as \
filed, and this is a later observation about it. Use `--blocked-on` when the \
report is parked on something specific, and `--unblock` when that condition \
has been met and verification is what remains.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    // An outsider looking for this reaches for the words below, not "note".
    search: &[
        "blocked",
        "blocker",
        "dependency",
        "annotate",
        "comment",
        "parked",
    ],
    // Nothing here needs a window: the backlog is a governed read/write.
    requires: Requires::Server,
    args: &[
        Arg::value(
            "id",
            "<report-id>",
            "The open report to annotate, from `ds feedback list`.",
        )
        .required(),
        Arg::value(
            "text",
            "<text>",
            "What is now known about this report; at most 1000 characters.",
        )
        .required(),
        Arg::value(
            "blocked-on",
            "<text>",
            "Name what the report waits on; marks it blocked on every later row.",
        ),
        Arg::switch("unblock", "Clear a previously named blocker."),
        Arg::value(
            "expect-version",
            "<version>",
            "Refuse if the report is no longer at this version.",
        ),
        crate::LANE_ARG,
    ],
    output: "\
`report` with its unchanged status and its new `version`, plus `blocked`, \
`blocked_on` and `note_count`. The report stays open and stays in \
`ds feedback list`, now carrying its reason.",
    examples: &[Example {
        command: "ds feedback note --id fb_01J2X --text 'Fix merged on run; waits on the ds-brain canary deploy.' --blocked-on 'deploy:ds-brain-canary' --yes --output json",
        note: "Leave this instead of leaving a touched report silent; silence is what forces the next reader to rescan.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<
        7,
        { 7 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() },
    >([
        ds_cli_contract::args::INVALID_NUMBER,
        crate::INVALID_TEXT,
        crate::NOT_FOUND,
        crate::CONFLICT,
        crate::NOT_PERMITTED,
        crate::SETTLED,
        crate::NOTE_LIMIT,
    ]),
    reference: Some("docs/reference/feedback.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = bounded_text(inputs.require("id")?, "id", MAX_ID_CHARS)?;
    let text = bounded_text(inputs.require("text")?, "text", MAX_NOTE_CHARS)?;
    let mut arguments = Map::from_iter([
        ("id".to_string(), json!(id)),
        ("text".to_string(), json!(text)),
    ]);
    if let Some(blocked_on) = inputs.value("blocked-on") {
        arguments.insert(
            "blocked_on".into(),
            json!(bounded_text(
                blocked_on,
                "blocked-on",
                MAX_BLOCKED_ON_CHARS
            )?),
        );
    }
    if inputs.switch("unblock") {
        arguments.insert("unblock".into(), json!(true));
    }
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
    crate::invoke_native(inputs, "note", arguments)
}

pub fn render(data: &Value) -> String {
    let report = &data["report"];
    let state = if data["blocked"].as_bool().unwrap_or(false) {
        format!(
            "blocked on {}",
            data["blocked_on"].as_str().unwrap_or("something unnamed")
        )
    } else {
        report["status"].as_str().unwrap_or("open").to_string()
    };
    format!(
        "feedback noted  {} (v{}, {}, {} note(s))\n",
        report["id"].as_str().unwrap_or("?"),
        report["version"].as_u64().unwrap_or(0),
        state,
        data["note_count"].as_u64().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_bounded_and_never_both_raises_and_clears_a_blocker() {
        assert!(bounded_text(&"x".repeat(MAX_NOTE_CHARS), "text", MAX_NOTE_CHARS).is_ok());
        assert!(bounded_text(&"x".repeat(MAX_NOTE_CHARS + 1), "text", MAX_NOTE_CHARS).is_err());
        // The owner of that exclusion is the kernel command, which is where the
        // wire contract lives; this pins that the flags reaching it are the two
        // it refuses together.
        let names: Vec<&str> = COMMAND.args.iter().map(|arg| arg.name).collect();
        assert!(names.contains(&"blocked-on") && names.contains(&"unblock"));
        assert!(
            ds_cli_auth::FeedbackCommand::Note {
                id: "f1".into(),
                text: "both".into(),
                blocked_on: Some("x".into()),
                unblock: true,
                expected_version: None,
            }
            .validate()
            .is_err()
        );
    }
}
