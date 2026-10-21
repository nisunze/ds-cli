//! Desktop and Server instance routing, replayed (project/instance isolation,
//! slice 2).
//!
//! `tests/fixtures/desktop-instance-cases.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/desktop-instance/cases.json`. Each case carries
//! the op (without the schema wrapper) and the answer the kernel must give: the
//! parsed answer, or `{"error": "<the exact Err string>"}` when the request
//! itself is malformed. ds-web replays the same file through the vendored WASM,
//! so no rule about which live runtime an operation belongs to, or whose reply
//! may still be applied, can be true on one host and not another.
//!
//! Two properties are asserted over the corpus rather than case by case,
//! because they are what the contract is actually about: **order never
//! decides** (every multi-candidate case is replayed in several permutations
//! and must answer identically) and **nothing is disclosed** (no answer names a
//! pairing token, an account or a credential audience, and no refusal names a
//! bridge address).
//!
//! Re-record `expected` with `DS_DESKTOP_INSTANCE_CASES_WRITE=1 cargo test
//! --test desktop_instance_cases` after a deliberate rule change, and say why
//! in the commit. That also refreshes the ds-web copy when `../ds-web` is
//! present.

use ds_command_kernel::desktop_instance::{evaluate, REFUSALS, SCHEMA};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/desktop-instance-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/desktop-instance-cases.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-web/src/tests/fixtures/desktop-instance/cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.desktop-instance-cases/v1");
    corpus
}

/// What the kernel answers for one request: its answer parsed, or its refusal
/// of the request itself as `{"error": ...}`.
fn ask(schema: &str, request: &Value) -> Value {
    let encoded =
        serde_json::to_vec(&json!({"schema": schema, "op": request})).expect("request encodes");
    match evaluate(&encoded) {
        Ok(answer) => serde_json::from_str(&answer).expect("the answer is JSON"),
        Err(error) => json!({ "error": error }),
    }
}

fn schema_of(case: &Value) -> &str {
    case.get("schema").and_then(Value::as_str).unwrap_or(SCHEMA)
}

fn answer(case: &Value) -> Value {
    ask(schema_of(case), &case["request"])
}

/// Every ordering of the candidates a case hands in, capped so a large set
/// still gets several orderings rather than a factorial explosion.
fn orderings(candidates: &[Value]) -> Vec<Vec<Value>> {
    fn all(items: &[Value]) -> Vec<Vec<Value>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut every = Vec::new();
        for index in 0..items.len() {
            let mut rest = items.to_vec();
            let head = rest.remove(index);
            for mut tail in all(&rest) {
                tail.insert(0, head.clone());
                every.push(tail);
            }
        }
        every
    }
    if candidates.len() <= 4 {
        return all(candidates);
    }
    let mut rotations = Vec::new();
    for offset in 0..candidates.len() {
        let mut rotated = candidates[offset..].to_vec();
        rotated.extend_from_slice(&candidates[..offset]);
        rotations.push(rotated);
    }
    let mut reversed = candidates.to_vec();
    reversed.reverse();
    rotations.push(reversed);
    rotations
}

/// The candidate set a case hands in, when it hands one in.
fn candidates(case: &Value) -> Option<&Vec<Value>> {
    match case["request"]["kind"].as_str() {
        Some("select" | "list") => case["request"]["candidates"].as_array(),
        _ => None,
    }
}

#[test]
fn the_kernel_answers_every_recorded_case_or_rerecords_it() {
    let mut corpus = corpus();
    let write = std::env::var_os("DS_DESKTOP_INSTANCE_CASES_WRITE").is_some();
    let mut drift = Vec::new();
    for case in corpus["cases"].as_array_mut().expect("cases") {
        let id = case["id"].as_str().expect("case id").to_owned();
        let actual = answer(case);
        if write {
            case["expected"] = actual;
        } else if case["expected"] != actual {
            drift.push(format!(
                "{id}:\n  recorded: {}\n  kernel:   {actual}",
                case["expected"]
            ));
        }
    }
    if write {
        let text = format!(
            "{}\n",
            serde_json::to_string_pretty(&corpus).expect("cases encode")
        );
        std::fs::write(own_copy(), &text).expect("write the kernel copy");
        if web_copy().parent().is_some_and(|dir| dir.is_dir()) {
            std::fs::write(web_copy(), &text).expect("write the ds-web copy");
        }
        return;
    }
    assert!(drift.is_empty(), "\n{}", drift.join("\n\n"));
}

