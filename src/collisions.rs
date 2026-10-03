//! Project collisions — how many there are, and what the operator reads while
//! detection runs.
//!
//! Detection itself is one governed report action over the project's own
//! transformer data; nothing is computed here. What IS decided here is the
//! thing two surfaces could answer differently: WHERE the pair count is read
//! from, in what precedence, and what an operator is told about a run that has
//! just finished.
//!
//! The precedence is five candidates deep because a zero-collision write
//! deliberately leaves `layers` empty — the terminal outcome is then an
//! equal-authority count source, not an error fallback — and because `regions`
//! is the reporter's backwards-compatible twin for `pairs`. A malformed value
//! stays unknown: a string is never coerced into a count the reporter never
//! wrote.
//!
//! Every phrase this module emits is a key with its parameters. The words are
//! the host's; the kernel never carries English.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA: &str = "collisions/1";

/// A backend count is usable only when it is an actual finite number.
pub fn pair_count(value: &Value) -> Option<u64> {
    let number = value.as_f64()?;
    if !number.is_finite() {
        return None;
    }
    Some(number.trunc().max(0.0) as u64)
}

/// Project the collision count from the durable status row, then from the
/// immediate action response while a targeted status refresh catches up.
///
/// The order is fixed and is the contract: `layers.collisions`, the terminal
/// outcome's `pairs`, its `regions` twin, then the same two on the response.
pub fn project_count(status_row: &Value, response: &Value) -> Option<u64> {
    let outcome = &status_row["report_metadata"]["outcomes"]["collision_detection"]["details"];
    for candidate in [
        &status_row["layers"]["collisions"],
        &outcome["pairs"],
        &outcome["regions"],
        &response["pairs"],
        &response["regions"],
    ] {
        if let Some(count) = pair_count(candidate) {
            return Some(count);
        }
    }
    None
}

/// One phrase the host renders: a translation key and the parameters it takes.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Phrase {
    pub key: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

impl Phrase {
    const fn plain(key: &'static str) -> Self {
        Self { key, count: None }
    }
    const fn counted(key: &'static str, count: u64) -> Self {
        Self {
            key,
            count: Some(count),
        }
    }
}

/// Running copy: a truthful count of what the reporter is checking.
///
/// Not a percentage and not a byte count — the action is one request that
/// answers when it answers, and this browser downloads no transformer for it.
pub fn checking(transformer_count: u64) -> Vec<Phrase> {
    vec![Phrase::counted(
        "pctl_collisions_checking",
        transformer_count,
    )]
}

/// Completion copy: whether a run happened, and the pair count only when the
/// targeted status refresh actually produced one.
pub fn completed(cached: bool, pairs: Option<u64>) -> Vec<Phrase> {
    let mut phrases = vec![Phrase::plain(if cached {
        "pctl_collisions_fresh"
    } else {
        "pctl_collisions_finished"
    })];
    if let Some(pairs) = pairs {
        phrases.push(match pairs {
            0 => Phrase::plain("pctl_collisions_pairs_none"),
            1 => Phrase::plain("pctl_collisions_pairs_one"),
            count => Phrase::counted("pctl_collisions_pairs_many", count),
        });
    }
    phrases
}

/// The single key a surface reads a project's collision answer under, outside
/// a run: never checked, checked but unreadable, or a count.
///
/// The same three-way fold `completed` applies to a finished run, asked as a
/// state rather than as copy — so `ds design collisions` and the card name one
/// condition one way instead of each holding its own `match`.
pub fn state_key(checked: bool, pairs: Option<u64>) -> &'static str {
    match (checked, pairs) {
        (false, _) => "pctl_collisions_never_checked",
        (true, None) => "pctl_collisions_unknown",
        (true, Some(0)) => "pctl_collisions_pairs_none",
        (true, Some(1)) => "pctl_collisions_pairs_one",
        (true, Some(_)) => "pctl_collisions_pairs_many",
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    /// Where does this project's pair count come from?
    Count {
        #[serde(default)]
        status_row: Value,
        #[serde(default)]
        response: Value,
    },
    /// What is the operator reading while detection runs?
    ///
    /// The count arrives as the host holds it — the card used to clamp it
    /// before calling, which is a decision and therefore `host_counts`'.
    Checking {
        #[serde(default, deserialize_with = "crate::host_counts::count")]
        transformer_count: u64,
    },
    /// What is the operator told once it answered?
    Completed {
        #[serde(default)]
        cached: bool,
        #[serde(default)]
        status_row: Value,
        #[serde(default)]
        response: Value,
    },
}

