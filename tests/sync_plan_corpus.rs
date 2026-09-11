//! The sync plan's named proof: a recorded corpus of inventories and the plan
//! each yields, frozen here and replayed byte-identically through the real
//! WASM in ds-web (`src/tests/sync-plan-parity.test.ts`). There is no
//! TypeScript predecessor by design (owner, 2026-09-11: one function, no
//! TypeScript in the syncing engine), so each case records its rationale
//! instead of an `expected_ts`.
//!
//! Re-record with `DS_SYNC_CORPUS_WRITE=1 cargo test --test sync_plan_corpus`
//! after a deliberate rule change, and say why in the commit.

use serde_json::{json, Value};
use std::path::PathBuf;

const SCHEMA: &str = "ds.sync-plan/v1";
const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const REL: &str = "ds-network-reporter@0.1.0+0123456789abcdef0123456789abcdef01234567";
const SOLAR: &str = "ds-solar-engine@0.1.0";

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sync-plan/corpus.json")
}

fn local(
    engine: &str,
    op: &str,
    sha: &str,
    base: Option<&str>,
    readable: bool,
    release: &str,
) -> Value {
    let mut row = json!({"engine": engine, "operation": op, "variant": "default", "sha256": sha,
        "size_bytes": 4096, "produced_at_ms": 1_000, "readable": readable, "engine_release": release});
    if let Some(base) = base {
        row["base_revision"] = json!(base);
    }
    row
}
fn remote(engine: &str, op: &str, sha: &str, revision: &str) -> Value {
    json!({"engine": engine, "operation": op, "variant": "default", "revision": revision, "sha256": sha, "published_at_ms": 900})
}
fn grant(expires_at_ms: u64, engines: Vec<(&str, &str)>) -> Value {
    json!({"grant_id": "g1", "install_id": "install-1", "project": "p",
        "engines": engines.iter().map(|(n, r)| json!({"name": n, "release": r})).collect::<Vec<_>>(),
        "expires_at_ms": expires_at_ms})
}
fn input(online: bool, grant: Option<Value>, local: Vec<Value>, remote: Vec<Value>) -> Value {
    let mut value = json!({"schema": SCHEMA, "project": "p", "install_id": "install-1", "now_ms": 10_000,
        "online": online, "trigger": "navigation", "remote_read_at_ms": 9_000, "local": local, "remote": remote});
    if let Some(grant) = grant {
        value["grant"] = grant;
    }
    value
}

