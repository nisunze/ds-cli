//! The run-scoped scenario of `ds design lv voltage-drop`.
//!
//! `--year`, `--outlook`, `--no-outlook`, `--load` and `--set` change what one
//! run checks, never the project. Each becomes an input ds-network's
//! voltage-drop method already reads — a `vd_*` project setting, a
//! `cust_category` load, a customer's `load` key, a category of the rule set's
//! `category_loads` — written into every job of the in-memory request before
//! the engine runs. The method decides everything from there; the result
//! echoes the scenario it ran.

use std::collections::{BTreeMap, BTreeSet};

use ds_cli_contract::{Failure, Inputs};
use ds_network::network::native_fast_lv::NativeFastLvJobV1;
use serde_json::{Map, Value, json};

/// The latest year after commissioning a run may check.
pub(super) const MAX_YEAR: u32 = 50;
/// The largest per-customer load a scenario may set, W.
pub(super) const MAX_LOAD_W: f64 = 100_000.0;
/// `--load` or `--set` flags one run may carry, each.
pub(super) const MAX_OVERRIDES: usize = 32;
const MAX_SETTING_NAME: usize = 64;
const MAX_CATEGORY: usize = 64;
const MAX_VALUE: usize = 256;
/// Category names a refusal lists.
const MAX_LISTED: usize = 50;

/// Settings with their own flag: `--set` refuses them and names the flag.
const FLAG_OWNED: &[(&str, &str)] = &[
    ("vd_design_year", "--year"),
    ("vd_outlook_years", "--outlook"),
    ("vd_staged_plan", "--no-outlook"),
];

struct CategoryLoad {
    /// As the caller wrote it.
    category: String,
    saturation_w: f64,
    initial_w: f64,
}

/// Every override of one run.
#[derive(Default)]
pub(super) struct Scenario {
    /// Project setting → value, as injected into `project_settings`.
    settings: BTreeMap<String, Value>,
    loads: Vec<CategoryLoad>,
}

impl Scenario {
    /// The scenario the flags describe, or the typed refusal of the first
    /// flag that is malformed, out of bounds or given twice.
    pub(super) fn parse(inputs: &Inputs) -> Result<Self, Failure> {
        let mut scenario = Scenario::default();
        if let Some(raw) = inputs.value("year") {
            let year = year(raw, "year", raw)?;
            scenario
                .settings
                .insert("vd_design_year".into(), json!(year));
        }
        let no_outlook = inputs.switch("no-outlook");
        if let Some(raw) = inputs.value("outlook") {
            if no_outlook {
                return Err(Failure::invalid(
                    "vd_scenario_conflict",
                    "`--outlook` and `--no-outlook` were both given",
                )
                .remedy("give the outlook years, or switch the outlook off"));
            }
            let years = outlook(raw)?;
            let text: Vec<String> = years.iter().map(u32::to_string).collect();
            scenario
                .settings
                .insert("vd_outlook_years".into(), Value::String(text.join(",")));
        }
        if no_outlook {
            scenario
                .settings
                .insert("vd_staged_plan".into(), Value::Bool(false));
        }

        let sets = inputs.repeated("set");
        bound_count(sets.len(), "set")?;
        for raw in sets {
            let (name, value) = setting(raw)?;
            if scenario.settings.contains_key(&name) {
                return Err(Failure::invalid(
                    "vd_scenario_conflict",
                    format!("`--set {name}` was given more than once"),
                )
                .remedy("give each setting once"));
            }
            scenario.settings.insert(name, value);
        }

        let loads = inputs.repeated("load");
        bound_count(loads.len(), "load")?;
        for raw in loads {
            let load = category_load(raw)?;
            let key = normalize(&load.category);
            if scenario
                .loads
                .iter()
                .any(|other| normalize(&other.category) == key)
            {
                return Err(Failure::invalid(
                    "vd_scenario_conflict",
                    format!("`--load {}` was given more than once", load.category),
                )
                .remedy("give each category's load once"));
            }
            scenario.loads.push(load);
        }
        Ok(scenario)
    }

