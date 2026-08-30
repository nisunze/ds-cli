//! `ds governance architecture history` — who changed what, bounded and paged.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{
    DEFAULT_HISTORY_LIMIT, DESCRIPTOR_ARG, HISTORY, INVALID_NUMBER, INVALID_TEXT, READ_TIMEOUT,
    history_request, invoke, require_fields,
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
    INVALID_NUMBER,
    INVALID_TEXT,
];

pub static COMMAND: Command = Command {
    id: "governance.architecture.history",
    path: &["governance", "architecture", "history"],
    contract: 1,
    summary: "Bounded, cursor-paged history of architecture revisions.",
    purpose: "\
Every edit creates an immutable revision; nothing overwrites the head in \
place. This walks those revisions newest first — who made each one, when, and \
which command it applied to which target — so a caller can see what moved \
under a plan before deciding whether that plan is still true. History is \
bounded: pass the `next_cursor` from a page to continue, and its absence \
means the history is exhausted.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "limit",
            "<count>",
            "Revisions per page (1-100). The page reports whether more exist.",
        )
        .default(DEFAULT_HISTORY_LIMIT),
        Arg::value(
            "cursor",
            "<cursor>",
            "Continue from the `next_cursor` a previous page returned.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "\
`revisions` newest first, each with `revision`, `actor`, `at`, `command_id`, \
`kind` and `target_id`; plus `next_cursor` when another page exists. No \
`next_cursor` means this is the end of the history, not an error.",
    examples: &[Example {
        command: "ds governance architecture history --limit 20 --output json",
        note: "Pass `.data.next_cursor` back as --cursor for the next page; its absence means the end.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/governance.md"),
    availability: ds_cli_desktop::ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let body = history_request(inputs.value("limit"), inputs.value("cursor"))?;
    let reply = invoke(
        inputs.value("desktop-descriptor"),
        &HISTORY,
        body,
        READ_TIMEOUT,
    )?;
    require_fields(&reply, HISTORY.operation, &["revisions"])?;
    Ok(reply)
}

pub fn render(data: &Value) -> String {
    let revisions = data["revisions"].as_array().map_or(&[][..], Vec::as_slice);
    let mut out = format!(
        "{}\n",
        ds_cli_desktop::ops::plural(revisions.len() as u64, "revision")
    );
    for entry in revisions {
        out.push_str(&format!(
            "  {:>7}  {:<24} {:<18} {:<24} {}\n",
            entry["revision"].as_u64().unwrap_or(0),
            entry["at"].as_str().unwrap_or("?"),
            entry["kind"].as_str().unwrap_or("?"),
            entry["target_id"].as_str().unwrap_or("—"),
            entry["actor"].as_str().unwrap_or("?"),
        ));
    }
    // Absence is information: it says the history ended here.
    match data["next_cursor"].as_str() {
        Some(cursor) if !cursor.is_empty() => {
            out.push_str(&format!("  more: --cursor {cursor}\n"));
        }
        _ => out.push_str("  end of history\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_HISTORY_LIMIT;
    use serde_json::json;

    #[test]
    fn the_limit_states_its_bound_and_its_default() {
        let limit = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "limit")
            .expect("limit flag");
        assert_eq!(limit.default, Some("20"));
        assert!(limit.summary.contains("1-100"));
        assert_eq!(MAX_HISTORY_LIMIT, 100);
    }

    #[test]
    fn an_absent_next_cursor_reads_as_the_end_not_as_a_gap() {
        let page = render(&json!({
            "revisions": [{
                "revision": 12, "actor": "ops@example.test", "at": "2026-08-29T09:00:00Z",
                "command_id": "c1", "kind": "update_node", "target_id": "survey-form-factory",
            }],
        }));
        assert!(page.contains("end of history"), "{page}");
        assert!(page.contains("update_node"), "{page}");

        let more = render(&json!({ "revisions": [], "next_cursor": "c-9" }));
        assert!(more.contains("--cursor c-9"), "{more}");
        assert!(!more.contains("end of history"), "{more}");
    }
}
