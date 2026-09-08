//! Offline report command decisions. Hosts own IO; export never requires sync.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEVELOPMENT_ENGINE: &str = "ds-network-reporter@0.1.0+development";
pub const LOCAL_ONLY_SCHEMA: u32 = 4;

fn valid_semver_core(value: &str) -> bool {
    let mut parts = value.split('.');
    (0..3).all(|_| {
        parts.next().is_some_and(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        })
    }) && parts.next().is_none()
}

/// Exact immutable Network Reporter identity: package semantic version plus
/// the producing source commit. Package upgrades must not strand retained
/// publications merely because they no longer use the current version.
pub fn valid_network_reporter_engine_identity(engine: &str) -> bool {
    let Some(identity) = engine.strip_prefix("ds-network-reporter@") else {
        return false;
    };
    let Some((version, source)) = identity.split_once('+') else {
        return false;
    };
    valid_semver_core(version)
        && source.len() == 40
        && source
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PublicationState {
    LocalOnly,
    Composed,
    Pending,
    Queued,
}

pub fn publication_state(
    engine: &str,
    enqueue: bool,
    queued: bool,
) -> Result<PublicationState, String> {
    if engine == DEVELOPMENT_ENGINE {
        return Ok(PublicationState::LocalOnly);
    }
    if !valid_network_reporter_engine_identity(engine) {
        return Err("Report engine identity is invalid".into());
    }
    Ok(if !enqueue {
        PublicationState::Composed
    } else if queued {
        PublicationState::Queued
    } else {
        PublicationState::Pending
    })
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    FormatOptions {
        #[serde(default)]
        setups: Vec<serde_json::Value>,
    },
    Formats {
        sheets: serde_json::Value,
        combined: bool,
    },
    Publication {
        engine: String,
        enqueue: bool,
        queued: bool,
    },
    Plan {
        requested: Option<Vec<String>>,
        inventory: Option<Vec<String>>,
        rooms: Vec<Room>,
        processors: usize,
        resident_limit: usize,
    },
    Archive {
        transformers: Vec<String>,
        purpose: String,
        lifecycle: Option<String>,
        groups: Vec<Group>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Room {
    name: String,
    available: bool,
    dirty: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Group {
    group_key: String,
    transformers: Vec<String>,
}
/// Names a report *produces*: the collision document and the combined set
/// under any of its spellings. They are output, never a participant, so no
/// lane may take one as a requested transformer. Canonicalized the way
/// `project_control` canonicalizes a status row, so an alias or a cased
/// spelling cannot smuggle one in.
pub fn is_reserved_report_identity(name: &str) -> bool {
    let canonical = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    matches!(
        canonical.as_str(),
        "collisions" | "combined_transformer" | "all_transformers" | "combined_transformers"
    )
}
fn names(values: Vec<String>) -> Result<BTreeSet<String>, String> {
    if values.len() > 2000 {
        return Err("Report scope exceeds 2000 rooms".into());
    }
    let mut result = BTreeSet::new();
    for name in values {
        if name.is_empty()
            || name.len() > 121
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || is_reserved_report_identity(&name)
        {
            return Err(format!("Invalid report transformer: {name}"));
        }
        result.insert(name);
    }
    Ok(result)
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 64 * 1024 * 1024 {
        return Err("Report command exceeds 64 MiB".into());
    }
    let command: Command = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let result = match command {
        Command::FormatOptions { setups } => {
            serde_json::json!({"options": crate::report_formats::options(&setups)?})
        }
        Command::Formats { sheets, combined } => {
            serde_json::json!({"outputs": crate::report_formats::plan(&sheets, combined)?})
        }
        Command::Publication {
            engine,
            enqueue,
            queued,
        } => serde_json::json!({"state": publication_state(&engine, enqueue, queued)?}),
        Command::Plan {
            requested,
            inventory,
            rooms,
            processors,
            resident_limit,
        } => {
            if processors == 0 || resident_limit == 0 || resident_limit > 64 {
                return Err("Invalid report concurrency budget".into());
            }
            if rooms.len() > 2000 {
                return Err("Report room inventory exceeds 2000".into());
            }
            let scope = match requested {
                Some(values) if !values.is_empty() => names(values)?,
                _ => {
                    let mut scope = names(inventory.ok_or("Prepare the complete project inventory before an all-project offline report")?)?;
                    scope.extend(names(
                        rooms
                            .iter()
                            .filter(|r| r.dirty)
                            .map(|r| r.name.clone())
                            .collect(),
                    )?);
                    scope
                }
            };
            let available: BTreeSet<_> = rooms
                .iter()
                .filter(|r| r.available)
                .map(|r| r.name.as_str())
                .collect();
            let missing: Vec<_> = scope
                .iter()
                .filter(|n| !available.contains(n.as_str()))
                .cloned()
                .collect();
            let concurrency = scope.len().min(processors).min(resident_limit);
            serde_json::json!({"names":scope,"missing":missing,"concurrency":concurrency})
        }
        Command::Archive {
            transformers,
            purpose,
            lifecycle,
            groups,
        } => {
            if purpose != "report_archive" || lifecycle.as_deref() == Some("archived") {
                return Err(
                    "Apply one active report_archive consumer grouping before compounding reports"
                        .into(),
                );
            }
            let expected = names(transformers)?;
            let mut seen = BTreeSet::new();
            let mut output = Vec::new();
            for (index, group) in groups.into_iter().enumerate() {
                for name in &group.transformers {
                    if !expected.contains(name) || !seen.insert(name.clone()) {
                        return Err("The report_archive grouping must cover each compounded transformer exactly once".into());
                    }
                }
                output.push(serde_json::json!({"folder":format!("groups/group-{:03}",index+1),"group_key":group.group_key,"transformers":group.transformers}));
            }
            if seen != expected {
                return Err(
                    "The report_archive grouping is stale for the compounded transformer inventory"
                        .into(),
                );
            }
            serde_json::json!({"groups":output})
        }
    };
    serde_json::to_string(&serde_json::json!({"schema":"ds.report.command/v1","result":result}))
        .map_err(|e| e.to_string())
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = reportCommand)]
pub fn report_command_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_and_publication_are_separate() {
        let release = format!("ds-network-reporter@0.1.0+{}", "a".repeat(40));
        assert_eq!(
            publication_state(&release, true, false).unwrap(),
            PublicationState::Pending
        );
        assert_eq!(
            publication_state(&release, true, true).unwrap(),
            PublicationState::Queued
        );
        assert_eq!(
            publication_state(&release, false, false).unwrap(),
            PublicationState::Composed
        );
        assert_eq!(
            publication_state(DEVELOPMENT_ENGINE, true, true).unwrap(),
            PublicationState::LocalOnly
        );
        assert_eq!(
            publication_state(
                &format!("ds-network-reporter@0.2.1+{}", "b".repeat(40)),
                true,
                false,
            )
            .unwrap(),
            PublicationState::Pending,
        );
        assert!(publication_state("unknown", true, false).is_err());
        assert!(publication_state(
            &format!("ds-network-reporter@01.2.1+{}", "b".repeat(40)),
            true,
            false,
        )
        .is_err());
    }
    #[test]
    fn computed_identities_are_reserved_under_any_spelling() {
        assert!(is_reserved_report_identity("Collisions"));
        assert!(is_reserved_report_identity("all_transformers"));
        assert!(is_reserved_report_identity(" Combined-Transformers "));
        assert!(!is_reserved_report_identity("tx_a"));
        let input = serde_json::json!({"command":"plan","requested":["collisions"],"inventory":null,"rooms":[],"processors":32,"resident_limit":4});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
    }
    #[test]
    fn offline_inventory_never_silently_shrinks() {
        let input = serde_json::json!({"command":"plan","requested":null,"inventory":["tx_a","tx_b"],"rooms":[{"name":"tx_a","available":true,"dirty":false},{"name":"tx_c","available":true,"dirty":true}],"processors":32,"resident_limit":4});
        let result: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(
            result["result"]["names"],
            serde_json::json!(["tx_a", "tx_b", "tx_c"])
        );
        assert_eq!(result["result"]["missing"], serde_json::json!(["tx_b"]));
        assert_eq!(result["result"]["concurrency"], 3);
        let mut absent = input;
        absent["inventory"] = serde_json::Value::Null;
        assert!(evaluate(&serde_json::to_vec(&absent).unwrap()).is_err());
        absent["requested"] = serde_json::json!(["tx_a"]);
        assert!(evaluate(&serde_json::to_vec(&absent).unwrap()).is_ok());
    }
}