    /// Writes the scenario into every job and returns how many customer
    /// `load` keys each `--load` rewrote. A `--load` category that no job's
    /// `cust_category` (or voltage-drop `category_loads`) defines is refused:
    /// the engine would never read it.
    pub(super) fn apply(&self, jobs: &mut [NativeFastLvJobV1]) -> Result<Vec<usize>, Failure> {
        let mut rewritten = vec![0usize; self.loads.len()];
        let mut defined = vec![false; self.loads.len()];
        for job in jobs.iter_mut() {
            if !self.settings.is_empty() {
                inject_settings(&mut job.config_dfs, &self.settings);
            }
            for (index, load) in self.loads.iter().enumerate() {
                let key = normalize(&load.category);
                let in_workbook = set_category_rows(&mut job.config_dfs, &key, load);
                let in_rule_set = set_configured_loads(&mut job.config_dfs, &key, load);
                defined[index] |= in_workbook || in_rule_set;
                rewritten[index] += rewrite_customer_loads(&mut job.gdfs, &key, load.saturation_w);
            }
        }
        if let Some(index) = defined.iter().position(|defined| !defined) {
            let category = &self.loads[index].category;
            let known: Vec<String> = jobs
                .iter()
                .flat_map(|job| category_names(&job.config_dfs))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(MAX_LISTED)
                .collect();
            return Err(Failure::invalid(
                "vd_scenario_category_unknown",
                format!("`--load {category}` names a category no job's cust_category defines"),
            )
            .remedy(
                "name a cust_category clean_name from the request; export it with \
                 design.lv.project-export --project-config",
            )
            .detail(json!({ "category": category, "defined": known })));
        }
        Ok(rewritten)
    }

    /// The scenario block of the receipt and the result document: every
    /// override this run applied. Where the method took each value from is
    /// its report's `parameters.sources` (`project_settings.<name>`).
    pub(super) fn echo(&self, rewritten: &[usize]) -> Value {
        let loads: Vec<Value> = self
            .loads
            .iter()
            .zip(rewritten)
            .map(|(load, rewritten)| {
                json!({
                    "category": load.category,
                    "saturation_w": load.saturation_w,
                    "initial_w": load.initial_w,
                    "customer_loads_rewritten": rewritten,
                })
            })
            .collect();
        json!({
            "project_settings": self.settings,
            "loads": loads,
        })
    }
}

fn invalid(flag: &str, given: &str, message: String) -> Failure {
    Failure::invalid("vd_scenario_invalid", message)
        .remedy(format!(
            "pass --year 0..{MAX_YEAR}, --outlook as comma-separated years 0..{MAX_YEAR}, \
             --load <Category>=<saturation W>[:<initial W>] with 0 ≤ initial ≤ saturation ≤ \
             {MAX_LOAD_W} W, and --set vd_<name>=<value>"
        ))
        .detail(json!({ "flag": flag, "given": given }))
}

fn bound_count(count: usize, flag: &str) -> Result<(), Failure> {
    if count > MAX_OVERRIDES {
        return Err(Failure::invalid(
            "vd_scenario_invalid",
            format!("`--{flag}` was given {count} times"),
        )
        .remedy(format!("pass `--{flag}` at most {MAX_OVERRIDES} times"))
        .detail(json!({ "flag": flag, "given": count, "max": MAX_OVERRIDES })));
    }
    Ok(())
}

/// A whole year after commissioning, 0..=MAX_YEAR.
fn year(text: &str, flag: &str, given: &str) -> Result<u32, Failure> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|year| *year <= MAX_YEAR)
        .ok_or_else(|| {
            invalid(
                flag,
                given,
                format!("`--{flag}` takes whole years 0..{MAX_YEAR}; `{given}` is not one"),
            )
        })
}

