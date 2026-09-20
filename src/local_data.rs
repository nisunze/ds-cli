//! Local data — what one machine holds for a project, store by store, and
//! the one clean action over what may go.
//!
//! The roster is decided here, per host, once: which stores a host has,
//! what each is called, whether its contents are RETAINED (the machine's
//! only copy of work — a queued publication, a survey record, a design
//! room, a draft) or CLEANABLE (a replica of something the project record
//! holds and the machine can take again — a verified sync download, a tile
//! cache, a synced photo). The host reads counts and bytes off its stores
//! and hands them in; it never classifies. A store that keeps no inventory
//! says so (`bytes: null`, "size not tracked") rather than scanning.
//!
//! Two hosts answer the same command: the Server over its state directory
//! (`ds-sync-store` rows, `report-artifacts`, `survey-media` waiting and
//! synced — each photo with its thumbnail — and `sync-downloads`; the
//! Solar application root and the tile cache are keyed by identity, not
//! project, and are not rows here) and the browser over
//! its IndexedDB (survey records, design rooms, drafts, version downloads)
//! — one list from one command on either host. Cleaning removes only rows
//! of cleanable stores that were named, after confirmation, and answers
//! what it removed.

use serde::{Deserialize, Serialize};
use serde_json::json;
#[cfg(test)]
use serde_json::Value;

pub const SCHEMA: &str = "ds.local-data/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    Server,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    Retained,
    Cleanable,
}

/// One store the roster names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreSpec {
    pub id: &'static str,
    pub retention: Retention,
    /// One sentence a caller reads beside the count.
    pub note: &'static str,
}

/// The Server's stores, in the order the page shows them.
pub const SERVER_STORES: &[StoreSpec] = &[
    StoreSpec {
        id: "sync_store",
        retention: Retention::Retained,
        note: "publication rows, heads, grants, receipts and leases for this project in store.sqlite",
    },
    StoreSpec {
        id: "report_artifacts",
        retention: Retention::Retained,
        note: "sealed report batches waiting for or holding their publication",
    },
    StoreSpec {
        id: "survey_media_waiting",
        retention: Retention::Retained,
        note: "rotated survey photos not yet published; the only copy",
    },
    StoreSpec {
        id: "survey_media_synced",
        retention: Retention::Cleanable,
        note: "survey photos and thumbnails equal to the published head; taken again on demand",
    },
    StoreSpec {
        id: "sync_downloads",
        retention: Retention::Cleanable,
        note: "verified replicas of published heads; downloaded again by the next sync pass",
    },
];

/// The browser's stores, in the order the page shows them.
pub const WINDOW_STORES: &[StoreSpec] = &[
    StoreSpec {
        id: "survey",
        retention: Retention::Retained,
        note: "survey records held offline for this project",
    },
    StoreSpec {
        id: "design_rooms",
        retention: Retention::Retained,
        note: "working design rooms",
    },
    StoreSpec {
        id: "design_drafts",
        retention: Retention::Retained,
        note: "device-local version drafts",
    },
    StoreSpec {
        id: "version_downloads",
        retention: Retention::Cleanable,
        note: "downloaded version snapshots; downloaded again when opened",
    },
];

pub fn stores(host: Host) -> &'static [StoreSpec] {
    match host {
        Host::Server => SERVER_STORES,
        Host::Window => WINDOW_STORES,
    }
}

/// What a host read off one store.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reading {
    pub id: String,
    pub count: u64,
    /// Bytes where the store tracks them; `None` is "size not tracked".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// One row of the answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoreRow {
    pub id: String,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    pub size_tracked: bool,
    pub retention: Retention,
    pub note: String,
}

/// The whole-machine reading the host may add: the Server's state
/// directory or the browser's storage estimate.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_bytes: Option<u64>,
    /// Where the stores live, when the host has a path to name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub schema: String,
    pub host: Host,
    pub project: String,
    pub refreshed_at_ms: u64,
    pub storage: Storage,
    /// usage / quota in [0, 1] when both are known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_fraction: Option<f64>,
    pub stores: Vec<StoreRow>,
    /// The cleanable stores that hold something — what a clean would remove.
    pub cleanable: Vec<String>,
    pub cleanable_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanable_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub remedy: &'static str,
}

pub const UNKNOWN_STORE: &str = "unknown_store";
pub const STORE_RETAINED: &str = "store_retained";
pub const NOTHING_TO_CLEAN: &str = "nothing_to_clean";
pub const CONFIRMATION_REQUIRED: &str = "confirmation_required";

fn spec(host: Host, id: &str) -> Option<&'static StoreSpec> {
    stores(host).iter().find(|spec| spec.id == id)
}

/// The rows, in roster order, from what the host read. A store the host did
/// not read is shown empty — the roster is the contract, not the reading.
pub fn rows(host: Host, readings: &[Reading]) -> Vec<StoreRow> {
    stores(host)
        .iter()
        .map(|spec| {
            let reading = readings.iter().find(|reading| reading.id == spec.id);
            let bytes = reading.and_then(|reading| reading.bytes);
            StoreRow {
                id: spec.id.into(),
                count: reading.map_or(0, |reading| reading.count),
                size_tracked: bytes.is_some(),
                bytes,
                retention: spec.retention,
                note: spec.note.into(),
            }
        })
        .collect()
}

