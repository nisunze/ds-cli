//! Pure Settings model and edits. Hosts own snapshots, IO and authentication.
use super::canonical_parameter_name;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "ds.design-config/v1";
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_RULE_SET: &str = "rule_edcl";

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    SurveyOptions {
        forms: Vec<Value>,
        project_matches: bool,
    },
    MetadataField {
        metadata: Map<String, Value>,
        field: String,
        value: Value,
    },
    Read {
        document: Value,
        sheet_key: String,
        #[serde(default)]
        rule_set: Option<String>,
        offset: usize,
        limit: usize,
    },
    List {
        document: Value,
        offset: usize,
        limit: usize,
    },
    DiffPage {
        baseline: Value,
        current: Value,
        offset: usize,
        limit: usize,
    },
    Model {
        document: Value,
    },
    SheetModel {
        document: Value,
        sheet_key: String,
    },
    ParameterModel {
        document: Value,
        sheet_key: String,
        parameter: String,
    },
    ParameterOptions {
        document: Value,
        sheet_key: String,
        parameter: String,
        #[serde(default)]
        current: Value,
        #[serde(default)]
        select: bool,
    },
    FieldModels {
        document: Value,
        sheet_key: String,
    },
    FieldModel {
        document: Value,
        sheet_key: String,
        field: String,
    },
    FieldOptions {
        document: Value,
        sheet_key: String,
        field: String,
    },
    MetadataKeys {
        metadata: Value,
        models: Value,
    },
    /// Which design-data path does this project use — the Firestore documents,
    /// or the mirrored dataset the Combined column reflects?
    DesignDataPath {
        document: Value,
    },
    IsRuleSetSheet {
        sheet_key: String,
        #[serde(default)]
        document: Value,
    },
    IsParameterSheet {
        sheet_key: String,
    },
    IsSurveyParameter {
        value: String,
    },
    IsAliasColumn {
        value: String,
    },
    CanonicalParameterName {
        value: String,
    },
    /// The fence a sheet save may carry: the sheet as it was read when the
    /// gateway fences saves of that sheet, otherwise none.
    SaveFence {
        sheet_key: String,
        #[serde(default)]
        expected: Value,
    },
    AliasList {
        action: AliasAction,
        value: Value,
        #[serde(default)]
        alias: Value,
    },
    Coerce {
        control: Control,
        raw: Value,
        #[serde(default)]
        options: Vec<Value>,
    },
    Toggle {
        control: ToggleControl,
        current: Value,
        value: String,
        checked: bool,
        options: Vec<Value>,
    },
    Inactive {
        value: Value,
        options: Vec<Value>,
        #[serde(default)]
        case_insensitive: bool,
    },
    Render {
        value: Value,
    },
    Dirty {
        baseline: Value,
        current: Value,
    },
    Diff {
        baseline: Value,
        current: Value,
    },
    Compact {
        sheet_key: String,
        value: Value,
    },
    CompactRows {
        rows: Vec<Value>,
    },
    IsBlankRow {
        value: Value,
    },
    Columns {
        sheet_key: String,
        rows: Vec<Value>,
    },
    IsRuleSetMap {
        value: Value,
    },
    RuleSetNames {
        value: Value,
        #[serde(default)]
        document: Value,
        #[serde(default)]
        sheet_key: String,
    },
    RuleSetRows {
        value: Value,
        name: String,
    },
    RuleSetMetadata {
        value: Value,
        name: String,
    },
    WithRuleSetRows {
        value: Value,
        name: String,
        rows: Vec<Value>,
    },
    WithRuleSetMetadata {
        value: Value,
        name: String,
        metadata: Map<String, Value>,
    },
    NormalizeRuleSetName {
        value: String,
    },
    UniqueRuleSetName {
        existing: Vec<String>,
        base: String,
    },
    DuplicateRuleSet {
        value: Value,
        source: String,
        target: String,
        #[serde(default)]
        dirty: bool,
    },
    Save {
        document: Value,
        sheet_key: String,
        value: Value,
    },
    Set {
        document: Value,
        sheet_key: String,
        parameter: String,
        raw: Value,
    },
    /// Plan one admitted sheet copied from another project's configuration.
    SheetCopyPlan {
        project: String,
        source_project: String,
        source: Value,
        destination: Value,
        sheet_key: String,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasAction {
    Parse,
    Add,
    Remove,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Scalar,
    MetadataScalar,
    List,
    Enabled,
    CheckboxMap,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleControl {
    Multiselect,
    CheckboxMap,
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("config_input_too_large".into());
    }
    let request: Request = serde_json::from_slice(input).map_err(|_| "config_request_invalid")?;
    Ok(json!({"schema":SCHEMA,"result":apply(request)?}).to_string())
}

pub fn apply(request: Request) -> Result<Value, String> {
    use Request::*;
    Ok(match request {
        SurveyOptions {forms,project_matches} => json!(forms.iter().filter(|f|project_matches && f["enabled"]!=false && f["project_settings"]["enabled"]!=false).map(|f|json!({"slug":f["slug"],"label":f["display_name"].as_str().filter(|s|!s.is_empty()).or_else(||f["slug"].as_str()).unwrap_or_default()})).collect::<Vec<_>>()),
        DesignDataPath { document } => design_data_path(&document),
        MetadataField {mut metadata,field,value} => {if value.is_null() || value=="" {metadata.shift_remove(&field);} else {metadata.insert(field,value);} Value::Object(metadata)},
        Read {
            document,
            sheet_key,
            rule_set,
            offset,
            limit,
        } => super::read_sheet(&document, &sheet_key, rule_set.as_deref(), offset, limit)?,
        List {
            document,
            offset,
            limit,
        } => {
            let model = model(&document);
            let mut page = super::bounded_page(
                model["sheets"].as_array().expect("model sheets"),
                offset,
                limit,
            )?;
            page["groups"] = model["groups"].clone();
            page
        }
        DiffPage {
            baseline,
            current,
            offset,
            limit,
        } => {
            let mut changes = Vec::new();
            diff(&baseline, &current, "", &mut changes);
            let mut page = super::bounded_page(&changes, offset, limit)?;
            page["dirty"] = json!(!changes.is_empty());
            page
        }
        Model { document } => model(&document),
        SheetModel {
            document,
            sheet_key,
        } => sheet_model(&document, &sheet_key).clone(),
        ParameterModel {
            document,
            sheet_key,
            parameter,
        } => member_model(&document, &sheet_key, "parameters", &parameter).clone(),
        ParameterOptions {
            document,
            sheet_key,
            parameter,
            current,
            select,
        } => {
            let model = member_model(&document, &sheet_key, "parameters", &parameter);
            let mut options = model_options(model);
            if select {
                if model["control"] != "select" {
                    options.clear();
                } else {
                    let current = js_string(&current).trim().to_owned();
                    if !current.is_empty() && !options.iter().any(|o| o["value"] == current) {
                        options.insert(0, json!({"value":current,"label":current}));
                    }
                }
            }
            json!(options)
        }
        FieldModels {
            document,
            sheet_key,
        } => sheet_model(&document, &sheet_key)
            .get("fields")
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or(json!({})),
        FieldModel {
            document,
            sheet_key,
            field,
        } => member_model(&document, &sheet_key, "fields", &field).clone(),
        FieldOptions {
            document,
            sheet_key,
            field,
        } => json!(model_options(member_model(
            &document, &sheet_key, "fields", &field
        ))),
        MetadataKeys { metadata, models } => {
            let mut out = Vec::new();
            for key in keys(&models).into_iter().chain(keys(&metadata)) {
                let key = key.trim();
                if !key.is_empty() && key != "table" && !out.contains(&key.to_owned()) {
                    out.push(key.to_owned());
                }
            }
            json!(out)
        }
        IsRuleSetSheet {
            sheet_key,
            document,
        } => json!(is_rule_set_sheet(&document, &sheet_key)),
        IsParameterSheet { sheet_key } => json!(is_parameter_sheet(&sheet_key)),
        IsSurveyParameter { value } => json!(matches!(
            canonical_parameter_name(&value).as_str(),
            "survey_forms_in_transformer_exports" | "transformer_export_survey_forms"
        )),
        IsAliasColumn { value } => json!(matches!(
            value
                .to_ascii_lowercase()
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
                .as_str(),
            "misspellednames" | "misspelledheader" | "aliases" | "values"
        )),
        CanonicalParameterName { value } => json!(canonical_parameter_name(&value)),
        SaveFence {
            sheet_key,
            expected,
        } => save_fence(&sheet_key, expected),
        AliasList {
            action,
            value,
            alias,
        } => {
            let mut out = alias_list(&value);
            let text = js_string(&alias).trim().to_owned();
            let key = text.to_lowercase();
            match action {
                AliasAction::Parse => {}
                AliasAction::Add => {
                    if valid_alias(&key) && !out.iter().any(|v| v.to_lowercase() == key) {
                        out.push(text);
                    }
                }
                AliasAction::Remove => out.retain(|v| v.to_lowercase() != key),
            }
            json!(out)
        }
        Coerce {
            control,
            raw,
            options,
        } => match control {
            Control::Scalar => scalar(&raw),
            Control::MetadataScalar => {
                if js_string(&raw).trim().is_empty() {
                    Value::Null
                } else {
                    scalar(&raw)
                }
            }
            Control::List => json!(string_list(&raw)),
            Control::Enabled => json!(enabled(&raw)),
            Control::CheckboxMap => checkbox_map(&raw, &options),
        },
        Toggle {
            control,
            current,
            value,
            checked,
            options,
        } => toggle(control, &current, &value, checked, &options),
        Inactive {
            value,
            options,
            case_insensitive,
        } => json!(inactive(&value, &options, case_insensitive)),
        Render { value } => json!(if value.is_object() || value.is_array() {
            value.to_string()
        } else {
            js_string(&value)
        }),
        Dirty { baseline, current } => json!(!settings_equal(&baseline, &current)),
        Diff { baseline, current } => {
            let mut changes = Vec::new();
            diff(&baseline, &current, "", &mut changes);
            json!({"dirty":!changes.is_empty(),"changes":changes})
        }
        Compact { sheet_key, value } => compact_sheet(&sheet_key, &value),
        CompactRows { rows } => json!(compact_rows(&rows)),
        IsBlankRow { value } => json!(blank_row(&value)),
        Columns { sheet_key, rows } => json!(columns(&sheet_key, &rows)),
        IsRuleSetMap { value } => json!(is_rule_set_map(&value)),
        RuleSetNames {
            value,
            document,
            sheet_key,
        } => json!(rule_set_names(&document, &sheet_key, &value)),
        RuleSetRows { value, name } => json!(rule_rows(&value, &name)),
        RuleSetMetadata { value, name } => Value::Object(rule_metadata(&value, &name)),
        WithRuleSetRows { value, name, rows } => with_rows(&value, &name, rows),
        WithRuleSetMetadata {
            value,
            name,
            mut metadata,
        } => {
            let mut base = rule_base(&value);
            metadata.insert("table".into(), json!(rule_rows(&value, &name)));
            base.insert(name, Value::Object(metadata));
            Value::Object(base)
        }
        NormalizeRuleSetName { value } => json!(normalize_rule_set_name(&value)),
        UniqueRuleSetName { existing, base } => json!(unique_rule_set_name(&existing, &base)),
        DuplicateRuleSet {
            value,
            source,
            target,
            dirty,
        } => duplicate_rule_set(&value, &source, &target, dirty)?,
        Save {
            document,
            sheet_key,
            value,
        } => save_sheet(&document, &sheet_key, &value)?,
        Set {
            document,
            sheet_key,
            parameter,
            raw,
        } => set_parameter(&document, &sheet_key, &parameter, &raw)?,
        SheetCopyPlan {
            project,
            source_project,
            source,
            destination,
            sheet_key,
        } => {
            super::plan_sheet_copy(&project, &source_project, &source, &destination, &sheet_key)?
                .plan
        }
    })
}

use ds_report_contracts::catalogue_aliases::{js_string, valid_alias};
fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}
pub use ds_report_contracts::catalogue_aliases::alias_list;
pub fn scalar(raw: &Value) -> Value {
    let Some(text) = raw.as_str() else {
        return raw.clone();
    };
    let trim = text.trim();
    if trim.eq_ignore_ascii_case("true") {
        return json!(true);
    }
    if trim.eq_ignore_ascii_case("false") {
        return json!(false);
    }
    if (trim.starts_with('[') && trim.ends_with(']'))
        || (trim.starts_with('{') && trim.ends_with('}'))
    {
        if let Ok(v) = serde_json::from_str(trim) {
            return v;
        }
    }
    if !trim.is_empty() {
        let number = if let Some(hex) = trim.strip_prefix("0x").or_else(|| trim.strip_prefix("0X"))
        {
            u64::from_str_radix(hex, 16).ok().map(|v| v as f64)
        } else if let Some(bin) = trim.strip_prefix("0b").or_else(|| trim.strip_prefix("0B")) {
            u64::from_str_radix(bin, 2).ok().map(|v| v as f64)
        } else if let Some(oct) = trim.strip_prefix("0o").or_else(|| trim.strip_prefix("0O")) {
            u64::from_str_radix(oct, 8).ok().map(|v| v as f64)
        } else {
            trim.parse::<f64>().ok()
        };
        if let Some(n) = number {
            return if n.is_finite() {
                if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 {
                    json!(n as i64)
                } else {
                    json!(n)
                }
            } else if trim == "Infinity" || trim == "-Infinity" || trim == "+Infinity" {
                Value::Null
            } else {
                raw.clone()
            };
        }
    }
    raw.clone()
}
fn enabled(value: &Value) -> bool {
    matches!(
        js_string(value).trim().to_lowercase().as_str(),
        "true" | "yes" | "1" | "on"
    )
}
fn string_list(value: &Value) -> Vec<String> {
    fn push(value: &Value, out: &mut Vec<String>, seen: &mut BTreeSet<String>) {
        let s = js_string(value).trim().to_owned();
        if !s.is_empty() && seen.insert(s.to_lowercase()) {
            out.push(s)
        }
    }
    fn visit(value: &Value, out: &mut Vec<String>, seen: &mut BTreeSet<String>) {
        match value {
            Value::Array(a) => {
                for v in a {
                    visit(v, out, seen)
                }
            }
            Value::Object(m) => {
                for (k, v) in m {
                    if matches!(js_string(v).trim().to_lowercase().as_str(), "true" | "yes") {
                        push(&json!(k), out, seen)
                    }
                }
            }
            Value::String(s) => {
                let t = s.trim();
                if t.starts_with('[') {
                    if let Ok(v) = serde_json::from_str::<Value>(t) {
                        visit(&v, out, seen);
                        return;
                    }
                }
                for part in t.split([',', '\n', '\r', ';']) {
                    push(&json!(part), out, seen)
                }
            }
            Value::Null => {}
            _ => push(value, out, seen),
        }
    }
    let mut out = Vec::new();
    visit(value, &mut out, &mut BTreeSet::new());
    out
}
fn option_values(options: &[Value]) -> Vec<String> {
    options.iter().map(|v| js_string(&v["value"])).collect()
}
fn checkbox_map(value: &Value, options: &[Value]) -> Value {
    let names = option_values(options);
    let mut out: Map<String, Value> = names.iter().map(|s| (s.clone(), json!(false))).collect();
    if let Some(map) = value.as_object() {
        for (k, v) in map {
            if let Some(name) = names
                .iter()
                .find(|n| n.to_lowercase() == k.trim().to_lowercase())
            {
                out.insert(name.clone(), json!(enabled(v)));
            }
        }
    } else if value.is_string() || value.is_array() {
        let selected: BTreeSet<_> = string_list(value)
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        for n in names {
            out.insert(n.clone(), json!(selected.contains(&n.to_lowercase())));
        }
    }
    Value::Object(out)
}
fn inactive(value: &Value, options: &[Value], ignore_case: bool) -> Vec<String> {
    let key = |s: String| if ignore_case { s.to_lowercase() } else { s };
    let active: BTreeSet<_> = option_values(options).into_iter().map(key).collect();
    string_list(value)
        .into_iter()
        .filter(|s| !active.contains(&key(s.clone())))
        .collect()
}
fn toggle(
    control: ToggleControl,
    current: &Value,
    value: &str,
    checked: bool,
    options: &[Value],
) -> Value {
    match control {
        ToggleControl::CheckboxMap => {
            let mut map = checkbox_map(current, options);
            if map.get(value).is_some() {
                map[value] = json!(checked);
            }
            for v in inactive(current, options, true) {
                map[&v] = json!(true);
            }
            map
        }
        ToggleControl::Multiselect => {
            let mut selected = string_list(current);
            selected.retain(|v| v != value);
            if checked {
                selected.push(value.into())
            }
            let names = option_values(options);
            let mut out: Vec<_> = names
                .iter()
                .filter(|v| selected.contains(v))
                .cloned()
                .collect();
            out.extend(selected.into_iter().filter(|v| !names.contains(v)));
            json!(out)
        }
    }
}

