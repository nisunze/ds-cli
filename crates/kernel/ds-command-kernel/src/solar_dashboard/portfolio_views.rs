//! Portfolio table, assumption and graph selection shared by Server and WASM.
use super::{first, number, Card, Dashboard, Section};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn column(key: &str) -> Value {
    let (token, title) = match key {
        "annual_energy_mwh_inv" => ("sol_col_energy_per_inv", "Energy (MWh/inv)"),
        "annual_mwh" => ("sol_col_annual_mwh", "Demand (MWh/yr)"),
        "availability_pct" => ("sol_col_availability", "Avail. (%)"),
        "battery_mwh" => ("sol_col_battery_mwh", "Storage (MWh)"),
        "capacity_factor_pct" => ("sol_col_cap_factor", "CF (%)"),
        "capex_per_customer" => ("sol_col_capex_per_customer", "Cost/customer"),
        "chosen_system" => ("sol_col_selected_system", "Selected system"),
        "city" => ("sol_col_site", "Site"),
        "co2_lifetime_t" => ("sol_col_co2_lifetime", "CO2 25yr (t)"),
        "co2_y1_t" => ("sol_col_co2_y1", "CO2 Yr 1 (t)"),
        "country" => ("sol_col_country", "Country"),
        "customers" => ("sol_col_customers", "Customers"),
        "daily_battery_mwh" => ("sol_col_battery_daily", "Battery (MWh/d)"),
        "daily_generator_mwh" => ("sol_col_generator_daily", "Generator (MWh/d)"),
        "daily_kwh" => ("sol_col_daily_kwh", "Energy/d (kWh)"),
        "daily_load_mwh" => ("sol_col_daily_load", "Avg load (MWh/d)"),
        "daily_solar_direct_mwh" => ("sol_col_solar_daily", "Solar (MWh/d)"),
        "dc_capacity_kw" => ("sol_col_dc_capacity", "DC capacity (kW)"),
        "dni_annual_kwh_m2" => ("sol_col_dni", "DNI (kWh/m2/yr)"),
        "energy_lifetime_gwh" => ("sol_col_energy_lifetime", "Energy 25yr (GWh)"),
        "energy_year1_mwh" => ("sol_col_energy_y1", "Energy Yr 1 (MWh)"),
        "equipment_total" => ("sol_col_equipment_total", "Equipment"),
        "fuel_lifetime_ml" => ("sol_col_fuel_lifetime", "Fuel 25yr (ML)"),
        "fuel_y1_kl" => ("sol_col_fuel_y1", "Fuel Yr 1 (kL)"),
        "generator_config" => ("sol_col_generator", "Generator"),
        "generator_kva" => ("sol_col_generator_kva", "Generator rating (kVA)"),
        "ghi_annual_kwh_m2" => ("sol_col_ghi", "GHI (kWh/m2/yr)"),
        "grand_total" => ("sol_col_grand_total", "Total Project"),
        "inverters" => ("sol_col_inverters", "Inverters"),
        "latitude" => ("sol_col_lat", "Lat."),
        "lcoe_hybrid" => ("sol_col_lcoe_hybrid", "LCOE Hybrid"),
        "lcoe_selected" => ("sol_col_lcoe_selected", "LCOE selected"),
        "lcoe_simple" => ("sol_col_lcoe_simple", "LCOE (XAF/kWh)"),
        "lcoe_solar_battery" => ("sol_col_lcoe_solar_battery", "LCOE Sol.+Batt"),
        "lcoe_thermal_only" => ("sol_col_lcoe_thermal", "LCOE Thermal"),
        "load_factor" => ("sol_col_load_factor", "Load factor"),
        "longitude" => ("sol_col_lon", "Lon."),
        "lv_km" => ("sol_col_bt_km", "LV (km)"),
        "markup_total" => ("sol_col_markup_total", "Markups"),
        "modules_per_inverter" => ("sol_col_modules_per_inverter", "Mod./inverter"),
        "modules_per_string" => ("sol_col_modules_per_string", "Mod./string"),
        "mv_km" => ("sol_col_mt_km", "MV (km)"),
        "network_capex" => ("sol_col_network_capex", "Network CAPEX"),
        "network_total" => ("sol_col_network_total", "Network Total"),
        "optimal_tilt_deg" => ("sol_col_tilt", "Tilt (deg)"),
        "panels" => ("sol_col_panels", "Panels"),
        "peak_hours" => ("sol_col_peak_hours", "Peak hours"),
        "peak_kw" => ("sol_col_peak_kw", "Peak (kW)"),
        "peak_sun_hours" => ("sol_col_peak_sun", "Peak sun hours (h/d)"),
        "plant_capex" => ("sol_col_plant_capex", "CAPEX"),
        "plant_equipment" => ("sol_col_plant_equipment", "Plant Equip."),
        "plant_markup" => ("sol_col_plant_markup", "Plant Markup"),
        "plant_total" => ("sol_col_plant_total", "Plant Total"),
        "poa_max_kwh_m2" => ("sol_col_poa_max", "POA Max (kWh/m2/yr)"),
        "pv_mw" => ("sol_col_pv_mw", "PV (MW)"),
        "renewable_pct" => ("sol_col_renewable_pct", "Renewable (%)"),
        "replacement_cost" => ("sol_col_replacement", "Replacement"),
        "selected_system" => ("sol_col_selected_system", "Selected system"),
        "storage_mwh" => ("sol_col_storage_mwh", "Storage (MWh)"),
        "strings_per_inverter" => ("sol_col_strings_per_inverter", "Strings/inv."),
        "system_max_dc_voltage" => ("sol_col_vdc_max", "Vdc max (V)"),
        "system_type" => ("sol_col_system", "System"),
        "tco" => ("sol_col_tco", "TCO"),
        "temp_max_c" => ("sol_col_temp_max", "Max T (C)"),
        "temp_max_month" => ("sol_col_max_month", "Max month"),
        "temp_mean_c" => ("sol_col_temp_mean", "Mean T (C)"),
        "temp_min_c" => ("sol_col_temp_min", "Min T (C)"),
        "temp_min_month" => ("sol_col_min_month", "Min month"),
        "transformers" => ("sol_col_transformers", "Transformers"),
        _ => return json!({"key":key,"label":label(key)}),
    };
    json!({"key":key,"label":title,"label_key":token})
}

