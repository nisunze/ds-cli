//! Which transformers may be printed, in what order, and which of them this
//! device holds — clean or dirty — decided once for the paired application and
//! for `ds`.
//!
//! The browser joined two inventories by hand (`prepare.ts`): the project's
//! status rows and this device's cached rooms, fenced to the signed-in
//! account. The join, the kind filter, the order, the version fallback chain
//! and the bound are this module's. It performs no IO: the host reports the
//! rows, the rooms and the account.

use crate::design::{canonical_row_name, is_special_row};
use crate::design_status_query::locale_cmp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;

/// One room this device holds, as its inventory reports it.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub name: String,
    /// The account that wrote the room; empty or absent = unowned.
    #[serde(default)]
    pub owner_email: Option<String>,
    #[serde(default)]
    pub local_dirty: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct PrintableRow {
    pub name: String,
    pub kind: String,
    /// This device holds a room for it — clean or dirty.
    pub cached: bool,
    pub dirty: bool,
    /// `metadata.version`, else the client's base server version, else 0 —
    /// read as JavaScript's `Number` read it (`null` for what it could not).
    pub server_version: Value,
    pub customers: Option<u64>,
    pub poles: Option<u64>,
    /// Eligibility from the held server receipt only. Rendering still reads
    /// the exact saved JSON under all three pins and validates current inputs.
    pub voltage_drop: VoltageDropPreview,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct VoltageDropPreview {
    pub state: String,
    /// The saved analysis is a calculated power flow with a compliance
    /// verdict, so the A4 voltage-drop report states that verdict. Incomplete,
    /// refused and reserved analyses only print that no verdict is issued.
    pub calculated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_confirmed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// The engine's own words for an analysis that is incomplete or was not
    /// calculated: its issues, or the calculation gate's reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct CountCase {
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct PreviewCases {
    /// Cases describe only the returned inventory; `more.omitted` remains
    /// explicit when the caller requested a partial inventory.
    pub smallest_customers: Option<CountCase>,
    pub largest_customers: Option<CountCase>,
    pub smallest_poles: Option<CountCase>,
    pub largest_poles: Option<CountCase>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct More {
    pub omitted: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct Inventory {
    pub total: usize,
    pub transformers: Vec<PrintableRow>,
    pub more: More,
    pub preview_cases: PreviewCases,
}

fn digest(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| {
        text.len() == 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// What a transformer's held voltage-drop analysis is, from its saved receipt
/// and whether this device holds newer unsaved work. Printing and the status
/// register both read this one classification.
pub(crate) fn voltage_drop(row: &Map<String, Value>, dirty: bool) -> VoltageDropPreview {
    let unavailable = |state: &str| VoltageDropPreview {
        state: state.into(),
        calculated: false,
        rule_confirmed: None,
        verdict: None,
        reason: None,
        analysis_sha256: None,
        content_digest: None,
        version: None,
    };
    if dirty {
        return unavailable("local_changes");
    }
    let Some(receipt) = row.get("voltage_drop_metadata").and_then(Value::as_object) else {
        return unavailable("missing");
    };
    match receipt.get("state").and_then(Value::as_str) {
        Some("missing") => return unavailable("missing"),
        Some("stale") => return unavailable("stale"),
        Some("ready") => {}
        _ => return unavailable("invalid_receipt"),
    }
    let metadata = row.get("metadata").unwrap_or(&Value::Null);
    let Some(version) = metadata["version"].as_u64().filter(|v| *v > 0) else {
        return unavailable("invalid_receipt");
    };
    let Some(content_digest) = digest(&metadata["content_digest"]) else {
        return unavailable("invalid_receipt");
    };
    let value = Value::Object(receipt.clone());
    let summary = &value["summary"];
    let status = value["status"].as_str().unwrap_or("");
    let verdict = match status {
        "reserved" => Some("reserved"),
        "refused" => Some("refused"),
        "incomplete" => Some("incomplete"),
        "calculated" => summary["verdict"].as_str().filter(|v| {
            matches!(
                *v,
                "complies" | "complies_with_reinforcement" | "does_not_comply" | "incomplete"
            )
        }),
        _ => None,
    };
    let valid = value["schema"] == "ds.lv-voltage-drop.analysis/v1"
        && value["method"] == "ds-lv-vd/1"
        && value["content_digest"] == content_digest
        && [
            "analysis_sha256",
            "inputs_digest",
            "analysis_inputs_digest",
            "processed_digest",
        ]
        .iter()
        .all(|key| digest(&value[*key]).is_some())
        && value["json_bytes"]
            .as_u64()
            .is_some_and(|bytes| (1..=32 * 1024 * 1024).contains(&bytes))
        && summary["method"] == value["method"]
        && summary["status"]
            == if status == "refused" {
                "not_calculated"
            } else {
                status
            }
        && summary.get("transformer_nature").is_some()
        && summary["transformer_nature"] == value["nature"]
        && verdict.is_some();
    if !valid {
        return unavailable("invalid_receipt");
    }
    let calculated = matches!(
        verdict,
        Some("complies" | "complies_with_reinforcement" | "does_not_comply")
    );
    let reason = if status == "reserved" {
        summary["calculation"].as_str().map(str::to_string)
    } else if calculated {
        None
    } else {
        let issues = summary["issues"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        (!issues.is_empty()).then(|| issues.join("; "))
    };
    VoltageDropPreview {
        state: "saved_json".into(),
        calculated,
        rule_confirmed: summary["rule_confirmed"].as_bool(),
        verdict: verdict.map(str::to_string),
        reason: reason.map(|text| crate::design_health::sanitize_message(&text)),
        analysis_sha256: value["analysis_sha256"].as_str().map(str::to_string),
        content_digest: Some(content_digest.into()),
        version: Some(version),
    }
}

fn count_case(rows: &[PrintableRow], poles: bool, largest: bool) -> Option<CountCase> {
    let mut candidates = rows
        .iter()
        .filter_map(|row| {
            (if poles { row.poles } else { row.customers }).map(|count| CountCase {
                name: row.name.clone(),
                count,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| {
        let count = if largest {
            b.count.cmp(&a.count)
        } else {
            a.count.cmp(&b.count)
        };
        count.then_with(|| locale_cmp(&a.name, &b.name))
    });
    candidates.into_iter().next()
}

/// Numeric version coercion for JSON status values, matching Number().
fn js_number(value: &Value) -> Value {
    let number = match value {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                if let Some(digits) = trimmed
                    .strip_prefix("0x")
                    .or_else(|| trimmed.strip_prefix("0X"))
                {
                    u64::from_str_radix(digits, 16)
                        .map(|n| n as f64)
                        .unwrap_or(f64::NAN)
                } else if let Some(digits) = trimmed
                    .strip_prefix("0b")
                    .or_else(|| trimmed.strip_prefix("0B"))
                {
                    u64::from_str_radix(digits, 2)
                        .map(|n| n as f64)
                        .unwrap_or(f64::NAN)
                } else if let Some(digits) = trimmed
                    .strip_prefix("0o")
                    .or_else(|| trimmed.strip_prefix("0O"))
                {
                    u64::from_str_radix(digits, 8)
                        .map(|n| n as f64)
                        .unwrap_or(f64::NAN)
                } else {
                    trimmed.parse::<f64>().unwrap_or(f64::NAN)
                }
            }
        }
        Value::Array(values) => {
            fn text(value: &Value) -> String {
                match value {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    Value::Array(values) => values.iter().map(text).collect::<Vec<_>>().join(","),
                    Value::Object(_) => "[object Object]".into(),
                    other => other.to_string(),
                }
            }
            return js_number(&Value::String(
                values.iter().map(text).collect::<Vec<_>>().join(","),
            ));
        }
        Value::Null => 0.0,
        _ => f64::NAN,
    };
    Number::from_f64(number)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// `a ?? b`: the first value that is neither absent nor null.
fn first_present(candidates: [Option<&Value>; 2]) -> Option<&Value> {
    candidates
        .into_iter()
        .flatten()
        .find(|value| !value.is_null())
}

fn printable_kind(row: &Map<String, Value>) -> Option<String> {
    let name = row.get("name").and_then(Value::as_str)?;
    if canonical_row_name(name).is_empty() || is_special_row(name) {
        return None;
    }
    match row.get("kind") {
        // Absent means an ordinary transformer; `null` never did.
        None => Some("individual".to_string()),
        Some(Value::String(kind)) if kind == "individual" => Some(kind.clone()),
        Some(_) => None,
    }
}

/// The printable inventory: individual transformers in name order, each with
/// whether this device holds it, bounded to `limit` rows.
pub fn printable(
    rows: &[Value],
    rooms: &[Room],
    current_user: &str,
    limit: usize,
) -> Result<Inventory, String> {
    if !(1..=500).contains(&limit) {
        return Err("printing.transformers accepts limit from 1 through 500.".into());
    }
    let current = current_user.trim().to_lowercase();
    // A room is this account's when it is unowned, or owned by the signed-in
    // account — never when no account is known. A later room for the same
    // canonical name replaces an earlier one, as the browser's Map did.
    let mut local: HashMap<String, &Room> = HashMap::new();
    for room in rooms {
        let owner = room
            .owner_email
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if owner.is_empty() || (!current.is_empty() && owner == current) {
            local.insert(canonical_row_name(&room.name), room);
        }
    }
    let mut all: Vec<(&Map<String, Value>, String, String)> = rows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|row| {
            let kind = printable_kind(row)?;
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some((row, name, kind))
        })
        .collect();
    all.sort_by(|left, right| locale_cmp(&left.1, &right.1));
    let total = all.len();
    let transformers: Vec<PrintableRow> = all
        .iter()
        .take(limit)
        .map(|(row, name, kind)| {
            let held = local.get(&canonical_row_name(name)).copied();
            let version = first_present([
                row.get("metadata")
                    .and_then(Value::as_object)
                    .and_then(|metadata| metadata.get("version")),
                row.get("client_base_server_version"),
            ])
            .map(js_number)
            .unwrap_or_else(|| Value::Number(0.into()));
            PrintableRow {
                name: name.clone(),
                kind: kind.clone(),
                cached: held.is_some(),
                dirty: held.is_some_and(|room| room.local_dirty),
                server_version: version,
                customers: row
                    .get("layers")
                    .and_then(|layers| layers["customers"].as_u64()),
                poles: row
                    .get("layers")
                    .and_then(|layers| layers["lv_poles"].as_u64()),
                voltage_drop: voltage_drop(row, held.is_some_and(|room| room.local_dirty)),
            }
        })
        .collect();
    let omitted = total - transformers.len();
    let preview_cases = PreviewCases {
        smallest_customers: count_case(&transformers, false, false),
        largest_customers: count_case(&transformers, false, true),
        smallest_poles: count_case(&transformers, true, false),
        largest_poles: count_case(&transformers, true, true),
    };
    Ok(Inventory {
        total,
        transformers,
        more: More { omitted },
        preview_cases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn room(name: &str, owner: Option<&str>, dirty: bool) -> Room {
        Room {
            name: name.into(),
            owner_email: owner.map(str::to_string),
            local_dirty: dirty,
        }
    }

    fn rows() -> Vec<Value> {
        vec![
            json!({"name": "tx_b", "kind": "individual", "client_cached": false}),
            json!({"name": "tx_a", "kind": "individual", "client_cached": true, "metadata": {"version": 4}}),
            json!({"name": "combined_transformer", "kind": "combined"}),
            json!({"name": "tx_c", "client_base_server_version": "7"}),
            json!({"name": "mv_data", "kind": null}),
            json!({"name": "TX_D", "kind": "individual", "metadata": {"version": null}, "client_base_server_version": "seven"}),
        ]
    }

    fn saved(verdict: &str) -> Value {
        json!({"name":"tx_a","metadata":{"version":7,"content_digest":"a".repeat(64)},
            "layers":{"customers":12,"lv_poles":6},
            "voltage_drop_metadata":{"state":"ready","schema":"ds.lv-voltage-drop.analysis/v1",
                "method":"ds-lv-vd/1","status":"calculated","nature":"new",
                "content_digest":"a".repeat(64),"analysis_sha256":"b".repeat(64),
                "inputs_digest":"c".repeat(64),"analysis_inputs_digest":"d".repeat(64),
                "processed_digest":"e".repeat(64),"json_bytes":1024,
                "summary":{"method":"ds-lv-vd/1","status":"calculated",
                    "transformer_nature":"new","verdict":verdict}}})
    }

    #[test]
    fn saved_json_candidates_keep_exact_head_pins_and_producer_verdict() {
        for verdict in [
            "complies",
            "complies_with_reinforcement",
            "does_not_comply",
            "incomplete",
        ] {
            let result = printable(&[saved(verdict)], &[], "", 500).unwrap();
            let preview = &result.transformers[0].voltage_drop;
            assert_eq!(preview.state, "saved_json");
            assert_eq!(preview.verdict.as_deref(), Some(verdict));
            assert_eq!(preview.analysis_sha256, Some("b".repeat(64)));
            assert_eq!(preview.content_digest, Some("a".repeat(64)));
            assert_eq!(preview.version, Some(7));
            assert_eq!(preview.calculated, verdict != "incomplete", "{verdict}");
        }
    }

    /// Only a calculated verdict makes an A4 voltage-drop report worth
    /// previewing; incomplete, refused and reserved analyses print a stub.
    #[test]
    fn only_a_saved_calculated_verdict_is_a_calculated_voltage_drop() {
        for status in ["incomplete", "refused", "reserved"] {
            let mut row = saved("incomplete");
            row["voltage_drop_metadata"]["status"] = json!(status);
            row["voltage_drop_metadata"]["summary"]["status"] = json!(if status == "refused" {
                "not_calculated"
            } else {
                status
            });
            let preview = &printable(&[row], &[], "", 500).unwrap().transformers[0].voltage_drop;
            assert_eq!(preview.state, "saved_json", "{status}");
            assert!(!preview.calculated, "{status}");
        }
        let dirty = printable(&[saved("complies")], &[room("tx_a", None, true)], "", 500).unwrap();
        assert!(!dirty.transformers[0].voltage_drop.calculated);
        let missing = printable(&rows(), &[], "", 500).unwrap();
        assert!(!missing.transformers[0].voltage_drop.calculated);
    }

    /// An analysis that is incomplete or was not calculated says why, in the
    /// engine's own words; a calculated verdict needs no reason.
    #[test]
    fn a_saved_analysis_without_a_verdict_carries_the_engine_reason() {
        for (status, summary_status, key, value, reason) in [
            (
                "refused",
                "not_calculated",
                "issues",
                json!(["the network has no transformer node to start from"]),
                Some("the network has no transformer node to start from"),
            ),
            (
                "incomplete",
                "incomplete",
                "issues",
                json!([
                    "no transfo_sizes row with electrical_params for 0 kVA",
                    "one more"
                ]),
                Some("no transfo_sizes row with electrical_params for 0 kVA; one more"),
            ),
            ("incomplete", "incomplete", "issues", json!([]), None),
            (
                "reserved",
                "reserved",
                "calculation",
                json!("method default: a fill_in transformer is not calculated"),
                Some("method default: a fill_in transformer is not calculated"),
            ),
        ] {
            let mut row = saved("incomplete");
            row["voltage_drop_metadata"]["status"] = json!(status);
            row["voltage_drop_metadata"]["summary"]["status"] = json!(summary_status);
            row["voltage_drop_metadata"]["summary"][key] = value;
            let preview = &printable(&[row], &[], "", 500).unwrap().transformers[0].voltage_drop;
            assert_eq!(preview.reason.as_deref(), reason, "{status}");
        }
        let mut calculated = saved("complies");
        calculated["voltage_drop_metadata"]["summary"]["issues"] = json!(["ignored"]);
        let preview = &printable(&[calculated], &[], "", 500).unwrap().transformers[0].voltage_drop;
        assert_eq!(preview.reason, None);
    }

    #[test]
    fn wrong_head_malformed_receipt_and_dirty_rooms_cannot_offer_a4_json() {
        for (path, value) in [
            ("/metadata/content_digest", json!("f".repeat(64))),
            ("/metadata/version", json!("7")),
            (
                "/voltage_drop_metadata/analysis_sha256",
                json!("B".repeat(64)),
            ),
            ("/voltage_drop_metadata/processed_digest", Value::Null),
            (
                "/voltage_drop_metadata/summary/transformer_nature",
                json!("upgrade"),
            ),
            ("/voltage_drop_metadata/json_bytes", json!(0)),
            ("/voltage_drop_metadata/summary/verdict", json!("success")),
        ] {
            let mut row = saved("complies");
            *row.pointer_mut(path).unwrap() = value;
            let result = printable(&[row], &[], "", 500).unwrap();
            assert_eq!(
                result.transformers[0].voltage_drop.state, "invalid_receipt",
                "{path}"
            );
            assert!(result.transformers[0]
                .voltage_drop
                .analysis_sha256
                .is_none());
        }
        let result = printable(&[saved("complies")], &[room("tx_a", None, true)], "", 500).unwrap();
        assert_eq!(result.transformers[0].voltage_drop.state, "local_changes");
        let mut row = saved("complies");
        row["voltage_drop_metadata"]["state"] = json!("stale");
        assert_eq!(
            printable(&[row], &[], "", 500).unwrap().transformers[0]
                .voltage_drop
                .state,
            "stale"
        );
    }

    #[test]
    fn outliers_use_only_returned_authoritative_counts_with_stable_ties() {
        let result = printable(
            &[
                json!({"name":"b","layers":{"customers":10,"lv_poles":6}}),
                json!({"name":"a","layers":{"customers":10,"lv_poles":3}}),
                json!({"name":"c","layers":{"customers":900,"lv_poles":100}}),
                json!({"name":"d","layers":{"customers":"1","lv_poles":-1}}),
            ],
            &[],
            "",
            2,
        )
        .unwrap();
        assert_eq!(
            result
                .preview_cases
                .smallest_customers
                .as_ref()
                .unwrap()
                .name,
            "a"
        );
        assert_eq!(
            result
                .preview_cases
                .largest_customers
                .as_ref()
                .unwrap()
                .name,
            "a"
        );
        assert_eq!(
            result.preview_cases.largest_poles.as_ref().unwrap().name,
            "b"
        );
        assert_eq!(result.more.omitted, 2);
        let missing = printable(&rows(), &[], "", 500).unwrap();
        assert!(missing.preview_cases.smallest_customers.is_none());
        assert_eq!(missing.transformers[0].voltage_drop.state, "missing");
    }

    #[test]
    fn joins_the_fleet_with_this_account_rooms_in_name_order_bounded() {
        let out = printable(
            &rows(),
            &[
                room("tx_a", Some("Owner@Example.com"), false),
                room("TX_D", None, true),
            ],
            "owner@example.com",
            2,
        )
        .unwrap();
        assert_eq!(out.total, 4);
        assert_eq!(out.more.omitted, 2);
        assert_eq!(out.transformers[0].name, "tx_a");
        assert!(out.transformers[0].cached && !out.transformers[0].dirty);
        assert_eq!(out.transformers[0].server_version, json!(4.0));
        assert_eq!(out.transformers[1].name, "tx_b");
        assert!(!out.transformers[1].cached);
        assert_eq!(out.transformers[1].server_version, json!(0));
        let all = printable(&rows(), &[room("TX_D", None, true)], "", 500).unwrap();
        let names: Vec<&str> = all
            .transformers
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(names, ["tx_a", "tx_b", "tx_c", "TX_D"]);
        assert_eq!(all.transformers[2].server_version, json!(7.0));
        // An unreadable version is `null`, never a guessed number.
        assert_eq!(all.transformers[3].server_version, Value::Null);
        assert!(all.transformers[3].cached && all.transformers[3].dirty);
    }

    #[test]
    fn another_account_room_and_an_unknown_account_never_count_as_this_device() {
        let other = printable(
            &rows(),
            &[room("tx_a", Some("other@example.com"), true)],
            "owner@example.com",
            1,
        )
        .unwrap();
        assert!(!other.transformers[0].cached && !other.transformers[0].dirty);
        let unknown = printable(
            &rows(),
            &[room("tx_a", Some("owner@example.com"), false)],
            "",
            1,
        )
        .unwrap();
        assert!(!unknown.transformers[0].cached);
    }

    #[test]
    fn the_bound_is_named() {
        assert!(printable(&[], &[], "", 0).is_err());
        assert!(printable(&[], &[], "", 501).is_err());
        assert_eq!(printable(&[], &[], "", 1).unwrap().total, 0);
    }
}