fn sheet_model<'a>(document: &'a Value, key: &str) -> &'a Value {
    document
        .get("models")
        .and_then(|v| v.get(key))
        .filter(|v| v.is_object())
        .unwrap_or(&Value::Null)
}
fn member_model<'a>(document: &'a Value, key: &str, member: &str, name: &str) -> &'a Value {
    let wanted = canonical_parameter_name(name);
    sheet_model(document, key)
        .get(member)
        .and_then(Value::as_object)
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| canonical_parameter_name(k) == wanted)
        })
        .map(|(_, v)| v)
        .unwrap_or(&Value::Null)
}
fn model_options(model: &Value) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    model["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| {
            let v = js_string(&o["value"]).trim().to_owned();
            if v.is_empty() || !seen.insert(v.clone()) {
                return None;
            }
            let mut out = json!({"value":v});
            if let Some(label) = o.get("label") {
                out["label"] = label.clone()
            }
            Some(out)
        })
        .collect()
}
fn is_rule_set_sheet(document: &Value, key: &str) -> bool {
    key == "lv_poles_rules" || sheet_model(document, key)["type"] == "rule_set"
}
fn is_parameter_sheet(key: &str) -> bool {
    matches!(key, "project_settings" | "transformer_settings")
}
fn model(document: &Value) -> Value {
    let mut groups = keys(&document["groups"]);
    let order = [
        "parameters",
        "engineering",
        "header_mapping",
        "data_cleaning",
    ];
    groups.sort_by_key(|g| order.iter().position(|s| s == g).unwrap_or(order.len()));
    let sheets:Vec<_>=document["sheets"].as_object().into_iter().flatten().map(|(key,value)|{
        let rules=is_rule_set_sheet(document,key);let names=rule_set_names(document,key,value);let rows=if rules{rule_rows(value,names.first().map(String::as_str).unwrap_or(DEFAULT_RULE_SET)).to_vec()}else{value.as_array().cloned().unwrap_or_default()};
        let parameters:Vec<_>=rows.iter().map(|row|{let name=js_string(&row["parameter"]);json!({"name":name,"canonical_name":canonical_parameter_name(&name),"model":member_model(document,key,"parameters",&name)})}).collect();
        json!({"key":key,"kind":if rules{"rule_set"}else if is_parameter_sheet(key){"parameters"}else{"table"},"columns":columns(key,&rows),"rule_sets":if rules{names}else{vec![]},"model":sheet_model(document,key),"parameters":if is_parameter_sheet(key){parameters}else{vec![]}})
    }).collect();
    json!({"groups":groups,"sheets":sheets})
}
fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.iter().all(blank),
        Value::Object(m) => m.values().all(blank),
        _ => false,
    }
}
fn blank_row(value: &Value) -> bool {
    !value.is_object() || blank(value)
}
fn compact_rows(rows: &[Value]) -> Vec<Value> {
    rows.iter().filter(|v| !blank_row(v)).cloned().collect()
}
fn entry_rows(entry: &Value) -> &[Value] {
    entry
        .as_array()
        .or_else(|| entry.get("table").and_then(Value::as_array))
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn is_rule_set_map(value: &Value) -> bool {
    value.as_object().is_some_and(|m| {
        !m.is_empty()
            && m.values()
                .all(|v| v.is_array() || v.is_object() && v["table"].is_array())
    })
}
fn rule_base(value: &Value) -> Map<String, Value> {
    if is_rule_set_map(value) {
        value.as_object().unwrap().clone()
    } else if value.is_array() {
        Map::from_iter([(DEFAULT_RULE_SET.into(), value.clone())])
    } else {
        Map::new()
    }
}
fn rule_rows<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    if is_rule_set_map(value) {
        entry_rows(&value[name])
    } else if name == DEFAULT_RULE_SET {
        value.as_array().map(Vec::as_slice).unwrap_or_default()
    } else {
        &[]
    }
}
fn rule_metadata(value: &Value, name: &str) -> Map<String, Value> {
    if is_rule_set_map(value) && value[name]["table"].is_array() {
        let mut m = value[name].as_object().cloned().unwrap_or_default();
        m.shift_remove("table");
        m
    } else {
        Map::new()
    }
}
fn with_rows(value: &Value, name: &str, rows: Vec<Value>) -> Value {
    let mut base = rule_base(value);
    let mut entry = rule_metadata(value, name);
    entry.insert("table".into(), json!(rows));
    base.insert(name.into(), Value::Object(entry));
    Value::Object(base)
}
fn rule_set_names(document: &Value, key: &str, value: &Value) -> Vec<String> {
    let mut names: Vec<String> = sheet_model(document, key)["rule_sets"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| js_string(&o["value"]).trim().to_owned())
        .collect();
    names.extend(if is_rule_set_map(value) {
        keys(value)
    } else {
        vec![DEFAULT_RULE_SET.into()]
    });
    let mut seen = BTreeSet::new();
    names.retain(|n| !n.is_empty() && seen.insert(n.clone()));
    if names.is_empty() {
        names.push(DEFAULT_RULE_SET.into())
    }
    names
}
pub fn compact_sheet(key: &str, value: &Value) -> Value {
    if key == "lv_poles_rules" && is_rule_set_map(value) {
        let mut out = value.clone();
        for name in keys(value) {
            out = with_rows(&out, &name, compact_rows(entry_rows(&value[&name])))
        }
        out
    } else if let Some(rows) = value.as_array() {
        json!(compact_rows(rows))
    } else {
        value.clone()
    }
}
pub fn normalize_rule_set_name(value: &str) -> String {
    let mut out = String::new();
    for c in value.trim().to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() || c == '-' {
            c
        } else {
            '_'
        };
        if c != '_' || !out.ends_with('_') {
            out.push(c)
        }
    }
    out.trim_matches('_').into()
}
fn unique_rule_set_name(existing: &[String], base: &str) -> String {
    let names: BTreeSet<_> = existing
        .iter()
        .map(|s| normalize_rule_set_name(s))
        .collect();
    let base = normalize_rule_set_name(base);
    let base = if base.is_empty() { "rule_copy" } else { &base };
    if !names.contains(base) {
        return base.into();
    } // At most existing.len()+1 attempts, independent of host time.
    (2..=existing.len() + 2)
        .map(|i| format!("{base}_{i}"))
        .find(|s| !names.contains(s))
        .expect("more candidates than existing names")
}
pub fn duplicate_rule_set(
    value: &Value,
    source: &str,
    target: &str,
    dirty: bool,
) -> Result<Value, &'static str> {
    if dirty {
        return Err("config_sheet_dirty");
    }
    let mut base = rule_base(value);
    let target = normalize_rule_set_name(target);
    if target.is_empty() {
        return Err("config_rule_set_name_invalid");
    }
    if base.keys().any(|k| normalize_rule_set_name(k) == target) {
        return Err("config_rule_set_exists");
    }
    let source_key = normalize_rule_set_name(source);
    let matches: Vec<_> = base
        .keys()
        .filter(|k| normalize_rule_set_name(k) == source_key)
        .collect();
    if matches.len() != 1 {
        return Err("config_rule_set_source_invalid");
    }
    let entry = &base[matches[0]];
    let mut copy = entry.as_object().cloned().unwrap_or_default();
    copy.insert("table".into(), json!(compact_rows(entry_rows(entry))));
    base.insert(target, Value::Object(copy));
    Ok(Value::Object(base))
}
fn columns(key: &str, rows: &[Value]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let all: Vec<_> = rows
        .iter()
        .flat_map(keys)
        .filter(|k| seen.insert(k.clone()))
        .collect();
    let (first, last): (&[&str], &[&str]) = match key {
        "lv_poles_rules" => (
            &[
                "priority",
                "position",
                "dev_angle_threshold_deg",
                "cable_size",
                "cable_size_ge_mm2",
                "notes",
            ],
            &["assembly_type", "stay", "struct_type"],
        ),
        "tag_groups" => (
            &[
                "group_id",
                "label",
                "domain",
                "behavior",
                "cardinality",
                "ordered",
                "requires_previous",
                "allow_skip",
                "allow_regress",
                "keep_history",
            ],
            &[
                "filterable",
                "show_in_status",
                "show_on_map",
                "sort_order",
                "is_active",
                "notes",
            ],
        ),
        "tags" => (
            &["tag_id", "label", "group_id", "color", "order", "meaning"],
            &[
                "billable",
                "reserve_design",
                "review_link_policy",
                "create_from_review",
                "is_terminal",
                "is_active",
            ],
        ),
        _ => (&[], &[]),
    };
    first
        .iter()
        .filter(|s| seen.contains(**s))
        .map(|s| (*s).to_owned())
        .chain(
            all.iter()
                .filter(|s| !first.contains(&s.as_str()) && !last.contains(&s.as_str()))
                .cloned(),
        )
        .chain(
            last.iter()
                .filter(|s| seen.contains(**s))
                .map(|s| (*s).to_owned()),
        )
        .collect()
}
fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<Value>) {
    if settings_equal(a, b) {
        return;
    }
    match (a, b) {
        (Value::Object(am), Value::Object(bm)) => {
            let ks: BTreeSet<_> = am.keys().chain(bm.keys()).collect();
            for k in ks {
                let p = format!("{path}/{}", k.replace('~', "~0").replace('/', "~1"));
                match(am.get(k),bm.get(k)){(Some(a),Some(b))=>diff(a,b,&p,out),(a,b)=>out.push(json!({"path":p,"before":a,"after":b,"removed":b.is_none(),"added":a.is_none()}))}
            }
        }
        (Value::Array(av), Value::Array(bv)) => {
            for i in 0..av.len().max(bv.len()) {
                let p = format!("{path}/{i}");
                match(av.get(i),bv.get(i)){(Some(a),Some(b))=>diff(a,b,&p,out),(a,b)=>out.push(json!({"path":p,"before":a,"after":b,"removed":b.is_none(),"added":a.is_none()}))}
            }
        }
        _ => out.push(json!({"path":path,"before":a,"after":b,"removed":false,"added":false})),
    }
}
/// A sheet write is bound to a real sheet in the fetched document. No arbitrary
/// top-level config field or server action can be supplied by a host.
pub fn save_sheet(document: &Value, key: &str, value: &Value) -> Result<Value, &'static str> {
    let old = document
        .get("sheets")
        .and_then(|s| s.get(key))
        .ok_or("config_sheet_unknown")?;
    if key.is_empty() || key.len() > 128 {
        return Err("config_sheet_unknown");
    }
    if is_rule_set_sheet(document, key) {
        if !value.is_array() && !is_rule_set_map(value) {
            return Err("config_sheet_shape_invalid");
        }
    } else if !value.is_array() || !old.is_array() {
        return Err("config_sheet_shape_invalid");
    }
    let rows: Vec<&Value> = if let Some(a) = value.as_array() {
        a.iter().collect()
    } else {
        value
            .as_object()
            .into_iter()
            .flatten()
            .flat_map(|(_, v)| entry_rows(v))
            .collect()
    };
    if rows.len() > 10000 || rows.iter().any(|v| !v.is_object()) {
        return Err("config_sheet_shape_invalid");
    }
    let prepared = if is_rule_set_sheet(document, key) && value.is_array() {
        with_rows(value, DEFAULT_RULE_SET, entry_rows(value).to_vec())
    } else {
        value.clone()
    };
    let value = &prepared;
    let mut compact = if is_rule_set_sheet(document, key) && is_rule_set_map(value) {
        let mut out = value.clone();
        for name in keys(value) {
            out = with_rows(&out, &name, compact_rows(entry_rows(&value[&name])))
        }
        out
    } else {
        compact_sheet(key, value)
    };
    if is_parameter_sheet(key) {
        for row in compact.as_array_mut().ok_or("config_sheet_shape_invalid")? {
            let name = canonical_parameter_name(
                row["parameter"]
                    .as_str()
                    .ok_or("config_parameter_unknown")?,
            );
            if name.is_empty() || row.get("value").is_none() {
                return Err("config_parameter_unknown");
            }
            row["parameter"] = json!(name);
            // The persistence contract already coerces these authored strings.
            // Prepare them here so every host verifies the bytes that are stored.
            if let Some(text) = row["value"].as_str() {
                let text = text.trim().to_lowercase();
                let value = match text.as_str() {
                    "true" | "yes" => Some(json!(true)),
                    "false" | "no" => Some(json!(false)),
                    _ => text.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| {
                        if v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_991.0 {
                            json!(v as i64)
                        } else {
                            json!(v)
                        }
                    }),
                };
                if let Some(value) = value {
                    row["value"] = value;
                }
            }
        }
    }
    if key == "project_settings" {
        governed_project_settings(&compact)?;
    }
    if serde_json::to_vec(&compact)
        .map_err(|_| "config_sheet_shape_invalid")?
        .len()
        > MAX_INPUT_BYTES
    {
        return Err("config_input_too_large");
    }
    Ok(compact)
}
/// Every published default a project_settings row may author must still
/// resolve: the map printing policy and the project context tolerance.
fn governed_project_settings(rows: &Value) -> Result<(), &'static str> {
    let settings = json!({"project_settings": rows});
    crate::printing::map_policy::resolve(&settings).map_err(|_| "config_parameter_invalid")?;
    crate::project_dataset_cache::context_tolerance_m(&settings)
        .map_err(|_| "config_parameter_invalid")?;
    Ok(())
}
pub fn set_parameter(
    document: &Value,
    key: &str,
    parameter: &str,
    raw: &Value,
) -> Result<Value, &'static str> {
    if !is_parameter_sheet(key) {
        return Err("config_parameter_sheet_required");
    }
    let mut rows = document["sheets"][key]
        .as_array()
        .cloned()
        .ok_or("config_sheet_unknown")?;
    let canonical = canonical_parameter_name(parameter);
    let indices: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r["parameter"]
                .as_str()
                .is_some_and(|s| canonical_parameter_name(s) == canonical)
        })
        .map(|(i, _)| i)
        .collect();
    if indices.is_empty()
        && key == "project_settings"
        && (crate::printing::map_policy::is_governed_parameter(&canonical)
            || crate::project_dataset_cache::is_governed_parameter(&canonical))
    {
        let value = if canonical == crate::printing::map_policy::INTERVAL
            && raw
                .as_str()
                .is_some_and(|s| matches!(s.trim(), "automatic" | "null"))
        {
            Value::Null
        } else {
            scalar(raw)
        };
        rows.push(json!({"parameter":canonical,"value":value}));
        let saved = save_sheet(document, key, &json!(rows))?;
        governed_project_settings(&saved)?;
        return Ok(saved);
    }
    if indices.len() != 1 {
        return Err("config_parameter_unknown");
    }
    let model = member_model(document, key, "parameters", parameter);
    let parsed = if canonical == crate::printing::map_policy::INTERVAL
        && raw
            .as_str()
            .is_some_and(|s| matches!(s.trim(), "automatic" | "null"))
    {
        Value::Null
    } else {
        scalar(raw)
    };
    let raw = &parsed;
    let value = if matches!(
        canonical.as_str(),
        "survey_forms_in_transformer_exports" | "transformer_export_survey_forms"
    ) {
        json!(string_list(raw))
    } else {
        match model["control"].as_str() {
            Some("checkbox_map") => checkbox_map(
                raw,
                model["options"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            ),
            Some("multi_select" | "multiselect") => json!(string_list(raw)),
            Some("checkbox" | "toggle") => json!(enabled(raw)),
            _ => scalar(raw),
        }
    };
    rows[indices[0]]["value"] = value;
    let saved = save_sheet(document, key, &json!(rows))?;
    if key == "project_settings" {
        governed_project_settings(&saved)?;
    }
    Ok(saved)
}

