//! What the core holds of one survey form, and what a read may do with it.
//!
//! The Server core (which is also the Desktop's core) keeps one read-only copy
//! per account, project and form: the rows the map loader reads, the filter
//! they were read under, when, and the cursor a delta refresh continues from.
//! The CLI, MCP and the Desktop window all read that one copy, so "what is held
//! here, under which filter, refreshed when" has one answer. It is a copy, not
//! a workspace: nothing is edited here and there is no outbox (the runtime
//! contract keeps Survey simple and rules out the old SQLite workspace).
//!
//! This module owns every decision over the copy: the filter and its
//! fingerprint, the manifest, reuse | delta | replace, the merge of a verified
//! stream, the local view, and the status projection. Hosts own bytes and
//! transport. The query plan and the stream's line grammar are
//! [`super::read`]'s (`plan`, `batch`, `finish`) and are not repeated here.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const SCHEMA: &str = "ds.survey.hold/v1";
/// The route and action every copy is read through: the map loader's own.
pub const QUERY_ACTION: &str = "query_entries";
/// `auto` reuses a copy younger than this without asking the cloud.
pub const DEFAULT_MAX_AGE_SECONDS: u64 = 300;
const MAX_SURVEYORS: usize = 200;
const MAX_TEXT: usize = 256;
const MAX_BOUNDARY_BYTES: usize = 1024 * 1024;
const BATCH_ROWS: usize = 64;

/// The working-area filter a copy was read under, in the fields `query_entries`
/// takes. The empty filter is the whole form.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_boundary: Option<AdminBoundary>,
    /// A GeoJSON Polygon or MultiPolygon drawn on the map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surveyors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdminBoundary {
    pub code: String,
    #[serde(default)]
    pub name: String,
}

impl Filter {
    /// The one spelling of this filter: validated, surveyors sorted and unique.
    pub fn canonical(mut self) -> Result<Filter, String> {
        if let Some([west, south, east, north]) = self.bbox {
            let finite = [west, south, east, north].iter().all(|v| v.is_finite());
            if !finite
                || !(-180.0..=180.0).contains(&west)
                || !(-180.0..=180.0).contains(&east)
                || !(-90.0..=90.0).contains(&south)
                || !(-90.0..=90.0).contains(&north)
                || west >= east
                || south >= north
            {
                return Err("bbox is not one WGS84 west,south,east,north rectangle".into());
            }
        }
        if let Some(admin) = &self.admin_boundary {
            if !bounded_text(&admin.code) || admin.name.len() > MAX_TEXT {
                return Err("the admin boundary is not one bounded code".into());
            }
        }
        if let Some(boundary) = &self.boundary {
            let kind = boundary["type"].as_str();
            if !matches!(kind, Some("Polygon" | "MultiPolygon"))
                || serde_json::to_vec(boundary).map_or(true, |b| b.len() > MAX_BOUNDARY_BYTES)
            {
                return Err("the drawn boundary is not one bounded GeoJSON polygon".into());
            }
        }
        for date in [&self.date_from, &self.date_to].into_iter().flatten() {
            let format = time::format_description::parse("[year]-[month]-[day]")
                .map_err(|_| "the date format is unavailable")?;
            if time::Date::parse(date, &format).is_err() {
                return Err("a date filter is not YYYY-MM-DD".into());
            }
        }
        if self.surveyors.len() > MAX_SURVEYORS || !self.surveyors.iter().all(|s| bounded_text(s)) {
            return Err("the surveyor filter is not a bounded list of names".into());
        }
        self.surveyors.sort();
        self.surveyors.dedup();
        Ok(self)
    }

    pub fn is_whole_form(&self) -> bool {
        self == &Filter::default()
    }

    /// Digest of the canonical filter, so two reads agree on "the same filter".
    pub fn fingerprint(&self) -> Result<String, String> {
        let value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        crate::report_export::jcs::sha256(&value)
    }

