//! Report output policy shared by native, WASM and the reporter. No IO.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The outputs a project that stores no recognised selection produces, the
/// saved voltage-drop JSON included. A project that stores a selection exports
/// exactly what it names, like every other output (owner correction
/// 2026-10-19). Reporting never recalculates analysis.
pub const DEFAULT_INDIVIDUAL: &[&str] = &["shp", "kmz", "xlsx", "voltage_drop"];
/// What an individual report invocation that names no outputs produces before
/// the project's own voltage-drop choice is applied ([`invocation_selection`]).
/// Prints are never implicit: each run names its papers.
pub const INVOCATION_DATA_DEFAULTS: &[&str] = &["shp", "kmz", "xlsx"];
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
/// The saved voltage-drop analysis JSON. Every LV process computes the
/// analysis; exporting it is a project output choice like any other (owner
/// correction 2026-10-19). It is produced when a selection names it and as the
/// exact companion of a selected A4 voltage-drop PDF, never as a requirement
/// of an unrelated output.
pub const VOLTAGE_DROP_ANALYSIS_OUTPUT: &str = "voltage_drop";
/// The analytical A4 voltage-drop PDF, rendered from the saved analysis.
pub const VOLTAGE_DROP_PDF_OUTPUT: &str = "voltage_drop_pdf";
/// The tabular outputs, one list for every reader that classes a token.
pub const TABULAR: &[&str] = &["xlsx", "network_information", "voltage_drop"];
/// Public archive/download filters, consumed directly by CLI declarations.
pub const DOWNLOAD_FORMATS: &[&str] = &[
    "xlsx", "shp", "kmz", "gpkg", "pdf", "png", "zip", "geojsonl",
];
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

/// How the per-transformer `voltage_drop_pdf` report is laid out. Saved
/// `voltage_drop` JSON is analysis data and has no print-layout choice. Owner ruling
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
    /// [`PRINT_PLACEMENTS`]. Data outputs, the saved voltage-drop JSON
    /// included, use ds-brain's precedence: exact output id, class, `*`, then
    /// both. An empty list refuses a data output on every lane.
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
/// The reporter applies this to the A4 PDF, never to the saved analysis bytes.
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
    let mut result = Vec::new();
    for format in INDIVIDUAL
        .iter()
        .filter(|format| !format.starts_with("pdf_"))
    {
        let definition = output(format)?;
        let (label, description, display_group) = match *format {
            "shp" => ("Shapefile layers (ZIP)", "Network layers as shapefiles in a ZIP archive.", "data_exports"),
            "kmz" => ("Google Earth layers (KMZ)", "Styled network layers for Google Earth.", "data_exports"),
            "xlsx" => ("Network workbook (XLSX)", "Classified network quantities and LV pole tables in an Excel workbook.", "reports"),
            "gpkg" => ("GeoPackage layers (GPKG)", "Network layers in one GeoPackage database.", "data_exports"),
            "geojson" => ("GeoJSON layers (ZIP)", "Network layers as GeoJSON files in a ZIP archive.", "data_exports"),
            "network_information" => ("Network information (JSON)", "Classified quantities with descriptions and units, plus transformer names and capacities; the same information table as the workbook, without geometry.", "data_exports"),
            "voltage_drop" => ("Voltage-drop results (JSON)", "Exact saved voltage-drop analysis and its status, provenance and available results. Exporting does not recalculate it.", "data_exports"),
            "voltage_drop_pdf" => ("Voltage-drop report (A4 PDF)", "A4 report presenting the saved voltage-drop analysis, with brief or extended detail. Reporting does not recalculate it.", "reports"),
            _ => return Err(format!("Output presentation is not declared: {format}")),
        };
        let file_format = definition.suffix.rsplit('.').next().unwrap_or("");
        let mut option = serde_json::json!({
            "value": format,
            "label": label,
            "description": description,
            "file_format": file_format,
            "display_group": display_group,
            "selection_field": if TABULAR.contains(format) { "tabular" } else { "geospatial" },
            "class": output_class(format),
        });
        if *format == "voltage_drop_pdf" {
            option["report_format"] = voltage_drop_report_option();
            option["paper"] = serde_json::json!("A4");
        }
        result.push(option);
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
            "description":"Sheets using this print template; available as PDF, PNG or JPEG.",
            "file_format":"pdf",
            "display_group":"reports",
            "class":"print",
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
    let format = if format == "geojsonzip" {
        "geojson"
    } else {
        format
    };
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
        format: format.into(),
        content_type,
        suffix: suffix.into(),
    })
}

