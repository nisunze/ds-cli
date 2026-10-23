//! The survey forms a delivered transformer report appends, produced on the
//! edge from the core's held copy.
//!
//! The cloud export (`ds-network-reporter` `handlers/export.rs`) appends each
//! form a project names in its settings as one spatial layer,
//! `survey_<form>`: the rows of the form's BigQuery survey view
//! (`{project}_survey_view.<form>`, built by ds-brain `view_service.go`) that
//! intersect the transformer's bounding rectangle, with their photos turned
//! into durable links. A local run appends the same layers from the core's
//! held copy of each form (`survey::hold`), so a report made on the Server,
//! the Desktop or by `ds report project export` carries the same forms.
//!
//! This module decides what that copy contributes and nothing else:
//!
//! * which forms the project appends ([`super::survey_append_forms`]);
//! * the layer each form becomes ([`survey_layer_name`], the one rule the
//!   cloud handler also calls);
//! * the view row a held entry becomes ([`SurveyViewColumns`],
//!   [`view_feature`]) — the survey view's own column vocabulary: `doc_id`,
//!   `created_at`, `created_by`, `connectivity_*` for a connected form, the
//!   five `detailed_location_*` members and one `data_<key>` per form field.
//!   The engine adds what the cloud adds after the view: the Rwanda
//!   `admin_*` columns and `form_slug`;
//! * which held rows a transformer's run stages ([`candidate_view`]): a box
//!   around the room, generous enough to hold the bounding rectangle the
//!   engine clips to exactly as the cloud does;
//! * the staged `ds.survey-layers/v1` document ([`survey_append`]) and the
//!   forms a run is made without, each with its reason.
//!
//! Hosts refresh the copy (`ds_project_data::survey_hold`), stage the bytes
//! and hand the engine the grant that turns photos into links; the engine
//! clips, stamps and writes.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::survey::hold::{self, Manifest, View};

/// The wire schema of the document a host stages beside a transformer's room
/// and the engine reads digest-bound (`survey_layers_asset`).
pub const SURVEY_LAYERS_SCHEMA: &str = "ds.survey-layers/v1";
/// The most features one staged document may carry, over every form.
pub const MAX_SURVEY_LAYER_FEATURES: usize = 100_000;
/// The most bytes one staged document may occupy: the engine's bound.
pub const MAX_SURVEY_LAYERS_BYTES: u64 = 64 * 1024 * 1024;
/// Degrees added around a room's extent when choosing the held rows a run
/// stages. The engine clips to the bounding rectangle exactly; this only
/// bounds the bytes. The rectangle is the transformer's points buffered by
/// 10 m (`print_layers::RECTANGLE_BUFFER_M`) and at most `10·√2` m outside
/// their extent; 0.002° is over 200 m everywhere a project can be.
pub const CANDIDATE_MARGIN_DEGREES: f64 = 0.002;

/// The `connectivity_*` columns a connected form's view carries, in the
/// view's order (`readFormSchema`, `has_connectivity`).
const CONNECTIVITY_COLUMNS: [&str; 4] = ["precedent", "antecedent", "parent_id", "distance_m"];
/// The `detailed_location_*` columns every form's view carries, in order.
const DETAILED_LOCATION_COLUMNS: [&str; 5] = [
    "accuracy",
    "altitude",
    "speed",
    "picked",
    "distance_to_pick",
];

/// The reason a run names for a form it was told to read only from the held
/// copy (`--survey-refresh local`) when this machine holds none.
pub const SURVEY_LAYER_NOT_HELD_REASON: &str =
    "survey layer: this machine holds no copy of the form and the run read only what is held";

/// The reason a run names for a form whose held copy could not be refreshed.
/// The cloud export likewise continues without a form whose read failed and
/// records the failure beside the result.
pub fn survey_layer_unrefreshed_reason(detail: &str) -> String {
    let detail: String = detail
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect();
    format!("survey layer: the held copy could not be refreshed ({detail})")
}

/// The layer one appended form becomes: `survey_` and the slug, lowercased,
/// every character outside `[a-z0-9_]` replaced by `_`, underscores trimmed;
/// `survey_form` when nothing is left. The cloud handler and the engine's
/// local path both call this, so the layer a report carries has one name.
pub fn survey_layer_name(form_slug: &str) -> String {
    let cleaned = form_slug
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if cleaned.is_empty() {
        "survey_form".into()
    } else {
        format!("survey_{cleaned}")
    }
}

