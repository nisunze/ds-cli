//! The Kernel-owned project model catalogue runs unchanged on native SQLite:
//! first arrivals persist at once, an equal source basis writes no row, and
//! failure/revocation keep or fence the held rows.
use ds_command_kernel::grid_project_catalog::{
    self as catalog, Changes, Held, Operation, Scope, Source, Step,
};
use ds_sync_store::sql_effect::execute;
use rusqlite::Connection;
use serde_json::{json, Value};

fn scope(project: &str) -> Scope {
    Scope {
        principal: "owner".into(),
        lane: "canary".into(),
        audience: "a".repeat(64),
        project: project.into(),
    }
}
fn head(model: &str, revision: &str, name: &str, digest: char) -> Value {
    json!({"model_id":model,"project_id":"template","display_name":name,"model_kind":"mv_line","lifecycle":"promoted",
        "state":"active","head_revision_id":revision,"head_version":1,"head_model_digest":digest.to_string().repeat(64),
        "updated_at":"2026-10-18T07:07:51Z"})
}
fn listing(models: &[Value]) -> Value {
    json!({"project":"template","models":models.iter().map(catalog::head_projection).collect::<Vec<_>>(),"more":false,"next_cursor":""})
}
/// One host transaction per operation, exactly as the native and browser
/// storage adapters run it.
fn run(db: &Connection, operation: Operation) -> Result<(Option<Held>, Option<Changes>), String> {
    let transaction = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut step = catalog::start(operation)?;
    for _ in 0..16 {
        match step {
            Step::Done { held, changes } => {
                transaction.commit().map_err(|e| e.to_string())?;
                return Ok((held, changes));
            }
            Step::Sql {
                statement,
                continuation,
            } => step = catalog::resume(continuation, execute(&transaction, &statement)?)?,
        }
    }
    Err("catalogue continuation exceeded its bound".into())
}
fn open() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    run(&db, Operation::Schema).unwrap();
    db
}
fn ids(held: &Held) -> Vec<String> {
    held.heads
        .iter()
        .map(|row| row.head["model_id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn first_arrivals_persist_at_once_and_an_equal_basis_writes_no_row() {
    let db = open();
    let room = scope("template");
    assert_eq!(
        run(
            &db,
            Operation::Read {
                scope: room.clone()
            }
        )
        .unwrap()
        .0,
        None
    );
    let older = [
        head(
            "m-ref",
            "r1",
            "Nyamagabe MV Model 1 - 2026-10-02 oracle reference copy",
            'a',
        ),
        head(
            "m-usage",
            "r1",
            "Nyamagabe MV Model 1 - structure usage oracle test copy",
            'b',
        ),
        head(
            "m-copy",
            "r1",
            "Gisagara MV Model 1 - Main line (rev3) - test copy",
            'c',
        ),
    ];
    let (held, changes) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&older),
            now: 100,
            expected: None,
        },
    )
    .unwrap();
    let held = held.unwrap();
    assert_eq!(changes.unwrap().added.len(), 3);
    assert!(held.heads.iter().all(|row| row.first_seen_at_ms == 100));
    let revision = held.observation.revision;

    // The same listing again: one cheap comparison, observation time only.
    let (held, changes) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&older),
            now: 200,
            expected: Some(revision),
        },
    )
    .unwrap();
    let (held, changes) = (held.unwrap(), changes.unwrap());
    assert!(changes.fresh && changes.added.is_empty() && changes.unchanged == 3);
    assert_eq!(held.observation.observed_at_ms, Some(200));
    assert!(held.heads.iter().all(|row| row.first_seen_at_ms == 100));

    // Three models created later arrive in one refresh and stay persisted.
    let mut newer = older.to_vec();
    newer.push(head(
        "m-main",
        "r9",
        "Gisagara MV Model 1 - Main line (rev3)",
        'd',
    ));
    newer.push(head("m-sub", "r9", "Gisagara MV Model 2 - Substation", 'e'));
    newer.push(head(
        "m-scope",
        "r9",
        "Gisagara MV Model 3 - Additional scope",
        'f',
    ));
    let (held, changes) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&newer),
            now: 300,
            expected: Some(held.observation.revision),
        },
    )
    .unwrap();
    let (held, changes) = (held.unwrap(), changes.unwrap());
    assert_eq!(changes.added, ["m-main", "m-scope", "m-sub"]);
    assert_eq!(held.heads.len(), 6);
    // Offline: the next read paints all six at once with no transport.
    let (offline, _) = run(
        &db,
        Operation::Read {
            scope: room.clone(),
        },
    )
    .unwrap();
    let view = catalog::view("template", offline.as_ref(), Source::Held, None, None).unwrap();
    assert_eq!(view["observation"]["state"], "ready");
    assert_eq!(view["mv_models"].as_array().unwrap().len(), 6);
    assert_eq!(
        view["mv_models"][0]["label"],
        "Gisagara MV Model 1 - Main line (rev3)"
    );
    let first_seen: Vec<u64> = offline
        .unwrap()
        .heads
        .iter()
        .map(|row| row.first_seen_at_ms)
        .collect();
    assert_eq!(first_seen.iter().filter(|t| **t == 300).count(), 3);
}