/// Comma-separated years, sorted and each once.
fn outlook(raw: &str) -> Result<Vec<u32>, Failure> {
    let mut years = BTreeSet::new();
    for part in raw.split(',') {
        years.insert(year(part, "outlook", raw)?);
    }
    Ok(years.into_iter().collect())
}

/// `vd_<name>=<value>`: a number, `true`/`false`, or text.
fn setting(raw: &str) -> Result<(String, Value), Failure> {
    let Some((name, value)) = raw.split_once('=') else {
        return Err(invalid(
            "set",
            raw,
            format!("`--set {raw}` is not <vd_name>=<value>"),
        ));
    };
    let name = name.trim();
    if !name.starts_with("vd_") {
        return Err(Failure::invalid(
            "vd_scenario_setting_refused",
            format!("`--set {name}` is not a voltage-drop setting"),
        )
        .remedy("--set takes vd_* project settings only")
        .detail(json!({ "setting": name })));
    }
    if let Some((_, flag)) = FLAG_OWNED.iter().find(|(owned, _)| *owned == name) {
        return Err(Failure::invalid(
            "vd_scenario_setting_refused",
            format!("`{name}` has its own flag, `{flag}`"),
        )
        .remedy(format!("pass `{flag}` instead of `--set {name}=…`"))
        .detail(json!({ "setting": name, "flag": flag })));
    }
    let well_formed = name.len() <= MAX_SETTING_NAME
        && name.len() > "vd_".len()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    let value = value.trim();
    if !well_formed || value.is_empty() || value.len() > MAX_VALUE {
        return Err(invalid(
            "set",
            raw,
            format!(
                "`--set {raw}` needs a lowercase vd_ name of at most {MAX_SETTING_NAME} \
                 characters and a value of 1..{MAX_VALUE} characters"
            ),
        ));
    }
    Ok((name.to_string(), setting_value(value)))
}

fn setting_value(text: &str) -> Value {
    match text.to_ascii_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    match text.parse::<f64>() {
        Ok(number) if number.is_finite() && number.fract() == 0.0 && number.abs() < 1e15 => {
            json!(number as i64)
        }
        Ok(number) if number.is_finite() => json!(number),
        _ => Value::String(text.to_string()),
    }
}

/// `<Category>=<saturation W>[:<initial W>]`; without an initial load the
/// category is flat at its saturation load every year.
fn category_load(raw: &str) -> Result<CategoryLoad, Failure> {
    let bad = |why: &str| invalid("load", raw, format!("`--load {raw}` {why}"));
    let Some((category, watts)) = raw.rsplit_once('=') else {
        return Err(bad("is not <Category>=<saturation W>[:<initial W>]"));
    };
    let category = category.trim();
    if category.is_empty() || category.chars().count() > MAX_CATEGORY || category.contains(',') {
        return Err(bad(&format!(
            "needs a category name of 1..{MAX_CATEGORY} characters without a comma"
        )));
    }
    let number = |text: &str| {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|watts| watts.is_finite())
    };
    let (saturation, initial) = match watts.split_once(':') {
        Some((saturation, initial)) => (number(saturation), number(initial)),
        None => {
            let saturation = number(watts);
            (saturation, saturation)
        }
    };
    let (Some(saturation_w), Some(initial_w)) = (saturation, initial) else {
        return Err(bad("needs watts as numbers"));
    };
    if !(saturation_w > 0.0 && saturation_w <= MAX_LOAD_W) {
        return Err(bad(&format!(
            "needs a saturation load above 0 and at most {MAX_LOAD_W} W"
        )));
    }
    if !(0.0..=saturation_w).contains(&initial_w) {
        return Err(bad("needs an initial load from 0 to the saturation load"));
    }
    Ok(CategoryLoad {
        category: category.to_string(),
        saturation_w,
        initial_w,
    })
}