fn cases() -> Vec<(&'static str, &'static str, Value)> {
    let g = || {
        Some(grant(
            10_000_000,
            vec![("network_reporter", REL), ("solar", SOLAR)],
        ))
    };
    vec![
        (
            "empty",
            "Nothing held anywhere: in sync, idle poll.",
            input(true, g(), vec![], vec![]),
        ),
        (
            "offline",
            "Offline plans nothing and waits for reconnect.",
            input(
                false,
                g(),
                vec![local("network_reporter", "export-a", A, None, true, REL)],
                vec![],
            ),
        ),
        (
            "local-only-with-grant",
            "A local result the record lacks uploads under a valid grant.",
            input(
                true,
                g(),
                vec![local("network_reporter", "export-a", A, None, true, REL)],
                vec![],
            ),
        ),
        (
            "local-only-no-grant",
            "No credential: open a grant first, the upload follows.",
            input(
                true,
                None,
                vec![local("network_reporter", "export-a", A, None, true, REL)],
                vec![],
            ),
        ),
        (
            "grant-expired",
            "An expired grant is renewed before uploading.",
            input(
                true,
                Some(grant(10_000, vec![("network_reporter", REL)])),
                vec![local("network_reporter", "export-a", A, None, true, REL)],
                vec![],
            ),
        ),
        (
            "grant-engine-missing",
            "The grant does not cover the artifact's engine release.",
            input(
                true,
                Some(grant(10_000_000, vec![("solar", SOLAR)])),
                vec![local("network_reporter", "export-a", A, None, true, REL)],
                vec![],
            ),
        ),
        (
            "remote-only",
            "A head this machine lacks downloads; no grant needed.",
            input(
                true,
                None,
                vec![],
                vec![remote("network_reporter", "export-a", A, "r1")],
            ),
        ),
        (
            "in-sync",
            "Equal digests: nothing to do, idle poll.",
            input(
                true,
                g(),
                vec![local(
                    "network_reporter",
                    "export-a",
                    A,
                    Some("r1"),
                    true,
                    REL,
                )],
                vec![remote("network_reporter", "export-a", A, "r1")],
            ),
        ),
        (
            "local-newer",
            "Computed on the current head with a different digest: upload as newer.",
            input(
                true,
                g(),
                vec![local(
                    "network_reporter",
                    "export-a",
                    B,
                    Some("r1"),
                    true,
                    REL,
                )],
                vec![remote("network_reporter", "export-a", A, "r1")],
            ),
        ),
        (
            "base-moved",
            "Computed on an older base: conflict, never overwritten.",
            input(
                true,
                g(),
                vec![local(
                    "network_reporter",
                    "export-a",
                    B,
                    Some("r0"),
                    true,
                    REL,
                )],
                vec![remote("network_reporter", "export-a", A, "r1")],
            ),
        ),
        (
            "unknown-base",
            "Computed with no head in view while one exists now: conflict.",
            input(
                true,
                g(),
                vec![local("network_reporter", "export-a", B, None, true, REL)],
                vec![remote("network_reporter", "export-a", A, "r1")],
            ),
        ),
        (
            "unreadable-local-no-remote",
            "Unreadable bytes and no remote copy: refused, never uploaded.",
            input(
                true,
                g(),
                vec![local("network_reporter", "export-a", A, None, false, REL)],
                vec![],
            ),
        ),
        (
            "unreadable-local-with-remote",
            "Unreadable bytes with a remote copy: the record is the truth, download.",
            input(
                true,
                g(),
                vec![local(
                    "network_reporter",
                    "export-a",
                    A,
                    Some("r1"),
                    false,
                    REL,
                )],
                vec![remote("network_reporter", "export-a", B, "r1")],
            ),
        ),
        (
            "bad-rows-do-not-poison",
            "A malformed identity and a duplicate are refused one at a time; the rest proceeds.",
            input(
                true,
                g(),
                vec![
                    local("network_reporter", "-bad", A, None, true, REL),
                    local("network_reporter", "dup", A, None, true, REL),
                    local("network_reporter", "dup", B, None, true, REL),
                    local("network_reporter", "ok", A, None, true, REL),
                ],
                vec![],
            ),
        ),
        (
            "mixed-engines-ordered",
            "A Solar batch is plain artifacts; actions are ordered by identity.",
            input(
                true,
                g(),
                vec![
                    local("solar", "calculate-city-b", A, None, true, SOLAR),
                    local("solar", "calculate-city-a", A, None, true, SOLAR),
                    local("network_reporter", "export-z", A, Some("r1"), true, REL),
                ],
                vec![
                    remote("network_reporter", "export-z", A, "r1"),
                    remote("network_reporter", "export-y", B, "r2"),
                ],
            ),
        ),
        (
            "grant-renewal-is-the-only-timer",
            "An upload pending under a grant that expires soon: the one timed wake, renewal before expiry.",
            input(true, Some(grant(10_000 + 10 * 60 * 1000 + 1_000, vec![("network_reporter", REL)])), vec![local("network_reporter", "export-a", A, None, true, REL)], vec![]),
        ),
        (
            "local-change-uses-held-heads",
            "A local change never re-reads the record; the held heads decide.",
            { let mut v = input(true, g(), vec![local("network_reporter", "export-a", A, None, true, REL)], vec![]); v["trigger"] = json!("local_change"); v },
        ),
        (
            "remote-push-reads-the-record",
            "A push from the messaging stream is the one reason to re-read without navigating.",
            { let mut v = input(true, g(), vec![], vec![]); v["trigger"] = json!("remote_push"); v },
        ),
        (
            "never-read-reads-once",
            "Heads never read are read once, whatever the trigger.",
            { let mut v = input(true, g(), vec![], vec![]); v["trigger"] = json!("local_change"); v.as_object_mut().unwrap().remove("remote_read_at_ms"); v },
        ),
    ]
}

