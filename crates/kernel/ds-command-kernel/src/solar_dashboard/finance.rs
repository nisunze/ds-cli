//! Financial presentation decisions belong to the shared native/WASM owner.
use super::{first, first_number, number};
use serde_json::{json, Value};

const MAX_ROWS: usize = 10_000;

fn rows(value: &Value) -> Result<&[Value], String> {
    if value.is_null() {
        return Ok(&[]);
    }
    let rows = value.as_array().ok_or("Financial rows must be an array")?;
    if rows.len() > MAX_ROWS || rows.iter().any(|r| !r.is_object()) {
        return Err("Invalid or excessive financial rows".into());
    }
    Ok(rows)
}

fn year(value: &Value) -> Result<i64, String> {
    let y = value.as_f64().ok_or("Financial year must be numeric")?;
    if !y.is_finite() || y.fract() != 0. || !(0. ..=10_000.).contains(&y) {
        return Err("Financial year is out of bounds".into());
    }
    Ok(y as i64)
}

fn order(rows: &mut [Value]) -> Result<(), String> {
    rows.sort_by_key(|r| r["year"].as_i64());
    if rows.windows(2).any(|p| p[0]["year"] == p[1]["year"]) {
        return Err("Duplicate financial years".into());
    }
    Ok(())
}

fn cashflow(analysis: &Value, frame: &Value) -> Result<Vec<Value>, String> {
    let embedded = &analysis["cashflow_data"];
    let populated = embedded.as_array().is_some_and(|v| !v.is_empty())
        || embedded.as_object().is_some_and(|v| !v.is_empty());
    let source = if populated {
        embedded.clone()
    } else {
        first(frame, &["data", "rows", "columns"])
    };
    // Native frames have a numeric row count and typed column values.
    let source = if source.is_number() {
        frame["columns"].clone()
    } else {
        source
    };
    let mut result = Vec::new();
    let fields: [(&str, &[&str]); 7] = [
        ("revenue", &["revenue", "revenues"]),
        ("opex", &["opex"]),
        ("capex", &["capex", "investment", "investments"]),
        ("replacement", &["replacement", "replacements"]),
        ("net", &["net_cash_flow"]),
        ("cumulative", &["cumulative_cash_flow"]),
        (
            "cumPv",
            &[
                "cumulative_pv_cash_flow",
                "cumulative_pv",
                "cumulative_discounted",
            ],
        ),
    ];
    if source.is_array() {
        for row in rows(&source)? {
            let mut output = json!({"year":year(&row["year"])?});
            for (target, keys) in fields {
                output[target] = json!(first_number(row, keys));
            }
            result.push(output);
        }
    } else if source.is_object() {
        let column = |keys: &[&str]| {
            keys.iter().find_map(|k| {
                source[*k]
                    .as_array()
                    .or_else(|| source[*k]["values"].as_array())
            })
        };
        if let Some(years) = column(&["years", "year"]) {
            if years.len() > MAX_ROWS {
                return Err("Excessive cashflow years".into());
            }
            for (i, y) in years.iter().enumerate() {
                let mut output = json!({"year":year(y)?});
                for (target, keys) in fields {
                    output[target] = json!(column(keys)
                        .and_then(|c| c.get(i))
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite()));
                }
                result.push(output);
            }
            for (_, keys) in fields {
                if column(keys).is_some_and(|c| c.len() > years.len()) {
                    return Err("Cashflow column extends beyond its years".into());
                }
            }
        }
    } else if !source.is_null() {
        return Err("Invalid cashflow source".into());
    }
    for row in &mut result {
        for (field, flag) in [
            ("net", "net_positive"),
            ("cumulative", "cumulative_positive"),
        ] {
            row[flag] = json!(number(row, field).map(|v| v >= 0.));
        }
    }
    order(&mut result)?;
    Ok(result)
}

fn donut(items: &[(&str, Option<f64>, &str)], hole: &str) -> Value {
    let data: Vec<Value> = items
        .iter()
        .filter_map(|(name, value, color)| {
            value
                .filter(|v| *v >= 0.)
                .map(|v| json!({"name":name,"value":v,"itemStyle":{"color":color}}))
        })
        .collect();
    if !data
        .iter()
        .any(|v| number(v, "value").is_some_and(|v| v > 0.))
    {
        return Value::Null;
    }
    json!({"tooltip":{"trigger":"item","formatter":"{b}: {d}%"},"series":[{"type":"pie","radius":[hole,"75%"],"center":["50%","50%"],"label":{"show":true,"formatter":"{b}\n{d}%","fontSize":11,"color":"#64748b"},"data":data}]})
}