#[test]
fn moved_heads_keep_their_retained_version_and_retired_heads_leave() {
    let db = open();
    let room = scope("template");
    let first = [head("m-1", "r1", "A", 'a'), head("m-2", "r1", "B", 'b')];
    let (held, _) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&first),
            now: 1,
            expected: None,
        },
    )
    .unwrap();
    let version = json!({"model_id":"m-1","revision_id":"r1","version":1,"model":{"digest":"a".repeat(64),"byte_length":42}});
    let (held_after, _) = run(
        &db,
        Operation::RetainVersion {
            scope: room.clone(),
            model_id: "m-1".into(),
            version: version.clone(),
        },
    )
    .unwrap();
    let held_after = held_after.unwrap();
    assert_eq!(
        held_after.observation.revision,
        held.unwrap().observation.revision,
        "retaining bytes moves no fence"
    );
    assert_eq!(
        held_after.heads[0].retained_version.as_ref().unwrap()["revision_id"],
        "r1"
    );
    // A version that is not the held head is refused without a write.
    let mut stale = version.clone();
    stale["revision_id"] = json!("r0");
    assert_eq!(
        run(
            &db,
            Operation::RetainVersion {
                scope: room.clone(),
                model_id: "m-1".into(),
                version: stale
            }
        )
        .unwrap_err(),
        catalog::VERSION_STALE
    );
    // m-1 moves, m-2 is retired.
    let (held, changes) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&[head("m-1", "r2", "A", 'c')]),
            now: 2,
            expected: Some(held_after.observation.revision),
        },
    )
    .unwrap();
    let (held, changes) = (held.unwrap(), changes.unwrap());
    assert_eq!(
        (changes.moved.as_slice(), changes.retired.as_slice()),
        (&["m-1".to_string()][..], &["m-2".to_string()][..])
    );
    assert_eq!(ids(&held), ["m-1"]);
    assert_eq!(held.heads[0].first_seen_at_ms, 1);
    assert_eq!(
        held.heads[0].retained_version.as_ref().unwrap()["revision_id"],
        "r1"
    );
    let view = catalog::view("template", Some(&held), Source::Refresh, None, None).unwrap();
    assert_eq!(view["mv_models"][0]["retained_current"], false);
    assert_eq!(view["mv_models"][0]["head_revision_id"], "r2");
}

#[test]
fn failure_labels_rows_revocation_fences_them_and_scopes_never_cross() {
    let db = open();
    let room = scope("template");
    let (held, _) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&[head("m-1", "r1", "A", 'a')]),
            now: 1,
            expected: None,
        },
    )
    .unwrap();
    let revision = held.unwrap().observation.revision;
    // A stale commit (another consumer answered first) writes nothing.
    let (held, changes) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&[]),
            now: 5,
            expected: Some(revision + 7),
        },
    )
    .unwrap();
    assert!(changes.unwrap().superseded);
    assert_eq!(ids(held.as_ref().unwrap()), ["m-1"]);
    let (held, _) = run(
        &db,
        Operation::MarkFailed {
            scope: room.clone(),
            expected: Some(revision),
        },
    )
    .unwrap();
    let held = held.unwrap();
    let view = catalog::view("template", Some(&held), Source::Held, None, None).unwrap();
    assert_eq!(view["observation"]["state"], "held");
    assert_eq!(view["observation"]["last_error"], catalog::REFRESH_FAILED);
    assert_eq!(
        view["mv_models"].as_array().unwrap().len(),
        1,
        "a failed refresh keeps the rows"
    );
    let (held, _) = run(
        &db,
        Operation::Revoke {
            scope: room.clone(),
        },
    )
    .unwrap();
    let held = held.unwrap();
    let view = catalog::view("template", Some(&held), Source::Held, None, None).unwrap();
    assert_eq!(view["observation"]["state"], "revoked");
    assert!(view["mv_models"].as_array().unwrap().is_empty());
    assert!(run(
        &db,
        Operation::RetainVersion {
            scope: room.clone(),
            model_id: "m-1".into(),
            version: json!({"model_id":"m-1","revision_id":"r1","version":1,"model":{"digest":"a".repeat(64),"byte_length":4}}),
        }
    )
    .is_err());
    // A deliberate successful refresh restores reuse.
    let (held, _) = run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: listing(&[head("m-1", "r1", "A", 'a')]),
            now: 9,
            expected: Some(held.observation.revision),
        },
    )
    .unwrap();
    assert!(held.as_ref().unwrap().servable());
    let (invalidated, _) = run(
        &db,
        Operation::Invalidate {
            scope: room.clone(),
        },
    )
    .unwrap();
    assert!(invalidated.unwrap().observation.needs_refresh);
    for other in [
        Scope {
            principal: "peer".into(),
            ..room.clone()
        },
        Scope {
            lane: "stable".into(),
            ..room.clone()
        },
        Scope {
            audience: "b".repeat(64),
            ..room.clone()
        },
        scope("other"),
    ] {
        assert_eq!(run(&db, Operation::Read { scope: other }).unwrap().0, None);
    }
}

#[test]
fn bounded_reads_refuse_rather_than_truncate() {
    let db = open();
    let room = scope("template");
    let mut wrong = listing(&[head("m-1", "r1", "A", 'a')]);
    wrong["models"][0]["head_model_digest"] = json!("not-a-digest");
    assert!(run(
        &db,
        Operation::Commit {
            scope: room.clone(),
            listing: wrong,
            now: 1,
            expected: None
        }
    )
    .is_err());
    assert_eq!(run(&db, Operation::Read { scope: room }).unwrap().0, None);
}