/// The numeric/boolean grammar used by processor presets. Text Settings controls
/// deliberately retain arbitrary strings; process settings admit only flags/numbers.
/// Sheets whose save the gateway fences atomically against the rows the edit
/// started from (`expected_data`; a moved sheet answers 409). The deployed
/// gateway fences `project_settings` only; another sheet cannot carry a
/// fence until the gateway fences it, so its save stays last-writer-wins.
pub const FENCED_SAVE_SHEETS: &[&str] = &["project_settings"];

pub fn save_fence(sheet_key: &str, expected: Value) -> Value {
    if expected.is_null() || !FENCED_SAVE_SHEETS.contains(&sheet_key) {
        return Value::Null;
    }
    expected
}

pub fn process_setting_value(raw: &Value) -> Option<Value> {
    match raw {
        Value::Bool(b) => Some(Value::Bool(*b)),
        Value::Number(n) => n.as_f64().filter(|v| v.is_finite()).map(|_| raw.clone()),
        Value::String(s) => {
            let text = s.trim().to_ascii_lowercase();
            match text.as_str() {
                "true" | "yes" | "on" => Some(Value::Bool(true)),
                "false" | "no" | "off" => Some(Value::Bool(false)),
                "" => None,
                _ => text
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(|v| json!(v)),
            }
        }
        _ => None,
    }
}

