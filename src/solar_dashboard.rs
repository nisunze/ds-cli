//! Deterministic dashboard composition shared by headless and browser hosts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Site,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub section: Section,
    pub report: Value,
}
#[derive(Serialize)]
pub struct Card {
    pub key: String,
    pub group: String,
    pub label: String,
    pub value: Value,
    pub unit: String,
}
#[derive(Serialize)]
pub struct Dashboard {
    pub schema: &'static str,
    pub section: Section,
    pub derived: Value,
    pub cards: Vec<Card>,
}

pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > 32 * 1024 * 1024 {
        return Err("Dashboard request exceeds 32 MiB".into());
    }
    let request: Request =
        serde_json::from_slice(bytes).map_err(|_| "Invalid closed Solar dashboard request")?;
    serde_json::to_string(&compose(request)?).map_err(|_| "Cannot encode dashboard".into())
}
pub fn compose(request: Request) -> Result<Dashboard, String> {
    if !request.report.is_object() {
        return Err("Dashboard report must be an object".into());
    }
    let mut cards = Vec::new();
    let mut card = |group: &str, key: &str, label: &str, value: Value, unit: &str| {
        cards.push(Card {
            key: key.into(),
            group: group.into(),
            label: label.into(),
            value,
            unit: unit.into(),
        });
    };
    let derived = match request.section {
        Section::Site => {
            let d = &request.report["02_site_prep"]["site_metrics"];
            if !d.is_object() {
                return Err("Site metrics are unavailable".into());
            }
            for (group, fields) in [
                (
                    "Location",
                    vec![
                        ("city", "City", ""),
                        ("country", "Country", ""),
                        ("timezone", "Timezone", ""),
                        ("latitude", "Latitude", "°"),
                        ("longitude", "Longitude", "°"),
                    ],
                ),
                (
                    "Solar geometry",
                    vec![
                        ("optimal_tilt_degrees", "Optimal tilt", "°"),
                        ("max_poa_kwh_m2_year", "Max POA", "kWh/m²/yr"),
                    ],
                ),
                (
                    "Weather",
                    vec![
                        ("annual_ghi_kwh_m2", "Annual GHI", "kWh/m²"),
                        ("peak_sun_hours_avg", "Peak sun hours", "h/day"),
                    ],
                ),
                (
                    "String configuration",
                    vec![
                        ("modules_per_string", "Modules/string", ""),
                        ("strings_per_inverter", "Strings/inverter", ""),
                        ("modules_per_inverter", "Modules/inverter", ""),
                        ("dc_capacity_kw", "DC capacity", "kW"),
                        ("module_capacity_w", "Module power", "W"),
                        ("system_max_dc_voltage", "Max DC voltage", "V"),
                    ],
                ),
                (
                    "Inverter reference",
                    vec![
                        ("capacity_kw", "Capacity", "kW"),
                        ("max_dc_voltage", "Max DC voltage", "V"),
                        ("annual_energy_mwh", "Annual energy", "MWh"),
                        ("capacity_factor_pct", "Capacity factor", "%"),
                    ],
                ),
                (
                    "Load profile",
                    vec![
                        ("peak_load_kw", "Peak load", "kW"),
                        ("daily_energy_kwh", "Daily energy", "kWh"),
                    ],
                ),
            ] {
                let field = match group {
                    "Location" => "location",
                    "Solar geometry" => "solar_geometry",
                    "Weather" => "weather_summary",
                    "String configuration" => "string_config",
                    "Inverter reference" => "inverter_reference",
                    _ => "load_profile",
                };
                for (key, label, unit) in fields {
                    card(
                        group,
                        &format!("{field}.{key}"),
                        label,
                        d[field][key].clone(),
                        unit,
                    );
                }
            }
            card(
                "Weather",
                "temperature.min_c",
                "Minimum temperature",
                first(&d["temperature"], &["min_c", "min_temp_c"]),
                "°C",
            );
            card(
                "Weather",
                "temperature.max_c",
                "Maximum temperature",
                first(&d["temperature"], &["max_c", "max_temp_c"]),
                "°C",
            );
            card(
                "Load profile",
                "total_customers",
                "Customers",
                d["total_customers"].clone(),
                "",
            );
            let factor = ratio(
                number(&d["load_profile"], "daily_energy_kwh"),
                number(&d["load_profile"], "peak_load_kw").map(|n| n * 24.),
                100.,
            );
            card(
                "Load profile",
                "load_factor_pct",
                "Load factor",
                factor.clone(),
                "%",
            );
            json!({"load_factor_pct":factor})
        }
    };
    Ok(Dashboard {
        schema: "ds-solar.dashboard/v1",
        section: request.section,
        derived,
        cards,
    })
}
fn number(d: &Value, key: &str) -> Option<f64> {
    d[key].as_f64().filter(|v| v.is_finite())
}
fn first(d: &Value, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|k| d.get(*k).filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or(Value::Null)
}
fn ratio(n: Option<f64>, d: Option<f64>, scale: f64) -> Value {
    n.zip(d)
        .filter(|(n, d)| *n >= 0. && *d > 0.)
        .map(|(n, d)| n / d * scale)
        .filter(|n| n.is_finite())
        .map_or(Value::Null, |n| json!(n))
}

