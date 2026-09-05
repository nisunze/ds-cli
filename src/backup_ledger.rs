//! Metadata-only recovery commands. Archive bytes are fetched only from an
//! explicit download plan; neither projection nor selection performs I/O.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Command {
    Project {
        rows: Vec<Row>,
    },
    Download {
        rows: Vec<Row>,
        selected_ids: Vec<String>,
    },
}
#[derive(Deserialize)]
pub struct Row {
    ds_project: Option<String>,
    transformer_name: Option<String>,
    event_epoch_seconds: Option<i64>,
    deleted_at: Option<String>,
    batch_id: Option<String>,
    backup_json_uri: Option<String>,
    backup_layer_uri: Option<String>,
    source_activity_json: Option<String>,
}
#[derive(Serialize, Clone)]
pub struct Archive {
    pub uri: String,
    pub filename: String,
    pub format: &'static str,
}
#[derive(Deserialize, Serialize, Clone)]
pub struct Activity {
    pub action: String,
    pub actor: Option<String>,
    pub at: String,
    pub at_ms: i64,
    pub status: Option<String>,
}
#[derive(Serialize)]
pub struct Projection {
    pub id: String,
    pub archive: Option<Archive>,
    pub previous_activity: Option<Activity>,
}
fn archive(raw: Option<&str>) -> Option<Archive> {
    let raw = raw?.trim();
    if raw.contains("..") {
        return None;
    }
    let uri = url::Url::parse(raw).ok()?;
    if uri.scheme() != "gs"
        || uri.host_str().is_none()
        || uri.query().is_some()
        || uri.fragment().is_some()
        || !uri.path().starts_with("/transformers/")
    {
        return None;
    }
    let filename = uri.path_segments()?.next_back()?.to_owned();
    let format = if filename.ends_with(".dsgrid") {
        "dsgrid"
    } else if filename.ends_with(".firestore.json") {
        "json"
    } else {
        return None;
    };
    Some(Archive {
        uri: raw.into(),
        filename,
        format,
    })
}
fn project(row: Row) -> Result<Projection, String> {
    let id = serde_json::to_string(&(
        &row.ds_project,
        &row.transformer_name,
        row.event_epoch_seconds,
        &row.deleted_at,
        &row.batch_id,
        &row.backup_layer_uri,
        &row.backup_json_uri,
    ))
    .map_err(|e| e.to_string())?;
    let events: Vec<Activity> = row
        .source_activity_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    // All timestamps are serialized by the archive writer from the same source
    // stamp. Never attach the deletion actor to a different historical event.
    let previous_activity = events
        .into_iter()
        .take(16)
        .filter(|e| e.at_ms > 0 && !e.at.is_empty() && e.status.as_deref() != Some("stale"))
        .filter(|e| {
            matches!(
                e.action.as_str(),
                "document_update"
                    | "create"
                    | "upload"
                    | "sketch"
                    | "draft"
                    | "process"
                    | "report"
                    | "retire"
                    | "restore"
            )
        })
        .max_by_key(|e| (e.at_ms, e.action != "document_update"));
    Ok(Projection {
        id,
        archive: archive(row.backup_layer_uri.as_deref())
            .or_else(|| archive(row.backup_json_uri.as_deref())),
        previous_activity,
    })
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 16 * 1024 * 1024 {
        return Err("Backup ledger exceeds 16 MiB metadata bound".into());
    }
    let command: Command = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let (rows, selection) = match command {
        Command::Project { rows } => (rows, None),
        Command::Download { rows, selected_ids } => (rows, Some(selected_ids)),
    };
    if rows.len() > 10_000 {
        return Err("Backup ledger exceeds 10000 rows".into());
    }
    let projected = rows
        .into_iter()
        .map(project)
        .collect::<Result<Vec<_>, _>>()?;
    let output = if let Some(ids) = selection {
        let ids: HashSet<_> = ids.into_iter().collect();
        if ids.is_empty() || ids.len() > 100 {
            return Err("Select between 1 and 100 backups per download".into());
        }
        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        for row in &projected {
            if ids.contains(&row.id) && seen.insert(&row.id) {
                targets.push(
                    row.archive
                        .clone()
                        .ok_or("Selected row has no downloadable archive")?,
                );
            }
        }
        if targets.len() != ids.len() {
            return Err("Selected backup is no longer in the ledger".into());
        }
        serde_json::json!({"targets":targets,"max_parallel":4})
    } else {
        serde_json::json!({"rows":projected})
    };
    serde_json::to_string(&output).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn run(value: Value) -> Value {
        serde_json::from_str(&evaluate(value.to_string().as_bytes()).unwrap()).unwrap()
    }
    #[test]
    fn native_archive_is_selectable_and_legacy_json_remains_downloadable() {
        let rows = json!([
            {"backup_json_uri":null,"backup_layer_uri":"gs://backup/transformers/a.dsgrid"},
            {"backup_json_uri":"gs://backup/transformers/b.firestore.json"},
            {"backup_json_uri":null,"backup_layer_uri":null}
        ]);
        let view = run(json!({"action":"project","rows":rows}));
        assert_eq!(view["rows"][0]["archive"]["format"], "dsgrid");
        assert_eq!(view["rows"][1]["archive"]["format"], "json");
        assert!(view["rows"][2]["archive"].is_null());
        let plan = run(
            json!({"action":"download","rows":rows,"selected_ids":[view["rows"][0]["id"],view["rows"][1]["id"]]}),
        );
        assert_eq!(plan["targets"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn missing_archive_and_stale_selection_are_refused() {
        let rows = json!([{}]);
        let view = run(json!({"action":"project","rows":rows}));
        for id in [view["rows"][0]["id"].clone(), json!("unknown")] {
            assert!(evaluate(
                json!({"action":"download","rows":rows,"selected_ids":[id]})
                    .to_string()
                    .as_bytes()
            )
            .is_err());
        }
        assert!(archive(Some("https://example.com/transformers/a.dsgrid")).is_none());
    }
    #[test]
    fn forensic_actor_stays_with_its_event_and_stale_report_is_not_activity() {
        let events = json!([
            {"action":"process","actor":"engineer","at":"earlier","at_ms":1000,"status":"ok"},
            {"action":"report","actor":"deleter","at":"later","at_ms":3000,"status":"stale"},
            {"action":"document_update","actor":null,"at":"last","at_ms":2000}
        ]);
        let result =
            run(json!({"action":"project","rows":[{"source_activity_json":events.to_string()}]}));
        assert_eq!(
            result["rows"][0]["previous_activity"]["action"],
            "document_update"
        );
        assert!(result["rows"][0]["previous_activity"]["actor"].is_null());
    }
}
