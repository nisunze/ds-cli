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
    dashboard.derived["unavailable_detail"] = json!(
        "source batch chart receipt cities/aderm_massenya/plots/plant/solar_battery_co2_emissions.png"
    );
    let html = html(
        &dashboard,
        "long_project_identity · long_portfolio_identity",
    );
    assert!(html.contains("overflow-wrap:anywhere"));
    assert!(html.contains("p{overflow-wrap:anywhere}"));
    assert!(html.contains("<p>source batch chart receipt cities/aderm_massenya/plots/plant/solar_battery_co2_emissions.png</p>"));
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
        "project_viable":false,"npv":{"npv":0,"value":100},"irr":{"irr":-0.05,"value":5},
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
    assert_eq!(d.derived["npv_profitable"], false);
    assert_eq!(d.derived["irr_viable"], false);
    assert!(d.derived["dscr_bankable"].is_null());
}
#[test]
fn finance_uses_engine_verdicts_and_leaves_missing_evidence_unknown() {
    for verdict in [Some(true), Some(false), None] {
        let mut analysis = json!({
            "parameters":{"wacc":0.08}, "npv":{"npv":0}, "irr":{"irr":0.08},
            "project_finance_analysis":{"dscr":{"min_dscr":0.5,"bankability_threshold":1.7}}
        });
        if let Some(verdict) = verdict {
            analysis["project_viable"] = json!(verdict);
            analysis["project_finance_analysis"]["dscr"]["is_bankable"] = json!(verdict);
        }
        let dashboard = compose(Request {
            section: Section::Finance,
            system: None,
            report: json!({"05_financial":{"hybrid_financial_analysis":analysis}}),
        })
        .unwrap();
        for key in [
            "project_viable",
            "npv_profitable",
            "irr_viable",
            "dscr_bankable",
        ] {
            assert_eq!(dashboard.derived[key], json!(verdict));
        }
    }
    let dashboard = compose(Request {
        section: Section::Finance,
        system: None,
        report: json!({"05_financial":{"hybrid_financial_analysis":{}}}),
    })
    .unwrap();
    assert!(dashboard.derived["dscr_threshold"].is_null());
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

#[test]
fn the_boq_result_section_is_the_sealed_plant_cost_section() {
    let report = json!({"04_plant_cost": {"hybrid_boq": {"grand_total": 12.5}}});
    let path = ["hybrid_boq".to_string()];
    let boq = read_section(&report, "boq", &path).unwrap();
    let costs = read_section(&report, "costs", &path).unwrap();
    assert_eq!(boq["value"], costs["value"]);
    assert_eq!(boq["value"]["grand_total"], 12.5);
    assert_eq!(boq["section"], "boq");
    assert!(read_section(&report, "bill", &[]).is_err());
}
