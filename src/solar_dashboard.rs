//! Deterministic dashboard composition shared by headless and browser hosts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
mod finance;
mod portfolio_finance;
pub(crate) mod portfolio_views;
mod static_charts;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Site,
    Plant,
    Finance,
    Boq,
    PortfolioFinance,
    PortfolioSite,
    PortfolioPlant,
    PortfolioBoq,
}
impl Section {
    pub fn is_portfolio(self) -> bool {
        matches!(
            self,
            Self::PortfolioFinance
                | Self::PortfolioSite
                | Self::PortfolioPlant
                | Self::PortfolioBoq
        )
    }
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
    if matches!(request.section, Section::PortfolioFinance) {
        return portfolio_finance::compose(request.report);
    }
    if matches!(
        request.section,
        Section::PortfolioSite | Section::PortfolioPlant | Section::PortfolioBoq
    ) {
        return portfolio_views::compose(request.section, request.report);
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
        Section::Boq => {
            let system = system_key(request.system);
            let costs = &request.report["04_plant_cost"];
            let plant = &costs[format!("{system}_boq")];
            let network = &costs["network_costs"];
            if !plant.is_object() && !network.is_object() {
                return Err("Selected BOQ is unavailable".into());
            }
            let currency = plant["currency"]
                .as_str()
                .or_else(|| network["currency"].as_str());
            if let (Some(a), Some(b)) = (plant["currency"].as_str(), network["currency"].as_str()) {
                if a != b {
                    return Err("Plant and network BOQ currencies differ".into());
                }
            }
            let totals = json!({"grand_total":sum(number(plant,"grand_total"),number(network,"grand_total")),
                "equipment_total":sum(number(plant,"equipment_total"),number(network,"equipment_total")),
                "markup_total":sum(number(plant,"markup_total"),number(network,"markup_total")),
                "existing_total":sum(number(plant,"existing_plant_total"),number(network,"existing_network_total"))});
            for (key, label) in [
                ("grand_total", "Total CAPEX"),
                ("equipment_total", "Equipment"),
                ("markup_total", "Markup"),
                ("existing_total", "Existing infrastructure"),
            ] {
                card(
                    "Bill of quantities",
                    key,
                    label,
                    totals[key].clone(),
                    currency.unwrap_or(""),
                );
            }
            json!({"currency":currency,"totals":totals,"network_rows":boq_rows(network)?,"plant_rows":boq_rows(plant)?})
        }
        Section::Finance => {
            let system = system_key(request.system);
            let d = &request.report["05_financial"][format!("{system}_financial_analysis")];
            if !d.is_object() {
                return Err("Financial analysis is unavailable".into());
            }
            let params = &d["parameters"];
            let npv = &d["npv"];
            let irr = &d["irr"];
            let lcoe = &d["lcoe"];
            let pfa = &d["project_finance_analysis"];
            let dscr = &pfa["dscr"];
            let mut values = json!({
                "currency":params["currency"].as_str().unwrap_or("XAF"),
                "wacc_pct":number(params,"wacc").map(|v|if v.abs()<1. {v*100.}else{v}),
                "npv":first_number(npv,&["npv","value"]),
                "irr_pct":first_number(irr,&["irr","value"]).map(|v|if v.abs()<1. {v*100.}else{v}),
                "lcoe_cost":first_number(lcoe,&["lcoe_discounted_with_network","breakeven_price_kwh","cost_per_kwh"]),
                "simple_payback":number(&d["payback"],"simple_payback_years"),
                "discounted_payback":number(&d["payback"],"discounted_payback_years"),
                "min_dscr":number(dscr,"min_dscr"), "avg_dscr":number(dscr,"avg_dscr"),
                "dscr_threshold":number(dscr,"bankability_threshold").or_else(||number(pfa,"min_dscr_target")).unwrap_or(1.2),
                "base_price":first_number(params,&["sell_price_kwh_base","sell_price_kwh_used","base_price","used_price"]),
                "lifetime":number(params,"project_lifetime_years").or_else(||number(lcoe,"project_lifetime_years")).unwrap_or(25.)
            });
            let ps = &d["project_summary"];
            let plant = number(ps, "plant_cost");
            let network = number(ps, "network_cost");
            let investment = sum(plant, network);
            values["investment_total"] = json!(investment);
            values["plant_share_pct"] = ratio(plant, investment, 100.);
            values["network_share_pct"] = ratio(network, investment, 100.);
            values["initial_investment"] =
                json!(first_number(ps, &["initial_investment", "total_capex"]));
            values["lifetime_opex"] = json!(sum(
                number(ps, "total_fuel_lifetime"),
                number(ps, "total_maintenance_lifetime")
            ));
            values["npv_profitable"] = json!(values["npv"].as_f64().map(|v| v >= 0.));
            values["irr_viable"] = json!(values["irr_pct"]
                .as_f64()
                .zip(values["wacc_pct"].as_f64())
                .map(|(a, b)| a > b));
            values["dscr_bankable"] = json!(values["min_dscr"]
                .as_f64()
                .zip(values["dscr_threshold"].as_f64())
                .map(|(a, b)| a >= b));
            for (key, source) in [
                ("simple_payback_viable", "simple_payback"),
                ("discounted_payback_viable", "discounted_payback"),
            ] {
                values[key] = json!(values[source]
                    .as_f64()
                    .zip(values["lifetime"].as_f64())
                    .map(|(a, b)| a > 0. && a <= b));
            }
            for (key, label, unit) in [
                ("wacc_pct", "WACC", "%"),
                ("npv", "NPV", values["currency"].as_str().unwrap_or("XAF")),
                ("irr_pct", "IRR", "%"),
                ("lcoe_cost", "LCOE", "/kWh"),
                ("simple_payback", "Simple payback", "years"),
                ("discounted_payback", "Discounted payback", "years"),
                ("min_dscr", "Minimum DSCR", ""),
                ("avg_dscr", "Average DSCR", ""),
                (
                    "investment_total",
                    "Initial investment",
                    values["currency"].as_str().unwrap_or("XAF"),
                ),
            ] {
                card("Financial analysis", key, label, values[key].clone(), unit);
            }
            charts = finance::project(&request.report["05_financial"], system, &mut values)?;
            values
        }
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
        Section::PortfolioFinance
        | Section::PortfolioSite
        | Section::PortfolioPlant
        | Section::PortfolioBoq => unreachable!("portfolio composition is handled above"),
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
fn boq_rows(doc: &Value) -> Result<Value, String> {
    if doc.is_null() {
        return Ok(json!([]));
    }
    let source = ["display_rows", "rows", "items"]
        .iter()
        .find_map(|key| doc.get(*key).filter(|v| !v.is_null()));
    let Some(source) = source else {
        return Ok(json!([]));
    };
    let rows = source.as_array().ok_or("BOQ rows must be an array")?;
    let mut n = 0u32;
    let mut result = Vec::new();
    for row in rows {
        let mut row = row.as_object().ok_or("BOQ row must be an object")?.clone();
        let structural = ["is_header", "is_subtotal", "is_total"].iter().try_fold(
            false,
            |structural, key| match row.get(*key) {
                None | Some(Value::Null) => Ok(structural),
                Some(Value::Bool(v)) => Ok(structural || *v),
                _ => Err("BOQ row flags must be booleans"),
            },
        )?;
        if !structural {
            n += 1;
        }
        row.insert("_n".into(), json!(n));
        result.push(Value::Object(row));
    }
    Ok(json!(result))
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
fn first_number(d: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| number(d, key))
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
    if let Some(charts) = dashboard.charts.as_object() {
        for (key, chart) in charts {
            body.push_str(&static_charts::render(key, chart));
        }
    }
    if let Some(tables) = dashboard.derived["tables"].as_array() {
        for table in tables {
            let title = table["title"].as_str().unwrap_or("");
            body.push_str(&format!("<h2>{}</h2>", escape(title)));
            let Some(rows) = table["rows"].as_array().filter(|r| !r.is_empty()) else {
                body.push_str("<p>Unavailable in this sealed result</p>");
                continue;
            };
            let columns = table["columns"].as_array().unwrap();
            body.push_str("<div class=table-scroll tabindex=0><table><thead><tr>");
            for column in columns {
                body.push_str(&format!(
                    "<th>{}</th>",
                    escape(column["label"].as_str().unwrap_or(""))
                ));
            }
            body.push_str("</tr></thead><tbody>");
            let mut all_rows = rows.clone();
            if table["totals"].as_object().is_some_and(|t| !t.is_empty()) {
                all_rows.push(table["totals"].clone());
            }
            for row in all_rows {
                body.push_str("<tr>");
                for column in columns {
                    let value = &row[column["key"].as_str().unwrap()];
                    let text = match value {
                        Value::Null => "—".into(),
                        Value::String(s) => s.clone(),
                        v => v.to_string(),
                    };
                    body.push_str(&format!("<td>{}</td>", escape(&text)));
                }
                body.push_str("</tr>");
            }
            body.push_str("</tbody></table></div>");
        }
    }
    if let Some(detail) = dashboard.derived["unavailable_detail"]
        .as_str()
        .filter(|v| !v.is_empty())
    {
        body.push_str(&format!("<p>{}</p>", escape(detail)));
    }
    if let Some(graphs) = dashboard.derived["graphs"].as_array() {
        for graph in graphs {
            if let Some(url) = graph["url"].as_str() {
                body.push_str(&format!(
                    "<figure><figcaption>{} · {}</figcaption><img src=\"{}\" alt=\"{}\"></figure>",
                    escape(graph["label"].as_str().unwrap_or("")),
                    escape(graph["city"].as_str().unwrap_or("")),
                    escape(url),
                    escape(graph["key"].as_str().unwrap_or(""))
                ));
            }
        }
    }
    if matches!(dashboard.section, Section::PortfolioFinance) {
        if let Some(formulas) = dashboard.derived["formulas"].as_object() {
            body.push_str("<h2>Financial formulas</h2><dl>");
            for (name, formula) in formulas {
                if let Some(formula) = formula.as_str() {
                    body.push_str(&format!(
                        "<dt>{}</dt><dd><code>{}</code></dd>",
                        escape(name),
                        escape(formula)
                    ));
                }
            }
            body.push_str("</dl>");
        }
        for (key, label) in [
            ("kpi_rows", "Financial comparison"),
            ("investment_rows", "Initial investment"),
            ("cashflow_rows", "Annual cash flow"),
            ("dscr_rows", "Annual DSCR"),
            ("environmental_rows", "Environment"),
        ] {
            if let Some(rows) = dashboard.derived[key].as_array().filter(|r| !r.is_empty()) {
                let columns: std::collections::BTreeSet<&str> = rows
                    .iter()
                    .filter_map(Value::as_object)
                    .flat_map(|r| r.keys().map(String::as_str))
                    .collect();
                body.push_str(&format!(
                    "<h2>{}</h2><div class=table-scroll tabindex=0><table><caption>{}</caption><thead><tr>",
                    escape(label),
                    escape(label)
                ));
                for column in &columns {
                    body.push_str(&format!("<th>{}</th>", escape(column)));
                }
                body.push_str("</tr></thead><tbody>");
                for row in rows {
                    body.push_str("<tr>");
                    for column in &columns {
                        let value = match &row[*column] {
                            Value::Null => "—".into(),
                            Value::String(s) => s.clone(),
                            v => v.to_string(),
                        };
                        body.push_str(&format!("<td>{}</td>", escape(&value)));
                    }
                    body.push_str("</tr>");
                }
                body.push_str("</tbody></table></div>");
            }
        }
    }
    for (key, title) in [("network_rows", "Network BOQ"), ("plant_rows", "Plant BOQ")] {
        if let Some(rows) = dashboard.derived[key]
            .as_array()
            .filter(|rows| !rows.is_empty())
        {
            body.push_str(&format!("<h2>{}</h2><div class=table-scroll tabindex=0><table><caption>{}</caption><thead><tr><th>No.</th><th>Designation</th><th>Description</th><th>Unit</th><th>Unit price</th><th>Quantity</th><th>Amount</th></tr></thead><tbody>",escape(title),escape(title)));
            for row in rows {
                body.push_str("<tr>");
                for value in [
                    row["_n"].clone(),
                    first(row, &["designation", "type"]),
                    row["description"].clone(),
                    row["unit"].clone(),
                    first(row, &["unit_cost", "unit_price"]),
                    first(row, &["quantity", "qty"]),
                    first(row, &["total", "total_cost"]),
                ] {
                    let value = match value {
                        Value::Null => "—".into(),
                        Value::String(s) => s,
                        v => v.to_string(),
                    };
                    body.push_str(&format!("<td>{}</td>", escape(&value)));
                }
                body.push_str("</tr>");
            }
            body.push_str("</tbody></table></div>");
        }
    }
    let style = "*{box-sizing:border-box}body{font:16px system-ui;background:#f6f7f8;color:#20262e;margin:24px}h1{font-size:26px;line-height:1.3;overflow-wrap:anywhere}h2{font-size:14px;color:#52606d}.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,200px),1fr));gap:16px}article{min-width:0;background:#fff;padding:16px;border-left:3px solid #0f766e;border-radius:6px;overflow-wrap:anywhere}small,strong,span{display:block}strong{font-size:24px;margin:8px 0}span{color:#52606d}figure{min-width:0;margin:24px 0;padding:20px;background:#fff;border-radius:8px}figcaption{font-weight:bold;overflow-wrap:anywhere}svg,img{display:block;max-width:100%;height:auto}svg{width:100%;max-width:1000px}.legend{display:flex;flex-wrap:wrap;gap:16px;list-style:none;padding:0}.legend span{display:inline}.table-scroll,details{max-width:100%;overflow:auto}table{border-collapse:collapse;min-width:100%;font-size:13px}td,th{padding:8px;border-bottom:1px solid #dce1e5;text-align:left}summary{cursor:pointer}dd{margin-left:16px;overflow-wrap:anywhere}@media(max-width:600px){body{margin:16px}figure{padding:12px}}";
    format!("<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>{}</title><style>{style}svg text{{fill:#52606d}}</style><h1>{}</h1>{}</html>",escape(title),escape(title),body)
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
    fn static_dashboard_bounds_long_identities_and_every_table_family() {
        let mut dashboard = compose(Request {
            section: Section::Site,
            report: json!({"02_site_prep":{"site_metrics":{}}}),
            system: None,
        })
        .unwrap();
        dashboard.section = Section::PortfolioFinance;
        dashboard.derived = json!({
            "tables":[{"title":"Cities","columns":[{"label":"City","key":"city"}],"rows":[{"city":"long_city_identity"}]}],
            "kpi_rows":[{"city":"long_city_identity","npv":1}],
            "network_rows":[{"designation":"line","quantity":1}],
            "plant_rows":[{"designation":"inverter","quantity":1}]
        });
        let html = html(
            &dashboard,
            "long_project_identity · long_portfolio_identity",
        );
        assert!(html.contains("overflow-wrap:anywhere"));
        assert!(html.contains("svg text{fill:#52606d}"));
        assert!(html.contains(".table-scroll,details{max-width:100%;overflow:auto}"));
        assert_eq!(html.matches("<table>").count(), 4);
        assert_eq!(
            html.matches("<div class=table-scroll tabindex=0><table>")
                .count(),
            4
        );
        assert_eq!(html.matches("</table></div>").count(), 4);
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
    #[test]
    fn finance_preserves_zero_authority_and_normalizes_negative_fractional_irr() {
        let d=compose(Request{section:Section::Finance,system:None,report:json!({"05_financial":{"hybrid_financial_analysis":{
            "parameters":{"wacc":0.08,"sell_price_kwh_base":0,"sell_price_kwh_used":300},
            "npv":{"npv":0,"value":100},"irr":{"irr":-0.05,"value":5},
            "lcoe":{"lcoe_discounted_with_network":0,"cost_per_kwh":300},
            "project_summary":{"plant_cost":30,"network_cost":70,"initial_investment":0,"total_capex":100}
        }}})}).unwrap();
        assert_eq!(d.derived["wacc_pct"], 8.);
        assert_eq!(d.derived["irr_pct"], -5.);
        for key in ["npv", "lcoe_cost", "base_price", "initial_investment"] {
            assert_eq!(d.derived[key], 0.);
        }
        assert_eq!(d.derived["plant_share_pct"], 30.);
        assert_eq!(d.derived["network_share_pct"], 70.);
        assert!(d.derived["min_dscr"].is_null());
        assert_eq!(d.derived["npv_profitable"], true);
        assert_eq!(d.derived["irr_viable"], false);
        assert!(d.derived["dscr_bankable"].is_null());
    }
    #[test]
    fn boq_preserves_rows_and_refuses_cross_currency_sums() {
        let report = json!({"04_plant_cost":{"hybrid_boq":{"currency":"XAF","grand_total":0,"display_rows":[{"is_header":true},{"quantity":0,"total":0},{"is_subtotal":true},{"quantity":1}]},"network_costs":{"currency":"XAF","grand_total":10}}});
        let d = compose(Request {
            section: Section::Boq,
            system: None,
            report: report.clone(),
        })
        .unwrap();
        assert_eq!(d.derived["totals"]["grand_total"], 10.);
        assert!(d.derived["totals"]["equipment_total"].is_null());
        assert_eq!(
            d.derived["plant_rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["_n"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1, 1, 2]
        );
        assert_eq!(d.derived["plant_rows"][1]["quantity"], 0);
        let rendered = html(&d, "BOQ");
        assert!(rendered.contains("<table>"));
        assert!(!rendered.contains("<script"));
        let mut foreign = report;
        foreign["04_plant_cost"]["network_costs"]["currency"] = json!("USD");
        assert!(compose(Request {
            section: Section::Boq,
            system: None,
            report: foreign
        })
        .is_err());
    }
}