/// The forms a project's receipt appends to a delivered transformer report,
/// as its settings spell them. Empty when the policy is off or names none.
pub fn survey_forms(receipt: &super::InputReceipt) -> Result<Vec<String>, String> {
    let sheets = receipt.sheets()?;
    Ok(sheets["project_settings"]
        .as_array()
        .map(|rows| super::survey_append_forms(rows))
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// The view row
// ---------------------------------------------------------------------------

/// The columns one form's survey view carries beyond its fixed ones, read
/// from every held row of the form. The cloud view derives them from the
/// form definition; a held copy has the entries, so a field is a column when
/// any held entry carries it — the same names, form-wide, so every row of a
/// layer has every column (null where the entry has no value), as a view row
/// does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SurveyViewColumns {
    /// The form carries connectivity, so its view has the four
    /// `connectivity_*` columns.
    pub connectivity: bool,
    /// `data` members, first-seen order. A member whose value is an object is
    /// a location (the view's geometry, never a column) or a map the view's
    /// scalar extraction cannot read; it has no column.
    pub data: Vec<String>,
}

impl SurveyViewColumns {
    pub fn of(rows: &[Value]) -> Self {
        let mut connectivity = false;
        let mut seen = BTreeSet::new();
        let mut objects = BTreeSet::new();
        let mut data = Vec::new();
        for row in rows {
            let properties = &row["properties"];
            connectivity |= properties["connectivity"].is_object();
            let Some(members) = properties["data"].as_object() else {
                continue;
            };
            for (key, value) in members {
                if value.is_object() {
                    objects.insert(key.clone());
                }
                if seen.insert(key.clone()) {
                    data.push(key.clone());
                }
            }
        }
        data.retain(|key| !objects.contains(key));
        Self { connectivity, data }
    }

    /// Every column the view row carries, in the view's order.
    pub fn names(&self) -> Vec<String> {
        let mut names = vec![
            "doc_id".to_string(),
            "created_at".to_string(),
            "created_by".to_string(),
        ];
        if self.connectivity {
            names.extend(
                CONNECTIVITY_COLUMNS
                    .iter()
                    .map(|c| format!("connectivity_{c}")),
            );
        }
        names.extend(
            DETAILED_LOCATION_COLUMNS
                .iter()
                .map(|c| format!("detailed_location_{c}")),
        );
        names.extend(self.data.iter().map(|key| format!("data_{key}")));
        names
    }
}

/// A scalar the view's `JSON_VALUE` extraction reads: an object or an array
/// reads as null.
fn scalar(value: &Value) -> Value {
    match value {
        Value::Object(_) | Value::Array(_) => Value::Null,
        other => other.clone(),
    }
}

/// A `data` member: a scalar, or a list — the view declares list fields
/// (photos, documents, multi-selects) as repeated columns.
fn data_value(value: &Value) -> Value {
    match value {
        Value::Object(_) => Value::Null,
        other => other.clone(),
    }
}

/// One held entry as its view row: a Feature with the entry's geometry and
/// the view's columns in the view's order.
pub fn view_feature(row: &Value, columns: &SurveyViewColumns) -> Value {
    let properties = &row["properties"];
    let mut out = Map::new();
    out.insert(
        "doc_id".into(),
        hold::held_row_id(row).map_or(Value::Null, Value::String),
    );
    out.insert(
        "created_at".into(),
        scalar(&properties["metadata"]["created_at"]),
    );
    out.insert(
        "created_by".into(),
        scalar(&properties["metadata"]["created_by"]),
    );
    if columns.connectivity {
        for column in CONNECTIVITY_COLUMNS {
            out.insert(
                format!("connectivity_{column}"),
                scalar(&properties["connectivity"][column]),
            );
        }
    }
    for column in DETAILED_LOCATION_COLUMNS {
        out.insert(
            format!("detailed_location_{column}"),
            scalar(&properties["detailed_location"][column]),
        );
    }
    for key in &columns.data {
        out.insert(format!("data_{key}"), data_value(&properties["data"][key]));
    }
    json!({
        "type": "Feature",
        "geometry": if row["geometry"].is_object() { row["geometry"].clone() } else { Value::Null },
        "properties": out,
    })
}

// ---------------------------------------------------------------------------
// The rows one run stages
// ---------------------------------------------------------------------------

fn extend_extent(value: &Value, extent: &mut Option<[f64; 4]>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Some(items) = value.as_array() else {
        return;
    };
    if let (Some(x), Some(y)) = (
        items.first().and_then(Value::as_f64),
        items.get(1).and_then(Value::as_f64),
    ) {
        if x.is_finite() && y.is_finite() {
            let e = extent.get_or_insert([x, y, x, y]);
            *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
        }
        return;
    }
    for item in items {
        extend_extent(item, extent, depth + 1);
    }
}

/// The box a transformer's run selects held rows by: the room's whole extent
/// widened by [`CANDIDATE_MARGIN_DEGREES`]. `None` when the room has no
/// coordinates, and then the run stages no rows — the cloud likewise appends
/// nothing when it has no rectangle to clip to.
pub fn candidate_view(room: &BTreeMap<String, Value>) -> Option<View> {
    let mut extent = None;
    for collection in room.values() {
        for feature in collection["features"].as_array().into_iter().flatten() {
            let geometry = &feature["geometry"];
            extend_extent(&geometry["coordinates"], &mut extent, 0);
            for part in geometry["geometries"].as_array().into_iter().flatten() {
                extend_extent(&part["coordinates"], &mut extent, 0);
            }
        }
    }
    let [west, south, east, north] = extent?;
    let margin = CANDIDATE_MARGIN_DEGREES;
    Some(View {
        bbox: Some([
            (west - margin).max(-180.0),
            (south - margin).max(-90.0),
            (east + margin).min(180.0),
            (north + margin).min(90.0),
        ]),
        updated_after: None,
    })
}

// ---------------------------------------------------------------------------
// What a run appends and omits
// ---------------------------------------------------------------------------

/// One form this machine holds, read once for a whole batch.
#[derive(Debug, Clone)]
pub struct HeldSurveyForm {
    pub form: String,
    /// Where the copy came from on this run: `held`, `delta` or `replace`.
    pub source: String,
    pub refreshed_at: String,
    pub rows: Vec<Value>,
    pub columns: SurveyViewColumns,
}

impl HeldSurveyForm {
    /// The copy read under the whole-form filter a report needs: a copy held
    /// under any narrower filter is not the form, and is refused here rather
    /// than appended as if it were.
    pub fn new(source: &str, manifest: &Manifest, rows: Vec<Value>) -> Result<Self, String> {
        if !manifest.filter.is_whole_form() {
            return Err(format!(
                "the held copy of {} is narrowed by a filter; a report appends the whole form",
                manifest.form
            ));
        }
        if rows.len() as u64 != manifest.rows {
            return Err(format!(
                "the held copy of {} does not hold the rows its manifest names",
                manifest.form
            ));
        }
        Ok(Self {
            form: manifest.form.clone(),
            source: source.to_string(),
            refreshed_at: manifest.refreshed_at.clone(),
            columns: SurveyViewColumns::of(&rows),
            rows,
        })
    }
}

/// One form a run's sheet is produced without, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurveyFormOmission {
    pub form: String,
    pub reason: String,
}

