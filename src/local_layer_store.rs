//! The prepared local layer store: the catalogue a machine keeps and each
//! layer's feature payload, in the Kernel's SQLite, fenced by identity.
//!
//! What a catalogue may hold, what a change does and what a removal may delete
//! are [`crate::local_layers`]' answers. This store keeps exactly the records
//! the host holds after those answers (the admitted registry and any rows a
//! reopen carried verbatim) and the payload each layer parks under its id.
//! A payload is opaque GeoJSON feature text: the store admits that it is one
//! bounded JSON array and counts its features, and never reads a geometry.
use crate::durable_store::{Input, Outcome};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::{Deserializer, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub const STORE: &str = "local_layer_store";
pub const INVALID: &str = "local_layer_store_invalid";
/// A catalogue holds what one reopen may read.
pub const MAX_RECORDS: usize = crate::local_layers::MAX_LAYERS;
pub const MAX_CATALOGUE_BYTES: usize = crate::local_layers::MAX_INPUT_BYTES;
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024 * 1024;
/// A payload is keyed by the id its record carries, verbatim: a carried row
/// keeps its payload even when its id cannot be admitted as a layer id.
const MAX_PAYLOAD_ID: usize = 1024;
const MAX_IDS: usize = 4096;
const MAX_SAFE: u64 = 9_007_199_254_740_991;

pub const CATALOGUE: &str = "CREATE TABLE IF NOT EXISTS local_layer_catalogue (owner TEXT PRIMARY KEY NOT NULL, records TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>=0 AND revision<=9007199254740991), updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=0 AND updated_at_ms<=9007199254740991))";
pub const PAYLOADS: &str = "CREATE TABLE IF NOT EXISTS local_layer_payloads (owner TEXT NOT NULL, layer_id TEXT NOT NULL, features TEXT NOT NULL, feature_count INTEGER NOT NULL CHECK(feature_count>=0), updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=0 AND updated_at_ms<=9007199254740991), PRIMARY KEY(owner, layer_id))";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// The held catalogue (`null` before the first write).
    Read,
    /// Replace the catalogue with the records the host holds.
    WriteCatalogue {
        records: Vec<Value>,
        revision: u64,
        now: u64,
    },
    /// Park one layer's payload.
    PutFeatures {
        layer_id: String,
        features: String,
        now: u64,
    },
    /// The catalogue and one layer's payload, committed together.
    PutSnapshot {
        records: Vec<Value>,
        revision: u64,
        layer_id: String,
        features: String,
        now: u64,
    },
    GetFeatures { layer_id: String },
    /// Delete exactly the payloads a `local_layers` receipt released.
    DeleteFeatures { layer_ids: Vec<String> },
    /// Storage hygiene: payloads no catalogue record names, GIS folder
    /// connections no record references, and the adapter tokens to drop
    /// (including those of `present_tokens` no connection names).
    Sweep {
        #[serde(default)]
        present_tokens: Vec<String>,
    },
}

fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}
fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}
fn time(value: u64) -> Result<i64, String> {
    if value <= MAX_SAFE {
        Ok(value as i64)
    } else {
        Err(invalid("a time or revision exceeds its bound"))
    }
}
fn payload_id(value: &str) -> Result<&str, String> {
    if value.is_empty() || value.len() > MAX_PAYLOAD_ID {
        Err(invalid("a payload id is the record's id, 1 to 1024 bytes"))
    } else {
        Ok(value)
    }
}

/// The catalogue the host holds: an array of objects, bounded like a reopen.
fn catalogue_text(records: &[Value]) -> Result<String, String> {
    if records.len() > MAX_RECORDS || records.iter().any(|record| !record.is_object()) {
        return Err(invalid("a catalogue is at most 2000 object records"));
    }
    let text = serde_json::to_string(records).map_err(|e| e.to_string())?;
    if text.len() > MAX_CATALOGUE_BYTES {
        return Err(invalid("a catalogue exceeds 16 MiB"));
    }
    Ok(text)
}

/// Admit a payload: one JSON array within its bound. Answers its length
/// without building a single feature.
fn feature_count(text: &str) -> Result<u64, String> {
    struct Count;
    impl<'de> Visitor<'de> for Count {
        type Value = u64;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a JSON array of features")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<u64, A::Error> {
            let mut count = 0;
            while seq.next_element::<IgnoredAny>()?.is_some() {
                count += 1;
            }
            Ok(count)
        }
    }
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err(invalid("a payload exceeds 256 MiB"));
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let count = (&mut deserializer)
        .deserialize_seq(Count)
        .and_then(|count| deserializer.end().map(|()| count))
        .map_err(|_| invalid("a payload is one JSON array of features"))?;
    Ok(count)
}

