use super::*;

#[test]
fn an_empty_journal_says_why_there_is_no_draft() {
    assert_eq!(
        render(&json!({ "reporting": "off", "journal_entries": 0, "drafts": [] })),
        "no draft: DS_SRE_REPORTING=off keeps the journal empty\n"
    );
    assert!(
        render(&json!({ "reporting": "on", "journal_entries": 3, "drafts": [] }))
            .contains("none of the 3 journalled invocation(s)")
    );
}

#[test]
fn a_draft_renders_as_one_submit_command_left_to_finish() {
    let rendered = render(&json!({
        "reporting": "on",
        "journal_entries": 1,
        "drafts": [{
            "title": "ds feedback.submit refused: confirmation_required",
            "component": "ds-cli/feedback",
            "kind": "friction",
            "severity": "minor",
            "evidence": ["release ds 0.1.6 (kernel abc1234), lane local"],
            "context": { "sre_event": "inv_1" },
            "detail": "Expected: <what you expected>.",
        }]
    }));
    assert!(rendered.starts_with(
        "ds feedback submit --title 'ds feedback.submit refused: confirmation_required' \
         --component ds-cli/feedback --kind friction --severity minor"
    ));
    assert!(rendered.contains("--context sre_event=inv_1"));
    assert!(rendered.contains("--agent <your name>"));
    assert!(rendered.ends_with("fill Expected and Acceptance in --detail before submitting\n"));
}

/// The draft `--replay-last` would attach is the one whose rendered command
/// carries it; another draft, or one without a replay, does not.
#[test]
fn the_draft_whose_replay_submit_attaches_renders_with_replay_last() {
    let draft = |event: &str| {
        json!({
            "title": format!("ds feedback.list crashed: panic {event}"),
            "component": "ds-cli/feedback", "kind": "bug", "severity": "major",
            "evidence": [], "context": { "sre_event": event },
            "detail": "Expected: <what you expected>.",
            "replay": { "command": "feedback.list" },
        })
    };
    let rendered = render(&json!({
        "reporting": "on", "journal_entries": 2,
        "drafts": [draft("inv_new"), draft("inv_old")],
        "replay_last": "inv_new",
    }));
    let lines: Vec<&str> = rendered.lines().collect();
    assert!(lines[0].contains(" --replay-last --agent"), "{rendered}");
    assert!(!lines[1].contains("--replay-last"), "{rendered}");
    let none = render(&json!({
        "reporting": "on", "journal_entries": 1, "drafts": [draft("inv_new")], "replay_last": null,
    }));
    assert!(!none.contains("--replay-last"), "{none}");
}