#[test]
fn the_ds_web_copy_is_the_same_file() {
    let Ok(shared) = std::fs::read_to_string(web_copy()) else {
        // A kernel checkout without its ds-web sibling still proves itself.
        return;
    };
    assert_eq!(
        shared, CASES,
        "the ds-web copy has drifted; re-record with DS_DESKTOP_INSTANCE_CASES_WRITE=1"
    );
}

/// The rule the whole contract turns on: **focus, recency, list order or a
/// last-active window never decides**. A decision that is a function of a set
/// cannot be tested by one ordering, so every case that hands in more than one
/// candidate is replayed in several — all of them, for the sets small enough —
/// and every ordering must answer the same bytes.
#[test]
fn no_ordering_of_the_candidates_changes_the_answer() {
    let corpus = corpus();
    let mut replayed = 0;
    let mut orderings_run = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let Some(candidates) = candidates(case) else {
            continue;
        };
        if candidates.len() < 2 {
            continue;
        }
        replayed += 1;
        let permutations = orderings(candidates);
        assert!(
            permutations.len() >= 2,
            "{id}: a multi-candidate case is replayed in at least two orderings"
        );
        for ordering in permutations {
            orderings_run += 1;
            let mut request = case["request"].clone();
            request["candidates"] = Value::Array(ordering);
            assert_eq!(
                ask(schema_of(case), &request),
                case["expected"],
                "{id}: an ordering of the candidates changed the answer"
            );
        }
    }
    assert!(
        replayed >= 15,
        "the corpus lost its multi-candidate cases ({replayed})"
    );
    assert!(
        orderings_run >= 40,
        "too few orderings were replayed ({orderings_run})"
    );
}

/// Isolation is a closed vocabulary: every op, every refusal code and every
/// outcome the contract names is exercised, and nothing it does not name is
/// answered.
#[test]
fn the_corpus_covers_every_op_refusal_and_outcome() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 60, "the corpus lost cases");
    let mut ids = BTreeSet::new();
    let mut ops = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    let mut target_reasons = BTreeSet::new();
    let mut descriptor_reasons = BTreeSet::new();
    let mut errors = 0;
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id), "{id} is recorded twice");
        assert!(
            case["why"].as_str().is_some_and(|why| why.len() > 40),
            "{id} does not say why it exists"
        );
        let expected = &case["expected"];
        let kind = case["request"]["kind"]
            .as_str()
            .expect("an op names its kind")
            .to_owned();
        // A case that deliberately names no op at all proves the wire is
        // closed; it does not enlarge the vocabulary.
        if expected.get("error").is_none() {
            ops.insert(kind);
        } else {
            errors += 1;
        }
        if let Some(code) = expected["refusal"]["code"].as_str() {
            refused.insert(code.to_owned());
            assert!(
                expected["refusal"]["remedy"]
                    .as_str()
                    .is_some_and(|remedy| remedy.len() > 10),
                "{id}: a refusal says what to do about it"
            );
        }
        if let Some(outcome) = expected["outcome"].as_str() {
            outcomes.insert(outcome.to_owned());
        }
        if let Some(reason) = expected["refusal"]["detail"]["reason"].as_str() {
            match expected["refusal"]["code"].as_str() {
                Some("desktop_target_mismatch") => target_reasons.insert(reason.to_owned()),
                Some("descriptor_unusable") => descriptor_reasons.insert(reason.to_owned()),
                other => panic!("{id}: {other:?} carries a reason nothing reads"),
            };
        }
    }
    assert_eq!(
        ops,
        [
            "admit_descriptor",
            "admit_result",
            "advance_generation",
            "list",
            "mint_request_id",
            "read_request_id",
            "select",
        ]
        .iter()
        .map(|op| (*op).to_owned())
        .collect::<BTreeSet<String>>(),
        "every op is exercised and no other exists"
    );
    assert_eq!(
        refused,
        REFUSALS
            .iter()
            .map(|code| (*code).to_owned())
            .collect::<BTreeSet<String>>(),
        "the refusal vocabulary is closed and every code is proven"
    );
    assert_eq!(
        outcomes,
        ["admitted", "advanced", "listed", "minted", "read", "refused", "routed"]
            .iter()
            .map(|outcome| (*outcome).to_owned())
            .collect::<BTreeSet<String>>()
    );
    assert_eq!(
        target_reasons,
        ["account", "audience", "lane", "malformed", "window"]
            .iter()
            .map(|reason| (*reason).to_owned())
            .collect::<BTreeSet<String>>(),
        "every way a named target can be the wrong one is proven"
    );
    // Each descriptor rule refuses in its own words, so an operator reading a
    // log knows which one their file broke.
    assert!(
        descriptor_reasons.len() >= 7,
        "every descriptor rule says which one it was: {descriptor_reasons:?}"
    );
    assert!(descriptor_reasons
        .iter()
        .any(|reason| reason.contains("loopback origin")));
    assert!(errors >= 10, "malformed requests are part of the contract");
}