fn read_catalogue(connection: &Connection, owner: &str) -> Result<Option<(Vec<Value>, u64)>, String> {
    let held = connection
        .query_row(
            "SELECT records,revision FROM local_layer_catalogue WHERE owner=?1",
            params![owner],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(sql)?;
    held.map(|(text, revision)| {
        serde_json::from_str::<Vec<Value>>(&text)
            .map(|records| (records, revision as u64))
            .map_err(|_| invalid("the stored catalogue is corrupt"))
    })
    .transpose()
}

fn write_catalogue(connection: &Connection, owner: &str, records: &[Value], revision: u64, now: u64) -> Result<(), String> {
    let text = catalogue_text(records)?;
    connection
        .execute(
            "INSERT INTO local_layer_catalogue(owner,records,revision,updated_at_ms) VALUES(?1,?2,?3,?4) ON CONFLICT(owner) DO UPDATE SET records=excluded.records,revision=excluded.revision,updated_at_ms=excluded.updated_at_ms",
            params![owner, text, time(revision)?, time(now)?],
        )
        .map_err(sql)?;
    Ok(())
}

/// Park a payload, replacing a held one.
fn put_features(connection: &Connection, owner: &str, id: &str, features: &str, now: u64) -> Result<(), String> {
    let id = payload_id(id)?;
    let count = feature_count(features)?;
    connection
        .execute(
            "INSERT INTO local_layer_payloads(owner,layer_id,features,feature_count,updated_at_ms) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,layer_id) DO UPDATE SET features=excluded.features,feature_count=excluded.feature_count,updated_at_ms=excluded.updated_at_ms",
            params![owner, id, features, count as i64, time(now)?],
        )
        .map_err(sql)?;
    Ok(())
}

/// The id a record carries, verbatim, when it carries one.
fn record_id(record: &Value) -> Option<&str> {
    record.get("id").and_then(Value::as_str).filter(|id| !id.is_empty())
}

/// The GIS folder connections the held records reference.
fn referenced_folders(records: &[Value]) -> BTreeSet<String> {
    records
        .iter()
        .filter_map(|record| record.get("sourceMetadata"))
        .filter(|metadata| metadata.get("kind").and_then(Value::as_str) == Some("gis_folder"))
        .filter_map(|metadata| metadata.get("handleKey").and_then(Value::as_str))
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Delete payloads no catalogue record names; answers how many.
fn prune(connection: &Connection, owner: &str) -> Result<u64, String> {
    let records = read_catalogue(connection, owner)?.map(|(records, _)| records).unwrap_or_default();
    let known: Vec<&str> = records.iter().filter_map(record_id).collect();
    let named = serde_json::to_string(&known).map_err(|e| e.to_string())?;
    let pruned = connection
        .execute(
            "DELETE FROM local_layer_payloads WHERE owner=?1 AND layer_id NOT IN (SELECT value FROM json_each(?2))",
            params![owner, named],
        )
        .map_err(sql)?;
    Ok(pruned as u64)
}

fn sweep(connection: &Connection, owner: &str, present: &[String]) -> Result<Value, String> {
    let records = read_catalogue(connection, owner)?.map(|(records, _)| records).unwrap_or_default();
    let orphans = prune(connection, owner)?;
    let connections = crate::folder_connections::retain_referenced_gis(connection, owner, &referenced_folders(&records))?;
    let stale = connections.len();
    let mut drop = connections;
    drop.extend(crate::folder_connections::unreferenced_tokens(connection, present)?);
    drop.sort();
    drop.dedup();
    Ok(json!({"orphan_payloads": orphans, "stale_connections": stale, "drop": drop}))
}

pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("local layers carry their payloads as text, never as bytes"));
    }
    connection.execute_batch(&format!("{CATALOGUE};{PAYLOADS}")).map_err(sql)?;
    let owner = input.identity.owner();
    let outcome = match operation {
        Operation::Read => {
            let held = read_catalogue(connection, &owner)?;
            Outcome::read(fields(json!({
                "records": held.as_ref().map(|(records, _)| records),
                "revision": held.map_or(0, |(_, revision)| revision),
            })))
        }
        Operation::WriteCatalogue { records, revision, now } => {
            write_catalogue(connection, &owner, &records, revision, now)?;
            Outcome::written(Map::new())
        }
        Operation::PutFeatures { layer_id, features, now } => {
            put_features(connection, &owner, &layer_id, &features, now)?;
            let mut outcome = Outcome::written(Map::new());
            outcome.generation_key = Some(format!("{STORE}.payloads"));
            return Ok(outcome);
        }
        Operation::PutSnapshot {
            records,
            revision,
            layer_id,
            features,
            now,
        } => {
            write_catalogue(connection, &owner, &records, revision, now)?;
            put_features(connection, &owner, &layer_id, &features, now)?;
            Outcome::written(Map::new())
        }
        Operation::GetFeatures { layer_id } => {
            let features: Option<String> = connection
                .query_row(
                    "SELECT features FROM local_layer_payloads WHERE owner=?1 AND layer_id=?2",
                    params![owner, payload_id(&layer_id)?],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql)?;
            let mut outcome = Outcome::read(fields(json!({"features": features})));
            outcome.generation_key = Some(format!("{STORE}.payloads"));
            return Ok(outcome);
        }
        Operation::DeleteFeatures { layer_ids } => {
            if layer_ids.len() > MAX_IDS {
                return Err(invalid("a delete names at most 4096 payloads"));
            }
            let mut deleted = 0;
            for id in &layer_ids {
                deleted += connection
                    .execute(
                        "DELETE FROM local_layer_payloads WHERE owner=?1 AND layer_id=?2",
                        params![owner, payload_id(id)?],
                    )
                    .map_err(sql)?;
            }
            let mut outcome = Outcome::written(fields(json!({"deleted": deleted})));
            outcome.generation_key = Some(format!("{STORE}.payloads"));
            return Ok(outcome);
        }
        Operation::Sweep { present_tokens } => Outcome::written(fields(sweep(connection, &owner, &present_tokens)?)),
    };
    Ok(Outcome {
        generation_key: Some(format!("{STORE}.catalogue")),
        ..outcome
    })
}

#[cfg(test)]
#[path = "local_layer_store_tests.rs"]
mod tests;
