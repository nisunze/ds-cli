//! Report output policy shared by native, WASM and the reporter. No IO.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The outputs an absent or empty selection produces. `voltage_drop` is on by
/// default (owner ruling 2026-09-28): ds-network solves every processed
/// transformer, and the report states those results.
pub const DEFAULT_INDIVIDUAL: &[&str] = &["shp", "kmz", "xlsx", "voltage_drop"];
pub const INDIVIDUAL: &[&str] = &[
    "shp",
    "kmz",
    "xlsx",
    "gpkg",
    "pdf_a0",
    "pdf_a3",
    "geojson",
    "network_information",
    "voltage_drop",
    "voltage_drop_pdf",
];
/// The tabular outputs, one list for every reader that classes a token.
pub const TABULAR: &[&str] = &["xlsx", "network_information", "voltage_drop"];
pub const COMBINED: &[&str] = &["shp", "xlsx", "geojsonseq"];
pub const OPTIONAL_COMBINED: &[&str] = &["gpkg", "network_information"];

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
    /// The engine's own page: the vector every delivered format is derived
    /// from (PDF by conversion, PNG/JPEG by rasterisation). It is the one
    /// fast preview those formats share — with text flattened to paths, so a
    /// viewer needs no font — and not a deliverable: ds-brain admits only the
    /// three above in a stored selection (`network_reporter_individual_policy`).
    Svg,
}

impl PrintArtifactFormat {
    fn prefix(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Svg => "svg",
        }
    }

    /// May a saved selection or a project's stored policy name this format?
    pub fn deliverable(self) -> bool {
        !matches!(self, Self::Svg)
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
        (PrintArtifactFormat::Svg, "svg__"),
    ]
    .into_iter()
    .find_map(|(kind, prefix)| format.strip_prefix(prefix).map(|id| (kind, id)))?;
    crate::printing::identifier(id).then_some((kind, id))
}

/// Where a selected output may be produced. A selection that names no
/// placement for a data output offers both; every print is only ever
/// `Desktop`, the lane of DS GridDesign Desktop and Linux Server alike.
/// ds-brain admits a request against this same matrix
/// (`network_reporter_individual_policy.go`), so the vocabulary and its
/// precedence are authored here rather than in two runtimes.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Desktop,
    Web,
}

/// Read a placement the way ds-brain reads one: trimmed and case-folded
/// (`network_reporter_individual_policy.go:408`). A selection the grant admits
/// must not be unreadable here — that is the failure this whole field exists to
/// end — and serialisation stays canonical `snake_case`, so a stored `Desktop`
/// is honoured once and written back lowercase.
impl<'de> Deserialize<'de> for Placement {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        match raw.trim().to_ascii_lowercase().as_str() {
            "desktop" => Ok(Self::Desktop),
            "web" => Ok(Self::Web),
            other => Err(serde::de::Error::custom(format!(
                "unknown output placement `{other}`, expected `desktop` or `web`"
            ))),
        }
    }
}

/// The answer for a data output no execution entry names.
pub const BOTH_PLACEMENTS: &[Placement] = &[Placement::Desktop, Placement::Web];
/// The one answer for print outputs, whatever the matrix says. Map sheets
/// use local layout context; the A4 voltage-drop PDF uses local Chromium.
pub const PRINT_PLACEMENTS: &[Placement] = &[Placement::Desktop];
/// The catch-all execution key, tried after the exact id and the class.
pub const EXECUTION_WILDCARD: &str = "*";
/// The three classes an execution entry may be keyed by. `print` is singular
/// because that is the token ds-brain matches (`:206`, `:225`); the selection
/// field that carries prints is plural, and the two are not the same word.
pub const EXECUTION_CLASSES: &[&str] = &["print", "geospatial", "tabular"];

/// The execution class of one output id, exactly as ds-brain computes it while
/// filtering the individual output list.
pub fn output_class(output_id: &str) -> &'static str {
    if TABULAR.contains(&output_id) {
        "tabular"
    } else if output_id == "voltage_drop_pdf"
        || output_id.starts_with("pdf_")
        || named_print_output(output_id).is_some()
    {
        "print"
    } else {
        "geospatial"
    }
}

pub const OUTPUT_SELECTION_SCHEMA: &str = "ds.design-output-selection/v1";

/// The selection field that carries [`VoltageDropReportFormat`], named once
/// for every host that describes it (`options`) or reads it.
pub const VOLTAGE_DROP_REPORT_FIELD: &str = "voltage_drop_report";

/// How the per-transformer `voltage_drop` report is laid out. Owner ruling
/// 2026-09-29: the extended report is the default, and the choice is the
/// project's, configured with its design exports.
#[derive(Debug, Clone, Copy, Default, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoltageDropReportFormat {
    /// Every sheet, with compact tables.
    #[default]
    Extended,
    /// The summary, the maps, the outlook and the remediation.
    Brief,
}

impl VoltageDropReportFormat {
    pub const ALL: &[Self] = &[Self::Extended, Self::Brief];

    pub fn token(self) -> &'static str {
        match self {
            Self::Extended => "extended",
            Self::Brief => "brief",
        }
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Read the way a [`Placement`] is read — trimmed and case-folded, as ds-brain
/// reads the same field — and refused by name otherwise, so a stored typo is
/// never silently a different report.
impl<'de> Deserialize<'de> for VoltageDropReportFormat {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        match raw.trim().to_ascii_lowercase().as_str() {
            "extended" => Ok(Self::Extended),
            "brief" => Ok(Self::Brief),
            other => Err(serde::de::Error::custom(format!(
                "unknown voltage-drop report format `{other}`, expected `extended` or `brief`"
            ))),
        }
    }
}

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
    /// Deployment placement, keyed by exact output id, by output class, or by
    /// `*`. The Settings output matrix has written this key since 2026-09-07
    /// and ds-brain has admitted it since; it was missing here, which is why
    /// every reader of an edited project refused. `deny_unknown_fields` stays:
    /// the guard was right, the field was absent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub execution: BTreeMap<String, Vec<Placement>>,
    /// The layout of the voltage-drop JSON and A4 PDF reports: `extended`
    /// (the default) or `brief`. Absent means `extended`; it has no effect
    /// while neither voltage-drop output is selected.
    //
    // `extended` is never written. Every reader older than this field
    // refuses a selection that carries it (`deny_unknown_fields` here,
    // `unknown output-selection field` in ds-brain), so only a project that
    // chose `brief` carries it, and a project that never chose keeps reading
    // everywhere, as extended. The schema states the default the skip hides.
    #[serde(default, skip_serializing_if = "VoltageDropReportFormat::is_default")]
    #[schemars(extend("default" = "extended"))]
    pub voltage_drop_report: VoltageDropReportFormat,
}

impl DesignOutputSelection {
    /// The voltage-drop report layout this selection asks for; `extended`
    /// when it never chose.
    pub fn voltage_drop_report_format(&self) -> VoltageDropReportFormat {
        self.voltage_drop_report
    }

    /// A selection a project stores, or a host saves, delivers: the shared
    /// preview page is never among its outputs.
    pub fn require_deliverable(&self) -> Result<(), String> {
        if self
            .prints
            .iter()
            .any(|print| print.formats.iter().any(|f| !f.deliverable()))
        {
            return Err(
                "svg is the shared print preview, not a deliverable output; select pdf, png or jpeg"
                    .into(),
            );
        }
        Ok(())
    }

    /// Which runtimes may produce one output of this selection. Prints — fixed
    /// A0/A3/A4 PDFs and every named spelling — always use
    /// [`PRINT_PLACEMENTS`]. Data outputs use ds-brain's precedence: exact
    /// output id, class, `*`, then both. An empty list refuses a data output
    /// on every lane.
    pub fn placements(&self, output_id: &str) -> &[Placement] {
        let class = output_class(output_id);
        if class == "print" {
            return PRINT_PLACEMENTS;
        }
        self.execution
            .get(output_id)
            .or_else(|| self.execution.get(class))
            .or_else(|| self.execution.get(EXECUTION_WILDCARD))
            .map_or(BOTH_PLACEMENTS, Vec::as_slice)
    }

    /// A selection a host authors or saves never places a print off the
    /// desktop. An entry reaching a print without `desktop` —
    /// its exact id, the `print` class, or `*` when no `print` entry shadows
    /// it — would have that print produced nowhere, silently.
    /// Refused rather than rewritten: the bytes a host chose are the bytes
    /// stored. Reading a stored selection never refuses; `placements` answers
    /// the desktop.
    pub fn require_desktop_prints(&self) -> Result<(), String> {
        // A data-only selection may deliberately run in the cloud. Its
        // wildcard cannot strand a print that the selection does not request.
        if !self
            .tokens()?
            .iter()
            .any(|token| output_class(token) == "print")
        {
            return Ok(());
        }
        let reaches_prints = |key: &str| {
            key == "print"
                || output_class(key) == "print"
                || (key == EXECUTION_WILDCARD && !self.execution.contains_key("print"))
        };
        match self.execution.iter().find(|(key, placements)| {
            reaches_prints(key) && !placements.contains(&Placement::Desktop)
        }) {
            Some((key, _)) => Err(format!(
                "output execution placement for {key} keeps prints off the desktop; prints render only on Desktop and the Linux Server, so place them `desktop`"
            )),
            None => Ok(()),
        }
    }

    /// The placement answer for every output this selection yields, so a host
    /// renders the matrix from one reply instead of asking per checkbox.
    pub fn placement_map(&self) -> Result<BTreeMap<String, Vec<Placement>>, String> {
        Ok(self
            .tokens()?
            .into_iter()
            .map(|token| {
                let placements = self.placements(&token).to_vec();
                (token, placements)
            })
            .collect())
    }