fn label(key: &str) -> String {
    key.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
pub(super) fn normalize(mut report: Value) -> Result<Value, String> {
    if report["schema_version"]
        .as_str()
        .is_some_and(|s| s.starts_with("ds-solar.portfolio-result/"))
    {
        let run = report["run_id"]
            .as_str()
            .ok_or("Portfolio run identity is unavailable")?
            .to_owned();
        let admitted = ds_solar_contracts::validate_portfolio_result_read(report, &run)
            .map_err(|e| e.to_string())?;
        let d = admitted.document;
        report = d["sections"].clone();
        report["meta"] = json!({"currency":d["assumptions"]["currency"],"project_years":d["assumptions"]["project_years"],
            "discount_rate":d["assumptions"]["discount_rate"],"portfolio_id":d["portfolio_id"],
            "graph_strategy":d["sections"]["representative_graphs"]["_strategy"],
            "representative_city":d["cities"].as_array().and_then(|cities| cities.iter().find(|city| city["city_id"] == d["representative_city_id"])).map(|city|city["display_name"].clone())});
    }
    if !report["representative_graphs"].is_null() {
        report["representative_graphs"] =
            crate::solar_portfolio_read::graph_groups(report["representative_graphs"].clone())?;
    }
    Ok(report)
}
fn rows(source: &Value) -> Result<Vec<Value>, String> {
    if source.is_null() {
        return Ok(Vec::new());
    }
    source
        .as_array()
        .filter(|r| {
            r.len() <= 10_000
                && r.iter()
                    .all(|r| r.as_object().is_some_and(|r| r.len() <= 128))
        })
        .cloned()
        .ok_or_else(|| "Invalid or excessive portfolio dashboard table".into())
}
fn table(
    title: &str,
    source: &Value,
    keys: &str,
    aggregations: &Value,
    totals: &str,
) -> Result<Value, String> {
    let source = rows(source)?;
    let mut mapped = Vec::new();
    let columns = keys.split_whitespace().map(column).collect::<Vec<_>>();
    for row in source {
        let mut projected = serde_json::Map::new();
        for key in keys.split_whitespace() {
            projected.insert(key.to_owned(), row[key].clone());
        }
        if let Some(structural) = row.get("_structural") {
            projected.insert("_structural".into(), structural.clone());
        }
        mapped.push(Value::Object(projected));
    }
    let mut values = serde_json::Map::new();
    for mapping in totals.split_whitespace() {
        let (key, aggregate) = mapping
            .split_once(':')
            .ok_or("Invalid owned dashboard total mapping")?;
        values.insert(key.to_owned(), aggregations[aggregate].clone());
    }
    Ok(
        json!({"title":title,"columns":columns,"rows":mapped,"totals":values,"available":!mapped.is_empty()}),
    )
}

/// Producer references are presentation addresses; immutable receipts retain generation identity.
pub(crate) fn graph_url(entry: &Value, depth: u8) -> Option<String> {
    if depth > 3 {
        return None;
    }
    if let Some(value) = entry.as_str() {
        if value.is_empty()
            || value.len() > 4096
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
            || value.contains('\\')
        {
            return None;
        }
        if value.starts_with("https://") {
            let url = url::Url::parse(value).ok()?;
            return (url.username().is_empty()
                && url.password().is_none()
                && url.host_str().is_some_and(|h| h.contains('.')))
            .then(|| value.to_owned());
        }
        return (!value.starts_with('/')
            && !value.contains(':')
            && value
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != "..")
            && [".png", ".jpg", ".jpeg", ".webp", ".svg"]
                .iter()
                .any(|suffix| value.ends_with(suffix)))
        .then(|| value.to_owned());
    }
    if let Some(url) = entry.get("url") {
        if let Some(url) = graph_url(url, depth + 1) {
            return Some(url);
        }
    }
    let object = entry.as_object()?;
    const FIELDS: &[&str] = &[
        "schema",
        "kind",
        "media_id",
        "logical_role",
        "bucket",
        "object",
        "generation",
        "size_bytes",
        "media_type",
        "sha256",
        "file_name",
    ];
    if object.len() != FIELDS.len()
        || FIELDS.iter().any(|key| !object.contains_key(*key))
        || entry["schema"] != "ds.media/v1"
        || entry["kind"] != "immutable"
    {
        return None;
    }
    let text = |key: &str| {
        entry[key]
            .as_str()
            .filter(|v| !v.is_empty() && v.trim() == *v && !v.chars().any(char::is_control))
    };
    if ["media_id", "logical_role"]
        .iter()
        .any(|key| text(key).is_none_or(|v| v.len() > 256))
    {
        return None;
    }
    let bucket = text("bucket")?;
    if !(3..=223).contains(&bucket.len())
        || !bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || !bucket.as_bytes()[0].is_ascii_alphanumeric()
        || !bucket.as_bytes()[bucket.len() - 1].is_ascii_alphanumeric()
    {
        return None;
    }
    let path = text("object")?;
    if path.len() > 4096
        || path.contains('\\')
        || path.chars().any(char::is_whitespace)
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return None;
    }
    let generation = text("generation")?;
    if generation.starts_with('0') || !generation.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !entry["size_bytes"]
        .as_u64()
        .is_some_and(|n| n > 0 && n <= 100 * 1024 * 1024)
    {
        return None;
    }
    let sha = text("sha256")?;
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let filename = text("file_name")?;
    if filename.len() > 256 || filename.contains(['/', '\\']) {
        return None;
    }
    let suffixes: &[&str] = match text("media_type")? {
        "image/png" => &[".png"],
        "image/jpeg" => &[".jpg", ".jpeg"],
        "image/webp" => &[".webp"],
        "image/svg+xml" => &[".svg"],
        _ => return None,
    };
    if !suffixes.iter().any(|suffix| filename.ends_with(suffix)) {
        return None;
    }
    fn encode(value: &str) -> String {
        value
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    Some(format!(
        "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media&generation={}",
        encode(bucket),
        encode(path),
        generation
    ))
}
fn graph_section(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "combined_irradiance_and_temperature"
        | "combined_analemma_and_poa"
        | "monthly_average_daily_energy" => "site",
        "daily_load_profile" => "demand",
        "existing_network" | "proposed_network" => "network",
        "energy_production" | "energy_profiles" | "degradation" | "maintenance_costs"
        | "fuel_consumption" | "co2_emissions" => "performance",
        "cost_breakdown" => "investment",
        "cash_flow" | "npv_sensitivity" | "financial_overview" => "financial_kpis",
        "tariff_analysis" | "lcoe_comparison" => "lcoe",
        _ => return None,
    })
}
pub(super) fn media(
    report: &Value,
    groups: &[&str],
    representative: Option<&str>,
) -> Result<Value, String> {
    let mut graphs = Vec::new();
    let rep = &report["representative_graphs"];
    for group in groups {
        if let Some(entries) = rep[*group].as_object() {
            if entries.len() > 128 {
                return Err("Excessive portfolio graph slots".into());
            }
            for (key, entry) in entries {
                if key.starts_with('_') {
                    continue;
                }
                let city = entry["city"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::trim)
                    .or_else(|| {
                        rep["_representative_city"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::trim)
                    })
                    .or(representative);
                graphs.push(json!({"section":group,"key":key,"label":label(key),"city":city,"url":graph_url(entry,0)}));
            }
        }
    }
    let mut unavailable = Vec::new();
    for entry in rows(&rep["_unavailable"])? {
        if let (Some(kind), Some(city), Some(field)) = (
            entry["kind"].as_str(),
            entry["city_id"].as_str(),
            entry["missing_field"].as_str(),
        ) {
            if graph_section(kind).is_some_and(|s| groups.contains(&s)) {
                unavailable.push(format!("{kind} ({city}): {field}"));
            }
        }
    }
    Ok(json!({"graphs":graphs,"unavailable_detail":unavailable.join("; ")}))
}