/// JSON object order and a small exact numeric value's textual notation do not
/// represent edits. Large distinct integer identifiers never collapse via f64.
pub fn settings_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            a == b
                || match (a.as_f64(), b.as_f64()) {
                    (Some(a), Some(b)) => a == b && a.abs() <= 9_007_199_254_740_991.0,
                    _ => false,
                }
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| settings_equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| settings_equal(a, b)))
        }
        _ => a == b,
    }
}

// ── Design data path ──────────────────────────────────────────────────────
// Which store the project's design data lives in: the Firestore documents
// (`firestore`) or the mirrored dataset the Combined column reflects
// (`mirrored`). The explicit parameter wins; two legacy spellings answer for
// projects that never had it; absent means Firestore.
//
// ds-brain keeps its own `useFirestoreDesignDataFromConfig` twin. That is NOT
// a duplicate to delete: it is the grant-side fence, evaluated where the caller
// cannot reach, and a client answer is never a substitute for it. This op is
// the CLIENT answer — one of them, instead of the browser's and the CLI's.

const DESIGN_DATA_EXPLICIT: &str = "use_firestore_design_data";
const DESIGN_DATA_LEGACY: [&str; 2] = ["design_data_source", "combined_source"];

/// `trim().toLowerCase()`, every separator run collapsed to one `_`, trimmed.
///
/// The host normalised in THREE passes — `/[^\w]+/g → '_'`, then `/_+/g → '_'`,
/// then a trim — and the middle one is load-bearing: `_` is `\w`, so a spelling
/// that already carried one next to a space (`Design_ Data Source`) or doubled
/// one (`use__firestore__design_data`) reached the second pass as a RUN of
/// underscores and left it as a single one. Keeping only the first pass answers
/// `design__data_source`, which matches no key, and the project silently falls
/// through to the next alias or to the default — a store switch nobody declared.
/// Treating `_` as a separator here is both passes at once.
fn config_token(value: &Value) -> String {
    let raw = match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let lowered = raw.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut pending = false;
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.push(ch);
        } else {
            pending = true;
        }
    }
    out
}

