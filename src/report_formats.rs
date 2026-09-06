//! Report output policy shared by native, WASM and the reporter. No IO.
use serde::Serialize;
use serde_json::Value;

pub const DEFAULT_INDIVIDUAL: &[&str] = &["shp", "kmz", "xlsx"];
pub const INDIVIDUAL: &[&str] = &["shp", "kmz", "xlsx", "gpkg", "pdf_a0", "pdf_a3", "geojson"];
pub const COMBINED: &[&str] = &["shp", "xlsx", "geojsonseq"];
pub const OPTIONAL_COMBINED: &[&str] = &["gpkg", "pdf_a0", "pdf_a3"];

/// Available vector-print presets. Selection is a set, never a single size.
/// The renderer uses this same type; adding a preset here requires exhaustive
/// dimensions and output metadata rather than a UI-only promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintPageSize {
    A0,
    A3,
}
impl PrintPageSize {
    pub const ALL: &[Self] = &[Self::A0, Self::A3];
    pub fn format(self) -> &'static str {
        match self {
            Self::A0 => "pdf_a0",
            Self::A3 => "pdf_a3",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::A0 => "PDF A0",
            Self::A3 => "PDF A3",
        }
    }
    pub fn millimetres(self) -> (f64, f64) {
        match self {
            Self::A0 => (1189.0, 841.0),
            Self::A3 => (420.0, 297.0),
        }
    }
}

pub fn named_setup_id(format: &str) -> Option<&str> {
    format
        .strip_prefix("pdf__")
        .filter(|id| crate::printing::identifier(id))
}

pub fn options(setups: &[Value]) -> Result<Vec<Value>, String> {
    if setups.len() > 200 {
        return Err("Printing setup catalog exceeds 200 entries".into());
    }
    let mut result: Vec<Value> = INDIVIDUAL.iter().filter(|f| !f.starts_with("pdf_")).map(|f|
        serde_json::json!({"value":f,"label":if *f == "geojson" { "GeoJSON".into() } else { f.to_ascii_uppercase() }})
    ).collect();
    let mut ids = std::collections::BTreeSet::new();
    for setup in setups {
        let id = setup["id"]
            .as_str()
            .ok_or("Printing setup ID is required")?;
        let name = setup["name"]
            .as_str()
            .ok_or("Printing setup name is required")?;
        if !crate::printing::identifier(id)
            || !ids.insert(id)
            || name.trim().is_empty()
            || name.len() > 160
        {
            return Err("Invalid or duplicate printing setup".into());
        }
        result.push(
            serde_json::json!({"value":format!("pdf__{id}"),"label":format!("PDF · {name}")}),
        );
    }
    Ok(result)
}

pub fn named_layouts(
    sheets: &Value,
) -> Result<std::collections::BTreeMap<String, crate::printing::Layout>, String> {
    let mut layouts = std::collections::BTreeMap::new();
    if let Some(raw) = sheets.get("printing_setups") {
        let rows = raw.as_array().ok_or("printing_setups must be an array")?;
        if rows.len() > 200 {
            return Err("Printing setup catalog exceeds 200 entries".into());
        }
        for row in rows {
            let layout: crate::printing::Layout =
                serde_json::from_value(row["layout"].clone()).map_err(|e| e.to_string())?;
            crate::printing::validate(&layout)?;
            if row["id"].as_str() != Some(layout.id.as_str())
                || row["revision"]
                    .as_str()
                    .is_none_or(|r| r.len() != 64 || !r.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                return Err("Printing setup identity or revision is invalid".into());
            }
            if layouts.insert(layout.id.clone(), layout).is_some() {
                return Err("Duplicate printing setup".into());
            }
        }
    }
    Ok(layouts)
}
pub const SETTING_ALIASES: &[&str] = &[
    "design_export_format",
    "design_export_formats",
    "transformer_export_formats",
    "tr_export_formats",
    "report_formats",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub output_id: String,
    pub format: String,
    pub content_type: &'static str,
    pub suffix: String,
}