    /// The `query_entries` body fields this filter adds.
    pub fn query_fields(&self) -> Map<String, Value> {
        let mut fields = Map::new();
        if let Some(bbox) = self.bbox {
            fields.insert("bbox".into(), json!(bbox));
        }
        if let Some(admin) = &self.admin_boundary {
            fields.insert(
                "filter_admin_boundary".into(),
                json!({"code": admin.code, "name": admin.name}),
            );
        }
        if let Some(boundary) = &self.boundary {
            fields.insert("filter_boundary".into(), boundary.clone());
        }
        if let Some(date) = &self.date_from {
            fields.insert("date_from".into(), json!(date));
        }
        if let Some(date) = &self.date_to {
            fields.insert("date_to".into(), json!(date));
        }
        if !self.surveyors.is_empty() {
            fields.insert("surveyors".into(), json!(self.surveyors));
        }
        fields
    }

    /// Whether a copy read under `self` answers a read under `wanted`: the same
    /// filter, or the whole form and a request narrowed only by a box, which
    /// the view applies locally. Dates, surveyors and boundaries are the
    /// backend's to apply; they are never re-derived here.
    pub fn covers(&self, wanted: &Filter) -> bool {
        self == wanted
            || (self.is_whole_form()
                && Filter {
                    bbox: None,
                    ..wanted.clone()
                }
                .is_whole_form())
    }
}

fn bounded_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT && value.trim() == value
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    Replace,
    Delta,
}

/// The last refresh, as it happened.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LastRead {
    pub mode: ReadMode,
    pub at: String,
    pub received: u64,
    pub upserts: u64,
    pub deletes: u64,
}

/// What is held of one form.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub project: String,
    pub form: String,
    pub filter: Filter,
    pub filter_sha256: String,
    /// Live rows held; deleted entries are never held.
    pub rows: u64,
    /// The newest `metadata.firestore_updated_at` held: where a delta resumes.
    #[serde(default)]
    pub cursor: Option<String>,
    pub created_at: String,
    pub refreshed_at: String,
    pub last: LastRead,
    /// Digest of the rows file, as written.
    pub rows_sha256: String,
}

/// How a read may use what is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// Reuse a covering copy younger than `max_age_seconds`, else refresh
    /// the changes since its cursor, else read the form.
    Auto { max_age_seconds: u64 },
    /// Refresh the changes since the cursor (read the form when none is held).
    Delta,
    /// Read the form again under the filter.
    Full,
    /// Answer only from what is held; never reach the cloud.
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Answer from the held copy as it is.
    Reuse,
    /// Ask the cloud. `body` is the whole `query_entries` request.
    Query { mode: ReadMode, body: Value },
    /// `Refresh::Local` and nothing covering is held.
    NotHeld,
}

/// Decide what one read does, given what is held.
pub fn decide(
    project: &str,
    form: &str,
    held: Option<&Manifest>,
    wanted: &Filter,
    refresh: Refresh,
    now: OffsetDateTime,
) -> Result<Decision, String> {
    let covering = held.filter(|manifest| manifest.filter.covers(wanted));
    let mode = match (refresh, covering) {
        (Refresh::Local, Some(_)) => return Ok(Decision::Reuse),
        (Refresh::Local, None) => return Ok(Decision::NotHeld),
        (Refresh::Full, _) | (_, None) => ReadMode::Replace,
        (Refresh::Delta, Some(_)) => ReadMode::Delta,
        (Refresh::Auto { max_age_seconds }, Some(manifest)) => {
            let refreshed = OffsetDateTime::parse(&manifest.refreshed_at, &Rfc3339)
                .map_err(|_| "the held copy's refresh time is unreadable")?;
            let age = (now - refreshed).whole_seconds();
            if age >= 0 && (age as u64) < max_age_seconds {
                return Ok(Decision::Reuse);
            }
            ReadMode::Delta
        }
    };
    // A delta re-reads the held copy's own filter (which may be the whole form
    // answering a narrower view); a replace reads what was asked.
    let filter = match (mode, covering) {
        (ReadMode::Delta, Some(manifest)) => &manifest.filter,
        _ => wanted,
    };
    let cursor = covering.and_then(|manifest| manifest.cursor.clone());
    // The query itself is the read policy's: delta needs a cursor, and a delta
    // asks for tombstones so deletions leave the copy.
    let plan = super::read::evaluate(&json!({
        "kind": "plan", "project": project, "form": form, "scope": true,
        "allow_unscoped": true, "mode": match mode { ReadMode::Replace => "replace", ReadMode::Delta => "delta" },
        "pending_overlay_count": 0, "cutoff_clock": true, "saved_cutoff": cursor,
        "cached_cutoff": Value::Null, "skip_without_cutoff": false,
        "cached_count": covering.map_or(0, |manifest| manifest.rows),
        "checkpoint_replace": false,
    }))?;
    let (mode, updated_after) = match plan["updated_after"].as_str() {
        Some(after) if mode == ReadMode::Delta => (ReadMode::Delta, Some(after.to_owned())),
        // No cursor to continue from: a delta degrades to a full read.
        _ => (ReadMode::Replace, None),
    };
    let mut body = Map::new();
    body.insert("action".into(), json!(QUERY_ACTION));
    body.insert("eds_project_id".into(), json!(project));
    body.insert("form_slugs".into(), json!([form]));
    body.insert("exclude_deleted".into(), json!(mode == ReadMode::Replace));
    if let Some(after) = updated_after {
        body.insert("updated_after".into(), json!(after));
    }
    let filter = if mode == ReadMode::Replace {
        wanted
    } else {
        filter
    };
    body.extend(filter.query_fields());
    Ok(Decision::Query {
        mode,
        body: Value::Object(body),
    })
}

