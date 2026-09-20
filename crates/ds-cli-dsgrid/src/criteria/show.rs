//! `ds dsgrid criteria show` — the clearance criteria of every criterion
//! set, the weather cases, and the maximum conductor temperature against
//! what REG rates.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::criteria_clearance_view;
use serde_json::{Value, json};

use crate::feature_codes;
use crate::mutation;

pub static COMMAND: Command = Command {
    id: "dsgrid.criteria.show",
    path: &["dsgrid", "criteria", "show"],
    contract: 1,
    summary: "Show clearance criteria, weather cases and max conductor temperature.",
    purpose: "\
Reads one DS Grid model and lists, per criterion set, the clearance voltage, \
the survey-point vertical and horizontal clearance cases, the wire clearance \
line and every clearance rule; the model's weather cases (label, \
temperature, wind pressure, ice); and the hottest weather case against the \
75 °C REG rates conductors at — a colder maximum conductor temperature is a \
finding, because it under-states sag. A set without clearance criteria is \
named so `criteria clearance set` can fill it. The project-wide comparison \
across models (contract 03 §5 `criteria compare`) is not this verb.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "\
The target; per set: id, label, assigned sections, clearance voltage, wire \
clearance line, survey-point vertical/horizontal cases, clearance rules, \
configured flag; the weather cases; the maximum conductor temperature with \
the standard's value; the findings.",
    examples: &[
        Example {
            command: "ds dsgrid criteria show --model local-e9b0ccbf92d7447b --output json",
            note: "Read the weather labels before `criteria clearance set`, and the 60 °C finding.",
            runnable: false,
        },
    ],
    refusals: feature_codes::READ_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "criteria",
        "clearance voltage",
        "weather case",
        "maximum conductor temperature",
        "75",
        "survey point clearance",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (target, opened) = feature_codes::open_read_only(inputs)?;
    let standard = ds_grid_engine::FeatureCodeStandard::bundled().ok();
    let view = criteria_clearance_view(opened.session.snapshot(), standard.as_ref());
    Ok(json!({
        "target": target,
        "engine": ds_grid_engine::ENGINE_VERSION,
        "sets": view.sets,
        "weather_cases": view.weather_cases,
        "max_conductor_temperature": view.max_conductor_temperature,
        "findings": view.findings,
        "staged": false,
        "persisted": false,
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    for set in data["sets"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "set {} ({}) sections {}  voltage {}  wire clearance line {}  {}\n",
            set["criterion_set_id"].as_str().unwrap_or("?"),
            set["label"].as_str().unwrap_or("?"),
            set["assigned_section_count"],
            set["clearance_voltage_kv"]
                .as_f64()
                .map(|kv| format!("{kv} kV"))
                .unwrap_or_else(|| "—".to_string()),
            set["wire_clearance_line"]["value_si"]
                .as_f64()
                .map(|m| format!("{m} m"))
                .unwrap_or_else(|| "—".to_string()),
            if set["configured"].as_bool().unwrap_or(false) {
                "configured"
            } else {
                "NOT configured"
            },
        ));
        for (which, key) in [
            ("vertical", "survey_point_vertical_cases"),
            ("horizontal", "survey_point_horizontal_cases"),
        ] {
            for case in set[key].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "  {which:<10} {} ({} °C, {} Pa, {})\n",
                    case["weather_label"].as_str().unwrap_or("?"),
                    case["temperature_c"],
                    case["wind_pressure_pa"],
                    case["condition"].as_str().unwrap_or("?"),
                ));
            }
        }
    }
    out.push_str("weather cases\n");
    for weather in data["weather_cases"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<32} {:>6} °C {:>8} Pa\n",
            weather["label"].as_str().unwrap_or("?"),
            weather["temperature_c"],
            weather["wind_pressure_pa"],
        ));
    }
    let max = &data["max_conductor_temperature"];
    out.push_str(&format!(
        "max conductor temperature {} ({} °C; standard {} °C){}\n",
        max["weather_label"].as_str().unwrap_or("—"),
        max["temperature_c"],
        max["standard_c"],
        if max["below_standard"].as_bool().unwrap_or(false) {
            " BELOW STANDARD"
        } else {
            ""
        },
    ));
    for finding in data["findings"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "finding  {}: {}\n",
            finding["code"].as_str().unwrap_or("?"),
            finding["detail"].as_str().unwrap_or("")
        ));
    }
    out
}
