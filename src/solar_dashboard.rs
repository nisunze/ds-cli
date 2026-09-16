//! Deterministic dashboard composition shared by headless and browser hosts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Site,
    Plant,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub section: Section,
    pub report: Value,
    #[serde(default)]
    pub system: Option<ds_solar_contracts::SystemType>,
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
    pub charts: Value,
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
    let mut charts = json!({});
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
        Section::Plant => {
            let system = system_key(request.system);
            let d = &request.report["04_plant_cost"][format!("{system}_summary")];
            if !d.is_object() {
                return Err("Plant metrics are unavailable".into());
            }
            let boq = &request.report["04_plant_cost"][format!("{system}_boq")];
            let energy = &d["energy_production_year_1"];
            let fraction = ratio(
                sum(
                    number(energy, "direct_solar_to_grid_mwh"),
                    number(energy, "battery_output_mwh"),
                ),
                number(energy, "net_energy_to_grid_mwh"),
                100.,
            );
            let fraction = fraction
                .as_f64()
                .map(|n| json!(n.min(100.)))
                .unwrap_or(Value::Null);
            let total_cost = number(boq, "grand_total")
                .or_else(|| {
                    sum(
                        number(boq, "total_equipment_cost"),
                        number(boq, "total_markup_cost"),
                    )
                })
                .map_or(Value::Null, |n| json!(n));
            for (key, label, unit) in [
                ("solar_power_capacity_mw", "PV capacity", "MW"),
                ("battery_storage_capacity_mwh", "Storage capacity", "MWh"),
                ("generator_capacity_mw", "Generator capacity", "MW"),
            ] {
                card("Capacity", key, label, d["capacity"][key].clone(), unit);
            }
            for (key, label, unit) in [
                ("net_energy_to_grid_mwh", "Net energy", "MWh"),
                ("direct_solar_to_grid_mwh", "Direct solar", "MWh"),
                ("battery_output_mwh", "Battery output", "MWh"),
                ("generator_output_mwh", "Generator output", "MWh"),
            ] {
                card("Year-one energy", key, label, energy[key].clone(), unit);
            }
            card(
                "Year-one energy",
                "renewable_fraction_pct",
                "Renewable fraction",
                fraction.clone(),
                "%",
            );
            card(
                "Investment",
                "total_cost",
                "Plant cost",
                total_cost.clone(),
                "",
            );
            charts = plant_charts(d, system);
            let energy_by_year = &d["energy_production_by_year"];
            let years = ordered_years(energy_by_year, "direct_solar_to_grid_mwh");
            let rows=years.iter().map(|year|json!({"year":year,"solar":energy_by_year["direct_solar_to_grid_mwh"][year],"battery":energy_by_year["battery_output_mwh"][year],"generator":energy_by_year["generator_output_mwh"][year],"net":energy_by_year["net_energy_to_grid_mwh"][year]})).collect::<Vec<_>>();
            let final_year = years.last();
            let final_net =
                final_year.and_then(|y| energy_by_year["net_energy_to_grid_mwh"][y].as_f64());
            let first_net = number(energy, "net_energy_to_grid_mwh");
            let degradation = final_net
                .zip(first_net)
                .filter(|(a, b)| *a > 0. && *a < *b)
                .map(|(a, b)| json!({"year":final_year,"final_net_mwh":a,"loss_pct":(1.-a/b)*100.}))
                .unwrap_or(Value::Null);
            json!({"renewable_fraction_pct":fraction,"total_cost":total_cost,"energy_rows":rows,"degradation":degradation,
                "watts_per_panel":ratio(number(&d["capacity"],"solar_power_capacity_mw"),number(&d["equipment_quantities"],"total_panels"),1_000_000.),
                "kwh_per_battery":ratio(number(&d["capacity"],"battery_storage_capacity_mwh"),number(&d["equipment_quantities"],"total_batteries"),1000.)})
        }
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
        charts,
    })
}
fn system_key(system: Option<ds_solar_contracts::SystemType>) -> &'static str {
    match system.unwrap_or(ds_solar_contracts::SystemType::Hybrid) {
        ds_solar_contracts::SystemType::Hybrid => "hybrid",
        ds_solar_contracts::SystemType::SolarBattery => "solar_battery",
        ds_solar_contracts::SystemType::ThermalOnly => "thermal_only",
    }
}
fn sum(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    a.zip(b).map(|(a, b)| a + b).filter(|n| n.is_finite())
}
fn ordered_years(d: &Value, key: &str) -> Vec<String> {
    let mut years = d[key]
        .as_object()
        .map(|o| {
            o.keys()
                .filter_map(|k| k.parse::<u32>().ok().map(|n| (n, k.clone())))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    years.sort_by_key(|(n, _)| *n);
    years.into_iter().map(|(_, k)| k).collect()
}
fn plant_charts(d: &Value, system: &str) -> Value {
    let energy = &d["energy_production_by_year"];
    let years = ordered_years(energy, "direct_solar_to_grid_mwh");
    let mut series = vec![
        json!({"name":"Direct Solar","type":"bar","stack":"energy","data":years.iter().map(|y|energy["direct_solar_to_grid_mwh"][y].clone()).collect::<Vec<_>>(),"itemStyle":{"color":"#D4A843"}}),
        json!({"name":"Battery Output","type":"bar","stack":"energy","data":years.iter().map(|y|energy["battery_output_mwh"][y].clone()).collect::<Vec<_>>(),"itemStyle":{"color":"#1A5276"}}),
    ];
    if system != "solar_battery" {
        series.push(json!({"name":"Generator","type":"bar","stack":"energy","data":years.iter().map(|y|energy["generator_output_mwh"][y].clone()).collect::<Vec<_>>(),"itemStyle":{"color":"#C0392B"}}));
    }
    let energy_chart = if years.is_empty() {
        Value::Null
    } else {
        json!({"tooltip":{"trigger":"axis","axisPointer":{"type":"shadow"}},"legend":{"bottom":0},"grid":{"left":60,"right":20,"top":20,"bottom":40},"xAxis":{"type":"category","data":years,"axisLabel":{"interval":4}},"yAxis":{"type":"value","name":"Energy (MWh)","min":0},"series":series})
    };
    let env = &d["environmental_by_year"];
    let years = ordered_years(env, "fuel_consumption_liters");
    let environment_chart = if years.is_empty() {
        Value::Null
    } else {
        json!({"tooltip":{"trigger":"axis"},"legend":{"bottom":0},"grid":{"left":60,"right":60,"top":20,"bottom":40},"xAxis":{"type":"category","data":years,"axisLabel":{"interval":4}},"yAxis":[{"type":"value","name":"Fuel (KL)","min":0},{"type":"value","name":"CO₂ (tons)","min":0}],"series":[{"name":"Fuel (KL)","type":"bar","yAxisIndex":0,"data":years.iter().map(|y|env["fuel_consumption_liters"][y].as_f64().map(|n|json!(n/1000.)).unwrap_or(Value::Null)).collect::<Vec<_>>(),"itemStyle":{"color":"#E53935"}},{"name":"CO₂ (tons)","type":"line","yAxisIndex":1,"data":years.iter().map(|y|env["co2_emissions_tons"][y].clone()).collect::<Vec<_>>(),"itemStyle":{"color":"#757575"}}]})
    };
    json!({"energy":energy_chart,"environment":environment_chart})
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
            system: None,
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
            let d=compose(Request{section:Section::Site,system:None,report:json!({"02_site_prep":{"site_metrics":{"load_profile":{"daily_energy_kwh":1,"peak_load_kw":peak}}}})}).unwrap();
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
    #[test]
    fn plant_preserves_zero_totals_and_orders_chart_pairs_with_correct_units() {
        let d=compose(Request{section:Section::Plant,system:None,report:json!({"04_plant_cost":{
            "hybrid_summary":{"energy_production_year_1":{"direct_solar_to_grid_mwh":20,"battery_output_mwh":30,"net_energy_to_grid_mwh":100},
                "energy_production_by_year":{"direct_solar_to_grid_mwh":{"10":10,"2":2},"battery_output_mwh":{"2":3}},
                "environmental_by_year":{"fuel_consumption_liters":{"10":2000,"2":1000},"co2_emissions_tons":{"2":4}}},
            "hybrid_boq":{"grand_total":0,"total_equipment_cost":100,"total_markup_cost":50}
        }})}).unwrap();
        assert_eq!(d.derived["total_cost"], 0.);
        assert_eq!(d.derived["renewable_fraction_pct"], 50.);
        assert_eq!(d.charts["energy"]["xAxis"]["data"], json!(["2", "10"]));
        assert_eq!(d.charts["energy"]["series"][1]["data"], json!([3, null]));
        assert_eq!(
            d.charts["environment"]["series"][0]["data"],
            json!([1., 2.])
        );
        assert_eq!(
            d.charts["environment"]["series"][1]["data"],
            json!([4, null])
        );
    }
    #[test]
    fn plant_selection_never_substitutes_another_system() {
        assert!(compose(Request {
            section: Section::Plant,
            system: Some(ds_solar_contracts::SystemType::SolarBattery),
            report: json!({"04_plant_cost":{"hybrid_summary":{}}})
        })
        .is_err());
    }
}