pub fn answer(host: Host, project: &str, readings: &[Reading], storage: Storage, now_ms: u64) -> Answer {
    let stores = rows(host, readings);
    let cleanable: Vec<&StoreRow> = stores
        .iter()
        .filter(|row| row.retention == Retention::Cleanable && row.count > 0)
        .collect();
    let cleanable_bytes = if cleanable.iter().all(|row| row.bytes.is_some()) && !cleanable.is_empty() {
        Some(cleanable.iter().filter_map(|row| row.bytes).sum())
    } else {
        None
    };
    let storage_fraction = match (storage.usage_bytes, storage.quota_bytes) {
        (Some(usage), Some(quota)) if quota > 0 => Some((usage as f64 / quota as f64).clamp(0.0, 1.0)),
        _ => None,
    };
    Answer {
        schema: SCHEMA.into(),
        host,
        project: project.into(),
        refreshed_at_ms: now_ms,
        storage,
        storage_fraction,
        cleanable_count: cleanable.iter().map(|row| row.count).sum(),
        cleanable: cleanable.iter().map(|row| row.id.clone()).collect(),
        cleanable_bytes,
        stores,
    }
}

/// What a clean would remove: the named cleanable stores (every cleanable
/// store when none is named) that hold something. Refused before anything
/// moves when a name is unknown, retained, or nothing would go; refused
/// `confirmation_required` until the caller confirms.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CleanPlan {
    pub host: Host,
    pub project: String,
    pub stores: Vec<StoreRow>,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

pub fn clean_plan(
    host: Host,
    project: &str,
    readings: &[Reading],
    named: &[String],
    confirmed: bool,
) -> Result<CleanPlan, Refusal> {
    for id in named {
        match spec(host, id) {
            None => {
                return Err(Refusal {
                    code: UNKNOWN_STORE,
                    message: format!("{id} is not a local store of this host"),
                    remedy: "name a store id from the status answer",
                })
            }
            Some(spec) if spec.retention == Retention::Retained => {
                return Err(Refusal {
                    code: STORE_RETAINED,
                    message: format!("{id} is retained: it is this machine's only copy"),
                    remedy: "publish or sync the work first; only cleanable stores may be cleaned",
                })
            }
            Some(_) => {}
        }
    }
    let stores: Vec<StoreRow> = rows(host, readings)
        .into_iter()
        .filter(|row| row.retention == Retention::Cleanable && row.count > 0)
        .filter(|row| named.is_empty() || named.iter().any(|id| *id == row.id))
        .collect();
    if stores.is_empty() {
        return Err(Refusal {
            code: NOTHING_TO_CLEAN,
            message: format!("nothing cleanable is held for project {project}"),
            remedy: "read the status first; a clean is offered only when a cleanable store holds something",
        });
    }
    let count = stores.iter().map(|row| row.count).sum();
    let bytes = if stores.iter().all(|row| row.bytes.is_some()) {
        Some(stores.iter().filter_map(|row| row.bytes).sum())
    } else {
        None
    };
    if !confirmed {
        return Err(Refusal {
            code: CONFIRMATION_REQUIRED,
            message: format!(
                "cleaning would remove {count} item(s) from {} for project {project}",
                stores.iter().map(|row| row.id.as_str()).collect::<Vec<_>>().join(", ")
            ),
            remedy: "repeat with --yes to confirm",
        });
    }
    Ok(CleanPlan {
        host,
        project: project.into(),
        stores,
        count,
        bytes,
    })
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Status {
        host: Host,
        project: String,
        #[serde(default)]
        readings: Vec<Reading>,
        #[serde(default)]
        storage: Storage,
        now_ms: u64,
    },
    CleanPlan {
        host: Host,
        project: String,
        #[serde(default)]
        readings: Vec<Reading>,
        #[serde(default)]
        stores: Vec<String>,
        #[serde(default)]
        confirmed: bool,
    },
}