    fn validate_execution(&self) -> Result<(), String> {
        if self.execution.len() > 128 {
            return Err("output execution placements exceed 128 entries".into());
        }
        for (key, placements) in &self.execution {
            // Bounded, not enumerated. ds-brain simply never consults a key
            // that matches no output id, class or wildcard, so refusing one
            // here would make a project it admits unreadable — the failure
            // this field exists to end.
            if key.is_empty()
                || key.len() > 128
                || key.chars().any(char::is_control)
                || placements.len() > 2
            {
                return Err(format!("invalid output execution placement for {key}"));
            }
            if placements
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != placements.len()
            {
                return Err(format!(
                    "output execution placement repeats a lane for {key}"
                ));
            }
        }
        Ok(())
    }

    pub fn tokens(&self) -> Result<Vec<String>, String> {
        if self.schema != OUTPUT_SELECTION_SCHEMA
            || self.prints.len() > 32
            || self.geospatial.len() > 8
            || self.tabular.len() > 8
        {
            return Err("invalid design output selection".into());
        }
        self.validate_execution()?;
        let mut tokens = Vec::new();
        tokens.extend(self.geospatial.iter().cloned());
        tokens.extend(self.tabular.iter().cloned());
        let mut layouts = std::collections::BTreeSet::new();
        for print in &self.prints {
            if !crate::printing::identifier(&print.layout_id)
                || !layouts.insert(&print.layout_id)
                || print.formats.is_empty()
                || print.formats.len() > 4
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

/// Repair the historical UI's fixed-output bucket choice without changing
/// what was selected. It treated every fixed token except `xlsx` as
/// geospatial, including the tabular `voltage_drop` JSON. Only known fixed
/// tokens move; unknown tokens keep their original field so a newer producer
/// is not silently reinterpreted by this reader.
fn canonicalize_fixed_output_fields(selection: &mut DesignOutputSelection) {
    let geospatial = std::mem::take(&mut selection.geospatial);
    let tabular = std::mem::take(&mut selection.tabular);
    for (was_tabular, tokens) in [(false, geospatial), (true, tabular)] {
        for token in tokens {
            let is_tabular = if INDIVIDUAL.contains(&token.as_str()) {
                TABULAR.contains(&token.as_str())
            } else {
                was_tabular
            };
            let target = if is_tabular {
                &mut selection.tabular
            } else {
                &mut selection.geospatial
            };
            if !target.contains(&token) {
                target.push(token);
            }
        }
    }
}

/// Normalize the two accepted authoring inputs into the one durable selection.
/// Hosts pass bytes through; format aliases and bounds live here with report
/// planning rather than being reimplemented in TypeScript.
pub fn normalize_output_selection(
    selection: Option<DesignOutputSelection>,
    legacy_formats: Option<Vec<String>>,
    execution: Option<BTreeMap<String, Vec<Placement>>>,
) -> Result<DesignOutputSelection, String> {
    if let Some(mut selection) = selection {
        if legacy_formats.is_some() {
            return Err("pass a versioned selection or legacy formats, not both".into());
        }
        // An incoming placement matrix survives the upgrade: an authoring host
        // may carry the one it already holds, and its own is not overwritten.
        if let Some(execution) = execution {
            if !execution.is_empty() {
                selection.execution = execution;
            }
        }
        canonicalize_fixed_output_fields(&mut selection);
        selection.tokens()?;
        selection.require_deliverable()?;
        selection.require_desktop_prints()?;
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
        // Trimmed, case-folded and aliased exactly as `normalize` and ds-brain
        // read the same token, so the selection a host renders and the plan a
        // report runs never name the output differently. A repeat collapses
        // for the same reason the grant's `selected` map collapses it.
        let token = alias_format(&raw.trim().to_ascii_lowercase()).to_string();
        if let Some((format, id)) = named_print_output(&token) {
            prints.entry(id.into()).or_default().insert(format);
        } else if token.contains("__") {
            return Err(format!("invalid named print output: {raw}"));
        } else if TABULAR.contains(&token.as_str()) {
            if !tabular.contains(&token) {
                tabular.push(token);
            }
        } else if !geospatial.contains(&token) {
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
        // A legacy token list carries no placement; the upgrade must not
        // invent one. Absent means both lanes, which is what the legacy
        // setting meant.
        execution: execution.unwrap_or_default(),
        // Nor a report layout: a legacy token list never chose one.
        voltage_drop_report: VoltageDropReportFormat::default(),
    };
    selection.tokens()?;
    selection.require_deliverable()?;
    selection.require_desktop_prints()?;
    Ok(selection)
}

/// Read whatever a project has actually stored in its export row: the
/// versioned selection, or any legacy shape `string_list` already understands
/// (comma or semicolon text, a token array, a truthy map, a JSON array in a
/// string). An absent or empty value is an empty selection rather than a
/// refusal, because reading a project's settings is not authoring them — the
/// authoring path (`normalize_output_selection`) still demands 1..32 tokens.
///
/// This is the one legacy reader. A host that re-derives it in its own
/// language gets a second, disagreeing answer; that is what this replaces.
pub fn stored_output_selection(value: &Value) -> Result<DesignOutputSelection, String> {
    if value
        .get("schema")
        .is_some_and(|s| s == OUTPUT_SELECTION_SCHEMA)
    {
        let mut selection: DesignOutputSelection = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid design output selection: {error}"))?;
        canonicalize_fixed_output_fields(&mut selection);
        // Read as stored: the engine reads a preview recipe's in-memory
        // policy through here too. Save paths refuse the preview page.
        selection.tokens()?;
        return Ok(selection);
    }
    let tokens = string_list(value);
    if tokens.is_empty() {
        return Ok(DesignOutputSelection {
            schema: OUTPUT_SELECTION_SCHEMA.into(),
            prints: Vec::new(),
            geospatial: Vec::new(),
            tabular: Vec::new(),
            execution: BTreeMap::new(),
            voltage_drop_report: VoltageDropReportFormat::default(),
        });
    }
    normalize_output_selection(None, Some(tokens), None)
}

pub fn output_selection_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(DesignOutputSelection))
        .expect("output selection schema")
}

/// The voltage-drop layout choice as a host renders it: the selection field
/// that carries it, the values it takes, and the one a project that never
/// chose gets.
pub fn voltage_drop_report_option() -> Value {
    serde_json::json!({
        "field": VOLTAGE_DROP_REPORT_FIELD,
        "values": VoltageDropReportFormat::ALL
            .iter()
            .map(|format| format.token())
            .collect::<Vec<_>>(),
        "default": VoltageDropReportFormat::default().token(),
    })
}

/// The voltage-drop report layout a project's sealed settings ask for: the
/// versioned selection's choice, read by the one reader, and `extended` for
/// an absent export row or any legacy shape, none of which could choose.
/// The reporter writes this into each voltage-drop document it produces.
pub fn project_voltage_drop_report_format(
    sheets: &Value,
) -> Result<VoltageDropReportFormat, String> {
    match project_output_setting(sheets)? {
        Some(row) if row["value"]["schema"] == OUTPUT_SELECTION_SCHEMA => {
            Ok(stored_output_selection(&row["value"])?.voltage_drop_report_format())
        }
        _ => Ok(VoltageDropReportFormat::default()),
    }
}

pub fn options(setups: &[Value]) -> Result<Vec<Value>, String> {
    if setups.len() > 200 {
        return Err("Printing setup catalog exceeds 200 entries".into());
    }
    let mut result: Vec<Value> = INDIVIDUAL.iter().filter(|f| !f.starts_with("pdf_")).map(|f|
        serde_json::json!({"value":f,"selection_field":if TABULAR.contains(f) { "tabular" } else { "geospatial" },"label":match *f {
            "geojson" => "GeoJSON".to_string(),
            "voltage_drop_pdf" => "Voltage drop · PDF A4".to_string(),
            _ => f.to_ascii_uppercase(),
        }})
    ).collect();
    // Both voltage-drop outputs share one report layout choice and its
    // default, so no host re-states `extended` as the default.
    for row in &mut result {
        if row["value"] == "voltage_drop" || row["value"] == "voltage_drop_pdf" {
            row["report_format"] = voltage_drop_report_option();
        }
        if row["value"] == "voltage_drop_pdf" {
            row["class"] = serde_json::json!("print");
            row["paper"] = serde_json::json!("A4");
        }
    }
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
            PrintArtifactFormat::Svg => ("image/svg+xml", format!(".{id}.svg")),
        };
        return Ok(Output {
            output_id: format!("{}__{id}", kind.prefix()),
            format: format!("{}__{id}", kind.prefix()),
            content_type,
            suffix,
        });
    }
    let (content_type, suffix) = match format {
        "network_information" => ("application/json", ".network-information.json"),
        "voltage_drop" => ("application/json", ".voltage-drop.json"),
        "voltage_drop_pdf" => ("application/pdf", " voltage drop.pdf"),
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

/// The one human-facing filename for an analytical voltage-drop PDF. The
/// transformer key remains unchanged in requests and receipts. Underscore
/// runs separate display words; the first ASCII letter of each word is
/// capitalized without changing the rest of the operator's spelling.
pub fn voltage_drop_pdf_filename(transformer: &str) -> String {
    let words: Vec<String> = transformer
        .trim()
        .split('_')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut bytes = word.as_bytes().to_vec();
            if let Some(first_letter) = bytes.iter_mut().find(|byte| byte.is_ascii_alphabetic()) {
                first_letter.make_ascii_uppercase();
            }
            // Only one ASCII byte is changed; the original UTF-8 remains valid.
            String::from_utf8(bytes).expect("ASCII case conversion preserves UTF-8")
        })
        .collect();
    let display = if words.is_empty() {
        transformer.trim().to_owned()
    } else {
        words.join(" ")
    };
    format!("{display} voltage drop.pdf")
}

