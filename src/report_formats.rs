//! Report output policy shared by native, WASM and the reporter. No IO.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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

#[derive(
    Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum PrintArtifactFormat {
    Pdf,
    Png,
    Jpeg,
}

impl PrintArtifactFormat {
    fn prefix(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }
}

/// Decode a named print artifact token while keeping the stable layout id
/// independent from file format. `jpg__id` is read as a JPEG alias but new
/// settings always serialize as `jpeg__id`.
pub fn named_print_output(format: &str) -> Option<(PrintArtifactFormat, &str)> {
    let (kind, id) = [
        (PrintArtifactFormat::Pdf, "pdf__"),
        (PrintArtifactFormat::Png, "png__"),
        (PrintArtifactFormat::Jpeg, "jpeg__"),
        (PrintArtifactFormat::Jpeg, "jpg__"),
    ]
    .into_iter()
    .find_map(|(kind, prefix)| format.strip_prefix(prefix).map(|id| (kind, id)))?;
    crate::printing::identifier(id).then_some((kind, id))
}

pub const OUTPUT_SELECTION_SCHEMA: &str = "ds.design-output-selection/v1";

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NamedPrintSelection {
    pub layout_id: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub formats: Vec<PrintArtifactFormat>,
}

fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DesignOutputSelection {
    pub schema: String,
    #[serde(default)]
    pub prints: Vec<NamedPrintSelection>,
    #[serde(default)]
    pub geospatial: Vec<String>,
    #[serde(default)]
    pub tabular: Vec<String>,
}

impl DesignOutputSelection {
    pub fn tokens(&self) -> Result<Vec<String>, String> {
        if self.schema != OUTPUT_SELECTION_SCHEMA
            || self.prints.len() > 32
            || self.geospatial.len() > 8
            || self.tabular.len() > 8
        {
            return Err("invalid design output selection".into());
        }
        let mut tokens = Vec::new();
        tokens.extend(self.geospatial.iter().cloned());
        tokens.extend(self.tabular.iter().cloned());
        let mut layouts = std::collections::BTreeSet::new();
        for print in &self.prints {
            if !crate::printing::identifier(&print.layout_id)
                || !layouts.insert(&print.layout_id)
                || print.formats.is_empty()
                || print.formats.len() > 3
            {
                return Err("invalid or duplicate named print selection".into());
            }
            let formats = print
                .formats
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            if formats.len() != print.formats.len() {
                return Err("named print formats must be unique".into());
            }
            if print.enabled {
                tokens.extend(
                    formats
                        .into_iter()
                        .map(|format| format!("{}__{}", format.prefix(), print.layout_id)),
                );
            }
        }
        Ok(tokens)
    }
}

/// Normalize the two accepted authoring inputs into the one durable selection.
/// Hosts pass bytes through; format aliases and bounds live here with report
/// planning rather than being reimplemented in TypeScript.
pub fn normalize_output_selection(
    selection: Option<DesignOutputSelection>,
    legacy_formats: Option<Vec<String>>,
) -> Result<DesignOutputSelection, String> {
    if let Some(selection) = selection {
        if legacy_formats.is_some() {
            return Err("pass a versioned selection or legacy formats, not both".into());
        }
        selection.tokens()?;
        return Ok(selection);
    }
    let formats = legacy_formats.ok_or("output selection is required")?;
    if formats.is_empty() || formats.len() > 32 {
        return Err("legacy output formats must contain 1..32 tokens".into());
    }
    let mut prints: BTreeMap<String, BTreeSet<PrintArtifactFormat>> = BTreeMap::new();
    let mut geospatial = Vec::new();
    let mut tabular = Vec::new();
    for raw in formats {
        let token = raw.to_ascii_lowercase();
        if let Some((format, id)) = named_print_output(&token) {
            prints.entry(id.into()).or_default().insert(format);
        } else if token == "xlsx" {
            tabular.push(token);
        } else if token.contains("__") {
            return Err(format!("invalid named print output: {raw}"));
        } else {
            geospatial.push(token);
        }
    }
    let selection = DesignOutputSelection {
        schema: OUTPUT_SELECTION_SCHEMA.into(),
        prints: prints
            .into_iter()
            .map(|(layout_id, formats)| NamedPrintSelection {
                layout_id,
                enabled: true,
                formats: formats.into_iter().collect(),
            })
            .collect(),
        geospatial,
        tabular,
    };
    selection.tokens()?;
    Ok(selection)
}

