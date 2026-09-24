//! `ds assets tree --folder Correspondence` — the thread index, headless.
//!
//! Owner ruling 2026-09-20: everything about a correspondence thread is
//! indexed under `Assets › Correspondence`. The index is the kernel's own
//! system-folder projection (`ds_command_kernel::assets::tree`), fed here
//! with what it needs and nothing more — the catalogue rows, the records and
//! the parties of the selected project, each read through the native
//! credential — so the answer is the same bytes the desktop's Assets tab
//! would render, computed by the same kernel, on a host with no window.
//!
//! Every other folder of the tree still comes through the paired window
//! (`tree.rs`), because the sources those roots need — transformer
//! inventories, print rooms, local data — have no headless reader yet. The
//! roots this read does not load are reported `not_loaded`, never rendered
//! empty.

use crate::CatalogueCommand as Catalogue;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use ds_client_core::project_correspondence::{Action, PartyFilters, RecordFilters};
use ds_command_kernel::assets::tree::SYSTEM_ROOTS;
use serde_json::{Map, Value, json};

/// The catalogue pages one index read walks; 200 rows each, so a project
/// past this lists with `truncated`.
const MAX_CATALOGUE_PAGES: usize = 20;
/// The record pages one index read walks; 100 rows each, the server's own
/// scan cap divided by its page.
const MAX_RECORD_PAGES: i64 = 10;

/// The system root the index lives under, spelled as the kernel spells it.
pub const ROOT: &str = "Correspondence";
const _: () = assert!(
    SYSTEM_ROOTS.len() == 12,
    "the kernel's roots moved; re-check ROOT"
);

pub const PROJECT_NOT_VISIBLE: Refusal = Refusal {
    code: "project_not_visible",
    when: "the selected project is not one this account is a member of",
    remedy: "choose an exact id from `ds auth project list`",
};

/// Whether a `--folder` names the index or a thread under it.
pub fn is_correspondence_folder(path: &str) -> bool {
    path.split('/')
        .next()
        .is_some_and(|head| head.eq_ignore_ascii_case(ROOT))
}

fn page_rows(answer: &Value, key: &str) -> Vec<Value> {
    answer[key].as_array().cloned().unwrap_or_default()
}

/// Every catalogue row the caller may read, up to the bound.
fn catalogue_rows(lane: &str) -> Result<(Vec<Value>, bool), Failure> {
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    let mut truncated = false;
    for _ in 0..MAX_CATALOGUE_PAGES {
        let page = crate::catalogue(
            lane,
            &Catalogue::List {
                limit: 200,
                cursor: cursor.clone(),
                folder_id: None,
                kind: None,
                status: None,
                sensitivity: None,
                since: None,
            },
        )?;
        rows.extend(page_rows(&page, "assets"));
        truncated |= page["truncated"] == true;
        if page["has_more"] != true {
            return Ok((rows, truncated));
        }
        cursor = page["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            return Ok((rows, truncated));
        }
    }
    Ok((rows, true))
}

/// Every record the caller may read, raw as the door answers them.
fn record_rows(lane: &str) -> Result<(Vec<Value>, bool), Failure> {
    let mut rows = Vec::new();
    for page in 1..=MAX_RECORD_PAGES {
        let answer = crate::correspondence_door(
            lane,
            &Action::RecordList(RecordFilters {
                limit: Some(100),
                page: Some(page),
                ..RecordFilters::default()
            }),
        )?;
        let batch = page_rows(&answer, "records");
        let total = answer["total"].as_i64().unwrap_or(0);
        rows.extend(batch);
        if answer["truncated"] == true {
            return Ok((rows, true));
        }
        if (page * 100) >= total {
            return Ok((rows, false));
        }
    }
    Ok((rows, true))
}

fn party_rows(lane: &str) -> Result<Vec<Value>, Failure> {
    let mut rows = Vec::new();
    for page in 1..=MAX_RECORD_PAGES {
        let answer = crate::correspondence_door(
            lane,
            &Action::PartyList(PartyFilters {
                include_archived: true,
                limit: Some(100),
                page: Some(page),
                ..PartyFilters::default()
            }),
        )?;
        let batch = page_rows(&answer, "parties");
        let total = answer["total"].as_i64().unwrap_or(0);
        rows.extend(batch);
        if (page * 100) >= total {
            break;
        }
    }
    Ok(rows)
}