pub(super) fn compose(section: Section, report: Value) -> Result<Dashboard, String> {
    let report = normalize(report)?;
    let mut tables = Vec::new();
    let s = &report["site"];
    let p = &report["performance"];
    let i = &report["investment"];
    let mut add = |title: &str,
                   source: &Value,
                   columns: &str,
                   aggregate: &Value,
                   totals: &str|
     -> Result<(), String> {
        tables.push(table(title, source, columns, aggregate, totals)?);
        Ok(())
    };
    let groups: &[&str] = match section {
        Section::PortfolioSite => {
            add(
                "Portfolio map coordinates",
                &report["maps"]["locations"],
                "city latitude longitude",
                &Value::Null,
                "",
            )?;
            add(
                "Site and solar resource",
                &first(s, &["site_table", "table"]),
                "city country latitude longitude ghi_annual_kwh_m2 dni_annual_kwh_m2 poa_max_kwh_m2 peak_sun_hours",
                &s["aggregations"],
                "ghi_annual_kwh_m2:ghi_mean dni_annual_kwh_m2:dni_mean poa_max_kwh_m2:poa_mean peak_sun_hours:peak_sun_hours_mean",
            )?;
            add(
                "Climate",
                &s["climate_table"],
                "city temp_mean_c temp_min_c temp_min_month temp_max_c temp_max_month",
                &s["aggregations"],
                "temp_mean_c:temp_mean_avg temp_min_c:temp_min_global temp_max_c:temp_max_global",
            )?;
            add(
                "PV configuration",
                &s["pv_config_table"],
                "city optimal_tilt_deg modules_per_string strings_per_inverter modules_per_inverter dc_capacity_kw system_max_dc_voltage annual_energy_mwh_inv capacity_factor_pct",
                &s["aggregations"],
                "dc_capacity_kw:total_dc_capacity_kw annual_energy_mwh_inv:total_annual_energy_mwh capacity_factor_pct:mean_capacity_factor_pct",
            )?;
            add(
                "Demand",
                &report["demand"]["table"],
                "city customers peak_kw daily_kwh annual_mwh load_factor peak_hours",
                &report["demand"]["aggregations"],
                "customers:total_customers peak_kw:total_peak_kw daily_kwh:total_daily_kwh annual_mwh:total_annual_mwh load_factor:mean_load_factor",
            )?;
            add(
                "Network and costs",
                &report["network"]["table"],
                "city customers transformers lv_km mv_km network_capex capex_per_customer",
                &report["network"]["aggregations"],
                "customers:total_customers transformers:total_transformers lv_km:total_lv_km mv_km:total_mv_km network_capex:total_network_capex capex_per_customer:mean_capex_per_customer",
            )?;
            &["site", "demand", "network"]
        }
        Section::PortfolioPlant => {
            add(
                "Scenario selection",
                &report["scenarios"]["table"],
                "city selected_system lcoe_hybrid lcoe_solar_battery lcoe_thermal_only lcoe_selected renewable_pct",
                &Value::Null,
                "",
            )?;
            add(
                "System selection rationale",
                &report["system_rationale"]["table"],
                "city chosen_system daily_kwh peak_kw load_factor_pct lcoe_hybrid lcoe_solar_battery lcoe_gap_sb_minus_hyb",
                &Value::Null,
                "",
            )?;
            add(
                "Plant sizing",
                &first(p, &["equipment_table", "table"]),
                "city system_type pv_mw panels storage_mwh inverters generator_config availability_pct",
                &p["aggregations"],
                "pv_mw:total_pv_mw panels:total_panels storage_mwh:total_storage_mwh inverters:total_inverters availability_pct:mean_availability_pct",
            )?;
            add(
                "Energy production",
                &p["energy_table"],
                "city daily_load_mwh energy_year1_mwh energy_lifetime_gwh daily_solar_direct_mwh daily_battery_mwh daily_generator_mwh renewable_pct",
                &p["aggregations"],
                "daily_load_mwh:total_daily_load_mwh energy_year1_mwh:total_energy_year1_mwh energy_lifetime_gwh:total_energy_lifetime_gwh daily_solar_direct_mwh:total_daily_solar_mwh daily_battery_mwh:total_daily_battery_mwh daily_generator_mwh:total_daily_generator_mwh renewable_pct:mean_renewable_pct",
            )?;
            add(
                "Plant costs",
                &p["cost_table"],
                "city plant_capex replacement_cost total_opex tco lcoe_simple",
                &p["aggregations"],
                "plant_capex:total_plant_capex replacement_cost:total_replacement_cost total_opex:total_opex tco:total_tco lcoe_simple:weighted_lcoe",
            )?;
            add(
                "Environmental impact",
                &p["environmental_table"],
                "city fuel_y1_kl co2_y1_t fuel_lifetime_ml co2_lifetime_t",
                &p["aggregations"],
                "fuel_y1_kl:total_fuel_y1_kl co2_y1_t:total_co2_y1_t fuel_lifetime_ml:total_fuel_lifetime_ml co2_lifetime_t:total_co2_lifetime_t",
            )?;
            &["performance"]
        }
        Section::PortfolioBoq => {
            let mut boq = rows(&i["merged_boq"])?;
            for row in &mut boq {
                let mut structural = false;
                for key in ["is_header", "is_subtotal", "is_total"] {
                    match row.get(key) {
                        None | Some(Value::Null) => {}
                        Some(Value::Bool(flag)) => structural |= flag,
                        _ => return Err("Portfolio BOQ row flags must be booleans".into()),
                    }
                }
                row["_structural"] = json!(structural);
                row["designation"] = first(row, &["designation", "description"]);
                row["unit_price"] = first(row, &["unit_price", "unit_cost"]);
                row["amount"] = first(row, &["amount", "total_cost", "total"]);
            }
            add(
                "Consolidated BOQ",
                &json!(boq),
                "no designation unit quantity unit_price amount",
                &Value::Null,
                "",
            )?;
            add(
                "Equipment by site",
                &i["city_boq_table"],
                "city inverters panels battery_mwh generator_config generator_kva equipment_total markup_total plant_capex",
                &i["aggregations"],
                "inverters:total_inverters panels:total_panels battery_mwh:total_battery_mwh generator_kva:total_generator_kva equipment_total:total_equipment markup_total:total_markup plant_capex:total_plant_capex",
            )?;
            add(
                "BOQ by site",
                &i["city_breakdown_table"],
                "city plant_equipment plant_markup plant_total network_total grand_total",
                &i["aggregations"],
                "plant_equipment:total_equipment plant_markup:total_markup plant_total:total_plant_capex network_total:total_network_capex grand_total:total_portfolio_capex",
            )?;
            add(
                "Lifetime cost projection",
                &p["cost_table"],
                "city plant_capex replacement_cost total_opex tco lcoe_simple",
                &p["aggregations"],
                "plant_capex:total_plant_capex replacement_cost:total_replacement_cost total_opex:total_opex tco:total_tco lcoe_simple:weighted_lcoe",
            )?;
            &[]
        }
        _ => return Err("Unknown portfolio table dashboard".into()),
    };
    let mut cards = Vec::new();
    let currency = report["meta"]["currency"]
        .as_str()
        .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()));
    let years = number(&report["meta"], "project_years")
        .filter(|v| v.fract() == 0. && (1. ..=100.).contains(v));
    let discount = number(&report["meta"], "discount_rate")
        .filter(|v| (0. ..=1.).contains(v))
        .map(|v| v * 100.);
    let representative = report["meta"]["representative_city"]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .map(str::trim)
        .or_else(|| {
            (report["meta"]["graph_strategy"] == "round_robin").then_some("Per graph (round robin)")
        });
    for (key, title, value, unit) in [
        ("currency", "Currency", json!(currency), ""),
        ("project_years", "Horizon", json!(years), "years"),
        ("discount_rate_pct", "Discount rate", json!(discount), "%"),
        (
            "representative",
            "Representative",
            json!(representative),
            "",
        ),
    ] {
        cards.push(Card {
            key: key.into(),
            group: "Assumptions".into(),
            label: title.into(),
            value,
            unit: unit.into(),
        });
    }
    let media = media(&report, groups, representative)?;
    let identities = tables
        .iter()
        .filter_map(|t| t["title"].as_str())
        .collect::<BTreeSet<_>>();
    if identities.len() != tables.len() {
        return Err("Duplicate owned portfolio table identity".into());
    }
    Ok(Dashboard {
        schema: "ds-solar.dashboard/v1",
        section,
        cards,
        charts: json!({}),
        derived: json!({"tables":tables,"graphs":media["graphs"],"unavailable_detail":media["unavailable_detail"]}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boq_selects_owned_amounts_and_uses_calculator_totals_without_recomputing() {
        let view = compose(Section::PortfolioBoq, json!({"investment":{"merged_boq":[{"designation":"Panels","unit_price":147,"unit_cost":99,"amount":735,"total":514}],"city_breakdown_table":[{"city":"A","grand_total":2}],"aggregations":{"total_portfolio_capex":42}}})).unwrap();
        assert_eq!(view.derived["tables"][0]["rows"][0]["unit_price"], 147);
        assert_eq!(view.derived["tables"][0]["rows"][0]["amount"], 735);
        assert_eq!(view.derived["tables"][2]["totals"]["grand_total"], 42);
        assert!(view.derived["tables"][0]["rows"][0]
            .get("unit_cost")
            .is_none());
    }
    #[test]
    fn attribution_and_exact_unavailable_slot_are_shared() {
        let view = compose(Section::PortfolioSite, json!({"meta":{"discount_rate":0.0875,"graph_strategy":"round_robin"},"representative_graphs":{"site":{"monthly_avg_daily_energy_image":{"city":"Beta","url":"plots/b.png"}},"_unavailable":[{"kind":"daily_load_profile","city_id":"beta","missing_field":"plots.daily_load_profile"},{"kind":"cost_breakdown","city_id":"beta","missing_field":"plots.cost"}]}})).unwrap();
        assert_eq!(view.cards[2].value, 8.75);
        assert_eq!(view.derived["graphs"][0]["city"], "Beta");
        assert_eq!(
            view.derived["unavailable_detail"],
            "daily_load_profile (beta): plots.daily_load_profile"
        );
        assert!(graph_url(&json!("javascript:alert(1)"), 0).is_none());
    }
    #[test]
    fn invalid_rows_and_unverified_receipts_refuse() {
        assert!(compose(
            Section::PortfolioPlant,
            json!({"performance":{"equipment_table":[1]}})
        )
        .is_err());
        assert!(graph_url(
            &json!({"schema":"ds.media/v1","kind":"immutable","bucket":"a","object":"../file.png"}),
            0
        )
        .is_none());
    }
}