/// The copy after one verified read: its rows, in document order, and its
/// manifest (whose `rows_sha256` the host fills in once the rows are written).
pub struct Applied {
    pub rows: Vec<Value>,
    pub manifest: Manifest,
}

/// Apply one `query_entries` NDJSON body to what is held. The body is checked
/// line by line by the read policy (`batch`, then `finish`): a cut-off stream,
/// rows after the summary, or a summary that disagrees with the rows is
/// refused and the held copy is left as it was.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    project: &str,
    form: &str,
    held: Option<(&Manifest, Vec<Value>)>,
    filter: &Filter,
    mode: ReadMode,
    body: &[u8],
    now: OffsetDateTime,
) -> Result<Applied, String> {
    let text = std::str::from_utf8(body).map_err(|_| "the survey stream is not text")?;
    let mut lines: Vec<Value> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut row: Value = serde_json::from_str(line)
            .map_err(|_| "the survey stream has a line that is not JSON")?;
        if row["type"] == "Feature" {
            row["index"] = json!(lines.len());
        }
        lines.push(row);
    }
    let mode_text = match mode {
        ReadMode::Replace => "replace",
        ReadMode::Delta => "delta",
    };
    let (mut received, mut finished) = (0_u64, false);
    let mut newest = held
        .as_ref()
        .and_then(|(manifest, _)| manifest.cursor.clone());
    let (mut upserts, mut deletes) = (Vec::new(), Vec::new());
    for chunk in lines.chunks(BATCH_ROWS) {
        let result = super::read::evaluate(&json!({
            "kind": "batch", "mode": mode_text, "expected_form_count": 1,
            "received": received, "finished": finished, "current_cutoff": newest,
            "rows": chunk, "pending_doc_ids": [],
        }))?;
        received = result["received"]
            .as_u64()
            .ok_or("the read policy lost its count")?;
        finished = result["finished"].as_bool() == Some(true);
        newest = result["newest_cutoff"].as_str().map(str::to_owned);
        for index in result["upsert_indices"].as_array().into_iter().flatten() {
            upserts.push(
                index
                    .as_u64()
                    .ok_or("the read policy returned a bad index")? as usize,
            );
        }
        for id in result["delete_ids"].as_array().into_iter().flatten() {
            deletes.push(
                id.as_str()
                    .ok_or("the read policy returned a bad id")?
                    .to_owned(),
            );
        }
    }
    super::read::evaluate(&json!({"kind": "finish", "finished": finished}))?;

    // Held rows by document id, in their held order; a replace starts empty.
    let (created_at, mut order, mut by_id) = match (mode, held) {
        (ReadMode::Delta, Some((manifest, rows))) => {
            let mut order = Vec::with_capacity(rows.len());
            let mut by_id = HashMap::with_capacity(rows.len());
            for row in rows {
                let id = super::read::doc_id(&row).ok_or("a held row has no document id")?;
                order.push(id.clone());
                by_id.insert(id, row);
            }
            (manifest.created_at.clone(), order, by_id)
        }
        (_, held) => (
            held.map_or_else(|| stamp(now), |(manifest, _)| manifest.created_at.clone()),
            Vec::new(),
            HashMap::new(),
        ),
    };
    let upsert_count = upserts.len() as u64;
    for index in upserts {
        let mut row = lines
            .get(index)
            .cloned()
            .ok_or("the read policy named a row that is not in the stream")?;
        if let Some(object) = row.as_object_mut() {
            object.remove("index");
        }
        let id = super::read::doc_id(&row).ok_or("a survey row has no document id")?;
        if by_id.insert(id.clone(), row).is_none() {
            order.push(id);
        }
    }
    let delete_count = deletes.len() as u64;
    for id in &deletes {
        by_id.remove(id);
    }
    let rows: Vec<Value> = order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect();
    let manifest = Manifest {
        schema: SCHEMA.into(),
        project: project.into(),
        form: form.into(),
        filter: filter.clone(),
        filter_sha256: filter.fingerprint()?,
        rows: rows.len() as u64,
        cursor: newest,
        created_at,
        refreshed_at: stamp(now),
        last: LastRead {
            mode,
            at: stamp(now),
            received,
            upserts: upsert_count,
            deletes: delete_count,
        },
        rows_sha256: String::new(),
    };
    Ok(Applied { rows, manifest })
}