/// Standalone HTML contains no script and escapes every source string.
pub fn html(dashboard: &Dashboard, title: &str) -> String {
    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }
    let mut body = String::new();
    let mut group = "";
    for card in &dashboard.cards {
        if group != card.group {
            if !group.is_empty() {
                body.push_str("</div>");
            }
            group = &card.group;
            body.push_str(&format!("<h2>{}</h2><div class=cards>", escape(group)));
        }
        let value = match &card.value {
            Value::Null => "—".into(),
            Value::String(s) => s.clone(),
            Value::Number(n) => n
                .as_f64()
                .map(|n| format!("{n:.2}"))
                .unwrap_or_else(|| n.to_string()),
            other => other.to_string(),
        };
        body.push_str(&format!(
            "<article><small>{}</small><strong>{}</strong><span>{}</span></article>",
            escape(&card.label),
            escape(&value),
            escape(&card.unit)
        ));
    }
    if !group.is_empty() {
        body.push_str("</div>");
    }
    format!("<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>{}</title><style>body{{font:16px system-ui;background:#0f172a;color:#e2e8f0;margin:40px}}h2{{font-size:14px;color:#94a3b8}}.cards{{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:16px}}article{{background:#1e293b;padding:16px;border-left:3px solid #38bdf8;border-radius:6px}}small,strong,span{{display:block}}strong{{font-size:24px;margin:8px 0}}span{{color:#94a3b8}}</style><h1>{}</h1>{}</html>",escape(title),escape(title),body)
}

/// Bounded semantic reads use the same report sections as dashboard hosts.
pub fn read_section(report: &Value, section: &str, path: &[String]) -> Result<Value, String> {
    let key = match section {
        "config" => "_config",
        "costs" => "04_plant_cost",
        "finance" => "05_financial",
        "identity" => "_root",
        "inputs" => "01_city_inputs",
        "plant" => "03_plant_design",
        "site" => "02_site_prep",
        "summary" => "09_unified",
        "templates" => "00_templates",
        _ => return Err("Unknown dashboard section".into()),
    };
    if path.len() > 8 || path.iter().any(|k| k.is_empty() || k.chars().count() > 120) {
        return Err("Invalid bounded dashboard path".into());
    }
    let mut value = report.get(key).ok_or("Dashboard section is unavailable")?;
    for key in path {
        value = value
            .as_object()
            .and_then(|v| v.get(key))
            .ok_or("Dashboard semantic path is unavailable")?;
    }
    let bytes = serde_json::to_vec(value).map_err(|_| "Invalid dashboard projection")?;
    let complete = bytes.len() <= 15000;
    let projection = if complete {
        value.clone()
    } else {
        match value {
            Value::Object(o) => {
                json!({"keys":o.keys().take(100).collect::<Vec<_>>(),"field_count":o.len()})
            }
            Value::Array(a) => json!({"item_count":a.len()}),
            Value::String(s) => {
                json!({"prefix":s.chars().take(400).collect::<String>(),"character_count":s.chars().count()})
            }
            _ => Value::Null,
        }
    };
    Ok(json!({"section":section,"path":path,"value":projection,"complete":complete}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn site_factor_and_static_output_share_the_same_projection() {
        let report = json!({"02_site_prep":{"site_metrics":{"location":{"city":"<script>alert(1)</script>"},"load_profile":{"daily_energy_kwh":4519.41,"peak_load_kw":384.66}}}});
        let d = compose(Request {
            section: Section::Site,
            report,
        })
        .unwrap();
        assert!((d.derived["load_factor_pct"].as_f64().unwrap() - 48.953899).abs() < 0.001);
        let html = html(&d, "Site");
        assert!(!html.contains("<script"));
        assert!(html.contains("&lt;script&gt;"));
    }
    #[test]
    fn missing_and_zero_denominators_are_unavailable() {
        for peak in [Value::Null, json!(0)] {
            let d=compose(Request{section:Section::Site,report:json!({"02_site_prep":{"site_metrics":{"load_profile":{"daily_energy_kwh":1,"peak_load_kw":peak}}}})}).unwrap();
            assert!(d.derived["load_factor_pct"].is_null());
        }
    }
    #[test]
    fn bounded_section_read_reports_omissions_and_refuses_foreign_paths() {
        let report = json!({"02_site_prep":{"site_metrics":{"city":"A"},"series":vec![1;10000]}});
        let read = read_section(&report, "site", &[]).unwrap();
        assert_eq!(read["complete"], false);
        let read = read_section(&report, "site", &["site_metrics".into(), "city".into()]).unwrap();
        assert_eq!(read["complete"], true);
        assert_eq!(read["value"], "A");
        assert!(read_section(&report, "site", &["../../secret".into()]).is_err());
        assert!(read_section(&report, "site", &vec!["city".into(); 9]).is_err());
    }
}