fn config_boolean(value: &Value) -> Option<bool> {
    if let Some(flag) = value.as_bool() {
        return Some(flag);
    }
    if let Some(number) = value.as_f64() {
        return Some(number != 0.0);
    }
    match config_token(value).as_str() {
        "true" | "yes" | "y" | "1" | "on" | "firestore" | "fs" => Some(true),
        "false" | "no" | "n" | "0" | "off" | "bigquery" | "bq" => Some(false),
        _ => None,
    }
}

/// The LAST raw key that normalises to the wanted spelling answers, not the
/// first. The deleted host built `new Map(Object.keys(row).map(...))` and read
/// it back, and a `Map` keeps the last write: a document carrying both
/// `Design Data Source` and `design_data_source` was read from the second one.
/// Returning the first instead silently answers from the row the operator
/// replaced.
fn matching_row_indexed(row: &Value, keys: &[&str]) -> Option<(usize, Value)> {
    let object = row.as_object()?;
    for (index, key) in keys.iter().enumerate() {
        let wanted = config_token(&Value::String((*key).to_string()));
        let found = object
            .iter()
            .rfind(|(raw, _)| config_token(&Value::String((*raw).clone())) == wanted);
        if let Some((_, value)) = found {
            return Some((index, value.clone()));
        }
    }
    None
}