#[test]
fn the_corpus_is_the_plan_the_kernel_gives_today() {
    let recorded: Vec<Value> = cases()
        .into_iter()
        .map(|(id, why, request)| {
            let text =
                ds_command_kernel::sync::evaluate(serde_json::to_vec(&request).unwrap().as_slice())
                    .unwrap();
            let expected: Value = serde_json::from_str(&text).unwrap();
            json!({"id": id, "why": why, "request": request, "expected": expected})
        })
        .collect();
    let document = json!({"schema": "ds.sync-plan-corpus/v1",
        "purpose": "The sync gate's one decision, recorded from ds-command-kernel::sync::plan. No TypeScript predecessor by design; `why` is the rationale per case.",
        "cases": recorded});
    let path = fixture_path();
    if std::env::var("DS_SYNC_CORPUS_WRITE").is_ok() {
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&document).unwrap() + "\n",
        )
        .unwrap();
    }
    let frozen: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("corpus fixture present"))
            .unwrap();
    assert_eq!(frozen, document, "the frozen corpus differs from today's plan; re-record deliberately with DS_SYNC_CORPUS_WRITE=1 and explain why");
}

#[test]
fn every_case_answers_its_rationale() {
    let frozen: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_path()).unwrap()).unwrap();
    let kinds = |case: &Value| -> Vec<String> {
        case["expected"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let kind = a["kind"].as_str().unwrap();
                match a.get("reason").and_then(Value::as_str) {
                    Some(reason) => format!("{kind}:{reason}"),
                    None => kind.to_string(),
                }
            })
            .collect()
    };
    let by_id: std::collections::BTreeMap<&str, &Value> = frozen["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c))
        .collect();
    assert_eq!(kinds(by_id["empty"]), Vec::<String>::new());
    assert_eq!(kinds(by_id["offline"]), vec!["offline"]);
    assert_eq!(
        kinds(by_id["local-only-no-grant"]),
        vec!["open_grant:no_grant", "upload:absent_remotely"]
    );
    assert_eq!(kinds(by_id["grant-expired"])[0], "open_grant:grant_expired");
    assert_eq!(
        kinds(by_id["grant-engine-missing"])[0],
        "open_grant:grant_engine_missing"
    );
    assert_eq!(kinds(by_id["remote-only"]), vec!["download:absent_locally"]);
    assert_eq!(kinds(by_id["in-sync"]), vec!["nothing:in_sync"]);
    assert_eq!(kinds(by_id["local-newer"]), vec!["upload:local_newer"]);
    assert_eq!(kinds(by_id["base-moved"]), vec!["conflict:base_moved"]);
    assert_eq!(kinds(by_id["unknown-base"]), vec!["conflict:base_moved"]);
    assert_eq!(
        kinds(by_id["unreadable-local-no-remote"]),
        vec!["refused:local_unreadable"]
    );
    assert_eq!(
        kinds(by_id["unreadable-local-with-remote"]),
        vec!["download:local_unreadable"]
    );
    assert_eq!(
        kinds(by_id["bad-rows-do-not-poison"]),
        vec![
            "upload:absent_remotely",
            "refused:identity_malformed",
            "refused:duplicate_identity"
        ]
    );
    assert_eq!(
        kinds(by_id["mixed-engines-ordered"]),
        vec![
            "download:absent_locally",
            "nothing:in_sync",
            "upload:absent_remotely",
            "upload:absent_remotely"
        ]
    );
    assert_eq!(
        by_id["grant-renewal-is-the-only-timer"]["expected"]["wake"],
        json!({"kind": "at", "at_ms": 11_000, "reason": "grant_renewal"})
    );
    assert_eq!(
        by_id["in-sync"]["expected"]["wake"],
        json!({"kind": "event"})
    );
    assert_eq!(
        by_id["offline"]["expected"]["wake"],
        json!({"kind": "event"})
    );
    assert_eq!(
        by_id["local-change-uses-held-heads"]["expected"]["refresh_remote"],
        json!({"needed": false, "reason": "held_heads_suffice"})
    );
    assert_eq!(
        by_id["remote-push-reads-the-record"]["expected"]["refresh_remote"],
        json!({"needed": true, "reason": "record_changed"})
    );
    assert_eq!(
        by_id["never-read-reads-once"]["expected"]["refresh_remote"],
        json!({"needed": true, "reason": "never_read"})
    );
    for case in frozen["cases"].as_array().unwrap() {
        assert!(
            case["expected"].get("next_check_ms").is_none(),
            "no poll cadence anywhere"
        );
    }
}