#[derive(Serialize)]
struct Reply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pairs: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    phrases: Vec<Phrase>,
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 4 * 1024 * 1024 {
        return Err("collisions_input_too_large".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("collisions_request_invalid: {error}"))?;
    let reply = match request {
        Request::Count {
            status_row,
            response,
        } => Reply {
            pairs: project_count(&status_row, &response),
            phrases: Vec::new(),
        },
        Request::Checking { transformer_count } => Reply {
            pairs: None,
            phrases: checking(transformer_count),
        },
        Request::Completed {
            cached,
            status_row,
            response,
        } => {
            let pairs = project_count(&status_row, &response);
            Reply {
                pairs,
                phrases: completed(cached, pairs),
            }
        }
    };
    serde_json::to_string(&reply).map_err(|_| "collisions_reply_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The precedence, asserted as a ladder: each candidate wins only while
    /// every earlier one is absent, and a disagreement is decided by position.
    #[test]
    fn the_precedence_is_five_candidates_deep() {
        let ladder = [
            json!({"layers": {"collisions": 1}}),
            json!({"report_metadata": {"outcomes": {"collision_detection": {"details": {"pairs": 2}}}}}),
            json!({"report_metadata": {"outcomes": {"collision_detection": {"details": {"regions": 3}}}}}),
        ];
        for (index, row) in ladder.iter().enumerate() {
            assert_eq!(project_count(row, &Value::Null), Some(index as u64 + 1));
        }
        let response = json!({"pairs": 4, "regions": 5});
        assert_eq!(project_count(&Value::Null, &response), Some(4));
        assert_eq!(project_count(&Value::Null, &json!({"regions": 5})), Some(5));
        // Two candidates disagree: the earlier one wins, every time.
        let mut disagreeing = ladder[0].clone();
        disagreeing["report_metadata"] = ladder[1]["report_metadata"].clone();
        assert_eq!(project_count(&disagreeing, &response), Some(1));
    }

    /// A zero-collision write leaves `layers` empty on purpose, so the terminal
    /// outcome answers and the project reads zero rather than unknown.
    #[test]
    fn a_zero_result_is_a_count_not_an_absence() {
        let row = json!({
            "layers": {},
            "report_metadata": {"outcomes": {"collision_detection": {"details": {"pairs": 0}}}}
        });
        assert_eq!(project_count(&row, &Value::Null), Some(0));
    }

    /// Malformed data stays unknown. A string count is the reporter writing
    /// something this module does not understand — never a zero.
    #[test]
    fn a_malformed_count_is_never_coerced() {
        for candidate in [
            json!("7"),
            json!(true),
            json!([7]),
            json!({"pairs": 7}),
            Value::Null,
        ] {
            assert_eq!(pair_count(&candidate), None);
        }
        let row = json!({"layers": {"collisions": "7"}});
        assert_eq!(project_count(&row, &Value::Null), None);
        assert_eq!(pair_count(&json!(-4)), Some(0));
        assert_eq!(pair_count(&json!(2.9)), Some(2));
    }

    #[test]
    fn the_operator_reads_keys_not_numbers_it_was_never_given() {
        assert_eq!(
            checking(3),
            vec![Phrase::counted("pctl_collisions_checking", 3)]
        );
        assert_eq!(
            completed(true, None),
            vec![Phrase::plain("pctl_collisions_fresh")]
        );
        assert_eq!(
            completed(false, Some(0)),
            vec![
                Phrase::plain("pctl_collisions_finished"),
                Phrase::plain("pctl_collisions_pairs_none")
            ]
        );
        assert_eq!(
            completed(false, Some(1))[1],
            Phrase::plain("pctl_collisions_pairs_one")
        );
        assert_eq!(
            completed(false, Some(9))[1],
            Phrase::counted("pctl_collisions_pairs_many", 9)
        );
    }

    /// A project that was never checked is not a project whose count could not
    /// be read, and neither is a project with zero pairs. One vocabulary, so a
    /// headless reader and the card cannot disagree about which it is.
    #[test]
    fn the_state_key_separates_never_checked_from_unknown_and_from_zero() {
        assert_eq!(state_key(false, None), "pctl_collisions_never_checked");
        assert_eq!(state_key(false, Some(3)), "pctl_collisions_never_checked");
        assert_eq!(state_key(true, None), "pctl_collisions_unknown");
        assert_eq!(state_key(true, Some(0)), "pctl_collisions_pairs_none");
        assert_eq!(state_key(true, Some(1)), "pctl_collisions_pairs_one");
        assert_eq!(state_key(true, Some(7)), "pctl_collisions_pairs_many");
        // The counted phrases are the same keys `completed` names, so the two
        // surfaces cannot drift apart.
        assert_eq!(completed(false, Some(0))[1].key, state_key(true, Some(0)));
        assert_eq!(completed(false, Some(1))[1].key, state_key(true, Some(1)));
        assert_eq!(completed(false, Some(7))[1].key, state_key(true, Some(7)));
    }

    #[test]
    fn evaluate_answers_all_three_questions() {
        let reply =
            evaluate(br#"{"op":"count","status_row":{"layers":{"collisions":2}}}"#).unwrap();
        assert_eq!(reply, r#"{"pairs":2}"#);
        let reply = evaluate(br#"{"op":"checking","transformer_count":12}"#).unwrap();
        assert!(reply.contains("pctl_collisions_checking"));
        assert!(reply.contains(r#""count":12"#));
        // The card used to clamp this on its way in; `host_counts` decides now,
        // so a count that is not one still reads as the phrase for none.
        for malformed in [
            br#"{"op":"checking","transformer_count":-1}"#.as_slice(),
            br#"{"op":"checking","transformer_count":0.5}"#.as_slice(),
            br#"{"op":"checking","transformer_count":null}"#.as_slice(),
            br#"{"op":"checking"}"#.as_slice(),
        ] {
            assert!(evaluate(malformed).unwrap().contains(r#""count":0"#));
        }
        let reply = evaluate(
            br#"{"op":"completed","cached":true,"status_row":{"layers":{"collisions":1}}}"#,
        )
        .unwrap();
        assert!(
            reply.contains("pctl_collisions_fresh") && reply.contains("pctl_collisions_pairs_one")
        );
        assert!(evaluate(br#"{"op":"detect"}"#).is_err());
    }
}