fn matching_row_value(row: &Value, keys: &[&str]) -> Option<Value> {
    matching_row_indexed(row, keys).map(|(_, value)| value)
}

fn config_rows(value: Option<Value>) -> Vec<Value> {
    value
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.is_object())
        .collect()
}

fn project_settings_rows(config: &Value) -> Vec<Value> {
    let settings_keys = ["project_settings", "project settings"];
    let mut rows = config_rows(matching_row_value(config, &settings_keys));
    for nested in ["sheets", "parameters", "config", "data"] {
        if let Some(value) = config.get(nested) {
            rows.extend(config_rows(matching_row_value(value, &settings_keys)));
        }
    }
    rows
}

/// The index of the spelling that answered, with its value.
///
/// The walk is SOURCE-major and spelling-minor, exactly as the deleted
/// `configParameterValue` was: every alias is looked for in one source before
/// the next source is opened. Asking spelling-major instead would let a legacy
/// alias buried in `parameters` outrank a newer one written at the top level —
/// the precedence chain a config read must never reorder.
fn config_parameter_indexed(config: &Value, parameters: &[&str]) -> Option<(usize, Value)> {
    if !config.is_object() {
        return None;
    }
    let mut direct = vec![config.clone()];
    for key in ["parameters", "settings", "project_settings"] {
        if let Some(value) = config.get(key) {
            direct.push(value.clone());
        }
    }
    for source in &direct {
        if let Some(found) = matching_row_indexed(source, parameters) {
            return Some(found);
        }
    }
    for row in project_settings_rows(config) {
        let parameter = matching_row_value(&row, &["parameter", "setting", "name", "key"])
            .unwrap_or(Value::Null);
        let parameter_token = config_token(&parameter);
        let Some(index) = parameters.iter().position(|candidate| {
            config_token(&Value::String((*candidate).to_string())) == parameter_token
        }) else {
            continue;
        };
        if let Some(value) =
            matching_row_value(&row, &["value", "setting_value", "default", "enabled"])
        {
            return Some((index, value));
        }
    }
    None
}