/// A local narrowing of held rows: a box and a change instant.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub bbox: Option<[f64; 4]>,
    pub updated_after: Option<String>,
}

impl View {
    /// Whether one held row is in the view. A row whose geometry has no
    /// coordinates is outside any box; one with no change time is outside any
    /// changed-after view.
    pub fn admits(&self, row: &Value) -> bool {
        if let Some([west, south, east, north]) = self.bbox {
            let Some([min_x, min_y, max_x, max_y]) = extent(&row["geometry"]) else {
                return false;
            };
            if max_x < west || min_x > east || max_y < south || min_y > north {
                return false;
            }
        }
        if let Some(after) = &self.updated_after {
            let Some(after) = OffsetDateTime::parse(after, &Rfc3339).ok() else {
                return false;
            };
            let changed = super::read::cutoff(row)
                .and_then(|value| OffsetDateTime::parse(&value, &Rfc3339).ok());
            if changed.is_none_or(|changed| changed <= after) {
                return false;
            }
        }
        true
    }
}

fn extent(geometry: &Value) -> Option<[f64; 4]> {
    fn walk(value: &Value, out: &mut Option<[f64; 4]>, depth: usize) {
        if depth > 6 {
            return;
        }
        if let Some(items) = value.as_array() {
            if let (Some(x), Some(y)) = (
                items.first().and_then(Value::as_f64),
                items.get(1).and_then(Value::as_f64),
            ) {
                let e = out.get_or_insert([x, y, x, y]);
                *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
                return;
            }
            items.iter().for_each(|item| walk(item, out, depth + 1));
        }
    }
    let mut out = None;
    walk(&geometry["coordinates"], &mut out, 0);
    if let Some(geometries) = geometry["geometries"].as_array() {
        for part in geometries {
            walk(&part["coordinates"], &mut out, 0);
        }
    }
    out
}

/// Held photos of one form, counted by the host from its media directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HeldMedia {
    pub originals: u64,
    pub thumbnails: u64,
}

/// What is held for a project: every form the project names (held or not),
/// and any held form it no longer names.
pub fn status(
    project: &str,
    forms: &[String],
    held: &[Manifest],
    media: &BTreeMap<String, HeldMedia>,
    now: OffsetDateTime,
) -> Value {
    let by_form: BTreeMap<&str, &Manifest> = held
        .iter()
        .map(|manifest| (manifest.form.as_str(), manifest))
        .collect();
    let mut names: Vec<&str> = forms.iter().map(String::as_str).collect();
    for form in by_form.keys() {
        if !names.contains(form) {
            names.push(form);
        }
    }
    let rows: Vec<Value> = names
        .into_iter()
        .map(|form| {
            let photos = media.get(form).copied().unwrap_or_default();
            match by_form.get(form) {
                None => json!({"form": form, "held": false, "in_project": true,
                                "photos": photos}),
                Some(manifest) => {
                    let age = OffsetDateTime::parse(&manifest.refreshed_at, &Rfc3339)
                        .map(|at| (now - at).whole_seconds().max(0))
                        .ok();
                    json!({
                        "form": form, "held": true,
                        "in_project": forms.iter().any(|f| f == form),
                        "rows": manifest.rows,
                        "filter": manifest.filter,
                        "whole_form": manifest.filter.is_whole_form(),
                        "filter_sha256": manifest.filter_sha256,
                        "refreshed_at": manifest.refreshed_at,
                        "age_seconds": age,
                        "cursor": manifest.cursor,
                        "last": manifest.last,
                        "photos": photos,
                    })
                }
            }
        })
        .collect();
    json!({
        "schema": SCHEMA,
        "project": project,
        "held_forms": held.len(),
        "held_rows": held.iter().map(|m| m.rows).sum::<u64>(),
        "forms": rows,
    })
}

