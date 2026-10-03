//! One data-driven catalogue for customer report messages and header aliases.
//! Locale belongs to the Network Template's existing project_settings sheet.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const LOCALE_SETTING: &str = "report_locale";
pub const SCHEMA: &str = "ds.report-messages/v1";
pub const REFUSAL_CODES: &[&str] = &[
    "report_locale_missing",
    "report_locale_invalid",
    "report_locale_unsupported",
    "report_catalogue_invalid",
    "report_catalogue_incomplete",
    "report_message_missing",
    "report_header_alias_invalid",
];
pub fn refusal_code(message: &str) -> Option<&'static str> {
    message.lines().find_map(|line| {
        let line = line
            .trim()
            .strip_prefix("ds-report: ")
            .unwrap_or(line.trim());
        REFUSAL_CODES
            .iter()
            .copied()
            .find(|code| line.starts_with(&format!("{code}:")))
    })
}
pub const DEFAULT_CATALOGUE: &str = include_str!("../data/report-messages.json");
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub schema: String,
    pub locales: BTreeMap<String, BTreeMap<String, String>>,
    pub headers: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Protocol IDs, selectors and enum literals in authored report scripts.
    #[serde(default)]
    pub script_code_literals: std::collections::BTreeSet<String>,
}
#[derive(Debug, Clone)]
pub struct Messages {
    pub locale: String,
    catalogue: Catalogue,
}
impl Messages {
    pub fn new(locale: Option<&str>, catalogue: Option<&Value>) -> Result<Self, String> {
        let locale = locale.filter(|s| !s.trim().is_empty()).ok_or(
            "report_locale_missing: set report_locale in the project's project_settings sheet",
        )?;
        let catalogue: Catalogue = match catalogue {
            Some(value) => serde_json::from_value(value.clone()),
            None => serde_json::from_str(DEFAULT_CATALOGUE),
        }
        .map_err(|e| format!("report_catalogue_invalid: {e}"))?;
        if catalogue.schema != SCHEMA || !catalogue.locales.contains_key("en") {
            return Err(
                "report_catalogue_invalid: version or English reference locale missing".into(),
            );
        }
        let _selected = catalogue.locales.get(locale)
            .ok_or_else(|| format!("report_locale_unsupported: {locale}: supply a complete locale in the report catalogue"))?;
        if !default_catalogue()
            .script_code_literals
            .is_subset(&catalogue.script_code_literals)
        {
            return Err(
                "report_catalogue_incomplete: retain the template's protected script literals"
                    .into(),
            );
        }
        let english = &catalogue.locales["en"];
        if default_catalogue().locales["en"]
            .keys()
            .any(|key| !english.contains_key(key))
        {
            return Err(
                "report_catalogue_incomplete: English reference must retain every governed message"
                    .into(),
            );
        }
        if catalogue.locales.values().any(|messages| {
            messages.keys().ne(english.keys()) || messages.values().any(|s| s.is_empty())
        }) {
            return Err(format!(
                "report_catalogue_incomplete: {locale}: every reference message needs a translation"
            ));
        }
        let placeholder = regex::Regex::new(r"\{[a-zA-Z_][a-zA-Z_0-9]*\}").unwrap();
        let placeholders = |text: &str| -> Vec<String> {
            let mut tokens: Vec<_> = placeholder
                .find_iter(text)
                .map(|m| m.as_str().to_owned())
                .collect();
            tokens.sort();
            tokens
        };
        for (language, messages) in &catalogue.locales {
            for (key, original) in english {
                if placeholders(original) != placeholders(&messages[key]) {
                    return Err(format!(
                        "report_catalogue_incomplete: {language}.{key}: placeholders must be preserved"
                    ));
                }
            }
        }
        Ok(Self {
            locale: locale.into(),
            catalogue,
        })
    }
    pub fn script_literal_is_code(&self, literal: &str) -> bool {
        self.catalogue.script_code_literals.contains(literal)
    }
    pub fn is_default_english(&self) -> bool {
        self.locale == "en" && self.catalogue.locales["en"] == default_catalogue().locales["en"]
    }
    pub fn get(&self, key: &str) -> Result<&str, String> {
        self.catalogue.locales[&self.locale]
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| {
                format!("report_message_missing: {key}: add it to every locale in the catalogue")
            })
    }
    /// API/template-authored message. Unknown project data remains exact;
    /// callers must invoke this only for presentation labels, never identities.
    pub fn label<'a>(&'a self, english: &'a str) -> &'a str {
        self.catalogue.locales[&self.locale]
            .get(english)
            .map(String::as_str)
            .unwrap_or(english)
    }
    pub fn header(&self, field: &str) -> Option<&str> {
        self.catalogue.locales[&self.locale]
            .get(&format!("header.{field}"))
            .map(String::as_str)
    }
    pub fn header_alias<'a>(&'a self, layer: &str, field: &'a str) -> &'a str {
        header_alias(&self.catalogue.headers, layer, field)
    }
    /// Template messages substitute named values without interpreting project data.
    pub fn format(&self, key: &str, values: &[(&str, &str)]) -> Result<String, String> {
        let mut text = self.get(key)?.to_owned();
        for (name, value) in values {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        Ok(text)
    }
    /// Translate a completed presentation sentence using catalogue templates.
    /// Only captured placeholder values (identities, numbers, source citations)
    /// are copied; no locale-dependent engineering calculation is performed.
    pub fn prose(&self, source: &str) -> String {
        if let Some(text) = self.catalogue.locales[&self.locale].get(source) {
            return text.clone();
        }
        let placeholder = regex::Regex::new(r"\{[a-zA-Z_][a-zA-Z_0-9]*\}").unwrap();
        for (key, english) in &self.catalogue.locales["en"] {
            let tokens: Vec<_> = placeholder.find_iter(english).collect();
            if tokens.is_empty() || !key.starts_with("prose.") {
                continue;
            }
            let mut pattern = String::from("(?s)^");
            let mut at = 0;
            for token in &tokens {
                pattern.push_str(&regex::escape(&english[at..token.start()]));
                pattern.push_str("(.*?)");
                at = token.end();
            }
            pattern.push_str(&regex::escape(&english[at..]));
            pattern.push('$');
            let re = regex::Regex::new(&pattern).expect("escaped template");
            if let Some(captures) = re.captures(source) {
                let mut text = self.catalogue.locales[&self.locale][key].clone();
                for (i, token) in tokens.iter().enumerate() {
                    text = text.replace(
                        token.as_str(),
                        &self.prose(captures.get(i + 1).unwrap().as_str()),
                    );
                }
                return text;
            }
        }
        source.to_owned()
    }
    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }
}
pub fn project_locale(sheets: &Value) -> Result<String, String> {
    let rows = sheets
        .get("project_settings")
        .filter(|v| v.as_array().is_some_and(|r| !r.is_empty()))
        .or_else(|| sheets.pointer("/parameters/project_settings"))
        .and_then(Value::as_array)
        .ok_or("report_locale_missing: project_settings missing")?;
    let values: Vec<_> = rows
        .iter()
        .filter(|row| {
            crate::report_formats::setting_parameter_key(row).as_deref() == Some(LOCALE_SETTING)
        })
        .collect();
    if values.len() != 1 {
        return Err(
            "report_locale_missing: exactly one report_locale project setting is required".into(),
        );
    }
    let locale = values[0]["value"]
        .as_str()
        .ok_or("report_locale_invalid: locale must be a nonempty string")?;
    if locale.is_empty()
        || locale.trim() != locale
        || locale.len() > 80
        || !locale
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("report_locale_invalid: use an exact catalogue locale key".into());
    }
    Ok(locale.into())
}
/// Complete catalogue overrides use the existing JSON-valued project-settings
/// parameter, which the project config API actually serves and saves.
pub fn project_catalogue(config: &Value) -> Result<Option<Value>, String> {
    let rows = config
        .get("project_settings")
        .filter(|v| v.as_array().is_some_and(|r| !r.is_empty()))
        .or_else(|| config.pointer("/parameters/project_settings"))
        .and_then(Value::as_array);
    let values: Vec<_> = rows
        .into_iter()
        .flatten()
        .filter(|row| {
            crate::report_formats::setting_parameter_key(row).as_deref() == Some("report_messages")
        })
        .collect();
    if values.len() > 1 {
        return Err("report_catalogue_invalid: duplicate report_messages project setting".into());
    }
    values
        .first()
        .map(|row| match row.get("value") {
            Some(Value::Object(_)) => Ok(row["value"].clone()),
            Some(Value::String(text)) => {
                serde_json::from_str(text).map_err(|e| format!("report_catalogue_invalid: {e}"))
            }
            _ => Err(
                "report_catalogue_invalid: report_messages must be a complete JSON catalogue"
                    .into(),
            ),
        })
        .transpose()
}
/// The same alias function is used by column filtering and workbook scheduling.
/// Header maps are the template's per-layer canonical-field → alias arrays.
pub fn header_alias<'a>(
    headers: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
    layer: &str,
    field: &'a str,
) -> &'a str {
    headers
        .get(layer)
        .into_iter()
        .chain(headers.get("*"))
        .flat_map(|map| map.iter())
        .find_map(|(canonical, aliases)| {
            (canonical == field || aliases.iter().any(|alias| alias == field))
                .then_some(canonical.as_str())
        })
        .unwrap_or(field)
}
/// Read the existing Network Template header-mapping sheets, whether the
/// API flattened them or Firestore grouped them. No consumer owns an alias list.
pub fn template_headers(
    config: &Value,
) -> Result<BTreeMap<String, BTreeMap<String, Vec<String>>>, String> {
    let catalogue = project_catalogue(config)?;
    let mut headers = catalogue
        .map(|v| serde_json::from_value::<Catalogue>(v).map(|c| c.headers))
        .transpose()
        .map_err(|e| format!("report_catalogue_invalid: {e}"))?
        .unwrap_or_else(|| default_catalogue().headers.clone());
    for (layer, sheet) in [
        ("customers", "customers_header"),
        ("lv_lines", "lv_lines_header"),
        ("lv_poles", "lv_poles_header"),
        ("service_cables", "service_cables_header"),
        ("tr", "transformer"),
    ] {
        let Some(rows) = config
            .get(sheet)
            .or_else(|| config.get("header_mapping").and_then(|v| v.get(sheet)))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for row in rows {
            let Some(field) = row.get("cleaned_header").and_then(Value::as_str) else {
                continue;
            };
            if field.trim().is_empty() {
                return Err("report_header_alias_invalid: empty cleaned_header".into());
            }
            let mut aliases = Vec::new();
            fn collect(value: &Value, aliases: &mut Vec<String>) {
                match value {
                    Value::Array(values) => {
                        for v in values {
                            collect(v, aliases);
                        }
                    }
                    Value::String(s) => {
                        for a in s.split('#').map(str::trim).filter(|s| !s.is_empty()) {
                            aliases.push(a.to_owned());
                        }
                    }
                    _ => {}
                }
            }
            if let Some(value) = row.get("misspelled_header") {
                collect(value, &mut aliases);
            }
            let map = headers.entry(layer.into()).or_default();
            for alias in &aliases {
                if map.iter().any(|(canonical, values)| {
                    canonical != field && (canonical == alias || values.contains(alias))
                }) {
                    return Err(format!(
                        "report_header_alias_invalid: {layer}.{alias} maps to multiple fields"
                    ));
                }
            }
            let values = map.entry(field.into()).or_default();
            values.extend(aliases);
            values.sort();
            values.dedup();
        }
    }
    Ok(headers)
}
pub fn default_catalogue() -> &'static Catalogue {
    static CATALOGUE: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
    CATALOGUE.get_or_init(|| serde_json::from_str(DEFAULT_CATALOGUE).expect("checked catalogue"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn missing_unknown_and_incomplete_locales_refuse_by_name() {
        assert!(Messages::new(None, None)
            .unwrap_err()
            .starts_with("report_locale_missing:"));
        assert!(Messages::new(Some("unknown"), None)
            .unwrap_err()
            .starts_with("report_locale_unsupported:"));
        let mut catalogue = serde_json::to_value(default_catalogue()).unwrap();
        catalogue["locales"]["fr"]
            .as_object_mut()
            .unwrap()
            .remove("SHEET INDEX");
        assert!(Messages::new(Some("fr"), Some(&catalogue))
            .unwrap_err()
            .starts_with("report_catalogue_incomplete:"));
    }
    #[test]
    fn an_override_cannot_remove_governed_messages_from_every_locale() {
        let mut catalogue = serde_json::to_value(default_catalogue()).unwrap();
        for language in ["en", "fr"] {
            catalogue["locales"][language]
                .as_object_mut()
                .unwrap()
                .remove("SHEET INDEX");
        }
        assert!(Messages::new(Some("en"), Some(&catalogue))
            .unwrap_err()
            .starts_with("report_catalogue_incomplete:"));
    }
    #[test]
    fn catalogue_retains_script_protocol_literals() {
        let messages = Messages::new(Some("fr"), None).unwrap();
        assert!(messages.script_literal_is_code("incomplete"));
        assert!(!messages.script_literal_is_code("Summary and compliance"));
        let mut catalogue = serde_json::to_value(default_catalogue()).unwrap();
        catalogue["script_code_literals"] = json!([]);
        assert!(Messages::new(Some("fr"), Some(&catalogue))
            .unwrap_err()
            .starts_with("report_catalogue_incomplete:"));
    }
    #[test]
    fn a_third_locale_and_template_alias_need_only_data() {
        let mut catalogue = serde_json::to_value(default_catalogue()).unwrap();
        catalogue["locales"]["rw"] = catalogue["locales"]["en"].clone();
        catalogue["locales"]["rw"]["SHEET INDEX"] = json!("INDANGAPAPURO");
        catalogue["headers"]["customers"]["house_number"] = json!(["numero_maison"]);
        let messages = Messages::new(Some("rw"), Some(&catalogue)).unwrap();
        assert_eq!(messages.get("SHEET INDEX").unwrap(), "INDANGAPAPURO");
        assert_eq!(
            messages.header_alias("customers", "numero_maison"),
            "house_number"
        );
    }
    #[test]
    fn served_template_aliases_share_one_function_and_refuse_collisions() {
        let headers = template_headers(&json!({"header_mapping":{"customers_header":[{"cleaned_header":"house_number","misspelled_header":["numero_maison#maison"]}]}})).unwrap();
        assert_eq!(
            header_alias(&headers, "customers", "maison"),
            "house_number"
        );
        assert!(template_headers(&json!({"customers_header":[{"cleaned_header":"a","misspelled_header":["x"]},{"cleaned_header":"b","misspelled_header":["x"]}]})).unwrap_err().starts_with("report_header_alias_invalid:"));
    }
    #[test]
    fn catalogue_override_uses_the_existing_json_parameter() {
        let catalogue = serde_json::to_value(default_catalogue()).unwrap();
        let rows = serde_json::json!([{"parameter":"report_locale","value":"fr"},{"parameter":"report_messages","value":catalogue}]);
        assert_eq!(
            project_catalogue(&serde_json::json!({"project_settings":rows})).unwrap(),
            Some(catalogue.clone())
        );
        assert_eq!(
            project_catalogue(&serde_json::json!({"parameters":{"project_settings":rows}}))
                .unwrap(),
            Some(catalogue)
        );
    }
    #[test]
    fn grouped_and_served_project_settings_have_the_same_locale() {
        let rows = json!([{"parameter":"report_locale","value":"fr"}]);
        assert_eq!(
            project_locale(&json!({"project_settings": rows})).unwrap(),
            "fr"
        );
        assert_eq!(
            project_locale(&json!({"parameters":{"project_settings": rows}})).unwrap(),
            "fr"
        );
    }
}