/// A catalogue row as the kernel's `Asset` reads it, or `None` when the row
/// carries a value outside the kernel's closed vocabularies — reported as
/// dropped, never allowed to sink the whole index.
fn kernel_row(row: &Value) -> Option<Value> {
    let mut row = row.clone();
    // The kernel's format vocabulary is closed; the catalogue's is free text.
    if let Some(format) = row["format"].as_str() {
        let known = ds_command_kernel::assets::Format::ALL
            .iter()
            .any(|candidate| candidate.as_str() == format);
        if !known {
            row["format"] = json!("unknown");
        }
    }
    serde_json::from_value::<ds_command_kernel::assets::Asset>(row.clone())
        .ok()
        .map(|_| row)
}

/// The index, rooted at `--folder`.
pub fn tree(lane: &str, arguments: &Value) -> Result<Value, Failure> {
    let folders = crate::catalogue(lane, &Catalogue::Folders)?;
    let (assets, assets_truncated) = catalogue_rows(lane)?;
    let (records, records_truncated) = record_rows(lane)?;
    let parties = party_rows(lane)?;

    let mut rows = Vec::with_capacity(assets.len());
    let mut dropped = 0usize;
    for row in &assets {
        match kernel_row(row) {
            Some(row) => rows.push(row),
            None => dropped += 1,
        }
    }
    let mut folder_rows = Vec::new();
    for folder in page_rows(&folders, "folders") {
        if serde_json::from_value::<ds_command_kernel::assets::Folder>(folder.clone()).is_ok() {
            folder_rows.push(folder);
        }
    }

    let mut request = Map::new();
    request.insert(
        "schema".into(),
        json!(ds_command_kernel::assets::REQUEST_SCHEMA),
    );
    request.insert("action".into(), json!("tree"));
    request.insert(
        "sources".into(),
        json!({ "pm_records": records, "pm_parties": parties }),
    );
    request.insert("folders".into(), Value::Array(folder_rows));
    request.insert("assets".into(), Value::Array(rows));
    for key in ["folder", "depth", "query", "kind"] {
        if let Some(value) = arguments.get(key) {
            request.insert(key.into(), value.clone());
        }
    }
    if let Some(link) = arguments.get("link").and_then(Value::as_str) {
        request.insert("link".into(), link_value(link)?);
    }
    let answer = ds_command_kernel::assets::evaluate(
        &serde_json::to_vec(&Value::Object(request))
            .map_err(|error| Failure::internal("assets_service_failed", error.to_string()))?,
    )
    .map_err(|error| {
        Failure::invalid("asset_request_invalid", error).remedy(crate::ASSET_REQUEST_INVALID.remedy)
    })?;
    let mut value: Value = serde_json::from_str(&answer)
        .map_err(|error| Failure::internal("assets_service_failed", error.to_string()))?;
    value["indexed_from"] = json!({
        "assets": assets.len(),
        "assets_dropped": dropped,
        "assets_truncated": assets_truncated,
        "records": records.len(),
        "records_truncated": records_truncated,
        "parties": parties.len(),
    });
    Ok(value)
}

/// `pm_task:<id>` / `pm_record:<id>` / `ds_object:<type>:<id>` as the
/// kernel's own `Link` value.
fn link_value(raw: &str) -> Result<Value, Failure> {
    let segments: Vec<&str> = raw.split(':').collect();
    match segments.as_slice() {
        ["pm_task", id] => Ok(json!({ "kind": "pm_task", "id": id })),
        ["pm_record", id] => Ok(json!({ "kind": "pm_record", "id": id })),
        ["ds_object", object_type, id] => {
            Ok(json!({ "kind": "ds_object", "object_type": object_type, "entity_id": id }))
        }
        _ => Err(
            Failure::invalid("invalid_link", "the link is not one of the three forms")
                .remedy(crate::INVALID_LINK.remedy),
        ),
    }
}
