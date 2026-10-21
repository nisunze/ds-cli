//! The DS envelope v1, replayed.
//!
//! `tests/fixtures/ds-envelope-cases.json` carries, per case, a command id, a
//! contract version and either the `data` of a success or the `refusal` of an
//! error, with the exact envelope both externals must emit: `ds` on stdout
//! under `--output json` and the desktop shell to the webview. The expected
//! envelopes were transcribed from ds-cli-contract's `success_envelope` and
//! `error_envelope`, so this file proves the kernel's `envelope::success` and
//! `envelope::error` answer what `ds` answers today; ds-cli replays the same
//! corpus through its own definition, so the two cannot drift.
//!
//! Byte-for-byte: the comparison is over the compact serialisation, so member
//! order is part of what is proven, not only membership.
//!
//! Re-record `expected` with `DS_ENVELOPE_CASES_WRITE=1 cargo test --test
//! envelope_cases` after a deliberate envelope change, and say why in the
//! commit.

use ds_command_kernel::envelope::{error, success, Class, Refusal, VERSION};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/ds-envelope-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ds-envelope-cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.envelope-cases/v1");
    corpus
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).expect("encodes")
}

/// The envelope the kernel builds for one case.
fn answer(case: &Value) -> Value {
    let id = case["id"].as_str().expect("case id");
    let request = &case["request"];
    let command = request["command"]
        .as_str()
        .expect("a case names its command");
    let contract = u32::try_from(request["contract"].as_u64().expect("a contract version"))
        .expect("a contract version fits u32");
    match request["kind"].as_str() {
        Some("success") => success(command, contract, request["data"].clone()),
        Some("error") => {
            let refusal: Refusal = serde_json::from_value(request["refusal"].clone())
                .unwrap_or_else(|e| panic!("{id}: the refusal does not read as one: {e}"));
            error(command, contract, &refusal)
        }
        other => panic!("{id}: {other:?} is not a case kind"),
    }
}

#[test]
fn the_kernel_answers_every_recorded_case_byte_for_byte_or_rerecords_it() {
    let mut corpus = corpus();
    let write = std::env::var_os("DS_ENVELOPE_CASES_WRITE").is_some();
    let mut drift = Vec::new();
    for case in corpus["cases"].as_array_mut().expect("cases") {
        let id = case["id"].as_str().expect("case id").to_owned();
        let actual = answer(case);
        if write {
            case["expected"] = actual;
        } else if compact(&case["expected"]) != compact(&actual) {
            drift.push(format!(
                "{id}:\n  recorded: {}\n  kernel:   {}",
                compact(&case["expected"]),
                compact(&actual)
            ));
        }
    }
    if write {
        let text = format!(
            "{}\n",
            serde_json::to_string_pretty(&corpus).expect("cases encode")
        );
        std::fs::write(own_copy(), &text).expect("write the kernel copy");
        return;
    }
    assert!(drift.is_empty(), "\n{}", drift.join("\n\n"));
}

