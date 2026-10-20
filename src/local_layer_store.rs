//! The prepared local layer store: the catalogue a machine keeps and each
//! layer's feature payload, in the Kernel's SQLite, fenced by identity.
//!
//! What a catalogue may hold, what a change does and what a removal may delete
//! are [`crate::local_layers`]' answers. This store keeps exactly the records
//! the host holds after those answers (the admitted registry and any rows a
//! reopen carried verbatim) and the payload each layer parks under its id.
//! A payload is opaque GeoJSON feature text: the store admits that it is one
//! bounded JSON array and counts its features, and never reads a geometry.
//!
//! The browser kept all of this in IndexedDB (`ds-web` / `app`). Each identity
//! imports that store once, in batches ([`crate::import_batches`]): the
//! retired catalogue merges behind any newer one, payloads never replace a
//! held payload, and the finishing call prunes payloads no record names
//! before it records the receipt. The source stays untouched.
use crate::durable_store::{held_receipt, Identity, Input, Outcome};
use crate::import_batches::{self, Batch};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::{Deserializer, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};

pub const STORE: &str = "local_layer_store";
pub const INVALID: &str = "local_layer_store_invalid";
/// The retired browser store every import receipt names.
pub const LEGACY_SOURCE: &str = "ds-web/app";
const LEGACY_DATABASE: &str = "ds-web";
const LEGACY_STORE: &str = "app";
/// The retired keys, in the order the host sends them: the catalogue before
/// the payloads it names.
pub const LEGACY_META_KEY: &str = "sketchLayersMeta";
pub const LEGACY_INLINE_KEY: &str = "sketchLayers";
pub const LEGACY_PAYLOAD_PREFIX: &str = "sketchLayerFeatures:";
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

/// One retired entry as the host read it: the key and the stored text.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyEntry {
    pub key: String,
    pub value: Value,
}

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
    /// Whether this identity's one-time import still has to run, and the
    /// retired keys the host reads, read-only.
    Legacy,
    /// One batch of retired entries, catalogue keys first.
    ImportEntries { entries: Vec<LegacyEntry>, now: u64 },
    /// Prune payloads no record names and record the receipt.
    ImportFinish { now: u64 },
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

