//! `ds map design setup` — discover or configure project Fast LV inputs.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const SURVEY_LAYER_ARG: Arg = Arg {
    name: "survey-layer",
    kind: ArgKind::Repeated,
    value: "<key>",
    required: false,
    default: None,
    choices: &[],
    summary: "Point survey layer to use as an additional customer source. Repeat.",
};

const TEMPORARY_LAYER_ARG: Arg = Arg {
    name: "temporary-layer",
    kind: ArgKind::Repeated,
    value: "<id-or-name>",
    required: false,
    default: None,
    choices: &[],
    summary: "Local Point layer to use as an additional customer source. Repeat.",
};

const POLE_SURVEY_LAYER_ARG: Arg = Arg {
    name: "pole-survey-layer",
    kind: ArgKind::Repeated,
    value: "<key>",
    required: false,
    default: None,
    choices: &[],
    summary: "Point survey layer to borrow as LV-pole evidence: only poles the drafted LV lines touch are used, as existing poles (tapping when at the source node). Repeat; pass none with --clear-pole-sources to clear.",
};

const POLE_TEMPORARY_LAYER_ARG: Arg = Arg {
    name: "pole-temporary-layer",
    kind: ArgKind::Repeated,
    value: "<id-or-name>",
    required: false,
    default: None,
    choices: &[],
    summary: "Local Point layer to borrow as LV-pole evidence. Repeat.",
};

const CLEAR_POLE_SOURCES_ARG: Arg = Arg {
    name: "clear-pole-sources",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Remove every configured pole source; drafted lv_poles alone are used.",
};

const SCOPE_ARG: Arg = Arg {
    name: "scope",
    kind: ArgKind::Value,
    value: "<user|project>",
    required: false,
    default: Some("user"),
    choices: &["user", "project"],
    summary: "Where the configuration is written: this browser's overlay (user) or the project network template's transformer_settings rows, applying to every browser and surface (project; needs network.template.propagate).",
};

const SURVEY_ONLY_ARG: Arg = Arg {
    name: "survey-only",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Exclude design customers; requires a survey or temporary layer.",
};

const SETTING_ARG: Arg = Arg {
    name: "setting",
    kind: ArgKind::Repeated,
    value: "<key=true|false|number>",
    required: false,
    default: None,
    choices: &[],
    summary: "Override one typed processor parameter. Repeat.",
};

const RESET_SETTINGS_ARG: Arg = Arg {
    name: "reset-settings",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Reset processor parameters to the selected preset before applying overrides.",
};

const PRESET_ARG: Arg = Arg {
    name: "preset",
    kind: ArgKind::Value,
    value: "<drafting|sketch>",
    required: false,
    default: None,
    choices: &["drafting", "sketch"],
    summary: "Project Fast LV process preset.",
};

const DRY_RUN_ARG: Arg = Arg {
    name: "dry-run",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Validate the requested setup without persisting it locally.",
};

const DEFAULT_LIMIT: &str = "20";