fn config_parameter_value(config: &Value, parameters: &[&str]) -> Option<Value> {
    config_parameter_indexed(config, parameters).map(|(_, value)| value)
}

pub fn design_data_path(document: &Value) -> Value {
    let explicit = config_parameter_value(document, &[DESIGN_DATA_EXPLICIT]);
    if let Some(firestore) = explicit.as_ref().and_then(config_boolean) {
        return json!({
            "path": if firestore { "firestore" } else { "mirrored" },
            "source": "explicit",
            "parameter": DESIGN_DATA_EXPLICIT,
            "reason_key": if firestore { "design_data_path_firestore_declared" } else { "design_data_path_mirrored_declared" },
        });
    }
    // ONE lookup over both spellings, because that is what the host asked:
    // `configParameterValue(config, ['design_data_source', 'combined_source'])`.
    // The value it finds decides or it does not; `parsedLegacy ?? true` never
    // reached past it to a second alias, and a config read that invents a
    // longer precedence chain than the documents were written against answers
    // for rows nobody declared.
    if let Some((index, value)) = config_parameter_indexed(document, &DESIGN_DATA_LEGACY) {
        if let Some(firestore) = config_boolean(&value) {
            return json!({
                "path": if firestore { "firestore" } else { "mirrored" },
                "source": "legacy",
                "parameter": DESIGN_DATA_LEGACY[index],
                "reason_key": if firestore { "design_data_path_firestore_legacy" } else { "design_data_path_mirrored_legacy" },
            });
        }
    }
    json!({
        "path": "firestore",
        "source": "default",
        "parameter": "",
        "reason_key": "design_data_path_firestore_default",
    })
}

