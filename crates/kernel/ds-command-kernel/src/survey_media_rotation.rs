//! A survey photo's rotation, as entry metadata.
//!
//! The camera original is immutable (`survey_photo_media`). What a person
//! turns is recorded on the ENTRY the photo belongs to, in one system-owned
//! data key, `data.ds_media_rotation`: a map from each rotated original's
//! canonical object path to its net clockwise quarter-turns and the storage
//! generation those turns were chosen against:
//!
//! ```json
//! {"ds_media_rotation": {"p1/forms/poles/e1/a.jpg": {"quarter_turns": 1, "generation": "1729"}}}
//! ```
//!
//! A survey create is a direct, offline-first Firestore write and may carry
//! the map; every later change is an ordinary `updated_data` change through
//! ds-brain's mutate under the entry's `expected_version`. A record whose
//! generation is not the one the reader holds is not applied: it was chosen
//! for different bytes. An unrotated photo has no record. Turns accumulate:
//! a further turn while an earlier one is still queued simply adds to the
//! net, so a pending turn never refuses another (`turn`).
//!
//! The thumbnail companion is regenerated from the original plus this net
//! turn (`survey_photo_media::thumbnail`) and records the turns it shows in
//! its object metadata (`source_quarter_turns`, ds-brain).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::survey_media_address;

pub const SCHEMA: &str = "ds.survey-media-rotation/v1";
/// The one system-owned entry data key; hosts never show it as a field.
pub const KEY: &str = "ds_media_rotation";
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// More rotated photos than any entry carries; a larger map is refused.
pub const MAX_RECORDS: usize = 256;

/// One original's rotation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Net clockwise quarter-turns, 1..=3 (0 is no record).
    pub quarter_turns: u8,
    /// The storage generation of the original the turns were chosen for.
    pub generation: String,
}

/// A positive decimal storage generation.
pub fn valid_generation(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|b| b.is_ascii_digit())
        && !value.starts_with('0')
}

/// The entry's data object: `properties.data` of a feature, a bare data
/// object, or a mirror row's top-level properties.
pub fn data_of(entry: &Value) -> Option<&Map<String, Value>> {
    let properties = entry
        .get("properties")
        .filter(|v| v.is_object())
        .unwrap_or(entry);
    properties
        .get("data")
        .and_then(Value::as_object)
        .or_else(|| properties.as_object())
}

/// Every well-formed record the entry data carries; malformed records are
/// not applied (and are refused by `turn`, never silently overwritten).
pub fn records(data: &Map<String, Value>) -> Vec<(String, Record)> {
    let Some(map) = data.get(KEY).and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(path, record)| {
            let record: Record = serde_json::from_value(record.clone()).ok()?;
            ((1..=3).contains(&record.quarter_turns) && valid_generation(&record.generation))
                .then(|| (path.clone(), record))
        })
        .collect()
}

/// The record for one original, if well formed.
pub fn record(data: &Map<String, Value>, object_path: &str) -> Option<Record> {
    records(data)
        .into_iter()
        .find(|(path, _)| path == object_path)
        .map(|(_, record)| record)
}

/// The turns to show for an original held at `generation`. Without a held
/// generation (a reader that only has the entry) the record applies as is.
pub fn quarter_turns(data: &Map<String, Value>, object_path: &str, generation: Option<&str>) -> u8 {
    match record(data, object_path) {
        Some(record) if generation.is_none_or(|g| g == record.generation) => record.quarter_turns,
        _ => 0,
    }
}

/// A turn admitted against the entry: what was shown, what will be, and the
/// exact `updated_data` patch the mutate (or a queued create) carries.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Turned {
    pub object_path: String,
    pub thumbnail_object_path: String,
    pub generation: String,
    pub previous_quarter_turns: u8,
    pub quarter_turns: u8,
    pub updated_data: Value,
}

fn degrees_to_turns(degrees: i64) -> Result<u8, String> {
    match degrees {
        90 | -270 => Ok(1),
        180 | -180 => Ok(2),
        270 | -90 => Ok(3),
        _ => Err(format!(
            "{degrees} is not a net quarter-turn; choose 90, 180 or 270 (negative counter-clockwise)"
        )),
    }
}

/// Add one turn to what the entry records for `object_path` at the
/// generation the person saw. Refuses a derived thumbnail, a non-canonical
/// path, a malformed generation and a malformed existing map.
pub fn turn(
    data: &Map<String, Value>,
    object_path: &str,
    generation: &str,
    degrees: i64,
) -> Result<Turned, String> {
    let address = survey_media_address::parse(object_path)?;
    if address.object_path != object_path {
        return Err(format!(
            "{object_path} is not the canonical object path {}",
            address.object_path
        ));
    }
    if address.object_path == address.thumbnail_object_path {
        return Err("Thumbnail images cannot be rotated".into());
    }
    if !valid_generation(generation) {
        return Err("the original's storage generation must be a positive decimal".into());
    }
    let delta = degrees_to_turns(degrees)?;
    let mut map = match data.get(KEY) {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(format!("the entry's {KEY} is not an object")),
    };
    if map.len() > MAX_RECORDS
        || map.values().any(|record| {
            serde_json::from_value::<Record>(record.clone())
                .ok()
                .is_none_or(|r| !(1..=3).contains(&r.quarter_turns) || !valid_generation(&r.generation))
        })
    {
        return Err(format!("the entry's {KEY} holds a malformed record"));
    }
    let previous = quarter_turns(data, object_path, Some(generation));
    let net = (previous + delta) % 4;
    if net == 0 {
        map.remove(object_path);
    } else {
        map.insert(
            object_path.to_owned(),
            json!({"quarter_turns": net, "generation": generation}),
        );
    }
    if map.len() > MAX_RECORDS {
        return Err(format!("an entry holds at most {MAX_RECORDS} rotated photos"));
    }
    Ok(Turned {
        object_path: address.object_path,
        thumbnail_object_path: address.thumbnail_object_path,
        generation: generation.to_owned(),
        previous_quarter_turns: previous,
        quarter_turns: net,
        updated_data: json!({ KEY: Value::Object(map) }),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    op: Op,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    /// The turns to show for one original of this entry.
    Read {
        entry: Value,
        path: String,
        #[serde(default)]
        generation: Option<String>,
    },
    /// One more turn: the new net and the `updated_data` patch.
    Turn {
        entry: Value,
        path: String,
        generation: String,
        degrees: i64,
    },
}

/// The JSON door (`surveyMediaRotation` in WASM).
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("survey media rotation request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid survey media rotation request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!("unsupported survey media rotation schema; expected {SCHEMA}"));
    }
    let empty = Map::new();
    let value = match request.op {
        Op::Read {
            entry,
            path,
            generation,
        } => {
            let data = data_of(&entry).unwrap_or(&empty);
            json!({
                "object_path": path,
                "quarter_turns": quarter_turns(data, &path, generation.as_deref()),
                "record": record(data, &path),
            })
        }
        Op::Turn {
            entry,
            path,
            generation,
            degrees,
        } => json!(turn(
            data_of(&entry).unwrap_or(&empty),
            &path,
            &generation,
            degrees
        )?),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "survey_media_rotation_tests.rs"]
mod tests;