/// The engine's own comparison of category names.
fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A config sheet where the engine looks for it: top level, else `sheets`.
fn sheet_mut<'a>(config: &'a mut Map<String, Value>, name: &str) -> Option<&'a mut Value> {
    if config.contains_key(name) {
        return config.get_mut(name);
    }
    config
        .get_mut("sheets")
        .and_then(|sheets| sheets.get_mut(name))
}

fn setting_name(row: &Value) -> Option<&str> {
    ["parameter", "clean_name", "name"]
        .iter()
        .find_map(|key| row.get(*key))
        .and_then(Value::as_str)
        .map(str::trim)
}

/// Replaces each scenario setting's rows in `project_settings` with one row
/// carrying the scenario value.
fn inject_settings(config: &mut Map<String, Value>, settings: &BTreeMap<String, Value>) {
    if sheet_mut(config, "project_settings").is_none() {
        config.insert("project_settings".into(), Value::Array(Vec::new()));
    }
    let Some(sheet) = sheet_mut(config, "project_settings") else {
        return;
    };
    if !sheet.is_array() {
        *sheet = Value::Array(Vec::new());
    }
    let Some(rows) = sheet.as_array_mut() else {
        return;
    };
    rows.retain(|row| setting_name(row).is_none_or(|name| !settings.contains_key(name)));
    for (name, value) in settings {
        rows.push(json!({ "parameter": name, "value": value }));
    }
}

fn category_name(row: &Map<String, Value>) -> Option<&str> {
    ["clean_name", "category"]
        .iter()
        .find_map(|key| row.get(*key))
        .and_then(Value::as_str)
}

