//! Bounded Survey mirror-read policy. Hosts own BQ NDJSON transport and durable
//! cache IO; this module decides what a read may change.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const SCHEMA: &str = "ds.survey.read/v1";
const MAX_BATCH_ROWS: usize = 64;
const MAX_PENDING_IDS: usize = 128;
const RETRY_DELAYS_MS: [u64; 4] = [1_500, 5_000, 15_000, 30_000];

pub(super) fn evaluate(request: &Value) -> Result<Value, String> {
    match request["kind"].as_str() {
        Some("pending_headers") => pending_headers(request),
        Some("plan") => plan(request),
        Some("batch") => batch(request),
        Some("finish") => finish(request),
        Some("retry") => retry(request),
        Some("complete") => complete(request),
        Some("integrity") => integrity(request),
        Some("dashboard") => dashboard(request),
        _ => Err("Unknown survey read operation".into()),
    }
}

fn pending_headers(request: &Value) -> Result<Value, String> {
    let rows = request["rows"]
        .as_array()
        .ok_or("Pending headers are required")?;
    if rows.len() > 64 {
        return Err("Pending header batch exceeds 64 rows".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for row in rows {
        if row["project_id"] == request["project"]
            && row["form_id"] == request["form"]
            && row["owner_uid"] == request["owner_uid"]
            && row["owner_uid"].as_str().is_some_and(|s| !s.is_empty())
            && row["status"] != "synced"
            && ["create", "update", "delete", "restore"]
                .contains(&row["operation"].as_str().unwrap_or(""))
        {
            if let Some(id) = row["doc_id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 1500)
            {
                ids.insert(id);
            }
        }
    }
    Ok(json!(ids))
}

// Dashboard reports are already a bounded backend aggregation. The browser
// supplies that receipt, never entry rows, and only renders this projection.
fn dashboard(request: &Value) -> Result<Value, String> {
    let report = request["report"]
        .as_object()
        .ok_or("dashboard report is required")?;
    let action = text(request, "action")?;
    let totals = report
        .get("totals")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let forms = totals
        .iter()
        .filter_map(|(slug, total)| {
            let active = total["active"].as_i64().unwrap_or(0);
            let deleted = total["deleted"].as_i64().unwrap_or(0);
            let all = total["total"].as_i64().unwrap_or(0);
            (active > 0 || deleted > 0 || all > 0).then(|| slug.clone())
        })
        .collect::<Vec<_>>();
    match action {
        "kpis" => Ok(
            json!({"totalActive":report.get("activeGrandTotal").and_then(Value::as_i64).unwrap_or(0),"totalDeleted":report.get("deletedGrandTotal").and_then(Value::as_i64).unwrap_or(0),"numSurveyors":report.get("bySurveyor").and_then(Value::as_object).map_or(0,Map::len),"medianPerSurveyor":nested(report,&["surveyorStats","median"]).and_then(Value::as_i64).unwrap_or(0),"minPerSurveyor":nested(report,&["surveyorStats","min"]).and_then(Value::as_i64).unwrap_or(0),"maxPerSurveyor":nested(report,&["surveyorStats","max"]).and_then(Value::as_i64).unwrap_or(0),"numForms":forms.len()}),
        ),
        "forms" => Ok(json!(forms)),
        "surveyors" => {
            let mut rows = report
                .get("bySurveyor")
                .and_then(Value::as_object)
                .map(|v| v.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            rows.sort();
            Ok(json!(rows))
        }
        "date_range" => {
            let mut dates = report
                .get("byDay")
                .and_then(Value::as_object)
                .map(|v| v.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            dates.sort();
            Ok(
                json!({"min":dates.first().cloned().unwrap_or_default(),"max":dates.last().cloned().unwrap_or_default()}),
            )
        }
        "filtered_total" => {
            let form = request["form"].as_str();
            let surveyor = request["surveyor"].as_str();
            let value = match (form, surveyor) {
                (None, None) => report
                    .get("activeGrandTotal")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                (Some(f), None) => totals
                    .get(f)
                    .and_then(|v| v["active"].as_i64())
                    .unwrap_or(0),
                (None, Some(s)) => nested(report, &["bySurveyor", s, "total"])
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                (Some(f), Some(s)) => nested(report, &["bySurveyor", s, f])
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            };
            Ok(json!(value))
        }
        "form_table" => {
            let mut rows = totals
                .iter()
                .filter(|(_, v)| {
                    v["active"].as_i64().unwrap_or(0) > 0
                        || v["deleted"].as_i64().unwrap_or(0) > 0
                        || v["total"].as_i64().unwrap_or(0) > 0
                })
                .map(|(s, v)| {
                    (
                        s.clone(),
                        v["active"].as_i64().unwrap_or(0),
                        v["deleted"].as_i64().unwrap_or(0),
                        v["total"].as_i64().unwrap_or(0),
                    )
                })
                .collect::<Vec<_>>();
            rows.sort_by(|a, b| b.3.cmp(&a.3));
            Ok(
                json!({"headers":["Form","Active","Deleted","Total"],"rows":rows.into_iter().map(|(s,a,d,t)|json!([form_label(&s),a,d,t])).collect::<Vec<_>>() }),
            )
        }
        "daily_table" => daily_table(report, &forms, request),
        "surveyor_table" => surveyor_table(report, &forms),
        "daily_chart" => daily_chart(report, &forms, request),
        "surveyors_chart" => surveyors_chart(report, request),
        "top_surveyors" => top_surveyors(report, request),
        "form_distribution" => form_distribution(&totals),
        "surveyor_rows" => surveyor_rows(report, &forms),
        "daily_pivot" => daily_pivot(report, &forms, request),
        "admin_rows" => admin_rows(report.get("byAdmin").unwrap_or(&Value::Null)),
        "admin_table" => admin_table(report.get("byAdmin").unwrap_or(&Value::Null)),
        _ => Err("dashboard action is invalid".into()),
    }
}

fn nested<'a>(m: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    let mut value: Option<&Value> = None;
    for (key, i) in keys.iter().zip(0..) {
        value = if i == 0 {
            m.get(*key)
        } else {
            value.and_then(Value::as_object).and_then(|v| v.get(*key))
        };
    }
    value
}
fn form_label(slug: &str) -> String {
    slug.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|x| x.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn requested_dates(report: &Map<String, Value>, request: &Value) -> Vec<String> {
    let mut d: Vec<String> = report
        .get("byDay")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    d.sort();
    d.into_iter()
        .filter(|x| {
            request["date_from"]
                .as_str()
                .map_or(true, |v| x.as_str() >= v)
                && request["date_to"]
                    .as_str()
                    .map_or(true, |v| x.as_str() <= v)
        })
        .collect()
}
fn parse_day(value: &str) -> Option<time::Date> {
    let format = time::format_description::parse("[year]-[month]-[day]").ok()?;
    time::Date::parse(value, &format).ok()
}
fn daily_pivot(
    report: &Map<String, Value>,
    forms: &[String],
    request: &Value,
) -> Result<Value, String> {
    let by = report
        .get("byDay")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut f = forms.to_vec();
    f.sort();
    let rows = requested_dates(report, request)
        .into_iter()
        .map(|d| {
            let day = by.get(&d).and_then(Value::as_object);
            let mut p = Map::new();
            let mut total = 0i64;
            for s in &f {
                let n = day
                    .and_then(|v| v.get(s))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                if n > 0 {
                    p.insert(s.clone(), json!(n));
                }
                total += n;
            }
            json!({"date":d,"perForm":p,"total":total})
        })
        .collect::<Vec<_>>();
    Ok(json!({"rows":rows,"formSlugs":f}))
}
fn daily_table(report: &Map<String, Value>, forms: &[String], r: &Value) -> Result<Value, String> {
    let pivot = daily_pivot(report, forms, r)?;
    let f = pivot["formSlugs"]
        .as_array()
        .ok_or("dashboard forms invalid")?;
    let mut h = vec![json!("Date")];
    h.extend(
        f.iter()
            .filter_map(Value::as_str)
            .map(|s| json!(form_label(s))),
    );
    h.push(json!("Total"));
    let rows = pivot["rows"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|x| {
            let mut row = vec![x["date"].clone()];
            for s in f {
                row.push(
                    x["perForm"][s.as_str().unwrap_or("")]
                        .clone()
                        .as_i64()
                        .map_or(json!(0), |v| json!(v)),
                );
            }
            row.push(x["total"].clone());
            json!(row)
        })
        .collect::<Vec<_>>();
    Ok(json!({"headers":h,"rows":rows}))
}
fn surveyor_rows(report: &Map<String, Value>, forms: &[String]) -> Result<Value, String> {
    let by = report
        .get("bySurveyor")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut f = forms.to_vec();
    f.sort();
    let mut rows = by
        .into_iter()
        .map(|(email, c)| {
            let c = c.as_object();
            let total = c
                .and_then(|x| x.get("total"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let mut per = Map::new();
            for s in &f {
                let n = c
                    .and_then(|x| x.get(s))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                if n != 0 {
                    per.insert(s.clone(), json!(n));
                }
            }
            json!({"email":email,"total":total,"perForm":per})
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| b["total"].as_i64().cmp(&a["total"].as_i64()));
    Ok(json!({"rows":rows,"formSlugs":f}))
}
fn surveyor_table(report: &Map<String, Value>, forms: &[String]) -> Result<Value, String> {
    let data = surveyor_rows(report, forms)?;
    let f = data["formSlugs"]
        .as_array()
        .ok_or("dashboard forms invalid")?;
    let mut h = vec![json!("Surveyor")];
    h.extend(
        f.iter()
            .filter_map(Value::as_str)
            .map(|s| json!(form_label(s))),
    );
    h.push(json!("Total"));
    let rows = data["rows"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|x| {
            let mut row = vec![x["email"].clone()];
            for s in f {
                row.push(
                    x["perForm"][s.as_str().unwrap_or("")]
                        .clone()
                        .as_i64()
                        .map_or(json!(0), |v| json!(v)),
                );
            }
            row.push(x["total"].clone());
            json!(row)
        })
        .collect::<Vec<_>>();
    Ok(json!({"headers":h,"rows":rows}))
}
fn daily_chart(report: &Map<String, Value>, forms: &[String], r: &Value) -> Result<Value, String> {
    let by = report
        .get("byDay")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let dates = requested_dates(report, r);
    let filtered = r["form"]
        .as_str()
        .map(|x| vec![x.to_owned()])
        .unwrap_or_else(|| forms.to_vec());
    if let (Some(first), Some(last)) = (
        dates.first().and_then(|d| parse_day(d)),
        dates.last().and_then(|d| parse_day(d)),
    ) {
        if (last - first).whole_days() > 10_000 {
            return Err("Survey dashboard date span exceeds 10000 days".into());
        }
    }
    let bucket = r["bucket"].as_str().unwrap_or("day");
    let mut buckets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for date in &dates {
        let key = if bucket == "month" {
            date.get(..7).unwrap_or(date).to_owned()
        } else if bucket == "week" {
            parse_day(date)
                .map(|d| {
                    (d - time::Duration::days(d.weekday().number_days_from_monday() as i64))
                        .to_string()
                })
                .unwrap_or_else(|| date.clone())
        } else {
            date.clone()
        };
        buckets.entry(key).or_default().push(date.clone());
    }
    if bucket == "day" {
        if let (Some(first), Some(last)) = (
            dates.first().and_then(|d| parse_day(d)),
            dates.last().and_then(|d| parse_day(d)),
        ) {
            let mut d = first;
            while d <= last {
                buckets.entry(d.to_string()).or_default();
                if d == last {
                    break;
                }
                d += time::Duration::days(1)
            }
        }
    }
    if bucket == "week" {
        if let (Some(first), Some(last)) = (
            dates.first().and_then(|d| parse_day(d)),
            dates.last().and_then(|d| parse_day(d)),
        ) {
            let mut day =
                first - time::Duration::days(first.weekday().number_days_from_monday() as i64);
            let end = last - time::Duration::days(last.weekday().number_days_from_monday() as i64);
            while day <= end {
                buckets.entry(day.to_string()).or_default();
                if day == end {
                    break;
                }
                day += time::Duration::days(7)
            }
        }
    }
    if bucket == "month" {
        if let (Some(first), Some(last)) = (
            dates.first().and_then(|d| parse_day(d)),
            dates.last().and_then(|d| parse_day(d)),
        ) {
            let (mut y, mut m) = (first.year(), first.month() as u8);
            let end = (last.year(), last.month() as u8);
            loop {
                buckets.entry(format!("{y}-{m:02}")).or_default();
                if (y, m) == end {
                    break;
                }
                if m == 12 {
                    y += 1;
                    m = 1
                } else {
                    m += 1
                }
            }
        }
    }
    let datasets=filtered.iter().enumerate().map(|(i,s)|json!({"label":form_label(s),"data":buckets.values().map(|days|days.iter().map(|d|by.get(d).and_then(Value::as_object).and_then(|x|x.get(s)).and_then(Value::as_i64).unwrap_or(0)).sum::<i64>()).collect::<Vec<_>>(),"backgroundColor":form_color(s,i)})).collect::<Vec<_>>();
    Ok(
        json!({"labels":buckets.keys().cloned().collect::<Vec<_>>(),"bucket":bucket,"bucketLabel":bucket,"rangeLabel":dates.first().zip(dates.last()).map(|(a,b)|if a==b {a.clone()}else{format!("{a} - {b}")}).unwrap_or_default(),"datasets":datasets}),
    )
}
fn surveyors_chart(report: &Map<String, Value>, r: &Value) -> Result<Value, String> {
    let a = report
        .get("activeSurveyorsByDay")
        .and_then(Value::as_object);
    let start = a
        .and_then(|x| x.get("startDate"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let values = a
        .and_then(|x| nested(x, &["counts", "all"]))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut labels = Vec::new();
    let mut data: Vec<f64> = Vec::new();
    let format = time::format_description::parse("[year]-[month]-[day]")
        .map_err(|_| "dashboard date format invalid")?;
    if let Ok(day) = time::Date::parse(start, &format) {
        for (i, v) in values.into_iter().enumerate() {
            if let Some(d) = day.checked_add(time::Duration::days(i as i64)) {
                let label = d.to_string();
                if r["date_from"]
                    .as_str()
                    .map_or(true, |x| label.as_str() >= x)
                    && r["date_to"].as_str().map_or(true, |x| label.as_str() <= x)
                {
                    labels.push(label);
                    data.push(v.as_f64().unwrap_or(0.0));
                }
            }
        }
    }
    let bucket = r["bucket"].as_str().unwrap_or("day");
    let range_label = labels
        .first()
        .zip(labels.last())
        .map(|(a, b)| {
            if a == b {
                a.clone()
            } else {
                format!("{a} - {b}")
            }
        })
        .unwrap_or_default();
    if bucket != "day" {
        let mut grouped: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for (label, value) in labels.iter().zip(data.iter()) {
            let key = parse_day(label)
                .map(|d| {
                    if bucket == "month" {
                        format!("{}-{:02}", d.year(), d.month() as u8)
                    } else {
                        (d - time::Duration::days(d.weekday().number_days_from_monday() as i64))
                            .to_string()
                    }
                })
                .unwrap_or_else(|| label.clone());
            grouped.entry(key).or_default().push(*value);
        }
        labels = grouped.keys().cloned().collect();
        data = grouped
            .values()
            .map(|values| {
                ((values.iter().sum::<f64>() / values.len() as f64) * 10.0).round() / 10.0
            })
            .collect();
    }
    Ok(
        json!({"labels":labels,"bucket":bucket,"bucketLabel":bucket,"rangeLabel":range_label,"data":data}),
    )
}
fn form_color(slug: &str, index: usize) -> &'static str {
    match slug {
        "lv_poles_as_built" => "#3b82f6",
        "edcl_customers_as_built_survey" => "#f97316",
        "field_notes" => "#22c55e",
        "service_cable_survey" => "#ef4444",
        "lv_lines_survey" => "#8b5cf6",
        "transformer_survey" => "#6b7280",
        _ => [
            "#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4", "#42d4f4", "#f032e6", "#bfef45",
            "#fabed4", "#469990",
        ][index % 10],
    }
}
fn top_surveyors(report: &Map<String, Value>, r: &Value) -> Result<Value, String> {
    let mut a = report
        .get("bySurveyor")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(e, c)| (e.clone(), c["total"].as_i64().unwrap_or(0)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(s) = r["surveyor"].as_str() {
        a.retain(|x| x.0 == s)
    }
    a.sort_by(|x, y| y.1.cmp(&x.1));
    a.truncate(r["limit"].as_u64().unwrap_or(10) as usize);
    Ok(
        json!({"labels":a.iter().map(|x|x.0.split('@').next().unwrap_or("")).collect::<Vec<_>>(),"data":a.iter().map(|x|x.1).collect::<Vec<_>>() }),
    )
}
fn form_distribution(t: &Map<String, Value>) -> Result<Value, String> {
    let mut a = t
        .iter()
        .filter_map(|(s, v)| {
            (v["active"].as_i64().unwrap_or(0) > 0
                || v["deleted"].as_i64().unwrap_or(0) > 0
                || v["total"].as_i64().unwrap_or(0) > 0)
                .then(|| (s, v["active"].as_i64().unwrap_or(0)))
        })
        .collect::<Vec<_>>();
    a.sort_by(|x, y| y.1.cmp(&x.1));
    Ok(
        json!({"labels":a.iter().map(|x|form_label(x.0)).collect::<Vec<_>>(),"data":a.iter().map(|x|x.1).collect::<Vec<_>>(),"colors":a.iter().enumerate().map(|(i,x)|form_color(x.0,i)).collect::<Vec<_>>() }),
    )
}
fn admin_rows(value: &Value) -> Result<Value, String> {
    let mut districts = value
        .as_object()
        .map(|m| m.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    districts.sort_by(|(_, a), (_, b)| {
        b["total"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["total"].as_i64().unwrap_or(0))
    });
    let mut rows = Vec::new();
    for (district, data) in districts {
        rows.push(json!({"district":district,"sector":"","cell":"","village":"","count":data["total"].as_i64().unwrap_or(0),"isDistrictHeader":true,"isSectorHeader":false,"isCellHeader":false}));
        let mut sectors = data["sectors"]
            .as_object()
            .map(|m| m.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        sectors.sort_by(|(_, a), (_, b)| {
            b["total"]
                .as_i64()
                .unwrap_or(0)
                .cmp(&a["total"].as_i64().unwrap_or(0))
        });
        for (sector, sdata) in sectors {
            rows.push(json!({"district":district,"sector":sector,"cell":"","village":"","count":sdata["total"].as_i64().unwrap_or(0),"isDistrictHeader":false,"isSectorHeader":true,"isCellHeader":false}));
            let mut cells = sdata["cells"]
                .as_object()
                .map(|m| m.iter().collect::<Vec<_>>())
                .unwrap_or_default();
            cells.sort_by(|(_, a), (_, b)| admin_total(b).cmp(&admin_total(a)));
            for (cell, raw) in cells {
                let total = admin_total(raw);
                let mut villages = raw
                    .get("villages")
                    .and_then(Value::as_object)
                    .map(|m| {
                        m.iter()
                            .filter_map(|(n, c)| c.as_i64().filter(|n| *n > 0).map(|c| (n, c)))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                villages.sort_by(|(_, a), (_, b)| b.cmp(a));
                if villages.is_empty() {
                    rows.push(json!({"district":district,"sector":sector,"cell":cell,"village":"","count":total,"isDistrictHeader":false,"isSectorHeader":false,"isCellHeader":false}));
                    continue;
                }
                rows.push(json!({"district":district,"sector":sector,"cell":cell,"village":"","count":total,"isDistrictHeader":false,"isSectorHeader":false,"isCellHeader":true}));
                for (village, count) in villages {
                    rows.push(json!({"district":district,"sector":sector,"cell":cell,"village":village,"count":count,"isDistrictHeader":false,"isSectorHeader":false,"isCellHeader":false}));
                }
            }
        }
    }
    Ok(json!(rows))
}
fn admin_total(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| v["total"].as_i64())
        .unwrap_or_else(|| {
            v["villages"]
                .as_object()
                .map(|m| m.values().filter_map(Value::as_i64).sum())
                .unwrap_or(0)
        })
}
fn admin_table(value: &Value) -> Result<Value, String> {
    let rows = admin_rows(value)?;
    let out = rows
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|r| {
            json!([
                if r["isDistrictHeader"] == true {
                    r["district"].clone()
                } else {
                    json!("")
                },
                if r["isSectorHeader"] == true {
                    r["sector"].clone()
                } else {
                    json!("")
                },
                if r["isCellHeader"] == true
                    || (!r["isDistrictHeader"].as_bool().unwrap_or(false)
                        && !r["isSectorHeader"].as_bool().unwrap_or(false)
                        && r["village"].as_str() == Some(""))
                {
                    r["cell"].clone()
                } else {
                    json!("")
                },
                r["village"].clone(),
                r["count"].clone()
            ])
        })
        .collect::<Vec<_>>();
    Ok(json!({"headers":["District","Sector","Cell","Village","Count"],"rows":out}))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    let text = value[key]
        .as_str()
        .ok_or_else(|| format!("{key} is required"))?;
    if text.is_empty()
        || text.len() > 1_500
        || text.trim() != text
        || text.chars().any(char::is_control)
    {
        return Err(format!("{key} is invalid"));
    }
    Ok(text)
}

fn bool(value: &Value, key: &str) -> Result<bool, String> {
    value[key]
        .as_bool()
        .ok_or_else(|| format!("{key} is required"))
}

fn nonnegative(value: &Value, key: &str) -> Result<u64, String> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("{key} is invalid"))
}

pub(super) fn canonical_cutoff(value: &Value) -> Option<String> {
    let parsed = if let Some(raw) = value.as_str() {
        OffsetDateTime::parse(raw, &Rfc3339).ok()?
    } else {
        let millis = value.as_i64().or_else(|| {
            value.as_f64().and_then(|raw| {
                if raw.is_finite() && raw >= i64::MIN as f64 && raw <= i64::MAX as f64 {
                    Some(raw.trunc() as i64)
                } else {
                    None
                }
            })
        })?;
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000).ok()?
    };
    parsed.to_offset(time::UtcOffset::UTC).format(&Rfc3339).ok()
}

fn plan(request: &Value) -> Result<Value, String> {
    text(request, "project")?;
    text(request, "form")?;
    let scope = bool(request, "scope")?;
    let allow_unscoped = bool(request, "allow_unscoped")?;
    let mode = request["mode"].as_str().ok_or("mode is required")?;
    if !matches!(mode, "replace" | "delta") {
        return Err("mode is invalid".into());
    }
    if !scope && !allow_unscoped {
        return Ok(json!({"schema": SCHEMA, "action": "skip", "reason": "scope_required"}));
    }
    if nonnegative(request, "pending_overlay_count")? > MAX_PENDING_IDS as u64 {
        return Ok(
            json!({"schema": SCHEMA, "action": "skip", "reason": "pending_overlay_too_large"}),
        );
    }
    let saved = if bool(request, "cutoff_clock")? {
        canonical_cutoff(&request["saved_cutoff"])
    } else {
        None
    };
    let cached = canonical_cutoff(&request["cached_cutoff"]);
    let cutoff = saved.or(cached);
    if mode == "delta" && cutoff.is_none() && bool(request, "skip_without_cutoff")? {
        return Ok(json!({"schema": SCHEMA, "action": "skip", "reason": "cutoff_required"}));
    }
    let cached_count = nonnegative(request, "cached_count")?;
    let checkpoint =
        mode == "replace" && (bool(request, "checkpoint_replace")? || cached_count == 0);
    Ok(json!({
        "schema": SCHEMA,
        "action": "query",
        "mode": mode,
        "updated_after": if mode == "delta" { cutoff } else { None },
        "include_tombstones": mode == "delta",
        "checkpoint": checkpoint,
    }))
}

fn feature_properties(feature: &Value) -> Option<&Map<String, Value>> {
    feature["properties"]
        .as_object()
        .or_else(|| feature.as_object())
}

pub(super) fn doc_id(feature: &Value) -> Option<String> {
    let properties = feature_properties(feature)?;
    for value in [
        properties.get("id"),
        properties.get("doc_id"),
        properties.get("feature_id"),
    ] {
        let Some(id) = value.and_then(Value::as_str) else {
            continue;
        };
        if !id.is_empty()
            && id.len() <= 1_500
            && id.trim() == id
            && !id.chars().any(char::is_control)
        {
            return Some(id.to_owned());
        }
    }
    None
}

pub(super) fn deleted(feature: &Value) -> bool {
    let Some(properties) = feature_properties(feature) else {
        return false;
    };
    properties.get("is_deleted").and_then(Value::as_bool) == Some(true)
        || properties
            .get("metadata")
            .and_then(|value| value["is_deleted"].as_bool())
            == Some(true)
}

pub(super) fn cutoff(feature: &Value) -> Option<String> {
    canonical_cutoff(&feature_properties(feature)?["metadata"]["firestore_updated_at"])
}

fn summary_valid(summary: &Value) -> Result<(), String> {
    let errors = summary["errors"]
        .as_array()
        .ok_or("Survey summary errors are invalid")?;
    for error in errors {
        let row = error
            .as_object()
            .ok_or("Survey summary errors are invalid")?;
        if row.get("form").and_then(Value::as_str).is_none()
            || row.get("error").and_then(Value::as_str).is_none()
        {
            return Err("Survey summary errors are invalid".into());
        }
    }
    if !errors.is_empty() {
        let names = errors
            .iter()
            .filter_map(|row| row["form"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("Survey layers failed: {names}"));
    }
    Ok(())
}

fn batch(request: &Value) -> Result<Value, String> {
    let mode = request["mode"].as_str().ok_or("mode is required")?;
    if !matches!(mode, "replace" | "delta") {
        return Err("mode is invalid".into());
    }
    let expected = nonnegative(request, "expected_form_count")?;
    let received = nonnegative(request, "received")?;
    let mut finished = bool(request, "finished")?;
    let mut newest = canonical_cutoff(&request["current_cutoff"]);
    let rows = request["rows"].as_array().ok_or("rows are required")?;
    if rows.len() > MAX_BATCH_ROWS {
        return Err("Survey read batch exceeds 64 rows".into());
    }
    let pending = request["pending_doc_ids"]
        .as_array()
        .ok_or("pending_doc_ids are required")?;
    if pending.len() > MAX_PENDING_IDS {
        return Err("Survey pending overlay exceeds 128 rows".into());
    }
    let mut pending_ids = std::collections::HashSet::with_capacity(pending.len());
    for id in pending {
        let id = id.as_str().ok_or("pending id is invalid")?;
        if id.is_empty() || id.len() > 1_500 || id.trim() != id || id.chars().any(char::is_control)
        {
            return Err("pending id is invalid".into());
        }
        pending_ids.insert(id.to_owned());
    }
    let mut upserts = Vec::new();
    let mut delete_ids = Vec::new();
    let mut next_received = received;
    for row in rows {
        if finished {
            return Err("Data arrived after the survey summary".into());
        }
        if row["__type"].as_str() == Some("summary") {
            if nonnegative(row, "form_count")? != expected
                || nonnegative(row, "total_features")? != next_received
            {
                return Err("Survey summary does not match the requested data".into());
            }
            summary_valid(row)?;
            finished = true;
            continue;
        }
        if row["type"].as_str() != Some("Feature") {
            return Err("Invalid survey stream row".into());
        }
        let index = nonnegative(row, "index")?;
        next_received = next_received
            .checked_add(1)
            .ok_or("Survey feature count is invalid")?;
        if let Some(value) = cutoff(row) {
            if newest.as_ref().is_none_or(|current| {
                OffsetDateTime::parse(&value, &Rfc3339).ok()
                    > OffsetDateTime::parse(current, &Rfc3339).ok()
            }) {
                newest = Some(value);
            }
        }
        let id = doc_id(row).ok_or("Survey feature has no stable document id")?;
        if pending_ids.contains(&id) {
            continue;
        }
        if deleted(row) {
            if mode == "delta" {
                delete_ids.push(id);
            }
        } else {
            upserts.push(index);
        }
    }
    Ok(json!({
        "schema": SCHEMA, "received": next_received, "finished": finished,
        "upsert_indices": upserts, "delete_ids": delete_ids, "newest_cutoff": newest,
    }))
}

fn finish(request: &Value) -> Result<Value, String> {
    if !bool(request, "finished")? {
        return Err("Survey download ended without a completion summary".into());
    }
    Ok(json!({"schema": SCHEMA, "action": "complete"}))
}

fn retry(request: &Value) -> Result<Value, String> {
    let attempt = nonnegative(request, "attempt")?;
    let delay = RETRY_DELAYS_MS
        .get(usize::try_from(attempt).unwrap_or(usize::MAX))
        .copied();
    Ok(json!({"schema": SCHEMA, "delay_ms": delay}))
}

fn complete(request: &Value) -> Result<Value, String> {
    let expected_uid = text(request, "expected_uid")?;
    let current_uid = text(request, "current_uid")?;
    let expected_project = text(request, "expected_project")?;
    let current_project = text(request, "current_project")?;
    let finished = bool(request, "finished")?;
    let action = if finished && expected_uid == current_uid && expected_project == current_project {
        "commit"
    } else {
        "discard"
    };
    Ok(json!({"schema": SCHEMA, "action": action}))
}

fn integrity_count(value: &Value, key: &str) -> Result<u64, String> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("integrity {key} is invalid"))
}

/// Project a bounded server-provided count receipt for presentation. This is
/// deliberately not a client reconciliation: the host supplies no entries and
/// the result only explains the backend's live/mirror/dashboard receipt.
fn integrity(request: &Value) -> Result<Value, String> {
    let forms = request["forms"]
        .as_object()
        .ok_or("integrity forms are required")?;
    if forms.len() > 512 {
        return Err("integrity forms exceed 512 rows".into());
    }
    let mut rows = Vec::with_capacity(forms.len());
    let mut firestore = 0_u64;
    let mut mirror = 0_u64;
    let mut dashboard = 0_u64;
    let mut delta = 0_i64;
    let mut mismatch = request["has_mismatch"].as_bool().unwrap_or(false);
    for (form, value) in forms {
        if form.is_empty() || form.len() > 256 || form.trim() != form {
            return Err("integrity form is invalid".into());
        }
        let firestore_count = integrity_count(value, "firestore")?;
        let mirror_count = integrity_count(value, "mirror")?;
        let dashboard_count = value["dashboard"].as_u64();
        let row_delta = value["delta"]
            .as_i64()
            .unwrap_or_else(|| firestore_count as i64 - mirror_count as i64);
        let row_mismatch = value["mismatch"]
            .as_bool()
            .unwrap_or(firestore_count != mirror_count);
        firestore = firestore
            .checked_add(firestore_count)
            .ok_or("integrity total is invalid")?;
        mirror = mirror
            .checked_add(mirror_count)
            .ok_or("integrity total is invalid")?;
        dashboard = dashboard
            .checked_add(dashboard_count.unwrap_or(0))
            .ok_or("integrity total is invalid")?;
        delta = delta
            .checked_add(row_delta)
            .ok_or("integrity total is invalid")?;
        mismatch |= row_mismatch;
        rows.push(json!({"form":form,"count":{"firestore":firestore_count,"mirror":mirror_count,"dashboard":dashboard_count,"delta":row_delta,"mismatch":row_mismatch}}));
    }
    rows.sort_by(|a, b| {
        let a_mismatch = a["count"]["mismatch"].as_bool().unwrap_or(false);
        let b_mismatch = b["count"]["mismatch"].as_bool().unwrap_or(false);
        b_mismatch
            .cmp(&a_mismatch)
            .then_with(|| a["form"].as_str().cmp(&b["form"].as_str()))
    });
    let progress = request["progress"].as_object();
    let total = progress
        .and_then(|row| row.get("forms_total"))
        .and_then(Value::as_u64)
        .unwrap_or(rows.len() as u64);
    let done = progress
        .and_then(|row| row.get("forms_completed"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(total);
    Ok(json!({
        "schema": SCHEMA, "rows": rows,
        "totals": if rows.is_empty() { Value::Null } else { json!({"firestore":firestore,"mirror":mirror,"dashboard":dashboard,"delta":delta,"mismatch":mismatch}) },
        "has_mismatch": mismatch,
        "progress": {"total":total,"done":done,"percent":if total > 0 { done * 100 / total } else { 0 }},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn feature(index: u64, id: &str, stamp: &str) -> Value {
        json!({"index":index,"type":"Feature", "properties":{"doc_id":id,"metadata":{"firestore_updated_at":stamp}}})
    }

    #[test]
    fn delta_batch_preserves_pending_rows_and_turns_tombstones_into_deletes() {
        let deleted = json!({"index":2,"type":"Feature", "properties":{"doc_id":"gone","metadata":{"firestore_updated_at":"2026-09-01T00:00:00Z","is_deleted":true}}});
        let reply = batch(&json!({"mode":"delta","expected_form_count":1,"received":0,"finished":false,"pending_doc_ids":["local"],"rows":[feature(0, "live","2026-08-01T00:00:00Z"), feature(1, "local","2026-08-02T00:00:00Z"), deleted]})).unwrap();
        assert_eq!(reply["upsert_indices"], json!([0]));
        assert_eq!(reply["delete_ids"], json!(["gone"]));
        assert_eq!(reply["newest_cutoff"], "2026-09-01T00:00:00Z");
    }

    #[test]
    fn plan_uses_only_the_replication_cutoff_and_never_scopes_implicitly() {
        let skip = plan(&json!({"project":"p","form":"f","scope":false,"allow_unscoped":false,"mode":"replace","cutoff_clock":false,"saved_cutoff":null,"cached_cutoff":null,"skip_without_cutoff":false,"checkpoint_replace":false,"cached_count":0,"pending_overlay_count":0})).unwrap();
        assert_eq!(skip["reason"], "scope_required");
        let reply = plan(&json!({"project":"p","form":"f","scope":true,"allow_unscoped":false,"mode":"delta","cutoff_clock":true,"saved_cutoff":"2026-08-01T00:00:00Z","cached_cutoff":"not-a-time","skip_without_cutoff":true,"checkpoint_replace":false,"cached_count":1,"pending_overlay_count":0})).unwrap();
        assert_eq!(reply["updated_after"], "2026-08-01T00:00:00Z");
        let numeric = batch(&json!({"mode":"delta","expected_form_count":1,"received":0,"finished":false,"current_cutoff":null,"pending_doc_ids":[],"rows":[json!({"index":0,"type":"Feature","properties":{"doc_id":"numeric","metadata":{"firestore_updated_at":1767225600000_i64}}})]})).unwrap();
        assert_eq!(numeric["newest_cutoff"], "2026-01-01T00:00:00Z");
        let mixed_offsets = batch(&json!({"mode":"delta","expected_form_count":1,"received":0,"finished":false,"current_cutoff":null,"pending_doc_ids":[],"rows":[feature(0,"offset","2026-01-01T01:00:00+01:00"),feature(1,"fraction","2026-01-01T00:00:00.11Z")]})).unwrap();
        assert_eq!(mixed_offsets["newest_cutoff"], "2026-01-01T00:00:00.11Z");
    }

    #[test]
    fn dashboard_buckets_and_admin_rows_are_kernel_projections() {
        let report = json!({"totals":{"live_form":{"active":8,"deleted":0,"total":8}},"byDay":{"2026-01-01":{"live_form":3},"2026-01-15":{"live_form":4},"2026-09-01":{"live_form":11}},"byAdmin":{"District":{"total":2,"sectors":{"Sector":{"total":2,"cells":{"Cell": {"total":2,"villages":{"Village":2}}}}}}}});
        let chart = dashboard(&json!({"action":"daily_chart","report":report,"bucket":"month","form":null,"date_from":null,"date_to":null})).unwrap();
        assert_eq!(chart["labels"].as_array().unwrap().len(), 9);
        assert_eq!(
            chart["datasets"][0]["data"],
            json!([7, 0, 0, 0, 0, 0, 0, 0, 11])
        );
        let weekly = dashboard(&json!({"action":"daily_chart","report":{"totals":{"live_form":{"active":8,"deleted":0,"total":8}},"byDay":{"2026-01-05":{"live_form":2},"2026-03-30":{"live_form":6}}},"bucket":"week","form":null,"date_from":null,"date_to":null})).unwrap();
        assert_eq!(weekly["labels"].as_array().unwrap().len(), 13);
        let rows = dashboard(&json!({"action":"admin_rows","report":report})).unwrap();
        assert!(rows
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["village"] == "Village"));
        let activity = dashboard(&json!({"action":"surveyors_chart","bucket":"week","date_from":null,"date_to":null,"report":{"activeSurveyorsByDay":{"startDate":"2026-05-11","counts":{"all":[1,2]}}}})).unwrap();
        assert_eq!(activity["data"], json!([1.5]));
    }

    #[test]
    fn late_uid_or_project_completion_is_discarded() {
        let reply = complete(&json!({"expected_uid":"a","current_uid":"b","expected_project":"p","current_project":"p","finished":true})).unwrap();
        assert_eq!(reply["action"], "discard");
    }

    #[test]
    fn integrity_projection_sorts_drift_and_bounds_progress_without_entry_rows() {
        let reply = integrity(&json!({
            "forms": {
                "clean": {"firestore": 3, "mirror": 3, "dashboard": 3, "delta": 0, "mismatch": false},
                "drift": {"firestore": 5, "mirror": 3, "dashboard": 4, "delta": 2, "mismatch": true}
            },
            "has_mismatch": false,
            "progress": {"forms_total": 2, "forms_completed": 9}
        })).unwrap();
        assert_eq!(reply["rows"][0]["form"], "drift");
        assert_eq!(reply["totals"]["delta"], 2);
        assert_eq!(reply["progress"], json!({"total":2,"done":2,"percent":100}));
    }
}
