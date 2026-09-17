//! Portfolio finance presentation, shared by native Server and WASM.
use super::{first_number, number, Card, Dashboard, Section};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ROWS: usize = 10_000;
const COLORS: [&str; 10] = [
    "#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b", "#e377c2", "#7f7f7f",
    "#bcbd22", "#17becf",
];

fn rows(v: &Value) -> Result<&[Value], String> {
    if v.is_null() {
        return Ok(&[]);
    }
    v.as_array()
        .filter(|r| {
            r.len() <= MAX_ROWS
                && r.iter()
                    .all(|r| r.as_object().is_some_and(|r| r.len() <= 128))
                && r.iter()
                    .filter_map(Value::as_object)
                    .flat_map(|r| r.keys())
                    .collect::<BTreeSet<_>>()
                    .len()
                    <= 128
        })
        .map(Vec::as_slice)
        .ok_or_else(|| "Invalid or excessive portfolio financial rows".into())
}
fn ordered(v: &Value) -> Result<Vec<Value>, String> {
    let mut out = rows(v)?.to_vec();
    let mut years = BTreeSet::new();
    for r in &mut out {
        let y = number(r, "year")
            .filter(|v| v.fract() == 0. && (0. ..=10_000.).contains(v))
            .ok_or("Invalid portfolio financial year")? as i64;
        if !years.insert(y) {
            return Err("Duplicate portfolio financial years".into());
        }
        r["year"] = json!(y);
    }
    out.sort_by_key(|r| r["year"].as_i64());
    Ok(out)
}
fn sum(r: &[Value], field: &str) -> Option<f64> {
    if r.is_empty() {
        return None;
    }
    r.iter()
        .try_fold(0., |s, r| number(r, field).map(|v| s + v))
        .filter(|s| s.is_finite())
}
fn donut(title: &str, values: &[(&str, Option<f64>)]) -> Result<Value, String> {
    if values.iter().any(|(_, v)| v.is_some_and(|v| v < 0.)) {
        return Err("Negative portfolio cost in a cost chart".into());
    }
    // Incomplete totals cannot truthfully define percentages.
    if values.iter().any(|(_, v)| v.is_none())
        || !values.iter().any(|(_, v)| v.is_some_and(|v| v > 0.))
    {
        return Ok(Value::Null);
    }
    let data: Vec<Value> = values.iter().enumerate().map(|(i, (name, v))|
        json!({"name":name,"value":v,"itemStyle":{"color":COLORS[i % COLORS.len()]}})).collect();
    Ok(
        json!({"title":{"text":title},"tooltip":{"trigger":"item"},"series":[{"type":"pie","radius":["40%","70%"],"data":data}]}),
    )
}
fn series(name: &str, kind: &str, data: Value, axis: usize, color: &str) -> Value {
    json!({"name":name,"type":kind,"data":data,"yAxisIndex":axis,"itemStyle":{"color":color},"symbol":"none"})
}
fn cartesian(title: &str, years: &[i64], axes: Value, series: Vec<Value>) -> Value {
    if series.is_empty() {
        return Value::Null;
    }
    json!({"title":{"text":title},"tooltip":{"trigger":"axis"},"legend":{"bottom":0},
        "grid":{"left":"10%","right":"12%","bottom":"15%","top":"15%"},
        "xAxis":{"type":"category","name":"Year","data":years.iter().map(i64::to_string).collect::<Vec<_>>()},
        "yAxis":axes,"series":series})
}
fn values(r: &[Value], field: &str, scale: f64) -> Value {
    json!(r
        .iter()
        .map(|r| number(r, field)
            .map(|v| v * scale)
            .filter(|v| v.is_finite()))
        .collect::<Vec<_>>())
}
fn sensitivity(
    s: &Value,
    key: &str,
    xfield: &str,
    irr: bool,
    xunit: &str,
    yunit: &str,
) -> Result<Value, String> {
    let mut out = Vec::new();
    let mut total = 0;
    let cities = rows(&s["city_sensitivities"])?;
    if cities.len() > 200 {
        return Err("Excessive portfolio sensitivity cities".into());
    }
    let sources: Vec<(&str, &Value)> = cities
        .iter()
        .map(|c| {
            (
                c["city"]
                    .as_str()
                    .or_else(|| c["city_id"].as_str())
                    .unwrap_or("City"),
                &c[key],
            )
        })
        .chain((!irr).then_some(("Portfolio", &s[key])))
        .collect();
    for (i, (name, source)) in sources.into_iter().enumerate() {
        if name.len() > 2048 {
            return Err("Excessive portfolio city label".into());
        }
        let mut points = Vec::new();
        for p in rows(source)? {
            let x = number(p, xfield).ok_or("Portfolio sensitivity coordinate is unavailable")?;
            let y = number(p, if irr { "irr" } else { "npv" })
                .map(|v| v * if irr { 100. } else { 1e-6 })
                .filter(|v| v.is_finite());
            points.push(json!([x, y]));
        }
        points.sort_by(|a, b| a[0].as_f64().unwrap().total_cmp(&b[0].as_f64().unwrap()));
        if points.windows(2).any(|p| p[0][0] == p[1][0]) {
            return Err("Duplicate portfolio sensitivity coordinates".into());
        }
        total += points.len();
        if total > 100_000 {
            return Err("Excessive portfolio sensitivity points".into());
        }
        if !points.is_empty() {
            out.push(series(
                name,
                "line",
                json!(points),
                0,
                COLORS[i % COLORS.len()],
            ));
        }
    }
    if out.is_empty() {
        return Ok(Value::Null);
    }
    Ok(
        json!({"title":{"text":key.replace('_'," ")},"tooltip":{"trigger":"axis"},"legend":{"type":"scroll","bottom":0},
        "xAxis":{"type":"value","name":xunit},"yAxis":{"type":"value","name":yunit},"series":out}),
    )
}

