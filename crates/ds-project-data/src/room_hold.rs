//! The core's copy of a project's transformer rooms and print inputs, on disk.
//!
//! A report is printed from what this machine holds. Which held room is read
//! again is the kernel's decision (`ds_command_kernel::pinned_context::plan`:
//! a held room is reused, a missing one or one whose head revision moved is
//! read whole); this module only keeps the bytes. One directory per account
//! and project under the layer root, beside the survey hold:
//!
//! * `rooms/<digest>.json` — one saved transformer room as the service
//!   answered it, with the revision and content digest it was saved at;
//! * `rooms.json` — the index a plan reads: per transformer, the revision,
//!   the layer feature counts and the digest of its room file, never geometry;
//! * `inputs.json` — what a batch needs besides its rooms, as the host last
//!   read it (the host decides that document's shape).
//!
//! Writes are whole-file and renamed into place under the room lock, so a
//! reader sees the previous copy or the next, never half. This crate never
//! opens a socket: the host reads the service and hands the answer here.

use ds_command_kernel::project_dataset_cache::Scope;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

mod acquisition;
pub use acquisition::{acquire, AcquiredRoom, AcquisitionError, RoomSource};

pub const SCHEMA: &str = "ds.report-hold/v1";
const ROOT: &str = "report-hold";
const VERSION: &str = "v1";
const ROOMS: &str = "rooms";
const INDEX: &str = "rooms.json";
const INPUTS: &str = "inputs.json";

/// One held transformer room: the saved layers exactly as the service
/// answered them, with the revision and digest they were saved at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub transformer: String,
    pub version: Option<i64>,
    pub content_digest: Option<String>,
    pub layers: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: String,
    rooms: BTreeMap<String, IndexRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexRow {
    version: Option<i64>,
    layers: BTreeMap<String, i64>,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    schema: String,
    document: Value,
}

/// The account's and project's directory. The scope is digested so no
/// identity appears in a path.
pub fn room_dir(root: &Path, scope: &Scope) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(SCHEMA.as_bytes());
    digest.update([0]);
    digest.update(scope.principal.as_bytes());
    digest.update([0]);
    digest.update(scope.project.as_bytes());
    root.join(ROOT)
        .join(VERSION)
        .join(format!("{:x}", digest.finalize()))
}

fn room_file(root: &Path, scope: &Scope, transformer: &str) -> PathBuf {
    room_dir(root, scope)
        .join(ROOMS)
        .join(format!("{:x}.json", Sha256::digest(transformer.as_bytes())))
}

fn lock(root: &Path, scope: &Scope) -> Result<fs::File, String> {
    ds_layer_store::exclusive_lock(&room_dir(root, scope).join(".lock"))
        .map_err(|_| "the report hold lock could not be taken".into())
}

fn index(root: &Path, scope: &Scope) -> Result<Index, String> {
    match fs::read(room_dir(root, scope).join(INDEX)) {
        Ok(bytes) => {
            let index: Index = serde_json::from_slice(&bytes)
                .map_err(|_| "the held room index is unreadable".to_string())?;
            if index.schema != SCHEMA {
                return Err("the held room index has an unsupported schema".into());
            }
            Ok(index)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Index {
            schema: SCHEMA.into(),
            rooms: BTreeMap::new(),
        }),
        Err(_) => Err("the held room index could not be read".into()),
    }
}

/// Features per layer: what a plan compares, and all it needs.
fn layer_counts(layers: &BTreeMap<String, Value>) -> BTreeMap<String, i64> {
    layers
        .iter()
        .map(|(name, layer)| {
            let features = layer
                .get("features")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            (name.clone(), i64::try_from(features).unwrap_or(i64::MAX))
        })
        .collect()
}