fn label(v: f64) -> String {
    if v.abs() < 1. {
        format!("{v:.2}")
    } else if v.abs() < 100. {
        format!("{v:.1}")
    } else {
        format!("{v:.0}")
    }
}

fn sensitivity(data: &Value, key: &str, values: &Value) -> Result<(Value, Value), String> {
    let is_wacc = key == "npv_vs_wacc";
    let is_capex = key == "irr_vs_investment";
    let is_irr = key.starts_with("irr");
    let reference = if is_capex {
        Some(0.)
    } else {
        number(values, if is_wacc { "wacc_pct" } else { "base_price" })
    };
    let threshold = if is_irr {
        number(values, "wacc_pct")
    } else {
        Some(0.)
    };
    let mut table = Vec::new();
    for row in rows(&data[key])? {
        let x = if is_wacc {
            number(row, "wacc_percent").or_else(|| number(row, "wacc").map(|v| v * 100.))
        } else {
            number(
                row,
                if is_capex {
                    "variation_percent"
                } else {
                    "price"
                },
            )
        }
        .ok_or("Sensitivity coordinate is unavailable")?;
        let y = number(row, if is_irr { "irr" } else { "npv" })
            .map(|v| if is_irr { v * 100. } else { v / 1e6 })
            .filter(|v| v.is_finite());
        table.push(json!({"x":label(x),"x_value":x,"y":y,
            "baseline":reference.map(|r|(x-r).abs()<if is_wacc {0.5}else{1.}),
            "favorable":y.zip(threshold).map(|(y,t)|if is_irr {y>t}else{y>=t})}));
    }
    if table.is_empty() {
        return Ok((Value::Null, json!([])));
    }
    table.sort_by(|a, b| {
        a["x_value"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["x_value"].as_f64().unwrap())
    });
    let mut lines = Vec::new();
    if let Some(t) = threshold {
        lines.push(json!({"yAxis":t,"lineStyle":{"color":"#E53935","type":"dashed"},"label":{"formatter":"Threshold"}}));
    }
    if let Some(r) = reference {
        // Pin the category to an actual coordinate, never a rounded category collision.
        if let Some(i) = table
            .iter()
            .position(|p| number(p, "x_value").is_some_and(|x| (x - r).abs() < 1e-9))
        {
            lines.push(json!({"xAxis":i,"lineStyle":{"color":"#43A047","type":"dotted"},"label":{"formatter":"Baseline"}}));
        }
    }
    let currency = values["currency"].as_str().unwrap_or("XAF");
    let x_name = if is_wacc {
        "WACC (%)".into()
    } else if is_capex {
        "CAPEX variation (%)".into()
    } else {
        format!("Price ({currency}/kWh)")
    };
    let y_name = if is_irr {
        "IRR (%)".into()
    } else {
        format!("NPV (M {currency})")
    };
    let chart = json!({"tooltip":{"trigger":"axis"},"grid":{"left":70,"right":20,"top":30,"bottom":50},
        "xAxis":{"type":"category","data":table.iter().map(|r|r["x"].clone()).collect::<Vec<_>>(),"name":x_name,"nameLocation":"center","nameGap":30},
        "yAxis":{"type":"value","name":y_name},"series":[{"type":"line","data":table.iter().map(|r|r["y"].clone()).collect::<Vec<_>>(),"connectNulls":false,"symbol":"circle","lineStyle":{"color":"#1976d2","width":2},"itemStyle":{"color":"#1976d2"},"markLine":{"silent":true,"data":lines}}]});
    Ok((chart, json!(table)))
}