fn category_names(config: &Map<String, Value>) -> Vec<String> {
    config
        .get("cust_category")
        .or_else(|| {
            config
                .get("sheets")
                .and_then(|sheets| sheets.get("cust_category"))
        })
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_object)
                .filter_map(category_name)
                .map(|name| name.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The category's `cust_category` rows take the scenario's saturation
/// (`peak_power_w`) and initial (`initial_power_w`) loads.
fn set_category_rows(config: &mut Map<String, Value>, key: &str, load: &CategoryLoad) -> bool {
    let Some(rows) = sheet_mut(config, "cust_category").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut found = false;
    for row in rows.iter_mut().filter_map(Value::as_object_mut) {
        if category_name(row).map(normalize).as_deref() == Some(key) {
            row.insert("peak_power_w".into(), json!(load.saturation_w));
            row.insert("initial_power_w".into(), json!(load.initial_w));
            found = true;
        }
    }
    found
}

/// A rule set's voltage-drop `category_loads` outrank `cust_category` for
/// the voltage drop, so the scenario replaces the category there too.
fn set_configured_loads(config: &mut Map<String, Value>, key: &str, load: &CategoryLoad) -> bool {
    let Some(rule_sets) = config
        .get_mut("lv_poles_rules")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut found = false;
    for rule_set in rule_sets.values_mut() {
        let Some(loads) = rule_set
            .get_mut("voltage_drop")
            .and_then(|block| block.get_mut("category_loads"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        for (name, entry) in loads.iter_mut() {
            if normalize(name) == key {
                *entry = json!({
                    "saturation_w": load.saturation_w,
                    "initial_w": load.initial_w,
                });
                found = true;
            }
        }
    }
    found
}

/// Each customer whose `load` key (`"<Category>, <W>"`) names the category
/// carries the scenario's saturation load.
fn rewrite_customer_loads(gdfs: &mut Map<String, Value>, key: &str, watts: f64) -> usize {
    let Some(features) = gdfs
        .get_mut("customers")
        .and_then(|layer| layer.get_mut("features"))
        .and_then(Value::as_array_mut)
    else {
        return 0;
    };
    let watts = if watts.fract() == 0.0 {
        format!("{}", watts as i64)
    } else {
        watts.to_string()
    };
    let mut rewritten = 0;
    for feature in features {
        let Some(properties) = feature.get_mut("properties").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(category) = properties
            .get("load")
            .and_then(Value::as_str)
            .and_then(|load| load.split(',').next())
            .map(|category| category.trim().to_string())
        else {
            continue;
        };
        if normalize(&category) == key {
            properties.insert("load".into(), Value::String(format!("{category}, {watts}")));
            rewritten += 1;
        }
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(tokens: &[&str]) -> Result<Scenario, Failure> {
        let mut all = vec![
            "--input".to_string(),
            "in.json".to_string(),
            "--out".to_string(),
            "out.json".to_string(),
        ];
        all.extend(tokens.iter().map(|token| token.to_string()));
        let inputs = ds_cli_contract::parse(&super::super::voltage_drop::COMMAND, &all)
            .expect("declared inputs");
        Scenario::parse(&inputs)
    }

    fn code(tokens: &[&str]) -> String {
        match scenario(tokens) {
            Ok(_) => "accepted".into(),
            Err(failure) => failure.code().to_string(),
        }
    }

    #[test]
    fn every_flag_becomes_the_setting_the_method_reads() {
        let parsed = scenario(&[
            "--year",
            "3",
            "--outlook",
            "5, 0,3,3",
            "--set",
            "vd_limit_pct=8",
            "--set",
            "vd_max_recommended_abc_mm2=70",
            "--set",
            "vd_note=text",
            "--load",
            "Residential=100:60",
            "--load",
            "Commercial=1500",
        ])
        .expect("a valid scenario");
        let echo = parsed.echo(&[4, 0]);
        assert_eq!(
            echo["project_settings"],
            json!({
                "vd_design_year": 3,
                "vd_outlook_years": "0,3,5",
                "vd_limit_pct": 8,
                "vd_max_recommended_abc_mm2": 70,
                "vd_note": "text",
            })
        );
        assert_eq!(
            echo["loads"],
            json!([
                { "category": "Residential", "saturation_w": 100.0, "initial_w": 60.0, "customer_loads_rewritten": 4 },
                { "category": "Commercial", "saturation_w": 1500.0, "initial_w": 1500.0, "customer_loads_rewritten": 0 },
            ])
        );
        let off = scenario(&["--no-outlook"]).expect("valid");
        assert_eq!(
            off.echo(&[])["project_settings"],
            json!({ "vd_staged_plan": false })
        );
        let none = scenario(&[]).expect("valid");
        assert_eq!(
            none.echo(&[]),
            json!({ "project_settings": {}, "loads": [] })
        );
    }

    #[test]
    fn set_refuses_what_is_not_a_voltage_drop_setting() {
        assert_eq!(
            code(&["--set", "cos_phi=0.9"]),
            "vd_scenario_setting_refused"
        );
        assert_eq!(
            code(&["--set", "pole_spacing=40"]),
            "vd_scenario_setting_refused"
        );
        // A setting with its own flag names the flag.
        let owned = scenario(&["--set", "vd_design_year=3"])
            .err()
            .expect("refused");
        assert_eq!(owned.code(), "vd_scenario_setting_refused");
        assert!(owned.remedy_text().unwrap_or("").contains("--year"));
        assert_eq!(code(&["--set", "vd_limit_pct"]), "vd_scenario_invalid");
        assert_eq!(code(&["--set", "vd_Limit=8"]), "vd_scenario_invalid");
        assert_eq!(code(&["--set", "vd_limit_pct="]), "vd_scenario_invalid");
    }

    #[test]
    fn bounds_and_repeats_are_refused_by_name() {
        assert_eq!(code(&["--year", "51"]), "vd_scenario_invalid");
        assert_eq!(code(&["--year", "-1"]), "vd_scenario_invalid");
        assert_eq!(code(&["--year", "2.5"]), "vd_scenario_invalid");
        assert_eq!(code(&["--outlook", "0,,5"]), "vd_scenario_invalid");
        assert_eq!(code(&["--outlook", "0,60"]), "vd_scenario_invalid");
        assert_eq!(code(&["--load", "Residential"]), "vd_scenario_invalid");
        assert_eq!(code(&["--load", "Residential=0"]), "vd_scenario_invalid");
        assert_eq!(
            code(&["--load", "Residential=100:150"]),
            "vd_scenario_invalid"
        );
        assert_eq!(
            code(&["--load", "Residential=200000"]),
            "vd_scenario_invalid"
        );
        assert_eq!(
            code(&["--load", "Res, idential=100"]),
            "vd_scenario_invalid"
        );
        assert_eq!(
            code(&["--outlook", "0,5", "--no-outlook"]),
            "vd_scenario_conflict"
        );
        assert_eq!(
            code(&["--load", "Residential=100", "--load", "residential=90"]),
            "vd_scenario_conflict"
        );
        assert_eq!(
            code(&["--set", "vd_limit_pct=8", "--set", "vd_limit_pct=9"]),
            "vd_scenario_conflict"
        );
        assert_eq!(code(&["--year", "0", "--outlook", "0,1"]), "accepted");
    }

    #[test]
    fn the_scenario_reaches_every_place_the_method_reads_a_load() {
        let mut job: NativeFastLvJobV1 = serde_json::from_value(json!({
            "transformer_name": "T1",
            "gdfs": { "customers": { "type": "FeatureCollection", "features": [
                { "type": "Feature", "geometry": null, "properties": { "load": "Residential, 250" } },
                { "type": "Feature", "geometry": null, "properties": { "load": "Commercial, 1500" } },
            ]}},
            "config_dfs": {
                "project_settings": [
                    { "parameter": "cos_phi", "value": 0.85 },
                    { "parameter": "vd_limit_pct", "value": 10 },
                ],
                "cust_category": [
                    { "clean_name": "Residential", "peak_power_w": 250, "initial_power_w": 60 },
                    { "clean_name": "Commercial", "peak_power_w": 1500 },
                ],
                "lv_poles_rules": { "rule_edcl": { "voltage_drop": {
                    "category_loads": { "Residential": { "saturation_w": 250, "initial_w": 60 } }
                }}},
            },
        }))
        .expect("a job");
        let parsed = scenario(&["--set", "vd_limit_pct=8", "--load", "residential=100"]).unwrap();
        let rewritten = parsed
            .apply(std::slice::from_mut(&mut job))
            .expect("applied");
        assert_eq!(rewritten, vec![1]);
        let config = &job.config_dfs;
        assert_eq!(
            config["project_settings"],
            json!([
                { "parameter": "cos_phi", "value": 0.85 },
                { "parameter": "vd_limit_pct", "value": 8 },
            ])
        );
        assert_eq!(config["cust_category"][0]["peak_power_w"], 100.0);
        assert_eq!(config["cust_category"][0]["initial_power_w"], 100.0);
        assert_eq!(config["cust_category"][1]["peak_power_w"], 1500);
        assert_eq!(
            config["lv_poles_rules"]["rule_edcl"]["voltage_drop"]["category_loads"]["Residential"],
            json!({ "saturation_w": 100.0, "initial_w": 100.0 })
        );
        let customers = &job.gdfs["customers"]["features"];
        assert_eq!(customers[0]["properties"]["load"], "Residential, 100");
        assert_eq!(customers[1]["properties"]["load"], "Commercial, 1500");

        let unknown = scenario(&["--load", "Industrial=5000"]).unwrap();
        let refused = unknown
            .apply(std::slice::from_mut(&mut job))
            .expect_err("refused");
        assert_eq!(refused.code(), "vd_scenario_category_unknown");
        assert_eq!(
            refused.detail_value().unwrap()["defined"],
            json!(["Commercial", "Residential"])
        );
    }
}
