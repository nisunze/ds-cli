use super::*;

/// A truncated answer means one of two opposite things, and the line under
/// the rows is the only place a person learns which.
///
/// A watermark read DRAINS: the rows it held back are the rows the next
/// call delivers, so listing again finishes the backlog. Telling that
/// caller to narrow instead would be the expensive mistake — a narrower
/// question is a different question with its own watermark, so it reads
/// from the top again and abandons the drain half-done. `--all` is the
/// read where narrowing IS the remedy, because it always returns the top.
#[test]
fn a_truncated_drain_says_list_again_and_a_truncated_top_read_says_narrow() {
    let page = |drains: bool| {
        json!({
            "view": "not_addressed", "total": 45, "truncated": true, "drains": drains,
            "reports": [{"id":"fb_1","status":"open","severity":"major",
                         "component":"ds-brain","title":"list rescans the backlog"}]
        })
    };
    let drained = render(&page(true));
    assert!(
        drained.contains("list again for the next chunk"),
        "a drain must name the next call, not a narrower question: {drained}"
    );
    assert!(!drained.contains("narrow with"));

    let top = render(&page(false));
    assert!(
        top.contains("narrow with --component") && top.contains("continue with --cursor"),
        "a truncated peek offers its continuation token and narrower filters: {top}"
    );
    assert!(!top.contains("list again"));
}

#[test]
fn an_explicit_cursor_page_requires_the_newly_returned_cursor() {
    for drains in [true, false] {
        let page = json!({
            "view":"not_addressed","total":45,"truncated":true,"drains":drains,
            "cursor_source":"request","cursor":"fb1.new.opaque","reports":[],
        });
        let text = render(&page);
        assert!(
            text.contains("continue with --cursor from this answer"),
            "{text}"
        );
        assert!(
            !text.contains("list again") && !text.contains("narrow with"),
            "{text}"
        );
    }
}

#[test]
fn opaque_cursor_bounds_match_the_native_owner_in_bytes() {
    for size in [741, MAX_CURSOR_BYTES] {
        let token = format!("fb1.{}", "a".repeat(size - 4));
        assert_eq!(bounded_cursor(&token).unwrap(), token);
    }
    assert!(bounded_cursor(&format!("fb1.{}", "a".repeat(MAX_CURSOR_BYTES - 3))).is_err());
    assert!(bounded_cursor(&format!("fb1.{}", "é".repeat(MAX_CURSOR_BYTES / 2))).is_err());
}

/// The cheapest answer has no rows to read, and the row above it carries
/// the blocker a reader would otherwise open the full report to find.
#[test]
fn nothing_changed_prints_one_line_and_a_blocked_row_names_its_blocker() {
    let quiet = render(&json!({"view":"not_addressed","changed":false,"reports":[]}));
    assert_eq!(quiet.lines().count(), 1, "{quiet}");
    assert!(quiet.contains("unchanged since this account last read it"));

    let blocked = render(&json!({
        "view":"not_addressed","total":1,"truncated":false,
        "reports":[{"id":"fb_1","status":"open","blocked":true,
                    "blocked_on":"deploy:ds-brain-canary","severity":"major",
                    "component":"ds-brain","title":"waits on a deploy"}]
    }));
    assert!(blocked.contains("blocked") && blocked.contains("waits on deploy:ds-brain-canary"));
}

/// The cheap answer is the one place a partial read could hide forever.
///
/// A stored report the backlog cannot decode is left out AND passed over by
/// the watermark, so it never appears in a later difference either. An
/// unchanged read still enumerated the store, so it can carry that count —
/// and printing one line and stopping would be exactly the silent partial
/// answer `scan_incomplete` was removed for being, minus the admission.
#[test]
fn a_report_the_backlog_could_not_read_is_named_even_on_the_cheap_answer() {
    let quiet = render(&json!({
        "view":"not_addressed","changed":false,"reports":[],"unreadable":2
    }));
    assert!(
        quiet.contains("unchanged since this account last read it"),
        "{quiet}"
    );
    assert!(
        quiet.contains("2 stored reports could not be read"),
        "an unchanged answer still hides a document nobody can see: {quiet}"
    );

    let clean = render(&json!({"view":"not_addressed","changed":false,"reports":[]}));
    assert_eq!(
        clean.lines().count(),
        1,
        "a read with nothing wrong says nothing about a failure that did not happen: {clean}"
    );
}

/// The `--limit` bound is declared, not discovered: the descriptor carries
/// it, and it is the bound `run` and the native owner enforce.
#[test]
fn the_limit_bound_is_declared_in_the_descriptor() {
    assert_eq!(LIMIT.range(), Some((1, crate::MAX_LIST_LIMIT)));
    let descriptor = ds_cli_contract::help::command_json_unchecked(&COMMAND);
    let limit = descriptor["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["name"] == "limit")
        .expect("--limit is declared");
    assert_eq!(limit["minimum"], 1);
    assert_eq!(limit["maximum"], 50);
    assert_eq!(limit["default"], "20");
}
