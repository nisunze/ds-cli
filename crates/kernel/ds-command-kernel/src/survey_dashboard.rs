//! The survey dashboard's numbers, aggregated on the edge.
//!
//! The dashboard report — totals per form, entries per day, per surveyor and
//! their spread, distinct surveyors per day, and the Rwanda administrative
//! breakdown — is a transform of survey rows the edge already holds, so it is
//! computed here (owner rule: survey dashboard aggregations run on an edge;
//! ds-brain only stores). The shape is exactly the report the dashboard
//! renders, so the CLI and the UI read one owner. Pure: the caller supplies the
//! held rows, the clock instant and the author; nothing here reads a store, a
//! clock or the network.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Map, Value};
use time::{format_description::well_known::Rfc3339, Date, Duration, OffsetDateTime};

pub const SCHEMA: &str = "ds.survey-dashboard/v1";
pub const REQUEST_INVALID: &str = "survey_dashboard_request_invalid";
/// Bounds: a large national project holds a few hundred thousand rows.
pub const MAX_FORMS: usize = 200;
pub const MAX_ROWS: usize = 2_000_000;
const MAX_TEXT: usize = 320;
const UNKNOWN: &str = "Unknown";

/// One form's held rows (GeoJSON features as `query_entries` returns them).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldForm {
    pub form: String,
    pub rows: Vec<Value>,
}

/// One aggregation: the held forms, whether the project is in Rwanda (the
/// administrative breakdown is Rwanda-only), and the stamp facts.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub forms: Vec<HeldForm>,
    #[serde(default)]
    pub rwanda: bool,
    pub generated_at: String,
    #[serde(default)]
    pub generated_by: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal {
        code: REQUEST_INVALID,
        message: message.into(),
    }
}

fn bounded(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT && !value.chars().any(char::is_control)
}

fn properties(row: &Value) -> Option<&Map<String, Value>> {
    row.get("properties")
        .and_then(Value::as_object)
        .or_else(|| row.as_object())
}