#[cfg(test)]
mod design_data_path_tests {
    use super::*;

    fn path(document: Value) -> Value {
        design_data_path(&document)
    }

    #[test]
    fn a_separator_beside_an_underscore_still_names_the_same_parameter() {
        // The host collapsed underscore RUNS too (`/_+/g → '_'`), so every
        // spelling below reached it as `use_firestore_design_data`. A token
        // that keeps the run answers for no key at all, and the project falls
        // through to a store it never declared.
        for spelling in [
            "Use_ Firestore Design Data",
            "use__firestore__design_data",
            "USE _ FIRESTORE _ DESIGN _ DATA",
            "  use_firestore_design_data  ",
        ] {
            let document = json!({ spelling: false });
            let result = path(document);
            assert_eq!(result["path"], "mirrored", "{spelling}");
            assert_eq!(result["source"], "explicit", "{spelling}");
        }
        // The same collapse decides a legacy alias and a row-shaped setting.
        let rows = json!({
            "project_settings": [{ "Parameter": "design__data_source", "Value": "bigquery" }],
        });
        assert_eq!(path(rows)["path"], "mirrored");
        // And a legacy alias spelt with the same seam still names itself.
        let legacy = path(json!({ "Combined_ Source": "bigquery" }));
        assert_eq!(legacy["path"], "mirrored");
        assert_eq!(legacy["source"], "legacy");
        assert_eq!(legacy["parameter"], "combined_source");
    }

    #[test]
    fn two_spellings_of_one_key_in_one_object_answer_from_the_last() {
        // `new Map(Object.keys(row).map(...))` keeps the LAST write, so the
        // host read the second spelling. Reading the first answers from the row
        // the operator replaced rather than the one they wrote.
        let document = json!({
            "Use Firestore Design Data": true,
            "use_firestore_design_data": false,
        });
        let result = path(document);
        assert_eq!(result["path"], "mirrored");
        assert_eq!(result["source"], "explicit");
    }

    #[test]
    fn the_explicit_parameter_wins_over_every_legacy_spelling() {
        let document = json!({
            "use_firestore_design_data": false,
            "design_data_source": "firestore",
            "combined_source": "firestore",
        });
        let result = path(document);
        assert_eq!(result["path"], "mirrored");
        assert_eq!(result["source"], "explicit");
    }

    #[test]
    fn legacy_aliases_answer_in_order_and_absence_defaults_to_firestore() {
        assert_eq!(
            path(json!({"design_data_source": "bigquery"}))["path"],
            "mirrored"
        );
        assert_eq!(
            path(json!({"combined_source": "BQ"}))["parameter"],
            "combined_source"
        );
        assert_eq!(
            path(json!({}))["reason_key"],
            "design_data_path_firestore_default"
        );
        assert_eq!(path(Value::Null)["path"], "firestore");
    }

    #[test]
    fn the_value_is_found_in_a_project_settings_sheet_row() {
        let document = json!({
            "sheets": {"project_settings": [
                {"parameter": "unrelated", "value": "x"},
                {"Parameter": "Use Firestore Design Data", "Value": "no"},
            ]},
        });
        let result = path(document);
        assert_eq!(result["path"], "mirrored");
        assert_eq!(result["source"], "explicit");
    }

    #[test]
    fn a_legacy_alias_never_outranks_the_source_it_was_found_after() {
        // The deleted host asked both spellings of ONE source before opening
        // the next: `combined_source` at the top level answers even though
        // `design_data_source` sits in `parameters`. Asking spelling-major
        // would have answered `mirrored` for a project that declared
        // Firestore where the operator could see it.
        let document = json!({
            "parameters": {"design_data_source": "bigquery"},
            "combined_source": "firestore",
        });
        let result = path(document);
        assert_eq!(result["path"], "firestore");
        assert_eq!(result["parameter"], "combined_source");
    }

    #[test]
    fn an_unparseable_legacy_value_answers_the_default_not_the_next_alias() {
        // `parsedLegacy ?? true`: the legacy lookup returned ONE value, and an
        // unreadable one left the default standing. Reaching past it to a
        // second alias decides for a project on a row its author never meant
        // to be read.
        let result = path(json!({"design_data_source": "maybe", "combined_source": "off"}));
        assert_eq!(result["path"], "firestore");
        assert_eq!(result["source"], "default");
    }

    #[test]
    fn an_unparseable_token_falls_through_rather_than_deciding() {
        let document = json!({"use_firestore_design_data": "maybe", "combined_source": "off"});
        let result = path(document);
        assert_eq!(result["path"], "mirrored");
        assert_eq!(result["source"], "legacy");
        assert_eq!(result["parameter"], "combined_source");
    }
}

#[cfg(test)]
mod governed_map_policy_tests {
    use super::*;
    #[test]
    fn first_authored_map_setting_is_bounded_and_preserves_held_input() {
        let original =
            json!({"sheets":{"project_settings":[{"parameter":"report_locale","value":"fr"}]}});
        let updated = set_parameter(
            &original,
            "project_settings",
            "keep_all_labels_at_high_density",
            &json!("true"),
        )
        .unwrap();
        assert_eq!(
            original["sheets"]["project_settings"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            crate::printing::map_policy::resolve(&json!({"project_settings":updated}))
                .unwrap()
                .keep_all_labels_at_high_density
        );
        for (key, raw) in [
            ("invented_print_setting", "true"),
            ("print_metric_grid_interval_m", "0"),
            ("print_metric_grid_enabled", "nonsense"),
        ] {
            assert!(set_parameter(&original, "project_settings", key, &json!(raw)).is_err());
        }
    }
}

#[cfg(test)]
#[path = "save_fence_tests.rs"]
mod save_fence_tests;

#[cfg(test)]
#[path = "governed_settings_tests.rs"]
mod governed_settings_tests;