/// What the corpus must exercise so a replay on the other side proves the
/// whole shape: at least one success, and for every class an error with and
/// without each optional member, so an omitted member and a present one are
/// both recorded for every retryable value.
#[test]
fn the_corpus_covers_every_class_with_and_without_each_optional_member() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    let mut ids = BTreeSet::new();
    let mut successes = 0;
    let mut with: BTreeMap<(&str, &str), BTreeSet<bool>> = BTreeMap::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id), "{id} is recorded twice");
        assert!(
            case["why"].as_str().is_some_and(|why| why.len() > 40),
            "{id} does not say why it exists"
        );
        let expected = &case["expected"];
        assert_eq!(
            expected["v"],
            Value::from(VERSION),
            "{id}: envelope version"
        );
        assert_eq!(
            expected["command"], case["request"]["command"],
            "{id}: command"
        );
        assert_eq!(
            expected["contract"], case["request"]["contract"],
            "{id}: contract"
        );
        match case["request"]["kind"].as_str() {
            Some("success") => {
                successes += 1;
                assert_eq!(expected["status"], "ok", "{id}");
                assert!(
                    expected.get("error").is_none(),
                    "{id}: a success carries no error"
                );
                assert!(
                    expected.get("data").is_some(),
                    "{id}: a success carries data"
                );
            }
            Some("error") => {
                assert_eq!(expected["status"], "error", "{id}");
                assert!(
                    expected.get("data").is_none(),
                    "{id}: an error carries no data"
                );
                let body = &expected["error"];
                let token = body["class"].as_str().expect("the class token");
                let class = Class::from_token(token)
                    .unwrap_or_else(|| panic!("{id}: {token} is not a class"));
                assert_eq!(
                    body["retryable"],
                    Value::from(class.retryable()),
                    "{id}: retryable is a function of the class alone"
                );
                assert!(
                    body["code"].as_str().is_some_and(|code| {
                        !code.is_empty()
                            && code
                                .chars()
                                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    }),
                    "{id}: a code is snake_case"
                );
                for member in ["remedy", "next", "detail"] {
                    let present = body.get(member).is_some();
                    with.entry((token, member)).or_default().insert(present);
                    if member == "next" {
                        assert!(
                            !present || body["next"].as_array().is_some_and(|n| !n.is_empty()),
                            "{id}: next is omitted rather than written empty"
                        );
                    }
                }
            }
            other => panic!("{id}: {other:?} is not a case kind"),
        }
    }
    assert!(successes >= 1, "the corpus lost its success case");
    for class in Class::ALL {
        for member in ["remedy", "next", "detail"] {
            let seen = with
                .get(&(class.token(), member))
                .cloned()
                .unwrap_or_default();
            assert_eq!(
                seen,
                BTreeSet::from([false, true]),
                "{} is recorded both with and without {member}",
                class.token()
            );
        }
    }
}

/// The member order every consumer may rely on: the four head members, then
/// `data` or `error`; inside `error`, the three required members, `retryable`,
/// then whichever of `remedy`, `next`, `detail` is present, in that order.
#[test]
fn every_recorded_envelope_writes_its_members_in_the_contract_order() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let expected = case["expected"]
            .as_object()
            .expect("an envelope is an object");
        let keys: Vec<&str> = expected.keys().map(String::as_str).collect();
        match expected["status"].as_str() {
            Some("ok") => assert_eq!(keys, ["v", "command", "contract", "status", "data"], "{id}"),
            Some("error") => {
                assert_eq!(
                    keys,
                    ["v", "command", "contract", "status", "error"],
                    "{id}"
                );
                let body: &Map<String, Value> = expected["error"].as_object().expect("error body");
                let mut order = vec!["class", "code", "message", "retryable"];
                order.extend(
                    ["remedy", "next", "detail"]
                        .into_iter()
                        .filter(|member| body.contains_key(*member)),
                );
                assert_eq!(
                    body.keys().map(String::as_str).collect::<Vec<_>>(),
                    order,
                    "{id}"
                );
            }
            other => panic!("{id}: {other:?} is not a status"),
        }
    }
}

/// A serializer that sorts map keys and one that keeps insertion order must
/// emit the same bytes for this corpus, so a replay on either side is
/// byte-for-byte: every object under `data` and `detail` is recorded with its
/// keys already sorted.
#[test]
fn every_recorded_data_and_detail_object_is_written_with_sorted_keys() {
    fn sorted(value: &Value, id: &str) {
        match value {
            Value::Object(members) => {
                let keys: Vec<&String> = members.keys().collect();
                let mut expected = keys.clone();
                expected.sort();
                assert_eq!(
                    keys, expected,
                    "{id}: an object is recorded with unsorted keys"
                );
                members.values().for_each(|member| sorted(member, id));
            }
            Value::Array(items) => items.iter().for_each(|item| sorted(item, id)),
            _ => {}
        }
    }
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        if let Some(data) = case["expected"].get("data") {
            sorted(data, id);
        }
        if let Some(detail) = case["expected"]["error"].get("detail") {
            sorted(detail, id);
        }
    }
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