/// The legacy format aliases, in one place. ds-brain applies exactly this map
/// while admitting a request (`networkReporterIndividualFormatAliases`), so a
/// stored `excel` is `xlsx` and a stored `A3_PDF` is `pdf_a3` for EVERY reader:
/// the planner, the selection the Settings matrix renders, and the grant. A
/// reader that skipped it answered `a3_pdf` — a token no catalogue offers and
/// no execution class claims — while the grant answered `pdf_a3`, class
/// `print`, and the two disagreed about where the output could run.
pub fn alias_format(lowercase: &str) -> &str {
    match lowercase {
        "shp.zip" | "shapefile" => "shp",
        "json" | "geojson.zip" => "geojson",
        "excel" => "xlsx",
        "pdf" | "a0_pdf" => "pdf_a0",
        "a3_pdf" => "pdf_a3",
        other => other,
    }
}

/// What is this artifact called?
///
/// One question, two kinds of evidence. A local artifact is named from the
/// output it is (`combined_output_filename` below, and the transformer form in
/// `project_control`); a cloud artifact is named from the locator it was signed
/// under. They belong in one module because a report an operator downloads from
/// the outbox and the same report downloaded from storage must not arrive under
/// two different names.
///
/// A storage locator carries the object path percent-encoded after `/o/`; any
/// other locator is a plain path or URL. The query string is never part of the
/// name — it is the signature.
pub fn artifact_filename(locator: &str) -> String {
    let path = match locator.split_once("/o/") {
        Some((_, encoded)) => encoded.split('?').next().unwrap_or(""),
        None => locator.split('?').next().unwrap_or(""),
    };
    let decoded = percent_decode(path).unwrap_or_else(|| path.to_string());
    let name = decoded.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        // A zero-length basename is not a name. The browser's own `?? 'file'`
        // could never fire, because splitting a string always yields at least
        // one member — so a locator ending in a separator produced an empty
        // filename and a download with no name. This is that fallback, working.
        return "file".into();
    }
    name.to_string()
}

/// Which door does this artifact open: the in-app PDF preview, or a download?
///
/// It belongs beside the name because it is a question ABOUT the name, and the
/// browser asked it with its own regular expression next to every button that
/// offered the choice — two readings of one rule, in a template, where a third
/// button would have made three.
pub fn artifact_opens_as_pdf(filename: &str) -> bool {
    filename.to_ascii_lowercase().ends_with(".pdf")
}

/// What an artifact IS, from the name it carries: the badge vocabulary the
/// report cards read a version's files under.
///
/// `.shp.zip` is a shapefile and a plain `.zip` is an archive — a distinction
/// the browser drew in a chain of `endsWith` calls sitting beside the colours,
/// so the card and a headless reader were free to disagree about what a file
/// is. The answer is a key: what it is CALLED and what colour it wears stay
/// with whoever renders it.
pub fn artifact_kind(filename: &str) -> &'static str {
    let name = filename.to_ascii_lowercase();
    if name.ends_with(".pdf") {
        "pdf"
    } else if name.ends_with(".xlsx") || name.ends_with(".xls") {
        "excel"
    } else if name.ends_with(".shp.zip") || name.contains(".shp.") {
        "shapefile"
    } else if name.ends_with(".kmz") {
        "kmz"
    } else if name.ends_with(".gpkg") {
        "gpkg"
    } else if name.ends_with(".zip") {
        "zip"
    } else {
        "other"
    }
}

/// The order those kinds are offered in: the deliverable first, the unreadable
/// last. An operator scanning one version's files reads the same sequence
/// wherever the files are listed.
pub const ARTIFACT_KIND_ORDER: [&str; 7] =
    ["pdf", "excel", "shapefile", "kmz", "gpkg", "zip", "other"];

/// One version's artifacts, grouped by what they are and ordered for reading.
///
/// Answers with POSITIONS in the list the caller sent, so no locator is copied
/// through the kernel to come back unchanged. A kind nothing matched is not a
/// group: an empty badge is a claim about a file that is not there.
pub fn artifact_groups(locators: &[String]) -> Vec<(&'static str, Vec<usize>)> {
    let kinds: Vec<&'static str> = locators
        .iter()
        .map(|locator| artifact_kind(&artifact_filename(locator)))
        .collect();
    let mut groups: Vec<(&'static str, Vec<usize>)> = Vec::new();
    for kind in ARTIFACT_KIND_ORDER {
        let items: Vec<usize> = kinds
            .iter()
            .enumerate()
            .filter(|(_, held)| **held == kind)
            .map(|(index, _)| index)
            .collect();
        if !items.is_empty() {
            groups.push((kind, items));
        }
    }
    groups
}

/// What one stored report version is called, what its download is called, and
/// what the folder inside that download is called.
///
/// Three decisions the browser held: a version is shown under its display
/// name, failing that the version label it was published with, failing that
/// its prefix; the folder inside the archive is that same name with every
/// character a filesystem refuses replaced; and a name that survives none of
/// that is the prefix again rather than an archive rooted at `/`.
pub fn combined_entry_naming(
    display_name: &str,
    version_label: &str,
    prefix: &str,
) -> (String, String, String) {
    let label = [display_name, version_label, prefix]
        .into_iter()
        .find(|candidate| !candidate.is_empty())
        .unwrap_or_default()
        .to_string();
    let mut folder = String::with_capacity(label.len());
    let mut replaced = false;
    for character in label.chars() {
        if matches!(
            character,
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        ) {
            // A RUN of refused characters is one separator, not one each.
            if !replaced {
                folder.push('_');
                replaced = true;
            }
        } else {
            folder.push(character);
            replaced = false;
        }
    }
    if folder.is_empty() {
        folder = prefix.to_string();
    }
    (label, folder, format!("{prefix}.zip"))
}

/// `decodeURIComponent` semantics: `%XX` is a byte, the result must be UTF-8,
/// and anything malformed decodes to nothing so the caller keeps the raw text.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The filename a combined or compounded outbox output carries.
///
/// Moved here from the local-report projection so that both halves of "what is
/// this artifact called" are decided by the one module that knows what an
/// output id means.
pub fn combined_output_filename(kind: &str, output_id: &str, format: &str) -> String {
    if kind == "compounded" {
        return "compounded_reports.zip".to_string();
    }
    if let Ok(definition) = output(output_id) {
        if definition.format == format {
            return format!("combined_transformer{}", definition.suffix);
        }
    }
    match output_id {
        "shp" => "combined_transformer.shp.zip".to_string(),
        "xlsx" => "combined_transformer.xlsx".to_string(),
        "geojsonseq" => "combined_transformer_geojsonl.zip".to_string(),
        _ => format!("combined_transformer.{format}"),
    }
}

pub fn normalize(values: &[String]) -> Vec<String> {
    let selected: Vec<_> = values
        .iter()
        .map(|value| {
            if let Some((kind, id)) = named_print_output(value.trim()) {
                return format!("{}__{id}", kind.prefix());
            }
            alias_format(&value.trim().to_ascii_lowercase()).to_string()
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
    // A project-wide PDF can grow beyond the durable artifact limit even when
    // every individual sheet is valid. Printing stays an explicit, separate
    // action on individual reports; Combined is a data-only deliverable.
    COMBINED
        .iter()
        .copied()
        .chain(
            OPTIONAL_COMBINED
                .iter()
                .copied()
                .filter(|f| selected.iter().any(|s| s == f)),
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

/// One parameter name reduced to the form the aliases are written in.
pub fn setting_parameter_key(row: &Value) -> Option<String> {
    row["parameter"]
        .as_str()
        .map(|s| s.trim().to_ascii_lowercase().replace('-', "_"))
}

/// The index of a project's export-selection row among settings rows, by any
/// of the five aliases and with the same hyphen/case normalisation the
/// planner reads with. Hosts patch through this instead of naming two of the
/// five aliases themselves.
pub fn output_setting_index(rows: &[Value]) -> Option<usize> {
    rows.iter().position(|row| {
        setting_parameter_key(row).is_some_and(|key| SETTING_ALIASES.contains(&key.as_str()))
    })
}

fn settings_rows(rows: &Value, label: &str) -> Result<(), String> {
    if !rows.is_array() {
        return Err(format!("Report {label} must be a row list"));
    }
    for row in rows.as_array().unwrap() {
        if !row.is_object()
            || row
                .get("parameter")
                .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return Err("Report project setting must be an object with a text parameter".into());
        }
    }
    Ok(())
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
        settings_rows(rows, "project_settings")?;
    }
    let rows = top
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .or_else(|| nested.and_then(Value::as_array));
    Ok(rows.and_then(|rows| output_setting_index(rows).map(|index| &rows[index])))
}

/// Bound and shape-check the settings rows a host hands over for patching.
pub fn settings_rows_for_patch(rows: &[Value]) -> Result<(), String> {
    if rows.len() > 10_000 {
        return Err("Project settings exceed 10,000 rows".into());
    }
    settings_rows(&Value::Array(rows.to_vec()), "project_settings")
}

/// Write the selection into the project's export row, creating the row under
/// the first alias when the sheet carries none. The alias set, the
/// normalisation and the created parameter name all live here, with the
/// planner that reads them back.
pub fn apply_output_selection(
    rows: &mut Vec<Value>,
    selection: &DesignOutputSelection,
) -> Result<String, String> {
    settings_rows_for_patch(rows)?;
    let mut selection = selection.clone();
    canonicalize_fixed_output_fields(&mut selection);
    selection.tokens()?;
    selection.require_desktop_prints()?;
    let value = serde_json::to_value(selection).map_err(|error| error.to_string())?;
    match output_setting_index(rows) {
        Some(index) => {
            rows[index]["value"] = value;
            Ok(setting_parameter_key(&rows[index]).unwrap_or_else(|| SETTING_ALIASES[0].into()))
        }
        None => {
            rows.push(serde_json::json!({"parameter":SETTING_ALIASES[0],"value":value}));
            Ok(SETTING_ALIASES[0].into())
        }
    }
}

/// Local iteration selection is an execution input, never a settings mutation.
/// Only exact formats with held, validated layouts can be rendered.
pub fn selected_plan(
    sheets: &Value,
    is_combined: bool,
    selection: Option<&DesignOutputSelection>,
) -> Result<Vec<Output>, String> {
    let Some(selection) = selection else {
        return plan(sheets, is_combined);
    };
    if is_combined {
        return Err("Local output overrides currently require individual transformers".into());
    }
    // A local override is one invocation's inventory, not the project's
    // deployment policy. Placement belongs to the saved selection ds-brain
    // admits against; an override carrying one would decide it per run.
    if !selection.execution.is_empty() {
        return Err("Local output overrides never carry an execution placement matrix".into());
    }
    // Likewise the report layout: the reporter reads it from the project's
    // sealed settings, so an override naming one would be silently ignored.
    if !selection.voltage_drop_report.is_default() {
        return Err(
            "Local output overrides never carry a voltage-drop report format; set it in the project's design exports"
                .into(),
        );
    }
    let tokens = selection.tokens()?;
    if tokens.is_empty() || tokens.len() > 32 {
        return Err("Local output selection must contain 1..32 outputs".into());
    }
    let layouts = named_layouts(sheets)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut outputs = Vec::new();
    for token in tokens {
        let output = output(&token)?;
        if named_print_output(&output.format).is_none()
            && !INDIVIDUAL.contains(&output.format.as_str())
        {
            return Err(format!(
                "Unsupported individual report output: {}",
                output.format
            ));
        }
        if !seen.insert(output.output_id.clone()) {
            return Err("Local output selection repeats an output".into());
        }
        if let Some((_, id)) = named_print_output(&output.format) {
            if !layouts.contains_key(id) {
                return Err(format!("Prepare printing setup {id} before selecting it"));
            }
        }
        outputs.push(output);
    }
    outputs.sort_by(|a, b| {
        let rank = |output: &Output| {
            INDIVIDUAL
                .iter()
                .position(|f| *f == output.format)
                .unwrap_or(INDIVIDUAL.len())
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.output_id.cmp(&b.output_id))
    });
    Ok(outputs)
}

/// How the settings under inspection were obtained. It decides which refusal a
/// host renders when they are incomplete, and it is an input because only the
/// host knows whether it just refreshed or is reading a held copy.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessMode {
    #[default]
    Refreshed,
    Offline,
}

impl ReadinessMode {
    pub fn token(self) -> &'static str {
        match self {
            Self::Refreshed => "refreshed",
            Self::Offline => "offline",
        }
    }
    /// The message key a host resolves. Kernel output names keys, never
    /// English, so the GUI and `ds` refuse in the same words in every
    /// language.
    pub fn message_key(self) -> &'static str {
        match self {
            Self::Refreshed => "printing_inputs_incomplete_refreshed",
            Self::Offline => "printing_inputs_incomplete_offline",
        }
    }
}

