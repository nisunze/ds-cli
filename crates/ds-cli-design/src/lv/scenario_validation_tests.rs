//! `--set` of a numeric `vd_*` setting is checked, never silently ignored.
use super::*;

fn code(tokens: &[&str]) -> String {
    let mut all = vec![
        "--input".to_string(),
        "in.json".to_string(),
        "--out".to_string(),
        "out.json".to_string(),
    ];
    all.extend(tokens.iter().map(|token| token.to_string()));
    let inputs = ds_cli_contract::parse(&super::super::voltage_drop::COMMAND, &all)
        .expect("declared inputs");
    match Scenario::parse(&inputs) {
        Ok(_) => "accepted".into(),
        Err(failure) => failure.code().to_string(),
    }
}

#[test]
fn a_non_numeric_or_out_of_range_numeric_setting_is_refused() {
    for given in [
        "vd_limit_pct=abc",
        "vd_limit_pct=true",
        "vd_limit_pct=0",
        "vd_limit_pct=-5",
        "vd_limit_pct=500",
        "vd_nominal_phase_voltage_v=24",
        "vd_max_new_circuits=many",
    ] {
        assert_eq!(code(&["--set", given]), "vd_scenario_invalid", "{given}");
    }
}

#[test]
fn in_range_numbers_and_unknown_text_settings_stay_accepted() {
    for given in [
        "vd_limit_pct=8",
        "vd_limit_pct=7.5",
        "vd_nominal_phase_voltage_v=230",
        "vd_max_recommended_abc_mm2=70",
        "vd_note=text",
    ] {
        assert_eq!(code(&["--set", given]), "accepted", "{given}");
    }
}