/// The proof the whole boundary rests on, asserted over the recorded answers
/// rather than over prose. A pairing token, an account and a credential
/// audience never appear in any answer at all; a bridge address never appears
/// in a refusal, which is the one place instances are described to a caller who
/// did not already hold their descriptors.
#[test]
fn no_recorded_answer_discloses_a_secret_an_account_or_an_address() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let recorded = serde_json::to_string(&case["expected"]).expect("encodes");
        let mut secrets: BTreeSet<String> = BTreeSet::new();
        let mut addresses: BTreeSet<String> = BTreeSet::new();
        collect(&case["request"], &mut secrets, &mut addresses);
        for secret in &secrets {
            assert!(
                !recorded.contains(secret.as_str()),
                "{id}: an answer named {secret}"
            );
        }
        if case["expected"].get("refusal").is_none() && case["expected"].get("error").is_none() {
            continue;
        }
        for address in &addresses {
            assert!(
                !recorded.contains(address.as_str()),
                "{id}: a refusal named {address}"
            );
        }
    }

    /// Every token, account and audience a request hands in, and every address.
    fn collect(value: &Value, secrets: &mut BTreeSet<String>, addresses: &mut BTreeSet<String>) {
        match value {
            Value::Object(members) => {
                for (key, member) in members {
                    if let Some(text) = member.as_str() {
                        if matches!(key.as_str(), "token" | "uid" | "audience_sha256")
                            && text.len() >= 5
                        {
                            secrets.insert(text.to_owned());
                        }
                        if key == "url" && text.len() >= 5 {
                            addresses.insert(text.to_owned());
                        }
                    }
                    collect(member, secrets, addresses);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect(item, secrets, addresses);
                }
            }
            _ => {}
        }
    }
}

/// An explicit target is honoured or refused, and never falls through. Stated
/// over the corpus so a later rule change cannot quietly make a named instance
/// resolve to a different one.
#[test]
fn no_recorded_selection_routes_anywhere_but_the_instance_that_was_named() {
    let corpus = corpus();
    let mut targeted = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let Some(target) = case["request"].get("target") else {
            continue;
        };
        targeted += 1;
        let Some(routed) = case["expected"]["route"]["instance_id"].as_str() else {
            assert_eq!(
                case["expected"]["outcome"], "refused",
                "{id}: a named target is honoured or refused, never anything else"
            );
            continue;
        };
        assert_eq!(
            Value::String(routed.to_owned()),
            target["instance_id"],
            "{id}: the operation went somewhere other than the instance that was named"
        );
        assert_eq!(
            case["expected"]["route"]["window"],
            target.get("window").cloned().unwrap_or(Value::Null),
            "{id}: the operation went to a window nobody named"
        );
    }
    assert!(targeted >= 12, "the corpus lost its explicit-target cases");
}

#[test]
fn the_corpus_keeps_the_order_of_keys_the_recorder_wrote() {
    // `preserve_order` is on: a re-recorded corpus must diff minimally.
    let corpus: Map<String, Value> = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(
        corpus.keys().collect::<Vec<_>>(),
        ["schema", "purpose", "source", "note", "cases"]
    );
}