pub fn output(format: &str) -> Result<Output, String> {
    if let Some(id) = named_setup_id(format) {
        return Ok(Output {
            output_id: format.into(),
            format: format.into(),
            content_type: "application/pdf",
            suffix: format!(".{id}.pdf"),
        });
    }
    let (content_type, suffix) = match format {
        "shp" => ("application/zip", ".shp.zip"),
        "kmz" => ("application/vnd.google-earth.kmz", ".kmz"),
        "xlsx" => (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ".xlsx",
        ),
        "gpkg" => ("application/geopackage+sqlite3", ".gpkg"),
        "pdf_a0" => ("application/pdf", ".a0.pdf"),
        "pdf_a3" => ("application/pdf", ".a3.pdf"),
        "geojson" => ("application/zip", "_geojson.zip"),
        "geojsonseq" => ("application/zip", "_geojsonl.zip"),
        "zip" => ("application/zip", ".zip"),
        _ => return Err(format!("Unsupported report format: {format}")),
    };
    Ok(Output {
        output_id: format.into(),
        format: if format == "geojson" {
            "geojsonzip".into()
        } else {
            format.into()
        },
        content_type,
        suffix: suffix.into(),
    })
}

pub fn normalize(values: &[String]) -> Vec<String> {
    let selected: Vec<_> = values
        .iter()
        .map(|value| {
            if named_setup_id(value.trim()).is_some() {
                return value.trim().to_string();
            }
            let lower = value.trim().to_ascii_lowercase();
            match lower.as_str() {
                "shp.zip" | "shapefile" => "shp".into(),
                "json" | "geojson.zip" => "geojson".into(),
                "excel" => "xlsx".into(),
                "pdf" | "a0_pdf" => "pdf_a0".into(),
                "a3_pdf" => "pdf_a3".into(),
                _ => lower,
            }
        })
        .collect();
    let mut ordered: Vec<_> = INDIVIDUAL
        .iter()
        .filter(|format| selected.iter().any(|v| v == **format))
        .map(|s| s.to_string())
        .collect();
    let named: std::collections::BTreeSet<_> = selected
        .iter()
        .filter(|f| named_setup_id(f).is_some())
        .cloned()
        .collect();
    ordered.extend(named);
    // Preserve the established absent/empty policy default. PDF is always opt-in.
    if ordered.is_empty() {
        DEFAULT_INDIVIDUAL.iter().map(|s| s.to_string()).collect()
    } else {
        ordered
    }
}

pub fn combined(selected: &[String]) -> Vec<&str> {
    COMBINED
        .iter()
        .copied()
        .chain(
            OPTIONAL_COMBINED
                .iter()
                .copied()
                .filter(|f| selected.iter().any(|s| s == f)),
        )
        .chain(
            selected
                .iter()
                .filter(|f| named_setup_id(f).is_some())
                .map(String::as_str),
        )
        .collect()
}

pub fn string_list(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().flat_map(string_list).collect(),
        Value::Object(items) => items
            .iter()
            .filter(|(_, enabled)| match enabled {
                Value::Bool(b) => *b,
                Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
                Value::String(s) => matches!(
                    s.trim().to_ascii_lowercase().as_str(),
                    "true" | "yes" | "1" | "y" | "on"
                ),
                _ => false,
            })
            .map(|(key, _)| key.clone())
            .collect(),
        Value::String(s) => {
            if s.trim().starts_with('[') {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    return string_list(&parsed);
                }
            }
            s.split([',', ';', '\n', '\r'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        }
        Value::Null => Vec::new(),
        other => vec![other.to_string()],
    }
}