fn stamp(now: OffsetDateTime) -> String {
    now.replace_nanosecond(0)
        .unwrap_or(now)
        .format(&Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    fn feature(id: &str, x: f64, updated: &str, deleted: bool) -> Value {
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":[x,-2.0]},
               "properties":{"id":id,"data":{"photo":"p"},
                             "metadata":{"firestore_updated_at":updated,"is_deleted":deleted}}})
    }

    fn stream(rows: &[Value]) -> Vec<u8> {
        let mut out = String::new();
        for row in rows {
            out.push_str(&row.to_string());
            out.push('\n');
        }
        out.push_str(
            &json!({"__type":"summary","form_count":1,"errors":[],"total_features":rows.len()})
                .to_string(),
        );
        out.into_bytes()
    }

    #[test]
    fn a_filter_has_one_spelling_and_says_what_it_covers() {
        let a = Filter {
            surveyors: vec!["b".into(), "a".into(), "a".into()],
            ..Filter::default()
        }
        .canonical()
        .unwrap();
        let b = Filter {
            surveyors: vec!["a".into(), "b".into()],
            ..Filter::default()
        }
        .canonical()
        .unwrap();
        assert_eq!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
        let whole = Filter::default();
        let boxed = Filter {
            bbox: Some([29.0, -3.0, 30.0, -2.0]),
            ..Filter::default()
        };
        assert!(whole.covers(&boxed), "the whole form answers a box locally");
        assert!(!boxed.covers(&whole));
        assert!(!whole.covers(&a), "surveyors are the backend's to apply");
        assert!(Filter {
            bbox: Some([30.0, -2.0, 29.0, -3.0]),
            ..Filter::default()
        }
        .canonical()
        .is_err());
        assert!(Filter {
            date_from: Some("yesterday".into()),
            ..Filter::default()
        }
        .canonical()
        .is_err());
    }

    #[test]
    fn a_read_reuses_refreshes_or_replaces_by_what_is_held() {
        let now = at("2026-09-25T10:00:00Z");
        let whole = Filter::default();
        assert!(matches!(
            decide("p", "f", None, &whole, Refresh::Local, now).unwrap(),
            Decision::NotHeld
        ));
        let Decision::Query { mode, body } = decide(
            "p",
            "f",
            None,
            &whole,
            Refresh::Auto {
                max_age_seconds: 300,
            },
            now,
        )
        .unwrap() else {
            panic!("nothing held must read the form")
        };
        assert_eq!(mode, ReadMode::Replace);
        assert_eq!(body["exclude_deleted"], true);
        assert!(body.get("updated_after").is_none());

        let applied = apply(
            "p",
            "f",
            None,
            &whole,
            ReadMode::Replace,
            &stream(&[feature("a", 29.5, "2026-09-25T09:00:00Z", false)]),
            at("2026-09-25T09:58:00Z"),
        )
        .unwrap();
        let held = applied.manifest;
        assert_eq!(
            decide(
                "p",
                "f",
                Some(&held),
                &whole,
                Refresh::Auto {
                    max_age_seconds: 300
                },
                now
            )
            .unwrap(),
            Decision::Reuse
        );
        let Decision::Query { mode, body } =
            decide("p", "f", Some(&held), &whole, Refresh::Delta, now).unwrap()
        else {
            panic!("a delta must query")
        };
        assert_eq!(mode, ReadMode::Delta);
        assert_eq!(body["updated_after"], "2026-09-25T09:00:00Z");
        assert_eq!(body["exclude_deleted"], false, "a delta must see deletions");
        let surveyed = Filter {
            surveyors: vec!["x".into()],
            ..Filter::default()
        };
        let Decision::Query { mode, body } = decide(
            "p",
            "f",
            Some(&held),
            &surveyed,
            Refresh::Auto {
                max_age_seconds: 300,
            },
            now,
        )
        .unwrap() else {
            panic!("another filter must query")
        };
        assert_eq!(mode, ReadMode::Replace);
        assert_eq!(body["surveyors"], json!(["x"]));
    }

    #[test]
    fn a_delta_upserts_and_drops_deleted_rows_and_a_broken_stream_changes_nothing() {
        let whole = Filter::default();
        let first = apply(
            "p",
            "f",
            None,
            &whole,
            ReadMode::Replace,
            &stream(&[
                feature("a", 29.1, "2026-09-25T09:00:00Z", false),
                feature("b", 29.2, "2026-09-25T09:01:00Z", false),
            ]),
            at("2026-09-25T09:02:00Z"),
        )
        .unwrap();
        assert_eq!(first.manifest.rows, 2);
        assert_eq!(
            first.manifest.cursor.as_deref(),
            Some("2026-09-25T09:01:00Z")
        );
        let delta = apply(
            "p",
            "f",
            Some((&first.manifest, first.rows.clone())),
            &whole,
            ReadMode::Delta,
            &stream(&[
                feature("b", 29.9, "2026-09-25T09:05:00Z", false),
                feature("a", 29.1, "2026-09-25T09:06:00Z", true),
                feature("c", 29.3, "2026-09-25T09:07:00Z", false),
            ]),
            at("2026-09-25T09:08:00Z"),
        )
        .unwrap();
        let ids: Vec<String> = delta
            .rows
            .iter()
            .filter_map(super::super::read::doc_id)
            .collect();
        assert_eq!(ids, ["b", "c"]);
        assert_eq!(delta.rows[0]["geometry"]["coordinates"][0], 29.9);
        assert_eq!(
            delta.manifest.cursor.as_deref(),
            Some("2026-09-25T09:07:00Z")
        );
        assert_eq!(delta.manifest.created_at, first.manifest.created_at);
        assert_eq!(
            (delta.manifest.last.upserts, delta.manifest.last.deletes),
            (2, 1)
        );
        let mut cut = stream(&[feature("d", 29.4, "2026-09-25T09:09:00Z", false)]);
        cut.truncate(cut.len() - 20);
        assert!(apply(
            "p",
            "f",
            Some((&delta.manifest, delta.rows.clone())),
            &whole,
            ReadMode::Delta,
            &cut,
            at("2026-09-25T09:10:00Z")
        )
        .is_err());
    }

    #[test]
    fn the_view_narrows_by_box_and_change_time() {
        let row = feature("a", 29.5, "2026-09-25T09:00:00Z", false);
        assert!(View {
            bbox: Some([29.0, -3.0, 30.0, -1.0]),
            ..View::default()
        }
        .admits(&row));
        assert!(!View {
            bbox: Some([30.0, -3.0, 31.0, -1.0]),
            ..View::default()
        }
        .admits(&row));
        assert!(View {
            updated_after: Some("2026-09-25T08:00:00Z".into()),
            ..View::default()
        }
        .admits(&row));
        assert!(!View {
            updated_after: Some("2026-09-25T09:00:00Z".into()),
            ..View::default()
        }
        .admits(&row));
    }

    #[test]
    fn status_names_held_and_unheld_forms() {
        let applied = apply(
            "p",
            "f",
            None,
            &Filter::default(),
            ReadMode::Replace,
            &stream(&[feature("a", 29.5, "2026-09-25T09:00:00Z", false)]),
            at("2026-09-25T09:00:30Z"),
        )
        .unwrap();
        let media = BTreeMap::from([(
            "f".to_owned(),
            HeldMedia {
                originals: 1,
                thumbnails: 1,
            },
        )]);
        let report = status(
            "p",
            &["f".into(), "g".into()],
            &[applied.manifest],
            &media,
            at("2026-09-25T09:01:00Z"),
        );
        assert_eq!(report["held_rows"], 1);
        assert_eq!(report["forms"][0]["age_seconds"], 30);
        assert_eq!(report["forms"][0]["photos"]["thumbnails"], 1);
        assert_eq!(report["forms"][1]["held"], false);
    }
}
