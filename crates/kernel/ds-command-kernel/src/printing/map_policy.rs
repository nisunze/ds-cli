//! Project-governed map printing policy, independent of paper and template.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LABELS: &str = "keep_all_labels_at_high_density";
pub const GRID: &str = "print_metric_grid_enabled";
pub const INTERVAL: &str = "print_metric_grid_interval_m";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapPolicy {
    pub keep_all_labels_at_high_density: bool,
    pub metric_grid_enabled: bool,
    /// None selects a round interval from the actual map frame and scale.
    pub metric_grid_interval_m: Option<f64>,
}
impl Default for MapPolicy {
    fn default() -> Self {
        Self {
            keep_all_labels_at_high_density: false,
            metric_grid_enabled: true,
            metric_grid_interval_m: None,
        }
    }
}
fn setting<'a>(config: &'a Value, key: &str) -> Result<Option<&'a Value>, String> {
    let config = config.get("sheets").unwrap_or(config);
    let rows = config
        .get("project_settings")
        .or_else(|| config.pointer("/parameters/project_settings"));
    let Some(rows) = rows else { return Ok(None) };
    let rows = rows
        .as_array()
        .ok_or("print_map_policy_invalid: project_settings must be rows")?;
    let matching = rows
        .iter()
        .filter(|row| crate::report_formats::setting_parameter_key(row).as_deref() == Some(key))
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [] => Ok(None),
        [row] => Ok(Some(&row["value"])),
        _ => Err(format!("print_map_policy_invalid: duplicate {key}")),
    }
}
/// Strict authored values: absent means the published default, malformed never does.
pub fn resolve(config: &Value) -> Result<MapPolicy, String> {
    let mut policy = MapPolicy::default();
    for (key, target) in [
        (LABELS, &mut policy.keep_all_labels_at_high_density),
        (GRID, &mut policy.metric_grid_enabled),
    ] {
        if let Some(value) = setting(config, key)? {
            *target = value
                .as_bool()
                .ok_or_else(|| format!("print_map_policy_invalid: {key} requires a boolean"))?;
        }
    }
    if let Some(value) = setting(config, INTERVAL)? {
        if !value.is_null() {
            let interval = value.as_f64().filter(|v| v.is_finite() && *v > 0. && *v <= 1_000_000.)
                .ok_or("print_map_policy_invalid: interval requires positive metres up to 1000000, or null for automatic")?;
            policy.metric_grid_interval_m = Some(interval);
        }
    }
    Ok(policy)
}
/// The CLI may author these published default settings before a project has a row.
pub fn is_governed_parameter(key: &str) -> bool {
    matches!(key, LABELS | GRID | INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn defaults_are_explicit_and_authored_project_values_are_independent() {
        let defaults = resolve(&json!({})).unwrap();
        assert!(!defaults.keep_all_labels_at_high_density);
        assert!(defaults.metric_grid_enabled);
        assert_eq!(defaults.metric_grid_interval_m, None);
        let authored = resolve(&json!({"sheets":{"project_settings":[
            {"parameter":LABELS,"value":true}, {"parameter":GRID,"value":false},
            {"parameter":INTERVAL,"value":250}, {"parameter":"unrelated","value":"retained"}]}}))
        .unwrap();
        assert!(authored.keep_all_labels_at_high_density);
        assert!(!authored.metric_grid_enabled);
        assert_eq!(authored.metric_grid_interval_m, Some(250.));
        assert_eq!(resolve(&json!({})).unwrap(), defaults);
    }
    #[test]
    fn invalid_values_never_become_defaults() {
        for (key, value) in [
            (LABELS, json!("false")),
            (GRID, json!(1)),
            (INTERVAL, json!(0)),
            (INTERVAL, json!(-1)),
        ] {
            assert!(
                resolve(&json!({"project_settings":[{"parameter":key,"value":value}]})).is_err()
            );
        }
        assert!(resolve(&json!({"project_settings":[{"parameter":GRID,"value":true},{"parameter":GRID,"value":false}]})).is_err());
        assert_eq!(
            resolve(&json!({"project_settings":[{"parameter":INTERVAL,"value":null}]}))
                .unwrap()
                .metric_grid_interval_m,
            None
        );
    }
}
