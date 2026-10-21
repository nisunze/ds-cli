use super::*;
use crate::transformer_status_store as status;
use rusqlite::Connection;
use serde_json::json;

fn scope(principal: &str) -> status::Scope {
    status::Scope {
        principal: principal.into(),
        lane: "canary".into(),
        audience: "a".into(),
        project: "p".into(),
    }
}

#[test]
fn shared_status_schema_roundtrips_multiple_fenced_scopes_in_one_database() {
    let db = Connection::open_in_memory().unwrap();
    execute(&db, &status::schema()).unwrap();
    let (mine, other) = (scope("u"), scope("v"));
    for (s, name) in [(&mine, "TX-1"), (&other, "TX-2")] {
        execute(
            &db,
            &status::write(s, &json!({"transformers":[{"name":name}]}), 123).unwrap(),
        )
        .unwrap();
    }
    let a = status::decode(&execute(&db, &status::read(&mine).unwrap()).unwrap())
        .unwrap()
        .unwrap();
    let b = status::decode(&execute(&db, &status::read(&other).unwrap()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(a.document["transformers"][0]["name"], "TX-1");
    assert_eq!(b.document["transformers"][0]["name"], "TX-2");
    execute(&db, &status::invalidate(&mine, true).unwrap()).unwrap();
    let failed = status::decode(&execute(&db, &status::read(&mine).unwrap()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(failed.document, a.document);
    assert_eq!(failed.sha256, a.sha256);
    assert_eq!(failed.observed_at_ms, a.observed_at_ms);
    assert_eq!(failed.revision, a.revision + 1);
}

#[test]
fn cold_refresh_cannot_cross_invalidation_of_absent_observation() {
    let db = Connection::open_in_memory().unwrap();
    execute(&db, &status::schema()).unwrap();
    let scope = scope("u");
    let before =
        status::admit_size(&execute(&db, &status::size(&scope).unwrap()).unwrap()).unwrap();
    assert_eq!(before, (None, false));
    execute(&db, &status::invalidate(&scope, false).unwrap()).unwrap();
    let after = status::admit_size(&execute(&db, &status::size(&scope).unwrap()).unwrap()).unwrap();
    assert_eq!(after, (Some(1), false));
    let status::Step::Sql {
        statement,
        continuation,
    } = status::start(status::Operation::Commit {
        scope: scope.clone(),
        document: json!({"transformers":[]}),
        now: 1,
        expected: before.0,
    })
    .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        status::resume(continuation, execute(&db, &statement).unwrap()).unwrap_err(),
        "status_refresh_superseded"
    );
    assert!(
        status::decode(&execute(&db, &status::read(&scope).unwrap()).unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn scalar_and_result_bounds_refuse_without_silent_truncation() {
    let db = Connection::open_in_memory().unwrap();
    for effect in [
        Statement::new("SELECT 1 UNION ALL SELECT 2", vec![], 1, 100),
        Statement::new("SELECT 'too many bytes'", vec![], 1, 5),
        Statement::new("SELECT x'0102'", vec![], 1, 100),
        Statement::new("SELECT ?1", vec![json!({})], 1, 100),
    ] {
        assert!(execute(&db, &effect).is_err());
    }
}