pub static COMMAND: Command = Command {
    id: "map.design.setup",
    path: &["map", "design", "setup"],
    contract: 1,
    summary: "Discover or configure project-scoped Fast LV inputs.",
    purpose: "\
With no configuration flags, reports the project preset, selected customer \
sources, available Point layers, and effective processor parameters. Source \
layers, --preset, and typed --setting overrides store the same project-scoped setup the application uses. The CLI \
names semantic layer keys only; IndexedDB addresses and processor wiring stay \
inside DS GridDesign. No design features or cloud data are changed.",
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        SURVEY_LAYER_ARG,
        TEMPORARY_LAYER_ARG,
        POLE_SURVEY_LAYER_ARG,
        POLE_TEMPORARY_LAYER_ARG,
        CLEAR_POLE_SOURCES_ARG,
        SCOPE_ARG,
        SURVEY_ONLY_ARG,
        PRESET_ARG,
        SETTING_ARG,
        RESET_SETTINGS_ARG,
        DRY_RUN_ARG,
        Arg::value(
            "limit",
            "<n>",
            "Report at most this many available layers per source kind; 1..200.",
        )
        .default(DEFAULT_LIMIT),
        DESCRIPTOR_ARG,
    ],
    output: "The selected preset, effective typed processor settings, and bounded customer-source inventories.",
    examples: &[
        Example {
            command: "ds map design setup --output json",
            note: "Discover project-scoped sources without changing setup.",
            runnable: false,
        },
        Example {
            command: "ds map design setup --survey-layer edcl_customers_survey --preset drafting --dry-run --output json",
            note: "Validate the mixed design + survey source before applying it.",
            runnable: false,
        },
        Example {
            command: "ds map design setup --pole-survey-layer edcl_poles_survey --dry-run --output json",
            note: "Borrow a surveyed pole layer: the model keeps only poles the drafted LV lines touch, as existing poles.",
            runnable: false,
        },
        Example {
            command: "ds map design setup --preset drafting --setting weld_tolerance=0.1 --setting keep_lv_lines_topology=true --output json",
            note: "Persist explicit parameters on top of the shared drafting preset.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the map is not ready, a layer is unavailable/non-Point/empty, or a processor parameter is unknown or has the wrong type",
            remedy: "run this command without configuration flags and inspect available layers and effective_settings",
        },
        crate::UNSUPPORTED,
        Refusal {
            code: "customer_source_required",
            when: "--survey-only was given with no --survey-layer and no --temporary-layer",
            remedy: "name a Point source layer, or omit --survey-only to retain design customers",
        },
        Refusal {
            code: "too_many_settings",
            when: "more than 64 --setting overrides were given",
            remedy: "pass at most 64 --setting overrides",
        },
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 200)? as usize;
    let survey_layers = inputs.repeated("survey-layer");
    let temporary_layers = inputs.repeated("temporary-layer");
    let survey_only = inputs.switch("survey-only");
    if survey_only && survey_layers.is_empty() && temporary_layers.is_empty() {
        return Err(Failure::invalid(
            "customer_source_required",
            "--survey-only requires at least one --survey-layer or --temporary-layer",
        )
        .remedy("name a Point source layer, or omit --survey-only to retain design customers"));
    }
    let mut arguments = Map::new();
    if !survey_layers.is_empty() || !temporary_layers.is_empty() {
        arguments.insert("surveyLayers".into(), json!(survey_layers));
        arguments.insert("temporaryLayers".into(), json!(temporary_layers));
        arguments.insert("includeDesignCustomers".into(), json!(!survey_only));
    }
    let pole_survey_layers = inputs.repeated("pole-survey-layer");
    let pole_temporary_layers = inputs.repeated("pole-temporary-layer");
    if !pole_survey_layers.is_empty()
        || !pole_temporary_layers.is_empty()
        || inputs.switch("clear-pole-sources")
    {
        arguments.insert("poleSurveyLayers".into(), json!(pole_survey_layers));
        arguments.insert("poleTemporaryLayers".into(), json!(pole_temporary_layers));
    }
    if let Some(preset) = inputs.value("preset") {
        arguments.insert("preset".into(), json!(preset));
    }
    if let Some(scope) = inputs.value("scope") {
        arguments.insert("scope".into(), json!(scope));
    }
    let settings = parse_settings(inputs.repeated("setting"))?;
    if !settings.is_empty() {
        arguments.insert("settings".into(), Value::Object(settings));
    }
    if inputs.switch("reset-settings") {
        arguments.insert("resetSettings".into(), json!(true));
    }
    if inputs.switch("dry-run") {
        arguments.insert("dryRun".into(), json!(true));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_PROCESS_CONFIGURE,
        Value::Object(arguments),
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;
    Ok(project_result(&result, limit))
}

fn project_result(result: &Value, limit: usize) -> Value {
    let empty = Vec::new();
    let temporary = result["availableTemporaryLayers"]
        .as_array()
        .unwrap_or(&empty);
    let surveys = result["availableSurveyLayers"].as_array().unwrap_or(&empty);
    let temporary_shown: Vec<Value> = temporary.iter().take(limit).cloned().collect();
    let survey_shown: Vec<Value> = surveys.iter().take(limit).cloned().collect();
    let temporary_omitted = temporary.len().saturating_sub(temporary_shown.len());
    let survey_omitted = surveys.len().saturating_sub(survey_shown.len());
    let mut projected = json!({
        "project": result["project"],
        "configured": result["configured"],
        "dry_run": result["dryRun"],
        "preset": result["preset"],
        "include_design_customers": result["includeDesignCustomers"],
        "temporary_layers": result["temporaryLayers"],
        "survey_layers": result["surveyLayers"],
        "pole_temporary_layers": result["poleTemporaryLayers"],
        "pole_survey_layers": result["poleSurveyLayers"],
        "scope": result["scope"],
        "project_rows_written": result["projectRowsWritten"],
        "effective_settings": result["effectiveSettings"],
        "available_temporary_layer_count": temporary.len(),
        "available_temporary_layers": temporary_shown,
        "available_survey_layer_count": surveys.len(),
        "available_survey_layers": survey_shown,
    });
    if temporary_omitted > 0 || survey_omitted > 0 {
        projected["more"] = json!({
            "available_temporary_layers_omitted": temporary_omitted,
            "available_survey_layers_omitted": survey_omitted,
            "remedy": format!("re-run with --limit {}", temporary.len().max(surveys.len()).min(200)),
        });
    }
    projected
}