/// Park a payload; `replace` false never touches a held one. Answers whether
/// a row was written.
fn put_features(connection: &Connection, owner: &str, id: &str, features: &str, now: u64, replace: bool) -> Result<bool, String> {
    let id = payload_id(id)?;
    let count = feature_count(features)?;
    let statement = if replace {
        "INSERT INTO local_layer_payloads(owner,layer_id,features,feature_count,updated_at_ms) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,layer_id) DO UPDATE SET features=excluded.features,feature_count=excluded.feature_count,updated_at_ms=excluded.updated_at_ms"
    } else {
        "INSERT INTO local_layer_payloads(owner,layer_id,features,feature_count,updated_at_ms) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,layer_id) DO NOTHING"
    };
    let changed = connection
        .execute(statement, params![owner, id, features, count as i64, time(now)?])
        .map_err(sql)?;
    Ok(changed > 0)
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

/// A reopened catalogue and the payloads it parks, by layer id.
type Split = (Vec<Value>, Vec<(String, String)>);

/// The oldest shape carried each row's features inline. Reopen it through
/// `local_layers` exactly as the browser did, and park each admitted layer's
/// features under the id the reopen gave it: a row's own id names its
/// features, rows without one take the unnamed payloads in order. A refused
/// reopen keeps the rows exactly as they were.
fn split_inline(records: &[Value], now: u64) -> Result<Option<Split>, String> {
    let request = json!({
        "schema": crate::local_layers::SCHEMA,
        "op": {"kind": "restore", "scope": {"host": "browser"}, "records": records, "now_ms": now},
    });
    let Ok(answer) = crate::local_layers::evaluate(request.to_string().as_bytes()) else {
        return Ok(None);
    };
    let answer: Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
    let Some(layers) = answer["registry"]["layers"].as_array() else {
        return Ok(None);
    };
    let mut named: HashMap<&str, &Value> = HashMap::new();
    let mut unnamed: Vec<&Value> = vec![];
    for record in records {
        let features = record.get("features").unwrap_or(&Value::Null);
        match record_id(record) {
            Some(id) => {
                named.insert(id, features);
            }
            None => unnamed.push(features),
        }
    }
    let mut unnamed = unnamed.into_iter();
    let mut payloads = vec![];
    for layer in layers {
        let id = layer["id"].as_str().ok_or_else(|| invalid("a reopened layer has no id"))?;
        let features = named
            .get(id)
            .copied()
            .or_else(|| unnamed.next())
            .filter(|features| features.is_array())
            .cloned()
            .unwrap_or_else(|| json!([]));
        payloads.push((id.to_owned(), features.to_string()));
    }
    let mut catalogue = layers.clone();
    if let Some(carried) = answer["receipt"]["carried"].as_array() {
        catalogue.extend(carried.iter().map(|row| row["raw"].clone()));
    }
    Ok(Some((catalogue, payloads)))
}

/// Merge retired records behind the held catalogue: a held id stays.
fn merge(connection: &Connection, owner: &str, legacy: Vec<Value>, now: u64) -> Result<(), String> {
    let (mut merged, revision) = read_catalogue(connection, owner)?.unwrap_or((vec![], 0));
    let held: BTreeSet<String> = merged.iter().filter_map(record_id).map(str::to_owned).collect();
    merged.extend(legacy.into_iter().filter(|record| record_id(record).is_none_or(|id| !held.contains(id))));
    write_catalogue(connection, owner, &merged, revision, now)
}

/// The retired catalogue text, read as the browser read it.
fn legacy_records(value: &Value) -> Option<Vec<Value>> {
    serde_json::from_str::<Vec<Value>>(value.as_str()?).ok()
}

fn import_entries(connection: &Connection, identity: &Identity, owner: &str, entries: Vec<LegacyEntry>, now: u64) -> Result<Value, String> {
    import_batches::open(connection, identity, STORE, LEGACY_SOURCE)?;
    if entries.len() > import_batches::MAX_ENTRIES {
        return Err(invalid("a batch carries at most 4096 entries"));
    }
    let meta = entries.iter().any(|entry| entry.key == LEGACY_META_KEY);
    let (mut count, mut skipped) = (0u64, 0u64);
    for entry in &entries {
        let written = match entry.key.as_str() {
            LEGACY_META_KEY => match legacy_records(&entry.value) {
                Some(records) => merge(connection, owner, records, now).map(|()| true)?,
                None => false,
            },
            // The browser read the oldest key only when no catalogue key existed.
            LEGACY_INLINE_KEY if !meta => match legacy_records(&entry.value) {
                Some(records) => match split_inline(&records, now)? {
                    Some((catalogue, payloads)) => {
                        merge(connection, owner, catalogue, now)?;
                        for (id, features) in payloads {
                            put_features(connection, owner, &id, &features, now, false)?;
                        }
                        true
                    }
                    None => merge(connection, owner, records, now).map(|()| true)?,
                },
                None => false,
            },
            key => match (key.strip_prefix(LEGACY_PAYLOAD_PREFIX), entry.value.as_str()) {
                (Some(id), Some(features)) if payload_id(id).is_ok() && feature_count(features).is_ok() => {
                    put_features(connection, owner, id, features, now, false)?
                }
                _ => false,
            },
        };
        if written {
            count += 1;
        } else {
            skipped += 1;
        }
    }
    let canonical = serde_json::to_string(&entries).map_err(|e| e.to_string())?;
    import_batches::tally(connection, identity, STORE, LEGACY_SOURCE, Batch { entries: &canonical, count, skipped })?;
    Ok(json!({"written": count, "skipped": skipped}))
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
    let identity = input.identity;
    let owner = identity.owner();
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
            put_features(connection, &owner, &layer_id, &features, now, true)?;
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
            put_features(connection, &owner, &layer_id, &features, now, true)?;
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
        Operation::Legacy => Outcome::read(fields(json!({
            "import_needed": held_receipt(connection, identity, STORE, LEGACY_SOURCE)?.is_none(),
            "legacy": {"database": LEGACY_DATABASE, "stores": [{"store": LEGACY_STORE,
                "keys": [LEGACY_META_KEY, LEGACY_INLINE_KEY], "prefixes": [LEGACY_PAYLOAD_PREFIX]}]},
        }))),
        Operation::ImportEntries { entries, now } => {
            time(now)?;
            Outcome::written(fields(import_entries(connection, identity, &owner, entries, now)?))
        }
        Operation::ImportFinish { now } => {
            time(now)?;
            let pruned = match held_receipt(connection, identity, STORE, LEGACY_SOURCE)? {
                Some(_) => 0,
                None => prune(connection, &owner)?,
            };
            let (receipt, ran) = import_batches::finish(connection, identity, STORE, LEGACY_SOURCE, now, pruned)?;
            let answer = fields(json!({"imported": ran, "receipt": receipt}));
            if ran {
                Outcome::written(answer)
            } else {
                Outcome::read(answer)
            }
        }
    };
    Ok(Outcome {
        generation_key: Some(format!("{STORE}.catalogue")),
        ..outcome
    })
}

#[cfg(test)]
#[path = "local_layer_store_tests.rs"]
mod tests;