pub fn output_selection_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(DesignOutputSelection))
        .expect("output selection schema")
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
        let mut option = serde_json::json!({
            "value":format!("pdf__{id}"),
            "label":format!("PDF · {name}"),
            "layout_id":id,
            "formats":["pdf","png","jpeg"]
        });
        if !setup["layout"].is_null() {
            let layout: crate::printing::Layout =
                serde_json::from_value(setup["layout"].clone())
                    .map_err(|e| format!("Invalid printing setup layout: {e}"))?;
            crate::printing::validate(&layout)?;
            option["paper"] = serde_json::to_value(layout.paper).map_err(|e| e.to_string())?;
            option["page_mm"] = serde_json::json!(layout.page_mm);
        }
        result.push(option);
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
    if let Some((kind, id)) = named_print_output(format) {
        let (content_type, suffix) = match kind {
            PrintArtifactFormat::Pdf => ("application/pdf", format!(".{id}.pdf")),
            PrintArtifactFormat::Png => ("image/png", format!(".{id}.png")),
            PrintArtifactFormat::Jpeg => ("image/jpeg", format!(".{id}.jpg")),
        };
        return Ok(Output {
            output_id: format!("{}__{id}", kind.prefix()),
            format: format!("{}__{id}", kind.prefix()),
            content_type,
            suffix,
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
            if let Some((kind, id)) = named_print_output(value.trim()) {
                return format!("{}__{id}", kind.prefix());
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
        .filter(|f| named_print_output(f).is_some())
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

fn project_output_setting(sheets: &Value) -> Result<Option<&Value>, String> {
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
    Ok(rows.and_then(|rows| {
        rows.iter().find(|row| {
            row["parameter"].as_str().is_some_and(|s| {
                SETTING_ALIASES.contains(&s.trim().to_ascii_lowercase().replace('-', "_").as_str())
            })
        })
    }))
}

pub fn inspect_project_settings(sheets: &Value) -> Result<Value, String> {
    let setting = project_output_setting(sheets)?;
    let outputs = plan(sheets, false)?;
    let layouts = named_layouts(sheets)?;
    let papers = outputs.iter().filter_map(|output| {
        let (_, id) = named_print_output(&output.format)?;
        let layout = layouts.get(id)?;
        Some(serde_json::json!({"output_id":output.output_id,"layout_id":id,"name":layout.name,"paper":layout.paper}))
    }).collect::<Vec<_>>();
    Ok(
        serde_json::json!({"source":if setting.is_some(){"project_settings"}else{"report_defaults"},
        "setting":setting,"outputs":outputs,"papers":papers}),
    )
}

pub fn plan(sheets: &Value, is_combined: bool) -> Result<Vec<Output>, String> {
    let raw = project_output_setting(sheets)?
        .map(|row| {
            if row["value"]["schema"] == OUTPUT_SELECTION_SCHEMA {
                serde_json::from_value::<DesignOutputSelection>(row["value"].clone())
                    .map_err(|error| format!("Invalid design output selection: {error}"))?
                    .tokens()
            } else {
                Ok(string_list(&row["value"]))
            }
        })
        .transpose()?
        .unwrap_or_default();
    let selected = normalize(&raw);
    let layouts = named_layouts(sheets)?;
    for format in &selected {
        if let Some((_, id)) = named_print_output(format) {
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
    fn settings_inspection_uses_saved_selection_and_actual_paper_not_names() {
        use crate::printing::{named_layout, Orientation, Paper};
        let layout = named_layout(
            Some("a0-looking-name".into()),
            None,
            Paper::A3,
            Orientation::Landscape,
        )
        .unwrap();
        let selection = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[
            {"layout_id":"a0-looking-name","enabled":true,"formats":["pdf"]},
            {"layout_id":"disabled","enabled":false,"formats":["pdf"]}],"geospatial":["gpkg"],"tabular":["xlsx"]});
        let sheets = json!({"project_settings":[{"parameter":"REPORT-FORMATS","value":selection}],
            "printing_setups":[{"id":layout.id,"revision":"a".repeat(64),"layout":layout}]});
        let result = inspect_project_settings(&sheets).unwrap();
        assert_eq!(result["setting"]["value"], selection);
        assert_eq!(
            result["outputs"],
            serde_json::to_value(plan(&sheets, false).unwrap()).unwrap()
        );
        assert_eq!(result["papers"].as_array().unwrap().len(), 1);
        assert_eq!(result["papers"][0]["paper"]["size"], "A3");
        assert_eq!(result["source"], "project_settings");
        let defaults = inspect_project_settings(&json!({})).unwrap();
        assert_eq!(defaults["source"], "report_defaults");
        assert!(defaults["setting"].is_null());
        assert_eq!(defaults["papers"], json!([]));
        let mut broken = sheets;
        broken["printing_setups"] = json!([]);
        assert!(inspect_project_settings(&broken)
            .unwrap_err()
            .contains("missing"));
    }

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

    #[test]
    fn versioned_output_matrix_selects_rasters_from_one_named_layout() {
        use crate::printing::{named_layout, Orientation, Paper};
        let layout = named_layout(
            Some("aderm-a3".into()),
            Some("ADERM A3".into()),
            Paper::A3,
            Orientation::Landscape,
        )
        .unwrap();
        let sheets = json!({
            "project_settings":[{"parameter":"design_export_format","value":{
                "schema":"ds.design-output-selection/v1",
                "prints":[{"layout_id":"aderm-a3","enabled":true,"formats":["pdf","jpeg"]}],
                "geospatial":["gpkg"],
                "tabular":["xlsx"]
            }}],
            "printing_setups":[{"id":"aderm-a3","revision":"a".repeat(64),"layout":layout}]
        });
        let outputs = plan(&sheets, false).unwrap();
        assert_eq!(
            outputs
                .iter()
                .map(|item| item.output_id.as_str())
                .collect::<Vec<_>>(),
            ["xlsx", "gpkg", "jpeg__aderm-a3", "pdf__aderm-a3"]
        );
        assert_eq!(outputs[2].content_type, "image/jpeg");
        assert_eq!(outputs[2].suffix, ".aderm-a3.jpg");
    }

    #[test]
    fn output_selection_normalization_has_one_alias_and_bound_policy() {
        let selection = normalize_output_selection(
            None,
            Some(vec![
                "PDF__ADERM-A3".into(),
                "jpg__aderm-a3".into(),
                "gpkg".into(),
                "xlsx".into(),
            ]),
        )
        .unwrap();
        assert_eq!(selection.schema, OUTPUT_SELECTION_SCHEMA);
        assert_eq!(selection.prints[0].layout_id, "aderm-a3");
        assert_eq!(
            selection.prints[0].formats,
            vec![PrintArtifactFormat::Pdf, PrintArtifactFormat::Jpeg]
        );
        assert_eq!(selection.geospatial, ["gpkg"]);
        assert_eq!(selection.tabular, ["xlsx"]);
        assert!(normalize_output_selection(None, Some(vec![])).is_err());
        assert!(normalize_output_selection(None, Some(vec!["gif__a3".into()])).is_err());
    }
}