pub fn inspect_project_settings(
    sheets: &Value,
    mode: Option<ReadinessMode>,
) -> Result<Value, String> {
    let mode = mode.unwrap_or_default();
    let refusal = serde_json::json!({
        "code":"printing_inputs_incomplete",
        "message_key":mode.message_key(),
        "mode":mode.token(),
    });
    let setting = project_output_setting(sheets)?;
    // The voltage-drop layout the reporter will write, or null when the
    // stored selection cannot be read (the issue below names why).
    let voltage_drop_report = project_voltage_drop_report_format(sheets)
        .ok()
        .map(VoltageDropReportFormat::token);
    let prepared = plan(sheets, false)
        .and_then(|outputs| named_layouts(sheets).map(|layouts| (outputs, layouts)));
    let (outputs, layouts) = match prepared {
        Ok(prepared) => prepared,
        Err(issue) => {
            return Ok(
                serde_json::json!({"source":if setting.is_some(){"project_settings"}else{"report_defaults"},
            "setting":setting,"ready":false,"issues":[issue],"outputs":[],"papers":[],"refusal":refusal,
            "voltage_drop_report":voltage_drop_report}),
            );
        }
    };
    let papers = outputs.iter().filter_map(|output| {
        let (_, id) = named_print_output(&output.format)?;
        let layout = layouts.get(id)?;
        Some(serde_json::json!({"output_id":output.output_id,"layout_id":id,"name":layout.name,"paper":layout.paper}))
    }).collect::<Vec<_>>();
    Ok(
        serde_json::json!({"source":if setting.is_some(){"project_settings"}else{"report_defaults"},
        "setting":setting,"ready":true,"issues":[],"outputs":outputs,"papers":papers,"refusal":Value::Null,
        "voltage_drop_report":voltage_drop_report}),
    )
}

fn selected_formats(sheets: &Value) -> Result<Vec<String>, String> {
    let raw = project_output_setting(sheets)?
        .map(|row| {
            // The versioned selection is read by the one reader; a legacy
            // value stays raw here so `normalize` keeps applying its format
            // aliases and silently ignoring a token no report can produce.
            if row["value"]["schema"] == OUTPUT_SELECTION_SCHEMA {
                stored_output_selection(&row["value"])?.tokens()
            } else {
                Ok(string_list(&row["value"]))
            }
        })
        .transpose()?
        .unwrap_or_default();
    Ok(normalize(&raw))
}

/// Whether an output needs the local print runtime: a named print setup, an
/// A0/A3 map sheet, or the analytical A4 voltage-drop document. The A4 PDF
/// needs no named layout, but still requires local Chromium printing.
pub fn is_printout(format: &str) -> bool {
    matches!(format, "pdf_a0" | "pdf_a3" | "voltage_drop_pdf")
        || named_print_output(format).is_some()
}

/// The cloud's plan: selected data outputs only. Every PDF, including the A4
/// voltage-drop report, is omitted before printing inputs are consulted. A
/// project's local print setup cannot fail a cloud data export.
pub fn plan_data_outputs(sheets: &Value) -> Result<Vec<Output>, String> {
    let formats: Vec<String> = selected_formats(sheets)?
        .into_iter()
        .filter(|format| !is_printout(format))
        .collect();
    if formats.len() > 32 {
        return Err("Report exceeds 32 selected output artifacts".into());
    }
    formats.iter().map(|f| output(f)).collect()
}