pub(super) fn compose(report: Value) -> Result<Dashboard, String> {
    let report = super::portfolio_views::normalize(report)?;
    let k = &report["financial_kpis"];
    if !k.is_object() {
        return Err("Portfolio financial analysis is unavailable".into());
    }
    let table = rows(&k["table"])?;
    let agg = &k["aggregations"];
    if !agg.is_null() && !agg.is_object() {
        return Err("Portfolio financial aggregations must be an object".into());
    }
    let params = &report["assumptions"]["financial_params"];
    let currency = report["meta"]["currency"]
        .as_str()
        .or_else(|| k["currency"].as_str())
        .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()));
    if let (Some(a), Some(b)) = (report["meta"]["currency"].as_str(), k["currency"].as_str()) {
        if a != b {
            return Err("Portfolio dashboard currency sources disagree".into());
        }
    }
    let years = first_number(&report["meta"], &["project_years"])
        .or_else(|| number(k, "project_years"))
        .filter(|v| v.fract() == 0. && (1. ..=100.).contains(v));
    if let (Some(a), Some(b)) = (
        number(&report["meta"], "project_years"),
        number(k, "project_years"),
    ) {
        if a != b {
            return Err("Portfolio dashboard horizon sources disagree".into());
        }
    }
    let money = currency
        .map(|c| format!("M {c}"))
        .unwrap_or_else(|| "Value (currency unavailable)".into());
    let horizon = years
        .map(|v| format!("{v:.0} years"))
        .unwrap_or_else(|| "horizon unavailable".into());
    let ratio_pct = |v: Option<f64>| v.filter(|v| (0. ..=1.).contains(v)).map(|v| v * 100.);
    let wacc = ratio_pct(
        number(&report["meta"], "discount_rate")
            .or_else(|| number(&params["wacc"], "derived_wacc")),
    );
    let hero = json!({"wacc":wacc,"equity":ratio_pct(number(&params["capital_structure"],"equity_ratio")),
        "debt":ratio_pct(number(&params["capital_structure"],"debt_ratio")),"debtCost":ratio_pct(number(&params["debt"],"interest_rate"))});
    let lcoe = &k["portfolio_lcoe"];
    let payback = &k["portfolio_payback"];
    let mut cash = ordered(&k["portfolio_cashflow"]["portfolio"])?;
    let mut dscr = ordered(&k["portfolio_dscr"]["portfolio"])?;
    let dscr_values: Vec<f64> = dscr
        .iter()
        .filter_map(|r| first_number(r, &["dscr_ratio_of_totals", "min_dscr"]))
        .collect();
    let minimum = number(agg, "portfolio_min_dscr_ratio_of_totals")
        .or_else(|| dscr_values.iter().copied().min_by(f64::total_cmp));
    let average = number(agg, "portfolio_mean_dscr_ratio_of_totals")
        .or_else(|| {
            (!dscr_values.is_empty())
                .then(|| dscr_values.iter().sum::<f64>() / dscr_values.len() as f64)
        })
        .filter(|v| v.is_finite());
    let threshold =
        first_number(&params["project_finance"], &["minimum_dscr", "min_dscr"]).filter(|v| *v > 0.);
    let minimum_year = minimum
        .and_then(|minimum| {
            dscr.iter()
                .find(|r| first_number(r, &["dscr_ratio_of_totals", "min_dscr"]) == Some(minimum))
        })
        .map(|r| r["year"].clone());
    for row in &mut dscr {
        let ratio = first_number(row, &["dscr_ratio_of_totals", "min_dscr"]);
        row["ratio"] = json!(ratio);
        row["mean_ratio"] = json!(first_number(row, &["mean_city_dscr", "avg_dscr"]));
        row["bankable"] = json!(ratio.zip(threshold).map(|(r, t)| r >= t));
    }
    for row in &mut cash {
        row["net_positive"] = json!(number(row, "net_cash_flow").map(|v| v >= 0.));
        row["has_replacement"] = json!(number(row, "replacement").map(|v| v > 0.));
    }
    let pb_simple = number(payback, "simple_payback_years");
    let pb_disc = number(payback, "discounted_payback_years");
    let pb_ok = |v: Option<f64>| v.zip(years).map(|(v, years)| v > 0. && v <= years);
    let capex = number(lcoe, "total_capex").or_else(|| sum(table, "initial_investment"));
    let opex = number(lcoe, "total_opex").or_else(|| sum(table, "nominal_opex"));
    let replacements =
        number(lcoe, "total_replacements").or_else(|| sum(table, "nominal_replacements"));
    let mut derived = json!({"currency":currency,"project_years":years,"hero":hero,
        "total_npv_millions":number(agg,"total_npv_millions").or_else(|| number(agg,"total_npv").map(|v|v/1e6)),
        "weighted_irr_percent":number(agg,"weighted_avg_irr_percent"),
        "portfolio_lcoe":first_number(lcoe,&["lcoe_simple_with_network","lcoe_simple_plant"]),
        "min_dscr":minimum,"avg_dscr":average,"dscr_threshold":threshold,"bankable":minimum.zip(threshold).map(|(a,b)| a>=b),
        "simple_payback":pb_simple,"discounted_payback":pb_disc,"payback_investment":number(payback,"total_investment"),
        "avg_annual_cashflow":number(payback,"avg_annual_cashflow"),"has_payback":pb_simple.is_some(),
        "simple_payback_viable":pb_ok(pb_simple),"discounted_payback_viable":pb_ok(pb_disc),
        "lcoe_capex":capex,"lcoe_opex":opex,"lcoe_replacements":replacements,"lcoe_energy":number(lcoe,"total_energy_kwh"),
        "cashflow_rows":cash,"dscr_rows":dscr,"kpi_rows":table,"finance_aggregations":agg});
    let npv = number(&derived, "total_npv_millions");
    let irr = number(&derived, "weighted_irr_percent");
    derived["npv_profitable"] = json!(npv.map(|v| v > 0.));
    derived["irr_above_wacc"] = json!(irr.zip(wacc).map(|(i, w)| i > w));
    derived["lcoe_positive"] = json!(number(&derived, "portfolio_lcoe").map(|v| v > 0.));
    derived["min_dscr_year"] = json!(minimum_year);
    let investment_rows = if report["investment"]["investment_table"].is_array() {
        &report["investment"]["investment_table"]
    } else {
        &report["investment"]["table"]
    };
    derived["investment_rows"] = json!(rows(investment_rows)?);
    derived["investment_aggregations"] = report["investment"]["aggregations"].clone();
    derived["environmental_rows"] = json!(rows(&report["environmental"]["table"])?);
    derived["breakeven_price"] = json!(number(&k["portfolio_sensitivity"], "breakeven_price"));
    let viable_count = agg["viable_count"]
        .as_str()
        .filter(|s| {
            s.len() <= 32 && {
                let p: Vec<_> = s.split('/').collect();
                p.len() == 2
                    && p.iter()
                        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            }
        })
        .map(str::to_owned)
        .or_else(|| {
            number(agg, "viable_count")
                .filter(|v| v.fract() == 0. && (0. ..=table.len() as f64).contains(v))
                .map(|v| format!("{v:.0}/{}", table.len()))
        })
        .unwrap_or_else(|| format!("—/{}", table.len()));
    derived["viable_count_label"] = json!(viable_count);
    derived["formulas"] = json!({
        "lcoe":r"LCOE_{simple} = \frac{CAPEX + OPEX + Remplacements}{E_{duree\ de\ vie}}",
        "npv":r"VAN = -I_0 + \sum_{t=1}^{n} \frac{CF_t}{(1+r)^t}",
        "irr":r"0 = -I_0 + \sum_{t=1}^{n} \frac{CF_t}{(1+TRI)^t}",
        "payback":r"Payback = min(t) : \sum_{i=1}^{t} CF_i \geq I_0",
        "discounted_payback":r"Payback_{actualise} = min(t) : \sum_{i=1}^{t} \frac{CF_i}{(1+r)^i} \geq I_0",
        "dscr":r"DSCR = \frac{EBITDA}{Service\ de\ la\ Dette} = \frac{Revenus - OPEX}{Principal + Interets}"
    });
    if let Some(key) = ["lcoe_simple_with_network", "lcoe_simple_plant"]
        .iter()
        .find(|key| number(lcoe, key).is_some())
    {
        if derived["finance_aggregations"].is_null() {
            derived["finance_aggregations"] = json!({});
        }
        derived["finance_aggregations"][format!("portfolio_{key}")] = lcoe[*key].clone();
    }
    let inv = &report["investment"]["aggregations"];
    let mut charts = json!({
        "investment":donut(&format!("Initial investment ({})",currency.unwrap_or("currency unavailable")),&[("Plant",number(inv,"total_plant_capex")),("Network",number(inv,"total_network_capex"))])?,
        "lifetime_costs":donut(&format!("Lifetime costs ({horizon})"),&[("CAPEX",capex),("OPEX",opex),("Replacements",replacements)])?,
        "lcoe":donut("LCOE cost components",&[("CAPEX",capex),("OPEX",opex),("Replacements",replacements)])?});
    if !cash.is_empty() {
        let y: Vec<i64> = cash.iter().map(|r| r["year"].as_i64().unwrap()).collect();
        let mut s = vec![
            series(
                "Net cash flow",
                "bar",
                values(&cash, "net_cash_flow", 1e-6),
                0,
                "#2ca02c",
            ),
            series(
                "Replacements",
                "bar",
                values(&cash, "replacement", -1e-6),
                0,
                "#ff7f0e",
            ),
            series(
                "Cumulative cash flow",
                "line",
                values(&cash, "cumulative_cash_flow", 1e-6),
                1,
                "#1f77b4",
            ),
        ];
        let marks: Vec<Value> = [("Simple",pb_simple),("Discounted",pb_disc)].iter().filter_map(|(name,v)|
            v.zip(years).filter(|(v,h)| *v>0. && *v<=*h).map(|(v,_)| json!({"xAxis":v.round().to_string(),"label":{"formatter":format!("{name}: {v:.1} years")}}))).collect();
        s[2]["markLine"] = json!({"silent":true,"symbol":"none","data":marks});
        charts["cashflow"] = cartesian(
            &format!("Cash flow ({horizon})"),
            &y,
            json!([{"type":"value","name":money},{"type":"value","name":format!("Cumulative ({money})"),"position":"right"}]),
            s.clone(),
        );
        s[2]["yAxisIndex"] = json!(0);
        let by_year: BTreeMap<i64, Option<f64>> = dscr
            .iter()
            .map(|r| {
                (
                    r["year"].as_i64().unwrap(),
                    first_number(r, &["dscr_ratio_of_totals", "min_dscr"]),
                )
            })
            .collect();
        if !dscr.is_empty() {
            s.push(series(
                "DSCR ratio of totals",
                "line",
                json!(y
                    .iter()
                    .map(|y| by_year.get(y).copied().flatten())
                    .collect::<Vec<_>>()),
                1,
                "#d62728",
            ));
        }
        charts["financial_horizon"] = cartesian(
            &format!("Financial overview ({horizon})"),
            &y,
            json!([{"type":"value","name":money},{"type":"value","name":"DSCR","position":"right"}]),
            s,
        );
    }
    if !dscr.is_empty() {
        let y: Vec<i64> = dscr.iter().map(|r| r["year"].as_i64().unwrap()).collect();
        let mut s = vec![
            series(
                "DSCR ratio of totals",
                "line",
                json!(dscr
                    .iter()
                    .map(|r| first_number(r, &["dscr_ratio_of_totals", "min_dscr"]))
                    .collect::<Vec<_>>()),
                0,
                "#d62728",
            ),
            series(
                "Mean city DSCR",
                "line",
                json!(dscr
                    .iter()
                    .map(|r| first_number(r, &["mean_city_dscr", "avg_dscr"]))
                    .collect::<Vec<_>>()),
                0,
                "#1f77b4",
            ),
        ];
        if let Some(t) = threshold {
            s.push(series(
                "Bankability threshold",
                "line",
                json!(vec![t; y.len()]),
                0,
                "#7f7f7f",
            ));
        }
        charts["dscr"] = cartesian(
            &format!("DSCR ({horizon})"),
            &y,
            json!({"type":"value","name":"DSCR"}),
            s,
        );
    }
    let sens = &k["portfolio_sensitivity"];
    for (name, key, x, irr, xunit, yunit) in [
        (
            "npv_price",
            "npv_vs_pricing",
            "price",
            false,
            format!("Price ({}/kWh)", currency.unwrap_or("currency unavailable")),
            format!("NPV ({money})"),
        ),
        (
            "npv_wacc",
            "npv_vs_wacc",
            "wacc_percent",
            false,
            "WACC (%)".into(),
            format!("NPV ({money})"),
        ),
        (
            "irr_price",
            "irr_vs_pricing",
            "price",
            true,
            format!("Price ({}/kWh)", currency.unwrap_or("currency unavailable")),
            "IRR (%)".into(),
        ),
        (
            "irr_capex",
            "irr_vs_investment",
            "variation_percent",
            true,
            "CAPEX change (%)".into(),
            "IRR (%)".into(),
        ),
    ] {
        charts[name] = sensitivity(sens, key, x, irr, &xunit, &yunit)?;
    }
    let mut cards = Vec::new();
    for (key, label, unit) in [
        ("total_npv_millions", "Portfolio NPV", money.as_str()),
        ("weighted_irr_percent", "Weighted city IRR", "%"),
        ("portfolio_lcoe", "Portfolio LCOE", currency.unwrap_or("")),
        ("min_dscr", "Minimum consolidated DSCR", ""),
        ("avg_dscr", "Mean consolidated DSCR", ""),
        ("simple_payback", "Simple payback", "years"),
        ("discounted_payback", "Discounted payback", "years"),
    ] {
        cards.push(Card {
            key: key.into(),
            group: "Portfolio finance".into(),
            label: label.into(),
            value: derived[key].clone(),
            unit: unit.into(),
        });
    }
    let representative = report["meta"]["representative_city"]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .map(str::trim)
        .or_else(|| {
            (report["meta"]["graph_strategy"] == "round_robin").then_some("Per graph (round robin)")
        });
    let media = super::portfolio_views::media(
        &report,
        &["investment", "financial_kpis", "lcoe"],
        representative,
    )?;
    derived["graphs"] = media["graphs"].clone();
    derived["unavailable_detail"] = media["unavailable_detail"].clone();
    Ok(Dashboard {
        schema: "ds-solar.dashboard/v1",
        section: Section::PortfolioFinance,
        derived,
        cards,
        charts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_irr_missing_dscr_and_unordered_years_preserve_meaning() {
        let input = json!({"meta":{"currency":"USD","project_years":20,"discount_rate":0.001},"financial_kpis":{
            "table":[{"initial_investment":0.25,"nominal_opex":0.5,"nominal_replacements":0.} ],
            "portfolio_cashflow":{"portfolio":[{"year":2,"net_cash_flow":-1.},{"year":1,"net_cash_flow":5.}]},
            "portfolio_dscr":{"portfolio":[{"year":2,"dscr_ratio_of_totals":null},{"year":1,"dscr_ratio_of_totals":1.5}]},
            "portfolio_sensitivity":{"city_sensitivities":[{"city":"A","irr_vs_pricing":[{"price":1.,"irr":-0.2},{"price":2.,"irr":null}]}]}}});
        let d = compose(input.clone()).unwrap();
        assert_eq!(d.derived["hero"]["wacc"], 0.1);
        assert_eq!(
            d.charts["irr_price"]["series"][0]["data"],
            json!([[1., -20.], [2., null]])
        );
        assert_eq!(
            d.charts["financial_horizon"]["series"][3]["data"],
            json!([1.5, null])
        );
        assert_eq!(d.derived["min_dscr"], 1.5);
        assert!(d.derived["bankable"].is_null());
        assert_eq!(d.charts["lcoe"]["series"][0]["data"][0]["value"], 0.25);
        let rendered = super::super::html(&d, "Portfolio");
        assert!(
            rendered.contains("USD") && rendered.contains("20 years") && rendered.contains("<svg")
        );
        assert!(!rendered.contains("<script"));
        assert!(rendered.contains("Annual cash flow") && rendered.contains("Financial formulas"));
        assert_eq!(d.derived["cashflow_rows"][0]["net_positive"], true);
        assert_eq!(d.derived["dscr_rows"][1]["bankable"], Value::Null);
        let mut duplicate = input;
        duplicate["financial_kpis"]["portfolio_cashflow"]["portfolio"][1]["year"] = json!(2);
        assert!(compose(duplicate).err().unwrap().contains("Duplicate"));
    }
    #[test]
    fn incompatible_metadata_is_refused_and_absent_costs_are_not_zeroed() {
        let input = json!({"meta":{"currency":"USD","project_years":20},"financial_kpis":{"currency":"XAF"}});
        assert!(compose(input).err().unwrap().contains("currency"));
        let d = compose(json!({"financial_kpis":{"table":[{"initial_investment":1.}]}})).unwrap();
        assert!(d.charts["lcoe"].is_null());
        assert!(d.derived["simple_payback"].is_null());
    }
}
