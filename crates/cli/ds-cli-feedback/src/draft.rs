//! Turn the last failed, crashed or refused `ds` invocations into ready
//! `feedback submit` payloads.
//!
//! `ds_client_core::reporter` keeps the invocation journal and builds the
//! draft (ds-command-kernel `docs/contracts/ds-cli-reliability-and-feedback.md`
//! §4); this command only reads that journal from the reporter's state on this
//! machine and hands the drafts back. It reaches no network and no account.

use ds_cli_auth::reliability::reporter;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::JOURNAL_UNAVAILABLE;

const INVALID_COUNT: Refusal = Refusal {
    code: "invalid_count",
    when: "--n is not a whole number from 1 to 10",
    remedy: "pass --n between 1 and 10, or drop it for the newest one",
};

pub static COMMAND: Command = Command {
    id: "feedback.draft",
    path: &["feedback", "draft"],
    contract: 1,
    summary: "Draft feedback from the last failed, crashed or refused ds command.",
    purpose: "Draft feedback from failed, crashed or refused local commands. Fill Expected and Acceptance, then submit with --replay-last. Reads only the local journal; no account or network.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::switch(
            "last",
            "Draft from the newest journalled failures and refusals.",
        )
        .required(),
        Arg::value("n", "<count>", "How many of the newest to draft, 1 to 10.").default("1"),
    ],
    output: "\
`drafts`, newest first: each a `feedback submit` payload with `title`, \
`component`, `kind`, `severity`, `evidence`, `context` (`sre_event`, \
`host_kind`, `lane`, `platform`), a `detail` whose Expected and Acceptance \
placeholders the caller fills, `agent` when known, `replay` when journalled \
(`version`, `source_sha`, `channel`, `os`, `arch`, `command`, `argv`, \
`redacted`, `elided`, `panic_location`), and `needs` naming what is left to \
add. `replay_last` is the `sre_event` of the draft whose replay `feedback \
submit --replay-last` attaches. `journal_entries` counts the journalled \
invocations; `reporting` is `off` when DS_SRE_REPORTING=off kept the journal \
empty.",
    examples: &[Example {
        command: "ds feedback draft --last --output json",
        note: "An empty `drafts` means no journalled invocation failed or was refused.",
        runnable: true,
    }],
    refusals: &[JOURNAL_UNAVAILABLE, INVALID_COUNT],
    reference: Some("docs/reference/feedback.md"),
    search: &["report failure", "last error", "incident"],
    requires: Requires::Server,
    availability: always,
};

fn always() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let count = inputs
        .value("n")
        .unwrap_or("1")
        .parse::<usize>()
        .ok()
        .filter(|count| (1..=reporter::MAX_DRAFTS).contains(count))
        .ok_or_else(|| {
            Failure::invalid(
                INVALID_COUNT.code,
                "`--n` must be a whole number from 1 to 10",
            )
            .remedy(INVALID_COUNT.remedy)
        })?;
    let (enabled, entries) = crate::journal()?;
    // The draft whose replay `feedback submit --replay-last` attaches, by
    // its SRE event id: the same failure, so the rendered command carries it.
    let replay_last = reporter::last_failure(&entries)
        .filter(|entry| entry.replay.is_some())
        .map(|entry| entry.invocation_id.clone());
    Ok(json!({
        "reporting": if enabled { "on" } else { "off" },
        "journal_entries": entries.len(),
        "drafts": reporter::draft(&entries, count),
        "replay_last": replay_last,
    }))
}

pub fn render(data: &Value) -> String {
    let drafts = data["drafts"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if drafts.is_empty() {
        return if data["reporting"] == "off" {
            "no draft: DS_SRE_REPORTING=off keeps the journal empty\n".to_string()
        } else {
            format!(
                "no draft: none of the {} journalled invocation(s) failed or was refused\n",
                data["journal_entries"].as_u64().unwrap_or(0)
            )
        };
    }
    let mut out = String::new();
    for draft in drafts {
        let mut line = format!(
            "ds feedback submit --title '{}' --component {} --kind {} --severity {}",
            draft["title"].as_str().unwrap_or(""),
            draft["component"].as_str().unwrap_or(""),
            draft["kind"].as_str().unwrap_or(""),
            draft["severity"].as_str().unwrap_or(""),
        );
        for evidence in draft["evidence"].as_array().into_iter().flatten() {
            line.push_str(&format!(
                " --evidence '{}'",
                evidence.as_str().unwrap_or("")
            ));
        }
        if let Some(context) = draft["context"].as_object() {
            for (key, value) in context {
                line.push_str(&format!(
                    " --context {key}={}",
                    value.as_str().unwrap_or("")
                ));
            }
        }
        if data["replay_last"].is_string() && draft["context"]["sre_event"] == data["replay_last"] {
            line.push_str(" --replay-last");
        }
        line.push_str(&format!(
            " --agent {} --detail '{}' --yes\n",
            draft["agent"].as_str().unwrap_or("<your name>"),
            draft["detail"].as_str().unwrap_or(""),
        ));
        out.push_str(&line);
    }
    out.push_str("fill Expected and Acceptance in --detail before submitting\n");
    out
}

#[cfg(test)]
#[path = "draft_tests.rs"]
mod tests;