/// One form a run appends, as planned: what the host staged for the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurveyLayerPlan {
    pub form: String,
    pub layer: String,
    /// `held`, `delta` or `replace`: how the copy was read for this run.
    pub source: String,
    pub refreshed_at: String,
    /// Rows the whole held copy carries.
    pub held_rows: u64,
    /// Rows staged for this transformer (inside the candidate box).
    pub staged: u64,
}

/// One form a run appended, as the receipt records it: the plan and the
/// features the engine kept inside the bounding rectangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurveyLayerReceipt {
    pub form: String,
    pub layer: String,
    pub source: String,
    pub refreshed_at: String,
    pub held_rows: u64,
    pub staged: u64,
    /// Features on the layer. A form with none inside the rectangle adds no
    /// layer, as in the cloud, and says so here with zero.
    pub features: u64,
}

/// What the engine says it appended for one form. Additive members from a
/// newer engine are accepted (owner 22 Oct).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SurveyLayerResult {
    pub form: String,
    pub layer: String,
    pub features: u64,
}

/// A host's resolution of the project's survey append for one transformer.
#[derive(Debug, Clone, Default)]
pub struct SurveyAppend {
    /// The staged `ds.survey-layers/v1` bytes and their SHA-256, when any
    /// form is appended.
    pub document: Option<(Vec<u8>, String)>,
    pub layers: Vec<SurveyLayerPlan>,
    pub omitted: Vec<SurveyFormOmission>,
}

