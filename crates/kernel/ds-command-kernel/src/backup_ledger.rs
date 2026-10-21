//! Metadata-only recovery commands. Archive bytes are fetched only from an
//! explicit download plan; neither projection nor selection performs I/O.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

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
    source_admin_bounds_json: Option<String>,
    deletion_reason: Option<String>,
    source_country: Option<String>,
    district: Option<String>,
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
    pub activities: Vec<Activity>,
    pub administrative_location: BTreeMap<String, String>,
    pub deletion_reason: Option<String>,
    pub district_required: bool,
    pub location_missing: bool,
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
    let mut activities: Vec<_> = events
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
        .collect();
    activities.sort_by_key(|e| std::cmp::Reverse((e.at_ms, e.action != "document_update")));
    let previous_activity = activities.first().cloned();
    let administrative_location: BTreeMap<String, String> = row
        .source_admin_bounds_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<BTreeMap<String, String>>(raw).ok())
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .filter(|(key, value)| key.len() <= 128 && value.len() <= 256 && !value.trim().is_empty())
        .collect();
    let district_required = row.source_country.as_deref().is_some_and(|country| {
        country.trim().eq_ignore_ascii_case("Rwanda") || country.trim().eq_ignore_ascii_case("RW")
    });
    let location_missing = row
        .district
        .as_deref()
        .is_none_or(|value| value.trim().is_empty());
    Ok(Projection {
        id,
        archive: archive(row.backup_layer_uri.as_deref())
            .or_else(|| archive(row.backup_json_uri.as_deref())),
        previous_activity,
        activities,
        administrative_location,
        deletion_reason: row
            .deletion_reason
            .filter(|reason| !reason.trim().is_empty()),
        district_required,
        location_missing,
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
        serde_json::json!({"delivery": if targets.len() == 1 { "file" } else { "zip" },"targets":targets,"max_parallel":4})
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
        assert_eq!(plan["delivery"], "zip");
        let single = run(
            json!({"action":"download","rows":rows,"selected_ids":[view["rows"][0]["id"],view["rows"][0]["id"]]}),
        );
        assert_eq!(single["delivery"], "file");
        assert_eq!(single["targets"].as_array().unwrap().len(), 1);
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
    #[test]
    fn rwanda_location_is_required_and_new_evidence_does_not_change_event_identity() {
        let bare = json!({"ds_project":"p","transformer_name":"t","backup_layer_uri":"gs://backup/transformers/t.dsgrid"});
        let mut evidence = bare.clone();
        evidence["source_country"] = json!("RW");
        evidence["deletion_reason"] = json!("Duplicate");
        evidence["source_admin_bounds_json"] = json!("{\"sector\":\"Gishubi\"}");
        let projected = run(json!({"action":"project","rows":[bare,evidence]}));
        assert_eq!(projected["rows"][0]["id"], projected["rows"][1]["id"]);
        assert_eq!(projected["rows"][1]["district_required"], true);
        assert_eq!(projected["rows"][1]["location_missing"], true);
        assert_eq!(
            projected["rows"][1]["administrative_location"]["sector"],
            "Gishubi"
        );
        assert_eq!(projected["rows"][1]["deletion_reason"], "Duplicate");
    }
}