pub fn plan(sheets: &Value, is_combined: bool) -> Result<Vec<Output>, String> {
    if !sheets.is_object() {
        return Err("Report sheets must be an object".into());
    }
    if sheets.get("parameters").is_some_and(|v| !v.is_object()) {
        return Err("Report parameters must be an object".into());
    }
    let top = sheets.get("project_settings");
    let nested = sheets
        .get("parameters")
        .and_then(|v| v.get("project_settings"));
    for rows in [top, nested].into_iter().flatten() {
        if !rows.is_array() {
            return Err("Report project_settings must be a row list".into());
        }
        for row in rows.as_array().unwrap() {
            if !row.is_object()
                || row
                    .get("parameter")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
            {
                return Err(
                    "Report project setting must be an object with a text parameter".into(),
                );
            }
        }
    }
    let rows = top
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .or_else(|| nested.and_then(Value::as_array));
    let raw = rows
        .and_then(|rows| {
            rows.iter().find(|row| {
                row["parameter"].as_str().is_some_and(|s| {
                    SETTING_ALIASES
                        .contains(&s.trim().to_ascii_lowercase().replace('-', "_").as_str())
                })
            })
        })
        .map(|row| string_list(&row["value"]))
        .unwrap_or_default();
    let selected = normalize(&raw);
    let layouts = named_layouts(sheets)?;
    for format in &selected {
        if let Some(id) = named_setup_id(format) {
            if !layouts.contains_key(id) {
                return Err(format!(
                    "Selected printing setup {id} is missing; refresh project printing inputs"
                ));
            }
        }
    }
    let formats = if is_combined {
        combined(&selected).iter().map(|s| s.to_string()).collect()
    } else {
        selected
    };
    if formats.len() > 32 {
        return Err("Report exceeds 32 selected output artifacts".into());
    }
    formats.iter().map(|f| output(f)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn named_setups_keep_identity_across_renames_and_require_sealed_layouts() {
        use crate::printing::{named_layout, Orientation, Paper};
        let layout = named_layout(
            Some("detail".into()),
            Some("Detail portrait".into()),
            Paper::A4,
            Orientation::Portrait,
        )
        .unwrap();
        let mut sheets = json!({"project_settings":[{"parameter":"design_export_format","value":["xlsx","pdf__detail"]}],"printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":layout}]});
        let outputs = plan(&sheets, false).unwrap();
        assert_eq!(
            outputs
                .iter()
                .map(|o| o.output_id.as_str())
                .collect::<Vec<_>>(),
            ["xlsx", "pdf__detail"]
        );
        assert_eq!(outputs[1].suffix, ".detail.pdf");
        sheets["printing_setups"][0]["layout"]["name"] = json!("Renamed");
        assert_eq!(plan(&sheets, false).unwrap(), outputs);
        assert_eq!(
            named_layouts(&sheets).unwrap()["detail"].page_mm,
            [210., 297.]
        );
        sheets["printing_setups"] = json!([]);
        assert!(plan(&sheets, false).unwrap_err().contains("missing"));
        assert!(named_setup_id("pdf__../outside").is_none());
        let options = options(&[json!({"id":"detail","name":"Renamed"})]).unwrap();
        assert!(options
            .iter()
            .any(|o| o["value"] == "pdf__detail" && o["label"] == "PDF · Renamed"));
        assert!(!options.iter().any(|o| o["value"] == "pdf_a0"));
    }

    #[test]
    fn pdf_selection_controls_both_report_shapes_and_disabled_legacy_settings_cannot_enable_it() {
        let sheets = json!({"project_settings":[
            {"parameter":"design_export_format", "value":{"pdf_a3":true,"pdf_a0":false,"xlsx":true}},
            {"parameter":"print_individual_page_sizes","value":["a0"]},
            {"parameter":"print_combined_page_sizes","value":["a0"]}
        ]});
        assert_eq!(
            plan(&sheets, false)
                .unwrap()
                .iter()
                .map(|o| o.output_id.as_str())
                .collect::<Vec<_>>(),
            ["xlsx", "pdf_a3"]
        );
        assert_eq!(
            plan(&sheets, true)
                .unwrap()
                .iter()
                .map(|o| o.output_id.as_str())
                .collect::<Vec<_>>(),
            ["shp", "xlsx", "geojsonseq", "pdf_a3"]
        );
        assert_eq!(output("pdf_a3").unwrap().content_type, "application/pdf");
    }
    #[test]
    fn settings_shapes_and_output_order_are_stable() {
        for value in [
            json!("A3_PDF;excel"),
            json!(["pdf_a3", "xlsx"]),
            json!({"pdf_a3":"yes","xlsx":true}),
        ] {
            let sheets = json!({"parameters":{"project_settings":[{"parameter":"design-export-formats","value":value}]}});
            assert_eq!(
                plan(&sheets, false)
                    .unwrap()
                    .iter()
                    .map(|o| o.output_id.as_str())
                    .collect::<Vec<_>>(),
                ["xlsx", "pdf_a3"]
            );
        }
        assert!(plan(&json!({"project_settings":{}}), false).is_err());
    }
}
