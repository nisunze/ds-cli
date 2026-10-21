//! `ds install list --search` is a question about the whole inventory.
//!
//! ds-brain's list route pages and takes no filter, so the filter runs in the
//! CLI. Running it over the first page alone made every installation past
//! that page read as absent — `matched: 0, truncated: false` for a machine
//! that demonstrably exists — which is the confident-empty answer wearing a
//! totals block.

use std::cell::RefCell;
use std::rc::Rc;

use ds_cli_contract::outcome::Failure;
use ds_cli_installs::list::{searched, whole_inventory};
use ds_command_kernel::installation_inventory::MAX_ROWS;
use serde_json::{Value, json};

/// The cursors a walk asked for, in order.
type Asked = Rc<RefCell<Vec<Option<String>>>>;

/// One record as ds-brain writes it, at its real size: the byte bound below
/// is only meaningful against rows that weigh what rows weigh.
fn record(install_id: &str) -> Value {
    json!({
        "install_id": install_id,
        "platform": "linux",
        "app_version": "0.1.3",
        "os_version": "Ubuntu 26.04 LTS (7.0.0-31-generic)",
        "webview_version": "WebKitGTK 2.48.1",
        "lane": "stable",
        "host_kind": "desktop",
        "account_uid": "u_0123456789abcdef0123456789abcdef",
        "account_email": "someone.with.a.long.address@example.test",
        "first_seen_at": "2026-01-01T00:00:00Z",
        "last_seen_at": "2026-09-18T00:00:00Z",
        "status": "active",
        "license_status": "active",
        "lease_expires_at": "2026-09-25T00:00:00Z",
        "policy_revision": 0,
        "retired": false,
        "retired_at": "",
        "retired_by": "",
        "reason": "",
    })
}

/// A two-page inventory: the machine an operator is looking for is on the
/// SECOND page.
fn two_pages() -> (impl FnMut(Option<String>) -> Result<Value, Failure>, Asked) {
    let asked: Asked = Rc::new(RefCell::new(Vec::new()));
    let log = asked.clone();
    let fetch = move |cursor: Option<String>| {
        log.borrow_mut().push(cursor.clone());
        Ok(match cursor.as_deref() {
            None => json!({
                "installs": [record("0cd65a76-a5be-440c-b82e-c3442b8c1e40"), record("1e2f3a4b-0000-4000-8000-000000000001")],
                "next_cursor": "1e2f3a4b-0000-4000-8000-000000000001",
            }),
            Some("1e2f3a4b-0000-4000-8000-000000000001") => json!({
                "installs": [record("9f9f9f9f-2222-4222-8222-222222222222")],
            }),
            Some(other) => panic!("no page starts after {other}"),
        })
    };
    (fetch, asked)
}

#[test]
fn a_search_finds_an_installation_on_the_second_page() {
    let (fetch, asked) = two_pages();
    let page = whole_inventory(None, fetch).expect("every page reads");
    assert_eq!(
        asked.borrow().as_slice(),
        &[
            None,
            Some("1e2f3a4b-0000-4000-8000-000000000001".to_owned())
        ],
        "the walk follows the cursor to the end"
    );
    assert_eq!(page["installs"].as_array().map(Vec::len), Some(3));
    assert!(page["next_cursor"].is_null(), "{page}");

    let projected = searched(page, "9f9f9f9f", false, 50).expect("projects");
    assert_eq!(projected["totals"]["matched"], 1, "{projected}");
    assert_eq!(projected["totals"]["shown"], 1, "{projected}");
    assert_eq!(projected["totals"]["received"], 3, "{projected}");
    assert_eq!(projected["totals"]["truncated"], false, "{projected}");
    assert_eq!(
        projected["groups"][0]["rows"][0]["install_id"],
        "9f9f9f9f-2222-4222-8222-222222222222"
    );
}

#[test]
fn a_walk_that_stops_short_never_reports_truncated_false() {
    // An inventory that never ends: every page carries a fresh cursor. The
    // walk stops at the kernel's own bounds — rows, or the request bytes a
    // few thousand real records reach first — and keeps the cursor, and the
    // projection then says the filter did not see everything, even when what
    // it did see matched nothing at all. The projection itself must still be
    // admitted: a walk that overran the kernel's bound would be a refusal,
    // not an answer.
    let mut served = 0usize;
    let page = whole_inventory(None, |cursor| {
        let start = cursor
            .as_deref()
            .and_then(|cursor| cursor.parse::<usize>().ok())
            .map_or(0, |last| last + 1);
        let installs: Vec<Value> = (start..start + 100)
            .map(|index| record(&format!("{index:036}")))
            .collect();
        served += installs.len();
        Ok(json!({ "installs": installs, "next_cursor": format!("{}", start + 99) }))
    })
    .expect("a bounded walk is not a failure");
    let read = page["installs"].as_array().map(Vec::len).unwrap_or(0);
    assert!(
        read <= MAX_ROWS,
        "{read} rows exceed the kernel's row bound"
    );
    assert!(read >= 1_000, "{read} rows is a walk that gave up early");
    assert_eq!(
        served, read,
        "the walk stops fetching where it stops reading"
    );
    assert!(page["next_cursor"].is_string(), "{}", page["next_cursor"]);

    let projected = searched(page, "nobody@example.test", false, 50).expect("projects");
    assert_eq!(projected["totals"]["matched"], 0, "{projected}");
    assert_eq!(
        projected["totals"]["truncated"], true,
        "a filter that did not finish reading cannot claim nothing was cut: {}",
        projected["totals"]
    );
    assert!(projected["next_cursor"].is_string(), "{projected}");
}

#[test]
fn a_cursor_that_does_not_move_ends_the_walk_with_the_cursor_kept() {
    let page = whole_inventory(Some("same".into()), |_| {
        Ok(json!({ "installs": [record("a")], "next_cursor": "same" }))
    })
    .expect("a stuck cursor is not a failure");
    assert_eq!(page["installs"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["next_cursor"], "same");
}

#[test]
fn the_search_input_says_it_reads_every_page() {
    let search = ds_cli_installs::list::COMMAND
        .arg("search")
        .expect("--search is declared");
    assert!(search.summary.contains("every page"), "{}", search.summary);
}