/// What this machine holds, in the shape `pinned_context::plan` reads as
/// `held`: every room here was read whole, so each one is `complete`.
pub fn held(root: &Path, scope: &Scope) -> Result<Vec<Value>, String> {
    Ok(index(root, scope)?
        .rooms
        .into_iter()
        .map(|(name, row)| {
            json!({"name": name, "layers": row.layers, "version": row.version, "complete": true})
        })
        .collect())
}

/// One held room, verified against the index; `None` when it is not held.
pub fn room(root: &Path, scope: &Scope, transformer: &str) -> Result<Option<Room>, String> {
    let Some(row) = index(root, scope)?.rooms.remove(transformer) else {
        return Ok(None);
    };
    let bytes = fs::read(room_file(root, scope, transformer))
        .map_err(|_| format!("the held room of {transformer} could not be read"))?;
    if format!("{:x}", Sha256::digest(&bytes)) != row.sha256 {
        return Err(format!(
            "the held room of {transformer} does not match its index"
        ));
    }
    let room: Room = serde_json::from_slice(&bytes)
        .map_err(|_| format!("the held room of {transformer} is unreadable"))?;
    if room.transformer != transformer {
        return Err(format!(
            "the held room of {transformer} names another transformer"
        ));
    }
    if room.version != row.version || layer_counts(&room.layers) != row.layers {
        return Err(format!(
            "the held room of {transformer} has inconsistent index metadata"
        ));
    }
    Ok(Some(room))
}

/// Read the complete active transformer roster and its checksum-verified saved
/// rooms for a preview. No discovery, provider read or partial roster fallback.
pub fn active_print_rooms(
    root: &Path,
    scope: &Scope,
) -> Result<Vec<ds_command_kernel::printing::adjacent_networks::Room>, String> {
    let document = inputs(root, scope)?.ok_or("no project print inventory is held")?;
    let rows = document["rows"]
        .as_array()
        .ok_or("held print inventory has no roster")?;
    let mut names = std::collections::BTreeSet::new();
    for row in rows {
        if !matches!(row["kind"].as_str(), Some("transformer" | "project_level"))
            || !matches!(
                row["state"].as_str(),
                Some("active" | "retired" | "deleted" | "missing")
            )
        {
            return Err("held print inventory contains an invalid lifecycle row".into());
        }
        if row["kind"] != "transformer" || row["state"] != "active" {
            continue;
        }
        let name = row["name"]
            .as_str()
            .filter(|name| !name.is_empty() && name.trim() == *name && name.len() <= 200)
            .ok_or("active print inventory has no transformer identity")?;
        if row["retired"] == true {
            return Err("active print inventory contains a retired transformer".into());
        }
        if !names.insert(name) || names.len() > 500 {
            return Err("active print inventory has duplicates or exceeds 500 rooms".into());
        }
    }
    if names.is_empty() {
        return Err("held print inventory has no active transformer".into());
    }
    names
        .into_iter()
        .map(|name| {
            let room = room(root, scope, name)?
                .ok_or_else(|| format!("{name} is not held; refresh the project print rooms"))?;
            let digest =
                ds_command_kernel::report_export::jcs::layers_content_digest(&room.layers)?;
            if room.content_digest.as_deref() != Some(digest.as_str()) {
                return Err(format!(
                    "{name} has no matching saved content digest; refresh its print room"
                ));
            }
            Ok(ds_command_kernel::printing::adjacent_networks::Room {
                transformer: room.transformer,
                version: room
                    .version
                    .filter(|v| *v > 0)
                    .ok_or_else(|| format!("{name} has no saved print revision"))?,
                layers: room.layers,
            })
        })
        .collect()
}

/// Hold one room read whole from the service, replacing any older copy.
pub fn hold_room(root: &Path, scope: &Scope, room: &Room) -> Result<(), String> {
    let _lock = lock(root, scope)?;
    hold_room_locked(root, scope, room)
}