/// The survey append for one transformer: every held form's rows inside the
/// room's candidate box as view rows, in one `ds.survey-layers/v1` document,
/// and the forms the host could not hold, named with their reasons.
pub fn survey_append(
    room: &BTreeMap<String, Value>,
    held: &[HeldSurveyForm],
    omitted: &[SurveyFormOmission],
) -> Result<SurveyAppend, String> {
    let mut forms = BTreeSet::new();
    for name in held
        .iter()
        .map(|form| form.form.as_str())
        .chain(omitted.iter().map(|omission| omission.form.as_str()))
    {
        if !forms.insert(name.trim().to_ascii_lowercase()) {
            return Err(format!("survey form {name:?} is named more than once"));
        }
    }
    if held.is_empty() {
        return Ok(SurveyAppend {
            document: None,
            layers: Vec::new(),
            omitted: omitted.to_vec(),
        });
    }
    let view = candidate_view(room);
    let mut entries = Vec::with_capacity(held.len());
    let mut layers = Vec::with_capacity(held.len());
    let mut total = 0_usize;
    let mut layer_names = BTreeSet::new();
    for form in held {
        let layer = survey_layer_name(&form.form);
        if !layer_names.insert(layer.clone()) {
            return Err(format!(
                "survey forms name the same report layer {layer:?} twice"
            ));
        }
        let features: Vec<Value> = match &view {
            Some(view) => form
                .rows
                .iter()
                .filter(|row| view.admits(row))
                .map(|row| view_feature(row, &form.columns))
                .collect(),
            None => Vec::new(),
        };
        total = total
            .checked_add(features.len())
            .filter(|total| *total <= MAX_SURVEY_LAYER_FEATURES)
            .ok_or_else(|| {
                format!(
                    "the survey layers staged for one transformer exceed {MAX_SURVEY_LAYER_FEATURES} features"
                )
            })?;
        layers.push(SurveyLayerPlan {
            form: form.form.clone(),
            layer: layer.clone(),
            source: form.source.clone(),
            refreshed_at: form.refreshed_at.clone(),
            held_rows: form.rows.len() as u64,
            staged: features.len() as u64,
        });
        entries.push(json!({"form": form.form, "layer": layer, "features": features}));
    }
    let bytes = serde_json::to_vec(&json!({
        "schema": SURVEY_LAYERS_SCHEMA,
        "forms": entries,
    }))
    .map_err(|error| format!("could not encode the survey layers: {error}"))?;
    if bytes.len() as u64 > MAX_SURVEY_LAYERS_BYTES {
        return Err(format!(
            "the survey layers staged for one transformer are {} bytes, over the 64 MiB bound",
            bytes.len()
        ));
    }
    let sha256 = super::sha256_hex(&bytes);
    Ok(SurveyAppend {
        document: Some((bytes, sha256)),
        layers,
        omitted: omitted.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survey::hold::{Filter, LastRead, ReadMode};

    fn entry(id: &str, x: f64, data: Value) -> Value {
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":[x,-2.5]},
               "properties":{"id":id,"doc_id":id,"data":data,
                             "detailed_location":{"accuracy":2.5,"altitude":1700.0,"speed":0.1,
                                                  "picked":false,"location":{"type":"Point","coordinates":[x,-2.5]}},
                             "metadata":{"created_at":"2025-12-03T11:33:33.984Z","created_by":"a@b.c"},
                             "form_slug":"transformer_survey","geometry":"{}","can_edit_this_entry":true}})
    }

    fn manifest(form: &str, rows: u64, filter: Filter) -> Manifest {
        Manifest {
            schema: hold::SCHEMA.into(),
            project: "p".into(),
            form: form.into(),
            filter_sha256: filter.fingerprint().unwrap(),
            filter,
            rows,
            cursor: None,
            created_at: "2026-09-25T00:00:00Z".into(),
            refreshed_at: "2026-09-25T01:00:00Z".into(),
            last: LastRead {
                mode: ReadMode::Replace,
                at: "2026-09-25T01:00:00Z".into(),
                received: rows,
                upserts: rows,
                deletes: 0,
            },
            rows_sha256: "0".repeat(64),
        }
    }

    #[test]
    fn a_layer_is_named_as_the_cloud_names_it() {
        assert_eq!(
            survey_layer_name("transformer_survey"),
            "survey_transformer_survey"
        );
        assert_eq!(survey_layer_name(" Field-Notes "), "survey_field_notes");
        assert_eq!(survey_layer_name("__"), "survey_form");
    }

    #[test]
    fn a_held_entry_becomes_its_view_row_in_the_view_order() {
        let rows = vec![
            entry(
                "a",
                29.78,
                json!({"photo":"projects/p/forms/f/a/photo/1.jpg","size":"25kVA"}),
            ),
            entry(
                "b",
                29.79,
                json!({"size":"50kVA","photos":["x.jpg","y.jpg"],"where":{"type":"Point","coordinates":[1,2]}}),
            ),
        ];
        let columns = SurveyViewColumns::of(&rows);
        assert!(!columns.connectivity);
        assert_eq!(columns.data, ["photo", "size", "photos"]);
        let row = view_feature(&rows[0], &columns);
        let keys: Vec<&str> = row["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, columns.names());
        assert_eq!(
            keys[..8],
            [
                "doc_id",
                "created_at",
                "created_by",
                "detailed_location_accuracy",
                "detailed_location_altitude",
                "detailed_location_speed",
                "detailed_location_picked",
                "detailed_location_distance_to_pick"
            ]
        );
        assert_eq!(row["properties"]["doc_id"], "a");
        assert_eq!(row["properties"]["created_by"], "a@b.c");
        assert_eq!(row["properties"]["detailed_location_accuracy"], 2.5);
        assert!(row["properties"]["detailed_location_distance_to_pick"].is_null());
        assert!(row["properties"]["data_photos"].is_null());
        assert_eq!(row["geometry"]["coordinates"][0], 29.78);
        let second = view_feature(&rows[1], &columns);
        assert_eq!(
            second["properties"]["data_photos"],
            json!(["x.jpg", "y.jpg"])
        );
        assert!(second["properties"]["data_photo"].is_null());
        assert!(second["properties"].get("data_where").is_none());
        // A connected form carries the four connectivity columns on every row.
        let mut connected = rows.clone();
        connected[1]["properties"]["connectivity"] = json!({"precedent":"a","distance_m":12.5});
        let columns = SurveyViewColumns::of(&connected);
        assert!(columns.connectivity);
        let first = view_feature(&connected[0], &columns);
        assert!(first["properties"]["connectivity_precedent"].is_null());
        assert_eq!(
            view_feature(&connected[1], &columns)["properties"]["connectivity_distance_m"],
            12.5
        );
    }

    #[test]
    fn a_run_stages_the_rows_around_its_room_and_names_what_it_omits() {
        let room = BTreeMap::from([(
            "lv_poles".to_string(),
            json!({"type":"FeatureCollection","features":[
                {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[29.780,-2.5]}},
                {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[29.781,-2.5],[29.782,-2.501]]}}]}),
        )]);
        let rows = vec![
            entry("near", 29.7825, json!({"size":"25kVA"})),
            entry("far", 29.9, json!({"size":"50kVA"})),
        ];
        let held = HeldSurveyForm::new(
            "held",
            &manifest("transformer_survey", 2, Filter::default()),
            rows.clone(),
        )
        .unwrap();
        let omitted = vec![SurveyFormOmission {
            form: "field_notes".into(),
            reason: SURVEY_LAYER_NOT_HELD_REASON.into(),
        }];
        let append = survey_append(&room, std::slice::from_ref(&held), &omitted).unwrap();
        let (bytes, sha256) = append.document.as_ref().unwrap();
        assert_eq!(*sha256, super::super::sha256_hex(bytes));
        let document: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(document["schema"], SURVEY_LAYERS_SCHEMA);
        assert_eq!(document["forms"][0]["layer"], "survey_transformer_survey");
        let staged = document["forms"][0]["features"].as_array().unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0]["properties"]["doc_id"], "near");
        assert_eq!(append.layers[0].held_rows, 2);
        assert_eq!(append.layers[0].staged, 1);
        assert_eq!(append.omitted, omitted);

        // A form is appended or omitted, never both; nothing held stages no
        // document; a narrowed copy is not the form.
        assert!(survey_append(
            &room,
            &[held],
            &[SurveyFormOmission {
                form: "Transformer_Survey".into(),
                reason: "x".into()
            }]
        )
        .is_err());
        assert!(survey_append(&room, &[], &omitted)
            .unwrap()
            .document
            .is_none());
        let narrowed = Filter {
            bbox: Some([29.0, -3.0, 30.0, -2.0]),
            ..Filter::default()
        };
        assert!(
            HeldSurveyForm::new("held", &manifest("transformer_survey", 2, narrowed), rows)
                .is_err()
        );
        // A room with no coordinates stages the form with no rows.
        let empty = BTreeMap::from([(
            "lv_poles".to_string(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":null}]}),
        )]);
        assert!(candidate_view(&empty).is_none());
    }
}