pub fn plan(sheets: &Value, is_combined: bool) -> Result<Vec<Output>, String> {
    let selected = selected_formats(sheets)?;
    if !is_combined {
        let layouts = named_layouts(sheets)?;
        for format in &selected {
            if let Some((_, id)) = named_print_output(format) {
                if !layouts.contains_key(id) {
                    return Err(format!(
                        "Selected printing setup {id} is not in the sealed project printing inputs"
                    ));
                }
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

    /// The name is the artifact's own, whatever evidence it came from: a signed
    /// storage locator, a plain path, or the output id of a local outbox row.
    #[test]
    fn one_artifact_has_one_name_from_either_kind_of_evidence() {
        // A storage locator carries the object path percent-encoded after /o/,
        // and the signature after ? is never part of the name.
        assert_eq!(
            artifact_filename(
                "https://host/v0/b/bucket/o/eds%2Fp1%2Fcombined%20report.xlsx?alt=media&token=t"
            ),
            "combined report.xlsx"
        );
        // A plain URL or path is the same question with less wrapping.
        assert_eq!(
            artifact_filename(
                "https://host/reports/combined_transformer.shp.zip?X-Goog-Signature=a"
            ),
            "combined_transformer.shp.zip"
        );
        assert_eq!(artifact_filename("/outbox/run/report.pdf"), "report.pdf");
        // Malformed encoding keeps the raw text rather than losing the name.
        assert_eq!(
            artifact_filename("https://host/o/a%2Fb%ZZ.pdf"),
            "a%2Fb%ZZ.pdf"
        );
        // A locator that ends in a separator has no basename at all.
        assert_eq!(artifact_filename("https://host/reports/"), "file");
        assert_eq!(artifact_filename(""), "file");
        // The local half answers the same question from the output it is.
        assert_eq!(
            combined_output_filename("compounded", "shp", "zip"),
            "compounded_reports.zip"
        );
        assert_eq!(
            combined_output_filename("combined", "shp", "zip"),
            "combined_transformer.shp.zip"
        );
        assert_eq!(
            combined_output_filename("combined", "xlsx", "xlsx"),
            "combined_transformer.xlsx"
        );
        assert_eq!(
            combined_output_filename("combined", "geojsonseq", "zip"),
            "combined_transformer_geojsonl.zip"
        );
        assert_eq!(
            combined_output_filename("combined", "unheard_of", "bin"),
            "combined_transformer.bin"
        );
    }
    use serde_json::json;
    /// The hazard the campaign opened with. `DesignOutputMatrix` has written an
    /// `execution` key into the stored selection since 2026-09-07 — including
    /// the empty map of its legacy upgrade — and this type refused it, so every
    /// reader of a project edited since then (settings inspection, the native
    /// reporter's export read, the `output_selection` op) failed.
    #[test]
    fn a_stored_selection_carrying_execution_placement_is_readable() {
        let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":[],"tabular":[],"execution":{}});
        serde_json::from_value::<DesignOutputSelection>(stored.clone())
            .expect("the empty execution map the legacy upgrade writes must deserialize");
        let sheets =
            json!({"project_settings":[{"parameter":"design_export_format","value":stored}]});
        assert!(
            plan(&sheets, false).is_ok(),
            "plan must read the stored selection"
        );
    }

    /// The cloud plans data only: a named print whose setup is not in the
    /// sealed inputs refuses `plan`, but never a cloud data export.
    #[test]
    fn the_cloud_plan_keeps_data_outputs_and_leaves_every_printout_out() {
        let sheets = json!({"project_settings":[{"parameter":"design_export_format",
            "value":"excel;A3_PDF;pdf__a0-landscape-missing;voltage_drop_pdf"}]});
        assert!(
            plan(&sheets, false).is_err(),
            "the missing setup still refuses a printing plan"
        );
        let planned: Vec<_> = plan_data_outputs(&sheets)
            .expect("a data plan never consults printing inputs")
            .into_iter()
            .map(|output| output.output_id)
            .collect();
        assert_eq!(planned, ["xlsx"]);
        assert!(is_printout("pdf_a3") && is_printout("pdf__a0-landscape-missing"));
        assert!(is_printout("voltage_drop_pdf"));
        assert!(!is_printout("xlsx"));
    }

    /// One stored token, one answer. `normalize` (and therefore `plan`, the
    /// native reporter and ds-brain) has always aliased `A3_PDF` to `pdf_a3`;
    /// the selection reader skipped the table and answered `a3_pdf`, which no
    /// catalogue offers and which `output_class` calls geospatial while the
    /// grant calls it `print` — so a project restricting prints to the desktop
    /// had this output shown as running anywhere.
    #[test]
    fn a_legacy_alias_reads_the_same_in_the_selection_as_in_the_plan() {
        let selection = stored_output_selection(&json!("A3_PDF;excel")).expect("legacy row reads");
        assert_eq!(selection.tokens().unwrap(), ["pdf_a3", "xlsx"]);
        assert_eq!(output_class("pdf_a3"), "print");
        assert_eq!(output_class("xlsx"), "tabular");
        let sheets = json!({"project_settings":[{"parameter":"design_export_format","value":"A3_PDF;excel"}]});
        let planned: Vec<_> = plan(&sheets, false)
            .expect("plan reads the same row")
            .into_iter()
            .map(|output| output.output_id)
            .collect();
        assert_eq!(planned, ["xlsx", "pdf_a3"]);
        // The class the grant filters on is the class this selection answers
        // for, so a project that keeps prints on the desktop keeps this one
        // there too.
        let restricted = normalize_output_selection(
            None,
            Some(vec!["A3_PDF".into(), "excel".into()]),
            Some(BTreeMap::from([(
                "print".to_string(),
                vec![Placement::Desktop],
            )])),
        )
        .expect("a legacy list upgrades with the placement its host holds");
        assert_eq!(restricted.placements("pdf_a3"), [Placement::Desktop]);
        assert_eq!(restricted.placements("xlsx"), BOTH_PLACEMENTS);
        // The same token twice is one selection, as it is for the grant.
        assert_eq!(
            stored_output_selection(&json!("excel,xlsx,SHP,shp"))
                .unwrap()
                .tokens()
                .unwrap(),
            ["shp", "xlsx"]
        );
    }

    /// ds-brain trims and case-folds a stored placement before matching it. A
    /// selection it admits must read here, and must be written back canonical.
    #[test]
    fn a_stored_placement_is_read_the_way_the_grant_reads_it() {
        let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,"geospatial":["gpkg"],
            "execution":{"gpkg":[" Desktop ","WEB"]}});
        let selection = stored_output_selection(&stored).expect("the grant admits this project");
        assert_eq!(selection.placements("gpkg"), BOTH_PLACEMENTS);
        assert_eq!(
            serde_json::to_value(&selection).unwrap()["execution"]["gpkg"],
            json!(["desktop", "web"])
        );
        assert!(stored_output_selection(
            &json!({"schema":OUTPUT_SELECTION_SCHEMA,"geospatial":["gpkg"],"execution":{"gpkg":["cloud"]}})
        )
        .is_err());
        // Case-folding must not smuggle a repeat past the duplicate rule.
        assert!(stored_output_selection(
            &json!({"schema":OUTPUT_SELECTION_SCHEMA,"geospatial":["gpkg"],"execution":{"gpkg":["web","WEB"]}})
        )
        .is_err());
    }

    /// The Settings page saves the whole sheet through `design_config::save`.
    /// That writer must not be a second author of the export row: the bytes the
    /// output matrix chose are the bytes that are stored.
    #[test]
    fn the_settings_sheet_save_leaves_the_export_row_exactly_as_chosen() {
        let value = json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "prints":[{"layout_id":"detail","enabled":true,"formats":["pdf"]}],
            "geospatial":["gpkg"],"tabular":["xlsx"],"execution":{"print":["desktop"]}});
        let rows = json!([
            {"parameter":"Design-Export-Format","value":value},
            {"parameter":"max_cable_size","value":"3 x 70"},
        ]);
        let document = json!({"sheets":{"project_settings":rows}});
        let saved = crate::design_config::save_sheet(&document, "project_settings", &rows)
            .expect("the settings sheet saves");
        let saved = saved.as_array().expect("rows");
        // The parameter name is reduced the way every reader of this row
        // reduces it, and the selection itself is untouched.
        assert_eq!(saved[0]["parameter"], "design_export_format");
        assert_eq!(saved[0]["value"], value);
        assert_eq!(
            setting_parameter_key(&saved[0]).as_deref(),
            Some("design_export_format")
        );
        let sheets = json!({"project_settings":saved});
        assert_eq!(
            project_output_setting(&sheets).unwrap().unwrap()["value"],
            value
        );
    }

    #[test]
    fn local_matrix_does_not_change_defaults_and_requires_held_layouts() {
        let mut layout = crate::printing::default_layout();
        layout.id = "detail".into();
        let sheets = json!({"project_settings":[{"parameter":"report_formats","value":["xlsx","pdf__detail"]}],
            "printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":layout}]});
        let selection: DesignOutputSelection =
            serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "prints":[{"layout_id":"detail","formats":["pdf"]}]}))
            .unwrap();
        let original = sheets.clone();
        assert_eq!(
            selected_plan(&sheets, false, Some(&selection))
                .unwrap()
                .iter()
                .map(|o| o.output_id.as_str())
                .collect::<Vec<_>>(),
            vec!["pdf__detail"]
        );
        assert_eq!(sheets, original);
        assert_eq!(selected_plan(&sheets, false, None).unwrap().len(), 2);
        assert!(selected_plan(&json!({}), false, Some(&selection)).is_err());
        assert!(selected_plan(&sheets, true, Some(&selection)).is_err());
        let empty = DesignOutputSelection {
            schema: OUTPUT_SELECTION_SCHEMA.into(),
            prints: vec![],
            geospatial: vec![],
            tabular: vec![],
            execution: BTreeMap::new(),
            voltage_drop_report: VoltageDropReportFormat::default(),
        };
        assert!(selected_plan(&sheets, false, Some(&empty)).is_err());
    }

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
        let result = inspect_project_settings(&sheets, None).unwrap();
        assert_eq!(result["setting"]["value"], selection);
        assert_eq!(
            result["outputs"],
            serde_json::to_value(plan(&sheets, false).unwrap()).unwrap()
        );
        assert_eq!(result["papers"].as_array().unwrap().len(), 1);
        assert_eq!(result["papers"][0]["paper"]["size"], "A3");
        assert_eq!(result["source"], "project_settings");
        let defaults = inspect_project_settings(&json!({}), None).unwrap();
        assert_eq!(defaults["source"], "report_defaults");
        assert!(defaults["setting"].is_null());
        assert_eq!(defaults["papers"], json!([]));
        let mut broken = sheets;
        broken["printing_setups"] = json!([]);
        let unreadied = inspect_project_settings(&broken, None).unwrap();
        assert_eq!(unreadied["ready"], false);
        assert_eq!(unreadied["setting"], broken["project_settings"][0]);
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
        assert!(plan(&sheets, false)
            .unwrap_err()
            .contains("not in the sealed project printing inputs"));
        assert!(named_setup_id("pdf__../outside").is_none());
        let options = options(&[json!({"id":"detail","name":"Renamed"})]).unwrap();
        assert!(options
            .iter()
            .any(|o| o["value"] == "pdf__detail" && o["label"] == "PDF · Renamed"));
        assert!(!options.iter().any(|o| o["value"] == "pdf_a0"));
    }

    #[test]
    fn pdf_selection_applies_only_to_individual_reports() {
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
            ["shp", "xlsx", "geojsonseq"]
        );
        assert_eq!(output("pdf_a3").unwrap().content_type, "application/pdf");
    }
    #[test]
    fn combined_data_does_not_require_a_selected_print_layout() {
        let sheets = json!({"project_settings":[{"parameter":"design_export_format","value":["xlsx","pdf__missing"]}]});
        assert!(plan(&sheets, false).is_err());
        assert_eq!(
            plan(&sheets, true)
                .unwrap()
                .iter()
                .map(|o| o.output_id.as_str())
                .collect::<Vec<_>>(),
            ["shp", "xlsx", "geojsonseq"]
        );
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

    /// ds-brain's precedence for a data output, reproduced: exact output id,
    /// then the output class, then `*`, then both lanes. The class token is
    /// `print`, singular, as `network_reporter_individual_policy.go:206,:225`
    /// matches it — and a print never consults the matrix (next test).
    #[test]
    fn execution_placement_follows_the_brains_precedence_exactly() {
        let selection = |execution: Value| -> DesignOutputSelection {
            serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "prints":[{"layout_id":"detail","formats":["pdf"]}],
                "geospatial":["gpkg"],"tabular":["xlsx"],"execution":execution}))
            .expect("selection deserializes")
        };
        let absent = selection(json!({}));
        for output in ["gpkg", "shp", "xlsx", "network_information"] {
            assert_eq!(absent.placements(output), BOTH_PLACEMENTS, "{output}");
        }
        let exact = selection(json!({"gpkg":["desktop"],"geospatial":["web"],"*":[]}));
        assert_eq!(exact.placements("gpkg"), [Placement::Desktop]);
        assert_eq!(exact.placements("shp"), [Placement::Web]);
        assert_eq!(exact.placements("xlsx"), [] as [Placement; 0]);
        let classes =
            selection(json!({"geospatial":["desktop"],"tabular":["web"],"*":["desktop"]}));
        assert_eq!(classes.placements("gpkg"), [Placement::Desktop]);
        assert_eq!(classes.placements("xlsx"), [Placement::Web]);
        assert_eq!(classes.placements("pdf__detail"), [Placement::Desktop]);
        assert_eq!(output_class("xlsx"), "tabular");
        assert_eq!(output_class("jpeg__detail"), "print");
        assert_eq!(output_class("pdf_a0"), "print");
        assert_eq!(output_class("shp"), "geospatial");
        assert_eq!(
            selection(json!({"gpkg":["desktop","web"]}))
                .placement_map()
                .unwrap()["gpkg"],
            vec![Placement::Desktop, Placement::Web]
        );
        // Bounded, not enumerated: a key naming nothing is inert, as it is in
        // Go, and a lane repeated or a third lane is refused outright.
        assert!(selection(json!({"not_an_output":["web"]})).tokens().is_ok());
        assert!(selection(json!({"gpkg":["web","web"]}))
            .tokens()
            .is_err_and(|error| error.contains("repeats a lane")));
        assert!(serde_json::from_value::<DesignOutputSelection>(
            json!({"schema":OUTPUT_SELECTION_SCHEMA,"execution":{"gpkg":["cloud"]}})
        )
        .is_err());
    }

    /// Printing is Desktop and the Linux Server only, both `Desktop`, and the
    /// cloud never renders a printout. Every print spelling the kernel's
    /// grammar reads resolves there under any stored matrix — absent, both
    /// lanes, `web` alone or refused on every lane, by exact id, class or `*` —
    /// and reading such a project never refuses it. Data placement is not
    /// touched by the rule.
    #[test]
    fn every_print_output_resolves_to_the_desktop_whatever_the_matrix_says() {
        let prints = [
            "pdf_a0",
            "pdf_a3",
            "voltage_drop_pdf",
            "pdf__detail",
            "png__detail",
            "jpeg__detail",
            "jpg__detail",
            "svg__detail",
        ];
        for output in prints {
            assert!(is_printout(output), "{output} is a print");
            assert_eq!(output_class(output), "print", "{output}");
        }
        for execution in [
            json!({}),
            json!({"print":["desktop","web"]}),
            json!({"print":["web"]}),
            json!({"print":[]}),
            json!({"*":["web"]}),
            json!({"*":[]}),
            json!({"pdf_a0":["web"],"pdf_a3":[],"pdf__detail":["web"],
                "png__detail":["web"],"jpeg__detail":["web"],"svg__detail":[]}),
        ] {
            // The fixed PDFs sit in `geospatial`, where the legacy upgrade
            // puts them; `output_class` still reads them as prints.
            let stored = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "prints":[{"layout_id":"detail","formats":["pdf","png","jpeg"]}],
                "geospatial":["pdf_a0","pdf_a3","voltage_drop_pdf","gpkg"],"tabular":["xlsx"],
                "execution":execution}))
            .expect("reading a stored selection never refuses its placement");
            for output in prints {
                assert_eq!(
                    stored.placements(output),
                    PRINT_PLACEMENTS,
                    "{output} under {execution}"
                );
            }
            let map = stored.placement_map().unwrap();
            for token in [
                "pdf_a0",
                "pdf_a3",
                "voltage_drop_pdf",
                "pdf__detail",
                "png__detail",
                "jpeg__detail",
            ] {
                assert_eq!(map[token], PRINT_PLACEMENTS, "{token} under {execution}");
            }
        }
        let legacy = stored_output_selection(&json!("A3_PDF;pdf;jpg__detail;xlsx")).unwrap();
        for (token, placements) in legacy.placement_map().unwrap() {
            let expected = if token == "xlsx" {
                BOTH_PLACEMENTS
            } else {
                PRINT_PLACEMENTS
            };
            assert_eq!(placements, expected, "{token}");
        }
        let data = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["gpkg"],"tabular":["xlsx"],"execution":{"print":["web"],"*":["web"]}}))
        .unwrap();
        assert_eq!(data.placements("gpkg"), [Placement::Web]);
        assert_eq!(data.placements("xlsx"), [Placement::Web]);
    }

    /// The door the silent loss came through, closed where a matrix is
    /// authored and saved. ds-brain's desktop grant reads the stored matrix
    /// for a print, so an entry that reaches one without `desktop` is refused
    /// by name — never rewritten — and a refused save leaves the row as it was.
    /// Keeping data on `web` alone stays open, and so does naming both lanes.
    #[test]
    fn a_saved_matrix_may_not_place_a_print_off_the_desktop() {
        let versioned = |execution: &Value| -> DesignOutputSelection {
            serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "prints":[{"layout_id":"detail","formats":["pdf"]}],
                "geospatial":["gpkg"],"tabular":["xlsx"],"execution":execution}))
            .expect("selection deserializes")
        };
        for (execution, key) in [
            (json!({"print":["web"]}), "print"),
            (json!({"print":[]}), "print"),
            (json!({"pdf__detail":["desktop"],"print":["web"]}), "print"),
            (
                json!({"pdf__detail":["web"],"print":["desktop"]}),
                "pdf__detail",
            ),
            (json!({"pdf_a3":["web"]}), "pdf_a3"),
            (json!({"jpeg__detail":[]}), "jpeg__detail"),
            (json!({"*":["web"]}), "*"),
            (json!({"*":[]}), "*"),
        ] {
            let selection = versioned(&execution);
            assert!(
                selection.tokens().is_ok(),
                "reading {execution} still works"
            );
            let refused = normalize_output_selection(Some(selection.clone()), None, None)
                .expect_err("authoring refuses it");
            assert!(
                refused.contains(&format!("placement for {key} keeps prints off the desktop")),
                "{execution}: {refused}"
            );
            let upgraded = normalize_output_selection(
                None,
                Some(vec!["pdf__detail".into(), "xlsx".into()]),
                Some(serde_json::from_value(execution.clone()).unwrap()),
            );
            assert_eq!(upgraded, Err(refused.clone()), "the legacy upgrade too");
            let mut rows = vec![json!({"parameter":"design_export_format","value":"xlsx"})];
            let held = rows.clone();
            assert_eq!(apply_output_selection(&mut rows, &selection), Err(refused));
            assert_eq!(rows, held, "a refused save leaves the row as it was");
        }
        let data_only: DesignOutputSelection = serde_json::from_value(json!({
            "schema": OUTPUT_SELECTION_SCHEMA,
            "geospatial": ["gpkg"], "tabular": ["xlsx"],
            "execution": {"*": ["web"]}
        }))
        .unwrap();
        assert!(normalize_output_selection(Some(data_only.clone()), None, None).is_ok());
        assert_eq!(data_only.placements("gpkg"), [Placement::Web]);
        assert_eq!(data_only.placements("xlsx"), [Placement::Web]);
        for execution in [
            json!({}),
            json!({"print":["desktop"]}),
            json!({"print":["desktop","web"]}),
            json!({"*":["web"],"print":["desktop"]}),
            json!({"*":["desktop","web"]}),
            json!({"geospatial":["web"],"tabular":["web"],"gpkg":[]}),
            json!({"not_an_output":["web"]}),
        ] {
            let selection = versioned(&execution);
            assert_eq!(
                normalize_output_selection(Some(selection.clone()), None, None).as_ref(),
                Ok(&selection),
                "{execution}"
            );
            let mut rows = Vec::new();
            apply_output_selection(&mut rows, &selection).unwrap();
            assert_eq!(
                rows[0]["value"],
                serde_json::to_value(&selection).unwrap(),
                "{execution} is stored as chosen"
            );
        }
    }

    /// The empty `execution` map the Settings matrix writes on the first edit
    /// of a legacy setting is not a restriction; it means both lanes, and it
    /// survives a save/read round trip without being serialized back.
    #[test]
    fn an_empty_execution_map_means_both_lanes_and_is_not_written_back() {
        let selection: DesignOutputSelection = serde_json::from_value(
            json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":["gpkg"],"tabular":[],"execution":{}}),
        )
        .unwrap();
        assert_eq!(selection.placements("gpkg"), BOTH_PLACEMENTS);
        assert_eq!(
            serde_json::to_value(&selection).unwrap(),
            json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":["gpkg"],"tabular":[]})
        );
    }

    /// One reader for every shape a project has ever stored, including the two
    /// raster tokens the deleted TypeScript mislaid into `geospatial`.
    #[test]
    fn stored_settings_of_every_shape_read_through_one_reader() {
        let read = |value: Value| stored_output_selection(&value).unwrap();
        let png = read(json!("png__detail,jpg__detail,xlsx"));
        assert_eq!(png.prints.len(), 1);
        assert_eq!(png.prints[0].layout_id, "detail");
        assert_eq!(
            png.prints[0].formats,
            vec![PrintArtifactFormat::Png, PrintArtifactFormat::Jpeg]
        );
        assert_eq!(png.geospatial, [] as [String; 0]);
        assert_eq!(png.tabular, ["xlsx"]);
        assert_eq!(read(json!("shp;kmz")).geospatial, ["shp", "kmz"]);
        assert_eq!(read(json!(["gpkg", "xlsx"])).tabular, ["xlsx"]);
        assert_eq!(read(json!({"shp":true,"kmz":false})).geospatial, ["shp"]);
        // Reading is not authoring: an absent or empty setting is an empty
        // selection, where the authoring normalizer demands 1..32 tokens.
        for empty in [json!(null), json!(""), json!([]), json!({})] {
            assert_eq!(read(empty).tokens().unwrap(), [] as [String; 0]);
        }
        let versioned = read(
            json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":[],"tabular":[],"execution":{"gpkg":["web"]}}),
        );
        assert_eq!(versioned.placements("gpkg"), [Placement::Web]);
        assert!(
            stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":"no"}))
                .is_err()
        );
    }

    /// The five aliases, the hyphen/case normalisation, and the row a sheet
    /// without any of them gets — the job `prepare.ts` did with two names and
    /// a refusal.
    #[test]
    fn the_export_row_is_patched_through_every_alias_and_created_when_absent() {
        let selection: DesignOutputSelection = serde_json::from_value(
            json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":["gpkg"],"tabular":[]}),
        )
        .unwrap();
        for alias in [
            "design_export_format",
            "design_export_formats",
            "transformer_export_formats",
            "tr_export_formats",
            "report_formats",
            "REPORT-FORMATS",
            " design-export-format ",
        ] {
            let mut rows = vec![
                json!({"parameter":"unrelated","value":1}),
                json!({"parameter":alias,"value":"shp"}),
            ];
            assert_eq!(
                apply_output_selection(&mut rows, &selection).unwrap(),
                setting_parameter_key(&json!({"parameter":alias})).unwrap(),
                "{alias} keeps the row it already has"
            );
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[1]["value"]["geospatial"], json!(["gpkg"]));
            assert_eq!(rows[0], json!({"parameter":"unrelated","value":1}));
        }
        let mut absent = vec![json!({"parameter":"unrelated","value":1})];
        assert_eq!(
            apply_output_selection(&mut absent, &selection).unwrap(),
            SETTING_ALIASES[0]
        );
        assert_eq!(absent.len(), 2);
        assert_eq!(absent[1]["parameter"], "design_export_format");
        assert_eq!(absent[1]["value"]["schema"], OUTPUT_SELECTION_SCHEMA);
        // A patched sheet reads back as the selection it was given.
        let sheets = json!({"project_settings":absent});
        assert_eq!(
            plan(&sheets, false)
                .unwrap()
                .iter()
                .map(|output| output.output_id.as_str())
                .collect::<Vec<_>>(),
            ["gpkg"]
        );
    }

    /// A local override is one run's inventory. Placement is the project's
    /// policy and ds-brain admits against it, so an override carrying one is
    /// refused rather than quietly deciding it per invocation.
    #[test]
    fn a_local_override_may_not_carry_execution_placement() {
        let mut layout = crate::printing::default_layout();
        layout.id = "detail".into();
        let sheets = json!({"project_settings":[{"parameter":"report_formats","value":["xlsx","pdf__detail"]}],
            "printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":layout}]});
        let selection: DesignOutputSelection = serde_json::from_value(
            json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[{"layout_id":"detail","formats":["pdf"]}],
                "execution":{"pdf__detail":["desktop"]}}),
        )
        .unwrap();
        assert!(selected_plan(&sheets, false, Some(&selection))
            .is_err_and(|error| error.contains("never carry an execution placement")));
    }

    /// The refusal the GUI and `ds` both render, as keys rather than English.
    #[test]
    fn an_unready_settings_read_names_its_refusal_by_key_and_mode() {
        let sheets = json!({"project_settings":[{"parameter":"design_export_format",
            "value":{"schema":OUTPUT_SELECTION_SCHEMA,"prints":[{"layout_id":"missing","formats":["pdf"]}]}}]});
        for (mode, key) in [
            (None, "printing_inputs_incomplete_refreshed"),
            (
                Some(ReadinessMode::Refreshed),
                "printing_inputs_incomplete_refreshed",
            ),
            (
                Some(ReadinessMode::Offline),
                "printing_inputs_incomplete_offline",
            ),
        ] {
            let result = inspect_project_settings(&sheets, mode).unwrap();
            assert_eq!(result["ready"], false);
            assert_eq!(result["refusal"]["code"], "printing_inputs_incomplete");
            assert_eq!(result["refusal"]["message_key"], key);
            assert_eq!(result["refusal"]["mode"], mode.unwrap_or_default().token());
            assert!(!result["issues"].as_array().unwrap().is_empty());
        }
        assert!(
            inspect_project_settings(&json!({}), Some(ReadinessMode::Offline)).unwrap()["refusal"]
                .is_null()
        );
    }

    /// The option rows the matrix renders. A named print row carries the
    /// layout id and the formats it may be produced in, so no host recovers
    /// them by slicing `pdf__` off a value; a data row carries neither,
    /// because it has no layout.
    #[test]
    fn every_option_row_declares_what_the_matrix_needs_without_string_surgery() {
        let rows = options(&[json!({"id":"detail","name":"Detail"})]).unwrap();
        assert!(!rows.is_empty());
        for row in &rows {
            let value = row["value"].as_str().expect("option value");
            if let Some((_, id)) = named_print_output(value) {
                assert_eq!(row["layout_id"], id);
                assert_eq!(row["formats"], json!(["pdf", "png", "jpeg"]));
            } else {
                assert!(row["layout_id"].is_null(), "{value} declares a layout");
                assert!(row["formats"].is_null(), "{value} declares formats");
                assert!(output(value).is_ok(), "{value} is not a report output");
            }
        }
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
            None,
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
        assert!(normalize_output_selection(None, Some(vec![]), None).is_err());
        assert!(normalize_output_selection(None, Some(vec!["gif__a3".into()]), None).is_err());
    }

    /// Voltage drop (owner ruling 2026-09-28): a per-transformer JSON result,
    /// tabular, and produced by default. A project that stored an explicit
    /// selection keeps exactly what it chose, and the combined report stays
    /// data-only: its summary sheet rides the combined workbook.
    #[test]
    fn voltage_drop_is_a_default_tabular_json_output() {
        assert_eq!(normalize(&[]), ["shp", "kmz", "xlsx", "voltage_drop"]);
        assert!(!normalize(&["shp".into(), "xlsx".into()]).contains(&"voltage_drop".to_string()));
        let definition = output("voltage_drop").unwrap();
        assert_eq!(definition.output_id, "voltage_drop");
        assert_eq!(definition.format, "voltage_drop");
        assert_eq!(definition.content_type, "application/json");
        assert_eq!(definition.suffix, ".voltage-drop.json");
        assert_eq!(output_class("voltage_drop"), "tabular");
        let selection = stored_output_selection(&json!("shp,voltage_drop,xlsx")).unwrap();
        assert_eq!(selection.geospatial, ["shp"]);
        assert_eq!(selection.tabular, ["voltage_drop", "xlsx"]);
        assert!(options(&[])
            .unwrap()
            .iter()
            .any(|option| option["value"] == "voltage_drop"));
        let planned = plan(&json!({}), false).unwrap();
        assert_eq!(
            planned.last().map(|o| o.output_id.as_str()),
            Some("voltage_drop")
        );
        assert!(!combined(&["voltage_drop".into()]).contains(&"voltage_drop"));
    }

    #[test]
    fn voltage_drop_a4_pdf_is_an_explicit_local_print_output() {
        assert!(!DEFAULT_INDIVIDUAL.contains(&"voltage_drop_pdf"));
        let pdf = output("voltage_drop_pdf").unwrap();
        assert_eq!(pdf.output_id, "voltage_drop_pdf");
        assert_eq!(pdf.format, "voltage_drop_pdf");
        assert_eq!(pdf.content_type, "application/pdf");
        assert_eq!(pdf.suffix, " voltage drop.pdf");
        assert_eq!(output_class("voltage_drop_pdf"), "print");
        assert!(is_printout("voltage_drop_pdf"));
        assert_eq!(
            voltage_drop_pdf_filename("fill_in_bukorota"),
            "Fill In Bukorota voltage drop.pdf"
        );
        assert_eq!(
            voltage_drop_pdf_filename("Agahehe"),
            "Agahehe voltage drop.pdf"
        );
        assert_eq!(
            voltage_drop_pdf_filename("_001_main"),
            "001 Main voltage drop.pdf"
        );
        assert_eq!(
            voltage_drop_pdf_filename("  fill__in_bukorota  "),
            "Fill In Bukorota voltage drop.pdf"
        );

        let selected = json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["voltage_drop_pdf"],
            "execution":{"voltage_drop_pdf":["web"]}});
        let selection = stored_output_selection(&selected).unwrap();
        assert_eq!(
            selection.placements("voltage_drop_pdf"),
            [Placement::Desktop]
        );
        assert!(normalize_output_selection(
            Some(serde_json::from_value(selected.clone()).unwrap()),
            None,
            None,
        )
        .is_err_and(|error| error.contains("keeps prints off the desktop")));
        let sheets = json!({"project_settings":[{"parameter":"design_export_format",
            "value":selected}]});
        let planned = plan(&sheets, false).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].output_id, pdf.output_id);
        assert_eq!(planned[0].content_type, pdf.content_type);
        assert!(plan_data_outputs(&sheets).unwrap().is_empty());
        let local = serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["voltage_drop_pdf"]}))
        .unwrap();
        assert_eq!(
            selected_plan(&json!({}), false, Some(&local))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn historical_fixed_output_buckets_are_repaired_without_losing_selection() {
        let raw = json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["shp","voltage_drop","xlsx"],
            "tabular":["xlsx","voltage_drop_pdf","network_information"],
            "execution":{"voltage_drop_pdf":["desktop"]}});
        let expected_geo = ["shp", "voltage_drop_pdf"];
        let expected_tabular = ["voltage_drop", "xlsx", "network_information"];
        let read = stored_output_selection(&raw).unwrap();
        assert_eq!(read.geospatial, expected_geo);
        assert_eq!(read.tabular, expected_tabular);
        assert_eq!(read.placements("voltage_drop_pdf"), [Placement::Desktop]);

        let authored: DesignOutputSelection = serde_json::from_value(raw).unwrap();
        let normalized = normalize_output_selection(Some(authored.clone()), None, None).unwrap();
        assert_eq!(normalized.geospatial, expected_geo);
        assert_eq!(normalized.tabular, expected_tabular);
        let mut rows = Vec::new();
        apply_output_selection(&mut rows, &authored).unwrap();
        assert_eq!(rows[0]["value"]["geospatial"], json!(expected_geo));
        assert_eq!(rows[0]["value"]["tabular"], json!(expected_tabular));
        assert_eq!(
            rows[0]["value"]["execution"],
            json!({"voltage_drop_pdf":["desktop"]})
        );
    }

    /// Owner ruling 2026-09-29: the extended voltage-drop report is the
    /// default. A project that never chose — no export row, a legacy token
    /// list, a versioned selection without the field — reads as extended,
    /// and its stored bytes stay free of the field every older reader
    /// refuses.
    #[test]
    fn a_selection_that_never_chose_a_voltage_drop_layout_is_extended() {
        let versioned = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],
            "geospatial":["shp"],"tabular":["voltage_drop"]});
        let selection = stored_output_selection(&versioned).unwrap();
        assert_eq!(
            selection.voltage_drop_report_format(),
            VoltageDropReportFormat::Extended
        );
        assert_eq!(serde_json::to_value(&selection).unwrap(), versioned);
        for stored in [json!(null), json!("shp,voltage_drop"), versioned.clone()] {
            assert_eq!(
                stored_output_selection(&stored)
                    .unwrap()
                    .voltage_drop_report_format(),
                VoltageDropReportFormat::Extended,
                "{stored}"
            );
        }
        for sheets in [
            json!({}),
            json!({"project_settings":[{"parameter":"report_formats","value":"shp,voltage_drop"}]}),
            json!({"project_settings":[{"parameter":"design_export_format","value":versioned}]}),
        ] {
            assert_eq!(
                project_voltage_drop_report_format(&sheets).unwrap(),
                VoltageDropReportFormat::Extended,
                "{sheets}"
            );
        }
        // An explicit `extended` is honoured and written back as the absence
        // it means, as an empty `execution` map is.
        let explicit = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "tabular":["voltage_drop"],"voltage_drop_report":"extended"}))
        .unwrap();
        assert_eq!(
            explicit.voltage_drop_report,
            VoltageDropReportFormat::Extended
        );
        assert!(serde_json::to_value(&explicit).unwrap()[VOLTAGE_DROP_REPORT_FIELD].is_null());
    }

    /// `brief` survives every path a selection travels: reading, authoring
    /// normalisation, the settings-row patch and the planner reading the row
    /// back — and the reporter's reader answers it.
    #[test]
    fn a_brief_voltage_drop_layout_round_trips_through_every_path() {
        let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],
            "geospatial":["shp"],"tabular":["xlsx","voltage_drop"],"voltage_drop_report":"brief"});
        let selection = stored_output_selection(&stored).unwrap();
        assert_eq!(
            selection.voltage_drop_report_format(),
            VoltageDropReportFormat::Brief
        );
        assert_eq!(serde_json::to_value(&selection).unwrap(), stored);
        // Read as ds-brain reads it: trimmed and case-folded, written canonical.
        let spelled = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "tabular":["voltage_drop"],"voltage_drop_report":" Brief "}))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&spelled).unwrap()[VOLTAGE_DROP_REPORT_FIELD],
            "brief"
        );
        let normalized = normalize_output_selection(Some(selection.clone()), None, None).unwrap();
        assert_eq!(normalized, selection);
        let mut rows = vec![json!({"parameter":"design_export_format","value":"shp"})];
        apply_output_selection(&mut rows, &normalized).unwrap();
        assert_eq!(rows[0]["value"], stored);
        let sheets = json!({"project_settings":rows});
        assert_eq!(
            project_voltage_drop_report_format(&sheets).unwrap(),
            VoltageDropReportFormat::Brief
        );
        assert_eq!(
            plan(&sheets, false)
                .unwrap()
                .iter()
                .map(|output| output.output_id.as_str())
                .collect::<Vec<_>>(),
            ["shp", "xlsx", "voltage_drop"]
        );
        let inspected = inspect_project_settings(&sheets, None).unwrap();
        assert_eq!(inspected["voltage_drop_report"], "brief");
        assert_eq!(
            inspect_project_settings(&json!({}), None).unwrap()["voltage_drop_report"],
            "extended"
        );
    }

    /// A stored value no report has is refused by name, never read as the
    /// default: a typo must not silently become a different report.
    #[test]
    fn an_unknown_voltage_drop_layout_is_refused_by_name() {
        for value in [json!("summary"), json!(""), json!(2), json!(null)] {
            let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "tabular":["voltage_drop"],"voltage_drop_report":value});
            let error = stored_output_selection(&stored).unwrap_err();
            assert!(
                error.starts_with("Invalid design output selection:"),
                "{value}: {error}"
            );
            let sheets =
                json!({"project_settings":[{"parameter":"design_export_format","value":stored}]});
            assert!(project_voltage_drop_report_format(&sheets).is_err());
            assert!(
                inspect_project_settings(&sheets, None).unwrap()["voltage_drop_report"].is_null()
            );
        }
        let error = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "voltage_drop_report":"summary"}))
        .unwrap_err();
        assert!(
            error.contains(
                "unknown voltage-drop report format `summary`, expected `extended` or `brief`"
            ),
            "{error}"
        );
    }

    /// Hosts learn the choice, its field and its default from the kernel: the
    /// selection schema and the `voltage_drop` option row both carry it.
    #[test]
    fn the_voltage_drop_layout_is_declared_with_its_default() {
        let schema = output_selection_schema();
        let property = &schema["properties"][VOLTAGE_DROP_REPORT_FIELD];
        assert_eq!(property["default"], "extended", "{schema}");
        assert!(!schema["required"]
            .as_array()
            .is_some_and(|required| required.iter().any(|r| r == VOLTAGE_DROP_REPORT_FIELD)));
        let text = schema.to_string();
        assert!(
            text.contains("\"extended\"") && text.contains("\"brief\""),
            "{text}"
        );
        let expected = json!({"field":"voltage_drop_report","values":["extended","brief"],"default":"extended"});
        assert_eq!(voltage_drop_report_option(), expected);
        let rows = options(&[]).unwrap();
        for row in &rows {
            if row["value"] == "voltage_drop" || row["value"] == "voltage_drop_pdf" {
                assert_eq!(row["report_format"], expected);
            } else {
                assert!(row["report_format"].is_null(), "{row}");
            }
            if row["value"] == "voltage_drop_pdf" {
                assert_eq!(row["selection_field"], "geospatial");
                assert_eq!(row["class"], "print");
                assert_eq!(row["paper"], "A4");
            }
            if row["value"] == "voltage_drop" {
                assert_eq!(row["selection_field"], "tabular");
            }
        }
    }

    /// The layout is project policy the reporter reads from the sealed
    /// settings; a one-run override naming one would be silently ignored.
    #[test]
    fn a_local_override_may_not_carry_a_voltage_drop_layout() {
        let sheets =
            json!({"project_settings":[{"parameter":"report_formats","value":"voltage_drop"}]});
        let brief: DesignOutputSelection = serde_json::from_value(json!({
            "schema":OUTPUT_SELECTION_SCHEMA,"tabular":["voltage_drop"],"voltage_drop_report":"brief"}))
        .unwrap();
        assert!(selected_plan(&sheets, false, Some(&brief))
            .is_err_and(|error| error.contains("never carry a voltage-drop report format")));
        let extended = DesignOutputSelection {
            voltage_drop_report: VoltageDropReportFormat::Extended,
            ..brief
        };
        assert_eq!(
            selected_plan(&sheets, false, Some(&extended))
                .unwrap()
                .len(),
            1
        );
    }
}