/// Canonical report filename. PDF stems use title case (the protected delivery
/// spelling); exchange and raster stems retain their canonical identifier.
/// All suffixes and media types come from `output`, including named layouts.
pub fn report_filename(stem: &str, format: &str) -> Result<String, String> {
    let definition = output(format)?;
    if format == "voltage_drop_pdf" {
        return Ok(voltage_drop_pdf_filename(stem));
    }
    let stem = if definition.content_type == "application/pdf" {
        pdf_stem(stem)
    } else {
        stem.to_owned()
    };
    Ok(format!("{stem}{}", definition.suffix))
}

pub fn pdf_stem(value: &str) -> String {
    let mut start = true;
    let mut result = String::with_capacity(value.len());
    for ch in value.chars() {
        if start {
            result.extend(ch.to_uppercase());
        } else {
            result.extend(ch.to_lowercase());
        }
        start = !ch.is_alphabetic();
    }
    result
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

/// Generated run identities belong in storage keys, never in download names.
/// Match only the dated legacy report family, preserving project, date, scope
/// and compound extensions. These constants also generate Brain's projection.
pub const REPORT_DOWNLOAD_ID_PATTERN: &str =
    r"(?i)^([0-9]{6}[-_][0-9]{4}[-_]report[-_].+?)[-_][0-9a-f]{32}([._-].*|)$";
pub const REPORT_DOWNLOAD_ID_REPLACEMENT: &str = "${1}${2}";

/// A presentation name, independent of the immutable storage locator. The
/// basename and header-safe characters are shared by local and cloud readers.
pub fn report_download_filename(filename: &str) -> String {
    static RUN_ID: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or("");
    let readable = RUN_ID
        .get_or_init(|| regex::Regex::new(REPORT_DOWNLOAD_ID_PATTERN).expect("report name pattern"))
        .replace(basename, REPORT_DOWNLOAD_ID_REPLACEMENT);
    let safe: String = readable
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(character, ':' | '*' | '?' | '"' | '<' | '>' | '|')
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let safe = safe.trim().trim_end_matches('.');
    if safe.is_empty() {
        "report".into()
    } else {
        safe.into()
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
    report_download_filename(name)
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

/// Technical file format for archive members and report filters. Historical
/// archive spellings are classified once here; this never probes another path.
pub fn artifact_file_format(filename: &str) -> String {
    let name = artifact_filename(&filename.replace('\\', "/")).to_ascii_lowercase();
    if name.ends_with(".shp.zip") || name.ends_with("_shp.zip") {
        "shp".into()
    } else if name.ends_with("_geojsonl.zip") || name.ends_with(".geojsonl.zip") {
        "geojsonl".into()
    } else {
        name.rsplit('.').next().unwrap_or("").to_owned()
    }
}
pub fn artifact_paper(filename: &str) -> Option<String> {
    let name = artifact_filename(filename).to_ascii_lowercase();
    ["a0", "a1", "a2", "a3", "a4"]
        .into_iter()
        .find(|paper| {
            name.split(|c: char| !c.is_ascii_alphanumeric())
                .any(|token| token == *paper)
        })
        .map(str::to_owned)
}
pub fn artifact_matches_format(filename: &str, format: &str) -> bool {
    let name = artifact_filename(filename).to_ascii_lowercase();
    match format {
        "zip" => name.ends_with(".zip"),
        "xlsx" | "gpkg" | "kmz" | "shp" | "pdf" | "png" | "geojsonl" => {
            artifact_file_format(&name) == format
        }
        _ => false,
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
    let label = if label == prefix && !prefix.is_empty() {
        report_download_filename(prefix)
    } else {
        label
    };
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
    let archive_name = report_download_filename(&format!("{prefix}.zip"));
    (label, folder, archive_name)
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
    // Combined exchange identities use their fixed canonical stem.
    report_filename("combined_transformer", output_id)
        .unwrap_or_else(|_| format!("combined_transformer.{format}"))
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
    // Preserve the established absent/empty/unknown-only policy default.
    // A recognized selection keeps exactly its outputs, in policy order: the
    // saved voltage-drop JSON only when selected, or as the companion of a
    // selected A4 voltage-drop PDF.
    if !selected
        .iter()
        .any(|token| INDIVIDUAL.contains(&token.as_str()) || named_print_output(token).is_some())
    {
        return DEFAULT_INDIVIDUAL.iter().map(|s| s.to_string()).collect();
    }
    let companion = selected.iter().any(|v| v == VOLTAGE_DROP_PDF_OUTPUT);
    let mut ordered: Vec<_> = INDIVIDUAL
        .iter()
        .filter(|format| {
            (companion && **format == VOLTAGE_DROP_ANALYSIS_OUTPUT)
                || selected.iter().any(|v| v == **format)
        })
        .map(|s| s.to_string())
        .collect();
    let named: std::collections::BTreeSet<_> = selected
        .iter()
        .filter(|f| named_print_output(f).is_some())
        .cloned()
        .collect();
    ordered.extend(named);
    ordered
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

pub(crate) fn project_output_setting(sheets: &Value) -> Result<Option<&Value>, String> {
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
    selected_plan_for_purpose(sheets, is_combined, selection, OutputPurpose::Delivery)
}

#[derive(PartialEq, Eq)]
enum OutputPurpose {
    Delivery,
    LocalReview,
    AuthoringPreview,
}

pub(crate) fn selected_local_review_plan(
    sheets: &Value,
    is_combined: bool,
    selection: Option<&DesignOutputSelection>,
) -> Result<Vec<Output>, String> {
    selected_plan_for_purpose(sheets, is_combined, selection, OutputPurpose::LocalReview)
}

/// Only the kernel's typed authoring-preview path enters here. Public format
/// lists always use the delivery planner, including a list containing SVG.
pub(crate) fn selected_authoring_preview_plan(
    sheets: &Value,
    selection: &DesignOutputSelection,
) -> Result<Vec<Output>, String> {
    let tokens = selection.tokens()?;
    if tokens.len() != 1
        || !named_print_output(&tokens[0])
            .is_some_and(|(format, _)| format == PrintArtifactFormat::Svg)
    {
        return Err("An authoring preview requires exactly one named SVG page".into());
    }
    selected_plan_for_purpose(
        sheets,
        false,
        Some(selection),
        OutputPurpose::AuthoringPreview,
    )
}

fn selected_plan_for_purpose(
    sheets: &Value,
    is_combined: bool,
    selection: Option<&DesignOutputSelection>,
    purpose: OutputPurpose,
) -> Result<Vec<Output>, String> {
    let stored;
    let selection = match selection {
        Some(selection) => selection,
        None if purpose == OutputPurpose::LocalReview => {
            let row = project_output_setting(sheets)?
                .ok_or("A local printing review requires a print output selection")?;
            stored = stored_output_selection(&row["value"])?;
            &stored
        }
        None => return plan(sheets, is_combined),
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
        // Membership is defined by selection tokens, not artifact wire formats.
        if named_print_output(&output.output_id).is_none()
            && !INDIVIDUAL.contains(&output.output_id.as_str())
        {
            return Err(format!(
                "Unsupported individual report output: {}",
                output.output_id
            ));
        }
        if !seen.insert(output.output_id.clone()) {
            return Err("Local output selection repeats an output".into());
        }
        if let Some((_, id)) = named_print_output(&output.format) {
            if layouts.get(id).is_some_and(|layout| layout.mv.is_some()) {
                return Err(format!("mv_print_requires_model_geometry: {id}: use report plan-profile with same-revision model projections"));
            }
            if !layouts.contains_key(id) {
                return Err(format!("Prepare printing setup {id} before selecting it"));
            }
        }
        outputs.push(output);
    }
    // An explicit delivery or review selection needs the saved-analysis
    // companion only beside the selected A4 voltage-drop PDF, the same rule
    // as the project policy. Other selected outputs do not ask for analysis.
    // Validate authored tokens first so a companion never admits an empty
    // or invalid selection.
    let needs_analysis =
        purpose != OutputPurpose::AuthoringPreview && seen.contains(VOLTAGE_DROP_PDF_OUTPUT);
    if needs_analysis && !seen.contains(VOLTAGE_DROP_ANALYSIS_OUTPUT) {
        outputs.push(output(VOLTAGE_DROP_ANALYSIS_OUTPUT)?);
    }
    if outputs.len() > 32 {
        return Err(
            "Report exceeds 32 selected output artifacts including the saved-analysis companion"
                .into(),
        );
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
    let map_policy = crate::printing::map_policy::resolve(sheets)?;
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
            "map_policy":map_policy,"voltage_drop_report":voltage_drop_report,"mv":crate::printing::mv::inspect(sheets, None)}),
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
        "map_policy":map_policy,"voltage_drop_report":voltage_drop_report,"mv":crate::printing::mv::inspect(sheets, None)}),
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

/// Whether producing these outputs reads the saved voltage-drop analysis: the
/// analysis JSON and the A4 PDF rendered from it, nothing else. This is the
/// one export requirement rule every host consumes before acquiring a saved
/// analysis or a transformer-nature projection. No other output waits on,
/// acquires, or becomes partial because of a missing, stale or failed analysis.
pub fn requires_voltage_drop_analysis<S: AsRef<str>>(outputs: &[S]) -> bool {
    outputs.iter().any(|output| {
        matches!(
            output.as_ref(),
            VOLTAGE_DROP_ANALYSIS_OUTPUT | VOLTAGE_DROP_PDF_OUTPUT
        )
    })
}

/// Whether the project's own output policy exports the saved voltage-drop
/// JSON: its stored selection names it (or the A4 PDF it accompanies), or the
/// project stores no recognised selection and [`DEFAULT_INDIVIDUAL`] applies.
pub fn project_exports_voltage_drop(sheets: &Value) -> Result<bool, String> {
    Ok(selected_formats(sheets)?
        .iter()
        .any(|format| format == VOLTAGE_DROP_ANALYSIS_OUTPUT))
}

/// The selection one individual report invocation runs. An explicit selection
/// is that run's inventory, exactly. Without one, the run produces
/// [`INVOCATION_DATA_DEFAULTS`] and exports the saved voltage-drop JSON
/// exactly when the project's output policy does. Exporting voltage drop is
/// the project's output choice like every other output; it is never a
/// requirement of the outputs that do not read it. Every native host (CLI,
/// Server and Desktop) resolves an omitted selection here.
pub fn invocation_selection(
    sheets: &Value,
    explicit: Option<DesignOutputSelection>,
) -> Result<DesignOutputSelection, String> {
    if let Some(selection) = explicit {
        return Ok(selection);
    }
    let mut tokens: Vec<String> = INVOCATION_DATA_DEFAULTS
        .iter()
        .map(|token| (*token).to_owned())
        .collect();
    if project_exports_voltage_drop(sheets)? {
        tokens.push(VOLTAGE_DROP_ANALYSIS_OUTPUT.to_owned());
    }
    normalize_output_selection(None, Some(tokens), None)
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
                if layouts.get(id).is_some_and(|layout| layout.mv.is_some()) {
                    return Err(format!("mv_print_requires_model_geometry: {id}: use the canonical project MV setup with report plan-profile; a transformer export cannot supply model geometry"));
                }
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
#[path = "report_formats_tests.rs"]
mod tests;