fn parse_settings(raw: &[String]) -> Result<Map<String, Value>, Failure> {
    if raw.len() > 64 {
        return Err(Failure::invalid(
            "too_many_settings",
            "--setting accepts at most 64 overrides",
        ));
    }
    let mut out = Map::new();
    for item in raw {
        let Some((key, raw_value)) = item.split_once('=') else {
            return Err(Failure::invalid(
                "invalid_setting",
                format!("processor setting `{item}` must be key=value"),
            ));
        };
        let key = key.trim();
        let raw_value = raw_value.trim();
        if key.is_empty() || raw_value.is_empty() {
            return Err(Failure::invalid(
                "invalid_setting",
                format!("processor setting `{item}` must have a key and value"),
            ));
        }
        let value = match raw_value.to_ascii_lowercase().as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => raw_value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map_or_else(
                    || {
                        Err(Failure::invalid(
                            "invalid_setting",
                            format!(
                                "processor setting `{key}` requires true, false, or a finite number"
                            ),
                        ))
                    },
                    |number| Ok(json!(number)),
                )?,
        };
        out.insert(key.to_string(), value);
    }
    Ok(out)
}

pub fn render(data: &Value) -> String {
    let surveys = data["survey_layers"].as_array().map_or(0, Vec::len);
    let temporary = data["temporary_layers"].as_array().map_or(0, Vec::len);
    let available = data["available_survey_layers"]
        .as_array()
        .map_or(0, Vec::len);
    format!(
        "Fast LV setup for {}\n  preset  {}\n  design customers  {}\n  selected temporary layers  {}\n  selected survey layers  {}\n  effective settings  {}\n  available Point survey layers  {}\n  {}\n",
        data["project"].as_str().unwrap_or("project"),
        data["preset"].as_str().unwrap_or("sketch"),
        if data["include_design_customers"].as_bool().unwrap_or(false) {
            "included"
        } else {
            "excluded"
        },
        temporary,
        surveys,
        data["effective_settings"].as_object().map_or(0, Map::len),
        available,
        if data["configured"].as_bool().unwrap_or(false) {
            "setup saved locally"
        } else if data["dry_run"].as_bool().unwrap_or(false) {
            "validated; nothing changed"
        } else {
            "discovery only; nothing changed"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_settings, project_result};
    use serde_json::json;

    #[test]
    fn parses_typed_processor_overrides() {
        let parsed = parse_settings(&[
            "keep_lv_lines_topology=true".into(),
            "weld_tolerance=0.1".into(),
        ])
        .expect("valid settings");
        assert_eq!(parsed["keep_lv_lines_topology"], json!(true));
        assert_eq!(parsed["weld_tolerance"], json!(0.1));
    }

    #[test]
    fn rejects_untyped_processor_overrides() {
        let error = parse_settings(&["weld_tolerance=near".into()]).unwrap_err();
        assert_eq!(error.code(), "invalid_setting");
    }

    #[test]
    fn bounds_available_source_inventories_without_truncating_selected_sources() {
        let projected = project_result(
            &json!({
                "project": "p",
                "temporaryLayers": [{"key": "selected"}],
                "surveyLayers": [],
                "availableTemporaryLayers": [{"key": "a"}, {"key": "b"}, {"key": "c"}],
                "availableSurveyLayers": [{"key": "survey"}],
            }),
            2,
        );
        assert_eq!(projected["temporary_layers"][0]["key"], "selected");
        assert_eq!(projected["available_temporary_layer_count"], 3);
        assert_eq!(
            projected["available_temporary_layers"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(projected["more"]["available_temporary_layers_omitted"], 1);
    }
}