/// The JSON door every host uses.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 1024 * 1024 {
        return Err("local data request exceeds its byte bound".into());
    }
    let request: Request =
        serde_json::from_slice(input).map_err(|error| format!("invalid local data request: {error}"))?;
    let value = match request {
        Request::Status {
            host,
            project,
            readings,
            storage,
            now_ms,
        } => json!({"status": "ok", "data": answer(host, &project, &readings, storage, now_ms)}),
        Request::CleanPlan {
            host,
            project,
            readings,
            stores,
            confirmed,
        } => match clean_plan(host, &project, &readings, &stores, confirmed) {
            Ok(plan) => json!({"status": "ok", "data": plan}),
            Err(refusal) => json!({"status": "refused", "refusal": refusal}),
        },
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(id: &str, count: u64, bytes: Option<u64>) -> Reading {
        Reading {
            id: id.into(),
            count,
            bytes,
        }
    }

    #[test]
    fn one_row_per_store_in_roster_order_with_count_and_retention() {
        let readings = [
            reading("survey", 12, None),
            reading("design_rooms", 3, None),
            reading("design_drafts", 5, None),
            reading("version_downloads", 2, None),
        ];
        let rows = rows(Host::Window, &readings);
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            ["survey", "design_rooms", "design_drafts", "version_downloads"]
        );
        assert_eq!(rows.iter().map(|row| row.count).collect::<Vec<_>>(), [12, 3, 5, 2]);
        assert!(rows.iter().all(|row| !row.size_tracked && row.bytes.is_none()));
        assert_eq!(rows[3].retention, Retention::Cleanable);
        assert!(rows[..3].iter().all(|row| row.retention == Retention::Retained));
    }

    #[test]
    fn a_store_the_host_did_not_read_is_shown_empty_not_missing() {
        let rows = rows(Host::Server, &[reading("sync_downloads", 4, Some(4096))]);
        assert_eq!(rows.len(), SERVER_STORES.len());
        let downloads = rows.iter().find(|row| row.id == "sync_downloads").unwrap();
        assert_eq!((downloads.count, downloads.bytes, downloads.size_tracked), (4, Some(4096), true));
        assert!(rows.iter().filter(|row| row.id != "sync_downloads").all(|row| row.count == 0));
    }

    #[test]
    fn the_answer_names_what_a_clean_would_remove_and_the_storage_fraction() {
        let readings = [
            reading("sync_downloads", 4, Some(4096)),
            reading("survey_media_synced", 2, Some(1000)),
            reading("survey_media_waiting", 1, Some(500)),
            reading("report_artifacts", 0, Some(0)),
        ];
        let storage = Storage {
            usage_bytes: Some(50),
            quota_bytes: Some(200),
            root: Some("/state".into()),
        };
        let answer = answer(Host::Server, "p", &readings, storage, 7);
        assert_eq!(answer.cleanable, ["survey_media_synced", "sync_downloads"]);
        assert_eq!((answer.cleanable_count, answer.cleanable_bytes), (6, Some(5096)));
        assert_eq!(answer.storage_fraction, Some(0.25));
        assert_eq!(answer.refreshed_at_ms, 7);
        let unknown = super::answer(Host::Window, "p", &[], Storage::default(), 1);
        assert_eq!(unknown.storage_fraction, None);
        assert!(unknown.cleanable.is_empty() && unknown.cleanable_bytes.is_none());
        let clamped = super::answer(
            Host::Window,
            "p",
            &[],
            Storage {
                usage_bytes: Some(300),
                quota_bytes: Some(200),
                root: None,
            },
            1,
        );
        assert_eq!(clamped.storage_fraction, Some(1.0));
    }

    #[test]
    fn a_clean_is_refused_before_anything_moves() {
        let readings = [reading("version_downloads", 2, None), reading("survey", 9, None)];
        let plan = clean_plan(Host::Window, "p", &readings, &[], false).unwrap_err();
        assert_eq!(plan.code, CONFIRMATION_REQUIRED);
        assert!(plan.message.contains("2 item(s)") && plan.message.contains("version_downloads"));
        let plan = clean_plan(Host::Window, "p", &readings, &[], true).unwrap();
        assert_eq!(plan.stores.len(), 1);
        assert_eq!((plan.count, plan.bytes), (2, None));
        assert_eq!(
            clean_plan(Host::Window, "p", &readings, &["survey".into()], true).unwrap_err().code,
            STORE_RETAINED
        );
        assert_eq!(
            clean_plan(Host::Window, "p", &readings, &["nope".into()], true).unwrap_err().code,
            UNKNOWN_STORE
        );
        assert_eq!(
            clean_plan(Host::Window, "p", &[reading("version_downloads", 0, None)], &[], true)
                .unwrap_err()
                .code,
            NOTHING_TO_CLEAN
        );
        // A Server clean of one named store leaves the others alone.
        let server = [reading("sync_downloads", 4, Some(40)), reading("survey_media_synced", 1, Some(9))];
        let plan = clean_plan(Host::Server, "p", &server, &["survey_media_synced".into()], true).unwrap();
        assert_eq!(plan.stores.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["survey_media_synced"]);
        assert_eq!(plan.bytes, Some(9));
    }

    #[test]
    fn the_json_door_answers_status_and_plans() {
        let input = json!({"op": "status", "host": "window", "project": "p", "readings": [{"id": "survey", "count": 3}], "now_ms": 5});
        let answer: Value = serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(answer["data"]["stores"][0]["count"], 3);
        assert_eq!(answer["data"]["stores"][0]["size_tracked"], false);
        let input = json!({"op": "clean_plan", "host": "window", "project": "p", "readings": [], "stores": []});
        let answer: Value = serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(answer["refusal"]["code"], NOTHING_TO_CLEAN);
    }
}