pub(super) fn project(
    financial: &Value,
    system: &str,
    values: &mut Value,
) -> Result<Value, String> {
    let analysis = &financial[format!("{system}_financial_analysis")];
    let ps = &analysis["project_summary"];
    let lcoe = &analysis["lcoe"];
    for key in [
        "plant_cost",
        "network_cost",
        "total_fuel_lifetime",
        "total_maintenance_lifetime",
        "replacement_cost",
        "total_project_lifetime_cost",
    ] {
        values[key] = json!(number(ps, key));
    }
    values["total_pv_cost"] = json!(number(lcoe, "total_pv_cost")
        .or_else(|| number(&lcoe["components"]["pv_costs"], "with_network")));
    values["has_project_summary"] = json!(ps.as_object().is_some_and(|p| !p.is_empty()));
    values["show_payback"] = json!(["simple_payback", "discounted_payback"]
        .iter()
        .any(|k| number(values, k).is_some_and(|v| v > 0.)));
    values["show_dscr"] = json!(
        number(values, "min_dscr").is_some_and(|v| v > 0.)
            && analysis["project_finance_analysis"]["status"] != "disabled"
    );
    let mut formulas = json!({});
    for (key, section, alternatives) in [
        (
            "lcoe",
            "lcoe",
            &["lcoe_discounted", "discounted_with_network"][..],
        ),
        ("npv", "npv", &["npv"][..]),
        ("irr", "irr", &["irr"][..]),
        ("simple_payback", "payback", &["simple_payback"][..]),
        ("discounted_payback", "payback", &["discounted_payback"][..]),
    ] {
        formulas[key] = alternatives
            .iter()
            .find_map(|k| {
                analysis[section]["formulas"]
                    .get(*k)
                    .filter(|v| v.as_object().is_some_and(|o| !o.is_empty()))
            })
            .cloned()
            .unwrap_or_else(|| json!({}));
    }
    values["formulas"] = formulas;
    let cashflow = cashflow(analysis, &financial[format!("{system}_cashflow_df")])?;
    let mut charts = json!({});
    charts["investment"] = donut(
        &[
            ("Plant", number(ps, "plant_cost"), "#1976d2"),
            ("Network", number(ps, "network_cost"), "#43A047"),
        ],
        "40%",
    );
    charts["lifetime"] = donut(
        &[
            ("CAPEX", number(values, "initial_investment"), "#1565C0"),
            ("Replacements", number(ps, "replacement_cost"), "#64B5F6"),
            ("Fuel", number(ps, "total_fuel_lifetime"), "#43A047"),
            (
                "Maintenance",
                number(ps, "total_maintenance_lifetime"),
                "#81C784",
            ),
        ],
        "50%",
    );
    let components = &lcoe["components"];
    let component = |key: &str, nested: &str| {
        number(&components[key], nested)
            .or_else(|| components[key].as_f64().filter(|v| v.is_finite()))
    };
    charts["lcoe"] = donut(
        &[
            ("CAPEX", component("capex", "total"), "#1976d2"),
            ("OPEX", component("opex", "pv_total"), "#43A047"),
            (
                "Replacements",
                component("replacements", "pv_total"),
                "#FB8C00",
            ),
            (
                "End of life",
                component("end_of_life", "pv_total"),
                "#E53935",
            ),
        ],
        "40%",
    );
    values["sensitivity_tables"] = json!({});
    for key in [
        "npv_vs_pricing",
        "npv_vs_wacc",
        "irr_vs_pricing",
        "irr_vs_investment",
    ] {
        let (chart, table) = sensitivity(&analysis["sensitivity_analysis"], key, values)?;
        charts[key] = chart;
        values["sensitivity_tables"][key] = table;
    }
    let currency = values["currency"].as_str().unwrap_or("XAF");
    charts["cashflow"] = if cashflow.is_empty() {
        Value::Null
    } else {
        json!({"tooltip":{"trigger":"axis"},"legend":{"bottom":0},"grid":{"left":70,"right":70,"top":30,"bottom":50},
            "xAxis":{"type":"category","data":cashflow.iter().map(|r|r["year"].to_string()).collect::<Vec<_>>()},
            "yAxis":[{"type":"value","name":format!("Cumulative (M {currency})")},{"type":"value","name":format!("Net flow (M {currency})"),"splitLine":{"show":false}}],
            "series":[{"name":"Investments","type":"bar","yAxisIndex":1,"stack":"cf","itemStyle":{"color":"#E53935"},"data":cashflow.iter().map(|r|number(r,"net").map(|v|v.min(0.)/1e6)).collect::<Vec<_>>()},
            {"name":"Net revenue","type":"bar","yAxisIndex":1,"stack":"cf","itemStyle":{"color":"#43A047"},"data":cashflow.iter().map(|r|number(r,"net").map(|v|v.max(0.)/1e6)).collect::<Vec<_>>()},
            {"name":"Cumulative","type":"line","connectNulls":false,"itemStyle":{"color":"#1565C0"},"data":cashflow.iter().map(|r|number(r,"cumulative").map(|v|v/1e6)).collect::<Vec<_>>()}]})
    };
    values["cashflow_rows"] = json!(cashflow);
    let dscr = &analysis["project_finance_analysis"]["dscr"];
    let mut yearly = Vec::new();
    for row in rows(&first(dscr, &["yearly", "yearly_dscr"]))? {
        yearly.push(json!({"year":year(&row["year"])? ,"dscr":number(row,"dscr")}));
    }
    order(&mut yearly)?;
    charts["dscr"] = if yearly.is_empty() {
        Value::Null
    } else {
        json!({"tooltip":{"trigger":"axis"},"grid":{"left":60,"right":20,"top":30,"bottom":40},"xAxis":{"type":"category","data":yearly.iter().map(|r|r["year"].to_string()).collect::<Vec<_>>()},"yAxis":{"type":"value","name":"DSCR (x)"},"series":[{"type":"line","connectNulls":false,"itemStyle":{"color":"#1565C0"},"data":yearly.iter().map(|r|r["dscr"].clone()).collect::<Vec<_>>(),"markLine":{"silent":true,"data":[{"yAxis":values["dscr_threshold"],"lineStyle":{"color":"#E53935","type":"dashed"},"label":{"formatter":"Bankability threshold"}}]}}]})
    };
    values["dscr_rows"] = json!(yearly);
    Ok(charts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solar_dashboard::{compose, Request, Section};
    fn dashboard(
        analysis: Value,
        frame: Value,
    ) -> Result<crate::solar_dashboard::Dashboard, String> {
        compose(Request {
            section: Section::Finance,
            system: None,
            report: json!({"05_financial":{"hybrid_financial_analysis":analysis,"hybrid_cashflow_df":frame}}),
        })
    }
    #[test]
    fn null_irr_is_a_gap_and_wacc_percent_is_authoritative() {
        let d = dashboard(
            json!({"parameters":{"wacc":0.08,"sell_price_kwh_base":147},"sensitivity_analysis":{
            "irr_vs_pricing":[{"price":147,"irr":null},{"price":160,"irr":-0.05}],
            "npv_vs_wacc":[{"wacc_percent":8,"wacc":0.9,"npv":0}]}}),
            Value::Null,
        )
        .unwrap();
        assert_eq!(
            d.charts["irr_vs_pricing"]["series"][0]["data"],
            json!([null, -5.])
        );
        assert!(d.derived["sensitivity_tables"]["irr_vs_pricing"][0]["favorable"].is_null());
        assert_eq!(
            d.derived["sensitivity_tables"]["npv_vs_wacc"][0]["x_value"],
            8.
        );
        assert_eq!(
            d.derived["sensitivity_tables"]["npv_vs_wacc"][0]["baseline"],
            true
        );
    }
    #[test]
    fn cashflow_and_dscr_keep_year_pairs_and_zero_values() {
        let d=dashboard(json!({"cashflow_data":[{"year":2,"net_cash_flow":null,"cumulative_pv_cash_flow":0},{"year":1,"net_cash_flow":0}],
            "project_finance_analysis":{"dscr":{"yearly":[{"year":2,"dscr":null},{"year":1,"dscr":0}]}}}),Value::Null).unwrap();
        assert_eq!(d.derived["cashflow_rows"][0]["year"], 1);
        assert_eq!(d.derived["cashflow_rows"][0]["net"], 0.);
        assert!(d.derived["cashflow_rows"][0]["capex"].is_null());
        assert_eq!(d.derived["cashflow_rows"][1]["cumPv"], 0.);
        assert_eq!(d.charts["dscr"]["xAxis"]["data"], json!(["1", "2"]));
        assert_eq!(d.charts["dscr"]["series"][0]["data"], json!([0., null]));
    }
    #[test]
    fn native_frame_fallback_and_small_costs_do_not_become_false_zeroes() {
        let d=dashboard(json!({"project_summary":{"plant_cost":1,"network_cost":2},"lcoe":{"components":{"capex":{"total":0},"opex":{"pv_total":1}},"formulas":{"discounted_with_network":{"latex":"fallback"}}}}),
            json!({"kind":"frame","rows":2,"columns":{"year":{"values":[2,1]},"net_cash_flow":{"values":[1,0]}}})).unwrap();
        assert_eq!(d.charts["investment"]["series"][0]["data"][0]["value"], 1.);
        assert_eq!(d.derived["cashflow_rows"][0]["net"], 0.);
        assert_eq!(d.charts["lcoe"]["series"][0]["data"][0]["value"], 0.);
        assert_eq!(d.derived["formulas"]["lcoe"]["latex"], "fallback");
        assert!(dashboard(
            json!({"cashflow_data":[{"year":1},{"year":1}]}),
            Value::Null
        )
        .is_err());
    }
}
