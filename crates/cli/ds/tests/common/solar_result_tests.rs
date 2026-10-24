// Copied from ds-solar 3adcc6d crates/ds-solar-contracts/tests/common/result.rs.
//! Minimal sealed result for contract and adapter admission tests; no calculation.
use ds_solar_contracts::{
    ArtifactManifest, CityIdentity, EngineIdentity, MonthlySeries, SiteSummary,
    SolarCalculationResult, WeatherSummary,
};

pub fn result() -> SolarCalculationResult {
    SolarCalculationResult {
        schema_version: "ds-solar.result/v1".into(),
        engine: EngineIdentity::current(),
        identity: CityIdentity {
            project_id: "project-1".into(),
            root: "solar".into(),
            city_id: "city-1".into(),
            display_name: "City One".into(),
        },
        run_id: Some("run-1".into()),
        input_digest: format!("sha256:{}", "a".repeat(64)),
        weather_digest: format!("sha256:{}", "b".repeat(64)),
        demand_noise_seed: 1,
        stages: vec![],
        site_summary: SiteSummary {
            weather_summary: WeatherSummary {
                annual_ghi_kwh_m2: 0.0,
                annual_dni_kwh_m2: 0.0,
                peak_sun_hours_avg: 0.0,
                temp_min_c: 0.0,
                temp_max_c: 0.0,
                temp_mean_c: 0.0,
                temp_min_month: 1,
                temp_max_month: 1,
                monthly_ghi_kwh_m2_day: MonthlySeries::zeroed(),
                monthly_dni_kwh_m2_day: MonthlySeries::zeroed(),
                monthly_temp_c: MonthlySeries::zeroed(),
            },
            load_profile: None,
            total_customers: None,
            customers_by_category: None,
            demand_shaping: None,
            pv: None,
        },
        plant_summary: vec![],
        artifacts: ArtifactManifest::new(),
        result_digest: String::new(),
        received: Default::default(),
    }
    .sealed()
}