fn hold_room_locked(root: &Path, scope: &Scope, room: &Room) -> Result<(), String> {
    let bytes = serde_json::to_vec(room).map_err(|_| "a room could not be encoded")?;
    let path = room_file(root, scope, &room.transformer);
    ds_layer_store::private::create_dir_all(path.parent().expect("a room file has a folder"))
        .map_err(|_| "the report hold could not be created")?;
    write_whole(&path, &bytes)?;
    let mut index = index(root, scope)?;
    index.rooms.insert(
        room.transformer.clone(),
        IndexRow {
            version: room.version,
            layers: layer_counts(&room.layers),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        },
    );
    let bytes = serde_json::to_vec(&index).map_err(|_| "the room index could not be encoded")?;
    write_whole(&room_dir(root, scope).join(INDEX), &bytes)
}

/// The inputs document the host last held for this account and project.
pub fn inputs(root: &Path, scope: &Scope) -> Result<Option<Value>, String> {
    match fs::read(room_dir(root, scope).join(INPUTS)) {
        Ok(bytes) => {
            let held: Inputs = serde_json::from_slice(&bytes)
                .map_err(|_| "the held print inputs are unreadable".to_string())?;
            if held.schema != SCHEMA {
                return Err("the held print inputs have an unsupported schema".into());
            }
            Ok(Some(held.document))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("the held print inputs could not be read".into()),
    }
}

/// Hold the inputs document, replacing the previous one whole.
pub fn hold_inputs(root: &Path, scope: &Scope, document: &Value) -> Result<(), String> {
    let _lock = lock(root, scope)?;
    let bytes = serde_json::to_vec(&Inputs {
        schema: SCHEMA.into(),
        document: document.clone(),
    })
    .map_err(|_| "the print inputs could not be encoded")?;
    write_whole(&room_dir(root, scope).join(INPUTS), &bytes)
}

fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("part");
    let result = ds_layer_store::private::create_file(&partial)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&partial, path));
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result.map_err(|_| "the report hold could not be written".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::pinned_context;

    fn scope(principal: &str) -> Scope {
        Scope {
            principal: principal.into(),
            project: "it_rwanda".into(),
        }
    }

    fn saved(name: &str, version: i64) -> Room {
        Room {
            transformer: name.into(),
            version: Some(version),
            content_digest: Some(format!("digest-{version}")),
            layers: BTreeMap::from([
                (
                    "tr".to_string(),
                    json!({"type": "FeatureCollection", "features": [{"type": "Feature"}]}),
                ),
                (
                    "customers".to_string(),
                    json!({"type": "FeatureCollection", "features": [{}, {}, {}]}),
                ),
            ]),
        }
    }

    fn plan(held: Vec<Value>, heads: Value, pins: &[&str]) -> pinned_context::Plan {
        pinned_context::plan(
            serde_json::from_value(json!({
                "schema": pinned_context::SCHEMA,
                "pins": pins,
                "held": held,
                "heads": heads,
            }))
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_held_room_reads_back_exactly_and_a_warm_plan_fetches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope("u1");
        assert!(held(dir.path(), &scope).unwrap().is_empty());
        assert_eq!(room(dir.path(), &scope, "t1").unwrap(), None);
        hold_room(dir.path(), &scope, &saved("t1", 4)).unwrap();
        hold_room(dir.path(), &scope, &saved("t2", 7)).unwrap();
        assert_eq!(
            room(dir.path(), &scope, "t1").unwrap(),
            Some(saved("t1", 4))
        );

        let heads = json!([{"name": "t1", "version": 4}, {"name": "t2", "version": 7}]);
        let warm = plan(held(dir.path(), &scope).unwrap(), heads, &["t1", "t2"]);
        assert_eq!(warm.counts.reused, 2);
        assert_eq!(warm.counts.rooms_fetched, 0);

        // No heads — the service did not answer — is no evidence of change.
        let offline = plan(
            held(dir.path(), &scope).unwrap(),
            json!([]),
            &["t1", "t2", "t3"],
        );
        assert_eq!(offline.counts.reused, 2);
        assert_eq!(offline.counts.rooms_fetched, 1, "only the unheld t3");
    }

    #[test]
    fn a_moved_head_fetches_that_room_and_holding_it_again_settles_the_plan() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope("u1");
        hold_room(dir.path(), &scope, &saved("t1", 4)).unwrap();
        let heads = json!([{"name": "t1", "version": 5}]);
        let moved = plan(held(dir.path(), &scope).unwrap(), heads.clone(), &["t1"]);
        assert_eq!(moved.counts.rooms_fetched, 1);
        assert_eq!(moved.fetch[0].reason, "head_moved");
        hold_room(dir.path(), &scope, &saved("t1", 5)).unwrap();
        let settled = plan(held(dir.path(), &scope).unwrap(), heads, &["t1"]);
        assert_eq!(settled.counts.rooms_fetched, 0);
        assert_eq!(
            room(dir.path(), &scope, "t1").unwrap().unwrap().version,
            Some(5)
        );
    }

    #[test]
    fn accounts_never_share_a_hold_and_a_changed_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        hold_room(dir.path(), &scope("u1"), &saved("t1", 4)).unwrap();
        hold_inputs(dir.path(), &scope("u1"), &json!({"roster": []})).unwrap();
        assert_eq!(room(dir.path(), &scope("u2"), "t1").unwrap(), None);
        assert_eq!(inputs(dir.path(), &scope("u2")).unwrap(), None);
        assert_eq!(
            inputs(dir.path(), &scope("u1")).unwrap(),
            Some(json!({"roster": []}))
        );

        let file = room_file(dir.path(), &scope("u1"), "t1");
        let mut changed: Room = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        changed.version = Some(99);
        fs::write(&file, serde_json::to_vec(&changed).unwrap()).unwrap();
        let refusal = room(dir.path(), &scope("u1"), "t1").unwrap_err();
        assert!(refusal.contains("does not match its index"), "{refusal}");
    }

    #[test]
    fn adjacent_preview_requires_the_complete_scoped_active_roster_and_saved_content() {
        let dir = tempfile::tempdir().unwrap();
        let owner = scope("u1");
        let mut first = saved("t1", 4);
        first.content_digest = Some(
            ds_command_kernel::report_export::jcs::layers_content_digest(&first.layers).unwrap(),
        );
        let mut second = first.clone();
        second.transformer = "t2".into();
        second.version = Some(7);
        hold_room(dir.path(), &owner, &first).unwrap();
        hold_inputs(
            dir.path(),
            &owner,
            &json!({"rows": [
                {"name":"t1","kind":"transformer","state":"active"},
                {"name":"t2","kind":"transformer","state":"active"},
                {"name":"old","kind":"transformer","state":"retired"},
                {"name":"overall","kind":"project_level","state":"active"}
            ]}),
        )
        .unwrap();
        assert!(active_print_rooms(dir.path(), &scope("u2"))
            .unwrap_err()
            .contains("no project print inventory"));
        assert!(active_print_rooms(dir.path(), &owner)
            .unwrap_err()
            .contains("t2 is not held"));
        hold_room(dir.path(), &owner, &second).unwrap();
        let rooms = active_print_rooms(dir.path(), &owner).unwrap();
        assert_eq!(rooms.len(), 2);
        assert_eq!(rooms[1].transformer, "t2");
        assert_eq!(rooms[1].version, 7);
        assert_eq!(rooms[1].layers, second.layers);

        second.content_digest = Some("mismatching saved content".into());
        hold_room(dir.path(), &owner, &second).unwrap();
        assert!(active_print_rooms(dir.path(), &owner)
            .unwrap_err()
            .contains("matching saved content digest"));
        hold_inputs(
            dir.path(),
            &owner,
            &json!({"rows":[{"name":"t1","kind":"transformer","state":"active","retired":true}]}),
        )
        .unwrap();
        assert!(active_print_rooms(dir.path(), &owner)
            .unwrap_err()
            .contains("retired transformer"));
    }
}