/// A member of the row's properties or of its `metadata`, as non-empty text.
fn field<'a>(properties: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    properties
        .get(key)
        .or_else(|| {
            properties
                .get("metadata")
                .and_then(|metadata| metadata.get(key))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn deleted(properties: &Map<String, Value>) -> bool {
    properties.get("is_deleted").and_then(Value::as_bool) == Some(true)
        || properties
            .get("metadata")
            .and_then(|metadata| metadata.get("is_deleted"))
            .and_then(Value::as_bool)
            == Some(true)
}

/// The UTC calendar day of a row's creation, from RFC 3339 text or epoch
/// milliseconds; a row without one is outside every per-day count.
fn created_day(properties: &Map<String, Value>) -> Option<Date> {
    let raw = properties.get("created_at").or_else(|| {
        properties
            .get("metadata")
            .and_then(|metadata| metadata.get("created_at"))
    })?;
    let instant = match raw {
        Value::String(text) => OffsetDateTime::parse(text.trim(), &Rfc3339).ok()?,
        Value::Number(number) => {
            let millis = number
                .as_i64()
                .or_else(|| number.as_f64().map(|n| n as i64))?;
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000).ok()?
        }
        _ => return None,
    };
    Some(instant.to_offset(time::UtcOffset::UTC).date())
}

fn day_text(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn admin(properties: &Map<String, Value>, level: &str) -> String {
    field(properties, &format!("admin_{level}"))
        .or_else(|| {
            properties
                .get("metadata")
                .and_then(|metadata| metadata.get("admin_bounds"))
                .and_then(|bounds| bounds.get(level))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .unwrap_or(UNKNOWN)
        .to_owned()
}

/// Median of integer totals as Brain computed it: the integer mean of the two
/// middle values for an even count.
fn spread(mut totals: Vec<u64>) -> Value {
    if totals.is_empty() {
        return json!({"median": 0, "min": 0, "max": 0});
    }
    totals.sort_unstable();
    let n = totals.len();
    let median = if n % 2 == 0 {
        (totals[n / 2 - 1] + totals[n / 2]) / 2
    } else {
        totals[n / 2]
    };
    json!({"median": median, "min": totals[0], "max": totals[n - 1]})
}

fn counts(map: &BTreeMap<String, u64>, total: u64) -> Value {
    let mut out: Map<String, Value> = map.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    out.insert("total".into(), json!(total));
    Value::Object(out)
}

/// Aggregate the held rows into the dashboard report.
pub fn aggregate(request: &Request) -> Result<Value, Refusal> {
    if request.forms.len() > MAX_FORMS {
        return Err(invalid(format!("more than {MAX_FORMS} forms")));
    }
    OffsetDateTime::parse(&request.generated_at, &Rfc3339)
        .map_err(|_| invalid("generated_at must be an RFC 3339 instant"))?;
    if request
        .generated_by
        .as_deref()
        .is_some_and(|by| !bounded(by))
    {
        return Err(invalid("generated_by is empty, unbounded or unsafe"));
    }
    let mut forms = BTreeSet::new();
    let mut rows = 0usize;
    for form in &request.forms {
        if !crate::survey::hold::valid_form_slug(&form.form) || !forms.insert(form.form.as_str()) {
            return Err(invalid(format!(
                "form {:?} is not one distinct form slug",
                form.form
            )));
        }
        rows += form.rows.len();
        if rows > MAX_ROWS {
            return Err(invalid(format!("more than {MAX_ROWS} rows")));
        }
    }

    let mut totals = Map::new();
    let mut grand = 0u64;
    let mut by_day: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut by_surveyor: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut surveyors_by_day: BTreeMap<Date, BTreeSet<String>> = BTreeMap::new();
    let mut by_admin: BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>> =
        BTreeMap::new();
    for form in &request.forms {
        let mut active = 0u64;
        for row in &form.rows {
            let Some(properties) = properties(row) else {
                return Err(invalid(format!("a {} row is not a feature", form.form)));
            };
            if deleted(properties) {
                continue;
            }
            active += 1;
            let day = created_day(properties);
            if let Some(day) = day {
                *by_day
                    .entry(day_text(day))
                    .or_default()
                    .entry(form.form.clone())
                    .or_default() += 1;
            }
            if let Some(surveyor) = field(properties, "created_by").filter(|s| bounded(s)) {
                *by_surveyor
                    .entry(surveyor.to_owned())
                    .or_default()
                    .entry(form.form.clone())
                    .or_default() += 1;
                if let Some(day) = day {
                    surveyors_by_day
                        .entry(day)
                        .or_default()
                        .insert(surveyor.to_owned());
                }
            }
            if request.rwanda {
                let district = admin(properties, "district");
                if district != UNKNOWN || properties.contains_key("admin_district") {
                    *by_admin
                        .entry(district)
                        .or_default()
                        .entry(admin(properties, "sector"))
                        .or_default()
                        .entry(admin(properties, "cell"))
                        .or_default()
                        .entry(admin(properties, "village"))
                        .or_default() += 1;
                }
            }
        }
        // A form with no active entry is not a dashboard form.
        if active > 0 {
            totals.insert(
                form.form.clone(),
                json!({"active": active, "deleted": 0, "total": active}),
            );
            grand += active;
        }
    }

    let by_day: Map<String, Value> = by_day
        .iter()
        .map(|(day, forms)| (day.clone(), counts(forms, forms.values().sum())))
        .collect();
    let surveyor_totals: Vec<u64> = by_surveyor.values().map(|f| f.values().sum()).collect();
    let by_surveyor: Map<String, Value> = by_surveyor
        .iter()
        .map(|(who, forms)| (who.clone(), counts(forms, forms.values().sum())))
        .collect();
    let active_by_day = match (
        surveyors_by_day.keys().next(),
        surveyors_by_day.keys().next_back(),
    ) {
        (Some(first), Some(last)) => {
            let mut series = Vec::new();
            let mut day = *first;
            while day <= *last {
                series.push(surveyors_by_day.get(&day).map_or(0, BTreeSet::len));
                day += Duration::days(1);
            }
            json!({"startDate": day_text(*first), "counts": {"all": series}})
        }
        _ => json!({}),
    };
    let by_admin: Map<String, Value> = by_admin
        .into_iter()
        .map(|(district, sectors)| {
            let mut district_total = 0u64;
            let sectors: Map<String, Value> = sectors
                .into_iter()
                .map(|(sector, cells)| {
                    let mut sector_total = 0u64;
                    let cells: Map<String, Value> = cells
                        .into_iter()
                        .map(|(cell, villages)| {
                            let cell_total: u64 = villages.values().sum();
                            sector_total += cell_total;
                            (cell, json!({"total": cell_total, "villages": villages}))
                        })
                        .collect();
                    district_total += sector_total;
                    (sector, json!({"total": sector_total, "cells": cells}))
                })
                .collect();
            (
                district,
                json!({"total": district_total, "sectors": sectors}),
            )
        })
        .collect();
    Ok(json!({
        "schema": SCHEMA,
        "generatedAt": request.generated_at,
        "generatedBy": request.generated_by,
        "source": "edge",
        "totals": totals,
        "activeGrandTotal": grand,
        "byDay": by_day,
        "bySurveyor": by_surveyor,
        "surveyorStats": spread(surveyor_totals),
        "activeSurveyorsByDay": active_by_day,
        "byAdmin": by_admin,
    }))
}

/// The JSON door: admit the request bytes, aggregate, answer the report.
pub fn evaluate(input: &[u8]) -> Result<String, Refusal> {
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| invalid(format!("invalid survey dashboard request: {error}")))?;
    let report = aggregate(&request)?;
    Ok(serde_json::to_string(&report).expect("a dashboard report encodes"))
}

#[cfg(test)]
#[path = "survey_dashboard_tests.rs"]
mod tests;
