use super::*;
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use crate::gateway_exchange::Failure;
use crate::retained_reads::Kind;

fn scope(project: &str) -> Scope {
    Scope {
        uid: "owner".into(),
        email: "owner@example.test".into(),
        lane: "canary".into(),
        audience: "firebase:ds-test;gateway:https://gateway.example.test".into(),
        project: project.into(),
    }
}
fn served(id: &str) -> Value {
    json!({"success": true, "data": {"assets": [{"asset_id": id, "name": id, "folder_path": "", "modified_at": null,
        "download_url": "https://signed.example.test/x", "nested": {"download_url": "signed"}}], "more": false, "index": null}})
}
fn query() -> IndexQuery {
    IndexQuery {
        order: Some("recent".into()),
        limit: Some(60),
        ..IndexQuery::default()
    }
}
fn refresh() -> IndexQuery {
    IndexQuery {
        refresh: Some(true),
        ..query()
    }
}
fn read(
    edge: &TestEdge,
    rooms: &RefCell<Rooms>,
    scope: Scope,
    query: IndexQuery,
) -> Result<TimelinePage, Refusal> {
    block_on(async {
        match begin(edge, rooms, scope, query, |_| {}).await? {
            Begun::Answer(page) => Ok(page),
            Begun::Acquire(acquisition) => acquire(edge, rooms, acquisition).await,
            Begun::Join(_) => panic!("nothing is in flight"),
        }
    })
}
fn first_id(page: &TimelinePage) -> &str {
    match page {
        TimelinePage::Served { rows, .. } => &rows[0].asset_id,
        TimelinePage::Unavailable { .. } => panic!("a served page"),
    }
}
fn cached(page: &TimelinePage) -> bool {
    matches!(page, TimelinePage::Served { cached: true, .. })
}
fn held_pages(edge: &TestEdge) -> i64 {
    edge.db
        .borrow()
        .query_row(
            "SELECT count(*) FROM retained_reads WHERE kind='assets_index_page'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn one_answer_serves_until_refresh_and_offline() {
    let edge = TestEdge::new();
    let rooms = RefCell::new(Rooms::default());
    edge.json(served("a1"));
    let first = read(&edge, &rooms, scope("kigali"), query()).unwrap();
    assert_eq!((first_id(&first), cached(&first)), ("a1", false));
    let repeated = read(&edge, &rooms, scope("kigali"), query()).unwrap();
    assert_eq!((first_id(&repeated), cached(&repeated)), ("a1", true));
    assert_eq!(
        edge.requests().len(),
        1,
        "a fresh held page answers without a request"
    );
    edge.online.set(false);
    assert!(
        cached(&read(&edge, &rooms, scope("kigali"), refresh()).unwrap()),
        "offline answers what it holds"
    );
    edge.online.set(true);
    edge.json(served("a2"));
    assert_eq!(
        first_id(&read(&edge, &rooms, scope("kigali"), refresh()).unwrap()),
        "a2"
    );
    assert_eq!(edge.requests().len(), 2);
    assert_eq!(edge.requests()[1].body.as_ref().unwrap()["refresh"], true);
}

#[test]
fn offline_without_a_held_page_refuses_and_sends_nothing() {
    let edge = TestEdge::new();
    edge.online.set(false);
    let refusal = read(
        &edge,
        &RefCell::new(Rooms::default()),
        scope("kigali"),
        query(),
    )
    .unwrap_err();
    assert_eq!(refusal.code, OFFLINE);
    assert!(edge.requests().is_empty());
}

#[test]
fn a_signed_url_never_enters_durable_storage() {
    let edge = TestEdge::new();
    edge.json(served("a1"));
    let page = read(
        &edge,
        &RefCell::new(Rooms::default()),
        scope("kigali"),
        query(),
    )
    .unwrap();
    let TimelinePage::Served { rows, .. } = &page else {
        panic!("served")
    };
    assert_eq!(
        rows[0].row["download_url"], "https://signed.example.test/x",
        "the caller still gets this read's URL"
    );
    let stored: String = edge
        .db
        .borrow()
        .query_row("SELECT value FROM retained_reads", [], |row| row.get(0))
        .unwrap();
    assert!(!stored.contains("download_url"), "{stored}");
    assert!(stored.contains("\"nested\":{}"), "{stored}");
}

#[test]
fn a_stale_first_page_paints_at_once_while_the_network_answers() {
    let edge = TestEdge::new();
    let rooms = RefCell::new(Rooms::default());
    edge.json(served("old"));
    read(&edge, &rooms, scope("kigali"), query()).unwrap();
    edge.now
        .set(edge.now.get() + index_read::FRESH_MS as u64 + 1);
    let saved = RefCell::new(Vec::new());
    let begun = block_on(begin(&edge, &rooms, scope("kigali"), query(), |page| {
        saved.borrow_mut().push(page.clone())
    }))
    .unwrap();
    assert!(matches!(begun, Begun::Acquire(_)));
    assert_eq!(saved.borrow().len(), 1);
    assert!(cached(&saved.borrow()[0]));
    let later = block_on(begin(
        &edge,
        &rooms,
        scope("kigali"),
        IndexQuery {
            cursor: Some("c1".into()),
            ..query()
        },
        |_| panic!("only a first page paints"),
    ));
    assert!(later.is_ok());
}

#[test]
fn simultaneous_reads_of_one_page_join_one_request() {
    let edge = TestEdge::new();
    let rooms = RefCell::new(Rooms::default());
    let Begun::Acquire(first) =
        block_on(begin(&edge, &rooms, scope("kigali"), query(), |_| {})).unwrap()
    else {
        panic!("acquire")
    };
    let joined = block_on(begin(&edge, &rooms, scope("kigali"), query(), |_| {})).unwrap();
    assert!(matches!(&joined, Begun::Join(ticket) if *ticket == first.ticket));
    let other = block_on(begin(
        &edge,
        &rooms,
        scope("kigali"),
        IndexQuery {
            kind: Some("doc".into()),
            ..query()
        },
        |_| {},
    ))
    .unwrap();
    assert!(
        matches!(other, Begun::Acquire(_)),
        "another query is its own read"
    );
    // An explicit refresh never joins an ordinary read; a second refresh joins the first.
    let Begun::Acquire(refreshing) =
        block_on(begin(&edge, &rooms, scope("kigali"), refresh(), |_| {})).unwrap()
    else {
        panic!("acquire")
    };
    assert!(matches!(
        block_on(begin(&edge, &rooms, scope("kigali"), refresh(), |_| {})).unwrap(),
        Begun::Join(_)
    ));
    edge.json(served("refreshed"));
    assert_eq!(
        first_id(&block_on(acquire(&edge, &rooms, refreshing)).unwrap()),
        "refreshed"
    );
    edge.json(served("superseded"));
    assert_eq!(
        block_on(acquire(&edge, &rooms, first)).unwrap_err().code,
        SUPERSEDED,
        "the refresh superseded the older read"
    );
}

#[test]
fn an_answer_after_a_committed_classification_is_never_published() {
    let edge = TestEdge::new();
    let rooms = RefCell::new(Rooms::default());
    edge.json(served("old"));
    read(&edge, &rooms, scope("kigali"), query()).unwrap();
    let Begun::Acquire(pending) =
        block_on(begin(&edge, &rooms, scope("kigali"), refresh(), |_| {})).unwrap()
    else {
        panic!("acquire")
    };
    let receipt = |project: &str, changed: Vec<String>| MutationReceipt {
        project_id: project.into(),
        asset_id: "a_123".into(),
        version: 3,
        audit_id: "audit-3".into(),
        changed,
    };
    let classify = |receipt| {
        block_on(classified(
            &edge,
            &rooms,
            scope("kigali"),
            scope("kigali"),
            "a_123".into(),
            2,
            receipt,
        ))
    };
    assert!(
        !classify(receipt("kigali", vec![])),
        "a receipt that changed nothing invalidates nothing"
    );
    assert!(!classify(receipt("another", vec!["kind".into()])));
    assert!(classify(receipt("kigali", vec!["kind".into()])));
    edge.json(served("obsolete"));
    assert_eq!(
        block_on(acquire(&edge, &rooms, pending)).unwrap_err().code,
        SUPERSEDED
    );
    let stored: String = edge
        .db
        .borrow()
        .query_row("SELECT value FROM retained_reads", [], |row| row.get(0))
        .unwrap();
    assert!(
        stored.contains("\"old\""),
        "the obsolete answer was not retained"
    );
    edge.json(served("new"));
    assert_eq!(
        first_id(&read(&edge, &rooms, scope("kigali"), query()).unwrap()),
        "new",
        "the stale page reads again"
    );
}

#[test]
fn an_access_refusal_forgets_the_projects_pages_for_this_account_only() {
    let edge = TestEdge::new();
    let rooms = RefCell::new(Rooms::default());
    for (project, kind) in [("kigali", "doc"), ("kigali", "sheet"), ("musanze", "doc")] {
        edge.json(served(project));
        read(
            &edge,
            &rooms,
            scope(project),
            IndexQuery {
                kind: Some(kind.into()),
                ..query()
            },
        )
        .unwrap();
    }
    edge.answer(Answer::Json(
        403,
        json!({"error": {"code": "FORBIDDEN", "message": "no access"}}),
    ));
    let refusal = read(
        &edge,
        &rooms,
        scope("kigali"),
        IndexQuery {
            kind: Some("doc".into()),
            ..refresh()
        },
    )
    .unwrap_err();
    assert_eq!(refusal.status, Some(403));
    assert_eq!(held_pages(&edge), 1, "only musanze's page stays");
    edge.online.set(false);
    let gone = read(
        &edge,
        &rooms,
        scope("kigali"),
        IndexQuery {
            kind: Some("sheet".into()),
            ..query()
        },
    )
    .unwrap_err();
    assert_eq!(gone.code, OFFLINE);
}

#[test]
fn a_temporary_failure_answers_a_usable_held_page() {
    let edge = TestEdge::new();
    let rooms = RefCell::new(Rooms::default());
    edge.json(served("held"));
    read(&edge, &rooms, scope("kigali"), query()).unwrap();
    edge.answer(Answer::Failed(Failure::Network));
    let page = read(&edge, &rooms, scope("kigali"), refresh()).unwrap();
    assert!(cached(&page));
    edge.answer(Answer::Json(503, json!({"error": "unavailable"})));
    let cold = read(&edge, &rooms, scope("musanze"), query()).unwrap_err();
    assert_eq!(
        cold.status,
        Some(503),
        "nothing held: the failure is the answer"
    );
}

#[test]
fn an_answer_for_one_account_is_never_retained_under_another() {
    let edge = TestEdge::new();
    let rooms = RefCell::new(Rooms::default());
    let Begun::Acquire(pending) =
        block_on(begin(&edge, &rooms, scope("kigali"), query(), |_| {})).unwrap()
    else {
        panic!("acquire")
    };
    edge.sign_in("someone-else");
    edge.json(served("owner's"));
    assert_eq!(
        block_on(acquire(&edge, &rooms, pending)).unwrap_err().code,
        SUPERSEDED
    );
    assert_eq!(held_pages(&edge), 0);
}

#[test]
fn a_brain_without_the_index_reads_as_unavailable() {
    let edge = TestEdge::new();
    edge.answer(Answer::Json(
        400,
        json!({"error": {"code": "VALIDATION_FAILED", "message": "unknown field view"}}),
    ));
    let page = read(
        &edge,
        &RefCell::new(Rooms::default()),
        scope("kigali"),
        query(),
    )
    .unwrap();
    assert_eq!(page, TimelinePage::Unavailable { rows: vec![] });
    edge.answer(Answer::Json(
        400,
        json!({"error": {"code": "VALIDATION_FAILED", "message": "unknown field view"}}),
    ));
    assert_eq!(
        block_on(row(&edge, "kigali".into(), "a1".into())).unwrap(),
        RowAnswer::Unavailable
    );
    edge.json(json!({"data": {"asset": {"asset_id": "a1", "name": "Plan"}, "versions": []}}));
    assert!(matches!(
        block_on(row(&edge, "kigali".into(), "a1".into())).unwrap(),
        RowAnswer::Served { .. }
    ));
}

#[test]
fn the_retained_page_is_the_assets_index_kind() {
    let edge = TestEdge::new();
    edge.json(served("a1"));
    read(
        &edge,
        &RefCell::new(Rooms::default()),
        scope("kigali"),
        query(),
    )
    .unwrap();
    let (kind, project, key): (String, String, String) = edge
        .db
        .borrow()
        .query_row("SELECT kind,project,key FROM retained_reads", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(
        (kind.as_str(), project.as_str(), key.len()),
        (Kind::AssetsIndexPage.name(), "kigali", 64)
    );
}
