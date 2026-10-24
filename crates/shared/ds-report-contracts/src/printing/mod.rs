//! Print-layout authority shared by CLI, Canvas2D preview and vector delivery.
//! Flat page elements in physical millimetres; no tile, HTML or remote-image IO.
pub mod adjacent_networks;
pub mod admin_reference;
pub mod attribution;
pub mod catalogue;
pub mod context;
pub mod diagnostics;
pub mod elements;
pub mod frames;
pub mod layout_admission;
pub mod map_policy;
pub mod mv;
pub mod mv_booklet;
pub mod mv_context;
pub mod mv_placement;
pub mod mv_settings;
pub mod names;
pub mod network_fit;
pub mod overrides;
pub mod page_role;
pub mod placement;
pub mod pole_categories;
pub mod preview_progress;
pub mod project_crs;
pub mod renderer_defaults;
pub mod resolved_styles;
pub mod sheet_margin;
pub mod standards;
pub mod structure_descriptions;
pub mod style_capture;
pub mod style_overrides;
pub mod table_columns;
pub mod table_headings;
pub mod table_rows;
pub mod table_values;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The settings row the native reporter reads per-transformer print views from.
pub const PRINTING_VIEWS_SETTING: &str = "printing_views";

mod artifact;
pub use artifact::{validate_artifact_presentation, ArtifactPresentation};

pub const SCHEMA: &str = "ds.print-layout/v2";
pub const LEGACY_SCHEMA: &str = "ds.print-layout/v1";
pub const MAX_LAYOUT_BYTES: usize = 750_000;
/// Bounded front-matter diagrams can hold the reviewed 275-element naming page.
pub const MAX_LAYOUT_ELEMENTS: usize = 400;
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// Explicit authored purpose and page role; legacy documents remain unclassified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<catalogue::Classification>,
    /// Paper identity only; exact governed style keys determine the pens.
    #[serde(
        default,
        skip_serializing_if = "crate::style_resolution::Ink::is_colour"
    )]
    pub ink_mode: crate::style_resolution::Ink,
    /// Governed attribution typography held by the printing document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<attribution::Style>,
    /// MV publication furniture in the existing shared printing library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mv: Option<Box<mv::Template>>,
    pub schema: String,
    pub id: String,
    pub name: String,
    /// Durable paper identity. `page_mm` is retained as an exact resolved
    /// dimension for renderers and must agree with this property in V2.
    #[serde(default)]
    pub paper: PaperSpec,
    pub page_mm: [f64; 2],
    /// Canonical back-to-front sheet order. The last element is front-most.
    pub elements: Vec<Element>,
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
    #[serde(default)]
    pub styles: BTreeMap<String, PrintStyle>,
    /// Logical print layer id -> governed Style Center ref. Every value is a
    /// predictable `_print` variant. The renderer receives resolved documents
    /// separately, so a template never embeds mutable global style JSON.
    #[serde(default)]
    pub style_refs: BTreeMap<String, String>,
    /// Exact semantic choices authored in the governed template. Legacy
    /// explicit refs remain readable; no host infers a key from a ref name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub style_keys: BTreeMap<String, crate::style_resolution::Key>,
    /// Context requested by this named layout. Desktop resolves exact locally
    /// installed catalog versions and may retain bounded derived sources with
    /// the transformer room. Hosted rendering deliberately ignores this list.
    #[serde(default)]
    pub context_layers: Vec<PrintContextLayer>,
    /// Optional measured-content placement; authored rectangles remain preferred positions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<Composition>,
    /// Derive print-only polygonal transformer service bounds from held network points.
    /// The generated transformer_bounds layer retains the identity feature's properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transformer_bounds: Option<TransformerBounds>,
    /// Retired 2026-09-25: no host prints a project overview or any combined
    /// sheet (docs/printing.md, "Combined report output"). Still accepted so
    /// stored layouts, which all carry it, keep parsing; never written, never read.
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub project_overview: bool,
    /// Round an automatic fit denominator upward to this increment; explicit cameras stay exact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_rounding: Option<f64>,
    /// Optional conservative stepped engineering footprint; absence keeps rectangular fitting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_fit: Option<network_fit::NetworkFit>,
    /// Project template limits on print scale, independent of paper size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_limits: Option<network_fit::ScaleLimits>,
    /// Back-to-front layer IDs. Unlisted layers follow in their input order.
    #[serde(default)]
    pub layer_order: Vec<String>,
    #[serde(default)]
    pub style_overrides: BTreeMap<String, PrintStyleOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<PrintFocus>,
    /// The words a title block prints for this project — client, contractor,
    /// consultant, project title, contract number, drawing status, the
    /// people who draw, check and approve — as `{key}` bindings the text
    /// elements name. Deterministic: the layout carries its own values, a
    /// project customises by editing them, a transformer overrides one
    /// element's text. Keys are identifiers; values at most 2000 bytes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub title_block: BTreeMap<String, String>,
    /// Scoped patches saved with this layout's optimistic project revision.
    /// Hosts apply only the exact focused engineering identity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub transformer_overrides: BTreeMap<String, overrides::PrintOverride>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TransformerBounds {
    /// Held network layer IDs whose coordinates define each convex service envelope.
    pub source_layers: Vec<String>,
    /// Transformer identity field shared by those layers, normally transfo.
    pub identity_field: String,
    /// Metric outward buffer; engineering geometry remains unchanged.
    pub buffer_m: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct PrintStyleOverride {
    /// Explicit physical dash lengths; governed documents supply the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash_mm: Option<Vec<f64>>,
    pub visible: Option<bool>,
    pub color: Option<String>,
    pub size: Option<f64>,
    pub opacity: Option<f64>,
    #[serde(default)]
    pub palette: BTreeMap<String, String>,
    /// Explicit colour for absent/unrecognised categories; otherwise inherit the governed fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette_fallback: Option<String>,
    /// Recolour an existing casing/outline together with the body, retaining its geometry and width.
    #[serde(default, skip_serializing_if = "PrintColorScope::is_body")]
    pub color_scope: PrintColorScope,
    /// Optional explicit category field for palette and size overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub size_by_value: BTreeMap<String, f64>,
    /// Bounded optical line-weight adjustment when the geographic scale changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_weight: Option<PrintPenScale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<PrintLabelOverride>,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrintColorScope {
    #[default]
    Body,
    BodyAndBoundary,
}
impl PrintColorScope {
    fn is_body(&self) -> bool {
        *self == Self::Body
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintPenScale {
    pub reference_denominator: f64,
    pub min_factor: f64,
    pub max_factor: f64,
}
impl PrintPenScale {
    pub fn factor(&self, denominator: f64) -> Result<f64, String> {
        if !bounded(self.reference_denominator, 10., 100_000_000.)
            || !bounded(denominator, 10., 100_000_000.)
            || !bounded(self.min_factor, 0.25, 1.)
            || !bounded(self.max_factor, 1., 4.)
        {
            return Err("line-weight scale needs a valid reference denominator and factors within 0.25..1 and 1..4".into());
        }
        Ok((self.reference_denominator / denominator)
            .sqrt()
            .clamp(self.min_factor, self.max_factor))
    }
}
/// Template-local label choices; physical sizes stay constant across paper policies.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct PrintLabelOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<names::LabelDisplay>,
    pub visible: Option<bool>,
    /// Paint placed labels below all map geometry, for background reference layers.
    pub below_geometry: Option<bool>,
    /// Label-local offset in paper millimetres; rotates with along-line labels.
    pub offset_mm: Option<[f64; 2]>,
    pub size_pt: Option<f64>,
    pub color: Option<String>,
    pub halo_color: Option<String>,
    pub halo_mm: Option<f64>,
    pub allow_overlap: Option<bool>,
    pub priority: Option<f64>,
    /// Print this feature property as the label instead of the governed
    /// text-field, e.g. the key number a map index assigns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintFocus {
    /// Paint the focus shade and outline; disabled styling remains retained.
    #[serde(default = "focus_visible", skip_serializing_if = "is_focus_visible")]
    pub visible: bool,
    pub buffer_m: f64,
    pub outside_color: String,
    pub outside_opacity: f64,
    pub border_color: String,
    pub border_width_mm: f64,
    /// Independent outline alpha; omission follows the outside shade's opacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_opacity: Option<f64>,
}

fn focus_visible() -> bool {
    true
}
fn is_focus_visible(value: &bool) -> bool {
    *value
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    /// Up to four table/legend roots; adjacent flow followers move with each root.
    pub movable: Vec<String>,
    pub gap_mm: f64,
    /// Smallest permitted schedule type relative to authored font. 1 preserves type; 0.7 permits a 30% reduction.
    #[serde(default = "opaque")]
    pub min_font_scale: f64,
    /// The paper edge furniture must keep inside of, in millimetres: 0 is
    /// allowed, 1 is the preferred least (see `diagnostics`). Crossing it is
    /// a typed warning, never an error.
    #[serde(default = "one_mm", skip_serializing_if = "is_one_mm")]
    pub collision_margin_mm: f64,
}
fn one_mm() -> f64 {
    diagnostics::DEFAULT_MARGIN_MM
}
fn is_one_mm(value: &f64) -> bool {
    *value == diagnostics::DEFAULT_MARGIN_MM
}

impl Layout {
    /// The collision margin this sheet is assessed with.
    pub fn collision_margin_mm(&self) -> f64 {
        self.composition
            .as_ref()
            .map_or(diagnostics::DEFAULT_MARGIN_MM, |c| c.collision_margin_mm)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintContextLayer {
    pub id: String,
    pub label: String,
    pub source: PrintContextSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintContextSource {
    /// Explicit locally synchronized survey form, independent of visibility.
    Survey {
        form: String,
    },
    /// Explicit complete local layer, including map.draw imports.
    LocalLayer {
        layer_id: String,
    },
    Catalog {
        country: String,
        layer: String,
    },
    GoogleOpenBuildings {
        buffer_m: f64,
        min_area_m2: f64,
        min_confidence: f64,
    },
    ElevationContours {
        buffer_m: f64,
        minor_interval_m: u16,
        index_interval_m: u16,
        sample_spacing_m: f64,
    },
    /// Every governed MV model in the active project's DS Grid catalogue.
    /// Desktop resolves exact heads through its immutable model cache and
    /// projects them one at a time; the layout only owns the spatial margin.
    ProjectDsgridMv {
        buffer_m: f64,
        /// Optional metric corridor drawn around the model routes. Independent
        /// of buffer_m, which selects nearby models for the print context.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        corridor: Option<PrintCorridor>,
    },
}

/// Physical appearance and ground distance of a model-route corridor.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintCorridor {
    /// Distance on each side of the route, in ground metres.
    pub distance_m: f64,
    /// Explicit legacy authored override; omission inherits the governed pen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Explicit legacy authored override; omission inherits the governed pen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
    /// Explicit legacy authored override; omission inherits the governed pen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    /// Explicit legacy authored override; omission inherits the governed pen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash_mm: Option<Vec<f64>>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    Map,
    Text,
    Legend,
    ScaleBar,
    NorthArrow,
    Logo,
    Rectangle,
    Table,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableBinding {
    /// Explicit plain-schedule phase; absent preserves legacy layout compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<table_columns::SchedulePhase>,
    /// Bind the exported workbook's columns, headings and rows, not a hand-picked subset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workbook: Option<WorkbookTableBinding>,
    #[serde(default)]
    pub layer: String,
    /// Optional companion layers, concatenated before sorting; absent companions add no rows.
    #[serde(default)]
    pub additional_layers: Vec<String>,
    /// Ordered property keys; headings and widths follow the same order.
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub headings: Vec<String>,
    #[serde(default)]
    pub widths: Vec<f64>,
    /// Plain map schedules fit measured contents by default; proportional keeps authored weights.
    #[serde(default, skip_serializing_if = "ColumnSizing::is_content")]
    pub column_sizing: ColumnSizing,
    /// Ordered maximum column widths in mm; auto-fit and truncate outliers.
    #[serde(default)]
    pub max_widths_mm: Vec<f64>,
    /// Optional ordered minimum widths in mm, independent of heading length.
    #[serde(default)]
    pub min_widths_mm: Vec<f64>,
    /// Include headings when measuring column widths. False measures data only;
    /// headings still render, truncated within their columns without wrapping.
    #[serde(default = "table_measure_headings_default")]
    pub measure_headings: bool,
    /// Index headings with a full key above each panel, or keep them literal.
    /// Defaults to `auto_index`: a heading never widens its column.
    #[serde(
        default,
        skip_serializing_if = "table_headings::HeadingMode::is_default"
    )]
    pub heading_mode: table_headings::HeadingMode,
    /// Print few-category column values as number indexes with a legend under
    /// every panel. Absent, the table's own default applies: `auto` for the
    /// customers (house connection) schedule, `off` for every other table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_key: Option<table_values::ValueKey>,
    /// Explicitly allow a visibly warned incomplete table for an outlier.
    #[serde(default)]
    pub overflow: TableOverflow,
    /// Maximum side-by-side panels, with repeated headings and no omitted rows.
    #[serde(default = "one_panel")]
    pub panels: usize,
    /// Optional exact row counts per adjacent panel. Heights may differ; every row must be assigned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panel_rows: Vec<usize>,
    #[serde(default = "panel_gap")]
    pub gap_mm: f64,
    /// Ordered row sort keys; does not affect the authored column order.
    #[serde(default)]
    pub sort_by: Vec<String>,
    /// Workbook InfoTable preserves its shared section bands and totals.
    #[serde(default)]
    pub presentation: TablePresentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_mm: Option<f64>,
    #[serde(default)]
    pub align: TableAlignment,
    /// Number this table's layer once before the sheet is drawn, so the map
    /// labels and this index print the same key numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<FeatureNumbering>,
}

/// One 1-based key number per feature of a table's layer, written into
/// `property` in ascending `order_by` order (numbers numerically, text
/// case-insensitively, ties in layer order). A key map declares it on its
/// transformer index; nothing outside the sheet is changed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FeatureNumbering {
    pub property: String,
    pub order_by: String,
}
impl Default for TableBinding {
    fn default() -> Self {
        Self {
            phase: None,
            workbook: None,
            layer: String::new(),
            additional_layers: vec![],
            columns: vec![],
            headings: vec![],
            widths: vec![],
            column_sizing: ColumnSizing::Content,
            max_widths_mm: vec![],
            min_widths_mm: vec![],
            measure_headings: true,
            heading_mode: table_headings::HeadingMode::AutoIndex,
            value_key: None,
            overflow: TableOverflow::TruncateWarn,
            panels: 1,
            panel_rows: Vec::new(),
            gap_mm: 3.,
            sort_by: vec![],
            presentation: TablePresentation::Auto,
            row_mm: None,
            align: TableAlignment::Right,
            numbering: None,
        }
    }
}
impl TableBinding {
    /// The value key this table prints with: its explicit choice, else the
    /// table's own default ([`table_values::ValueKey::table_default`]).
    pub fn value_key(&self) -> table_values::ValueKey {
        self.value_key
            .clone()
            .unwrap_or_else(|| table_values::ValueKey::table_default(self))
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColumnSizing {
    #[default]
    Content,
    Proportional,
}
impl ColumnSizing {
    fn is_content(&self) -> bool {
        *self == Self::Content
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TableOverflow {
    Error,
    #[default]
    TruncateWarn,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TableAlignment {
    Left,
    #[default]
    Right,
}
fn one_panel() -> usize {
    1
}
fn table_measure_headings_default() -> bool {
    true
}
fn panel_gap() -> f64 {
    3.
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TablePresentation {
    #[default]
    Auto,
    Plain,
    Excel,
    WorkbookInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkbookTableBinding {
    pub sheet: WorkbookSheet,
    /// Design schedules omit installed-connection-only fields. As-built prints retain them.
    #[serde(default)]
    pub as_built: bool,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkbookSheet {
    Customers,
    Poles,
    LvLines,
    ServiceCables,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LegendBinding {
    /// Edge that anchors the measured legend inside its fixed frame.
    /// Existing layouts retain their historical right anchor.
    #[serde(default)]
    pub align: TableAlignment,
    /// Physical indentation of category swatches and labels under their layer.
    #[serde(default = "legend_indent")]
    pub indent_mm: f64,
    /// Logical layer IDs in the desired order; empty selects all geometry layers.
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub titles: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub categorical: bool,
    /// Ordered source fields whose observed combinations each need a legend row.
    /// Omitted keeps the historical primary-colour category inference.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub category_fields: BTreeMap<String, Vec<String>>,
    #[serde(default = "legend_row")]
    pub row_mm: f64,
}
fn legend_indent() -> f64 {
    4.
}
fn legend_row() -> f64 {
    6.
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintViewport {
    /// Human-facing viewport name inside the template.
    pub name: String,
    /// Runtime slot bound to a project named geographic view at export time.
    pub view_slot: String,
    /// Geographic drawing is clipped to the owning map element's rectangle.
    #[serde(default = "yes")]
    pub clip: bool,
    /// Fit the design around these page elements using their rendered bounds.
    /// Explicit geographic cameras keep their authored scale and centre.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fit_around: Vec<String>,
    /// An optional coordinate grid over the map: lines, edge ticks and edge
    /// labels in Rwanda's planar grid or in WGS84 degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graticule: Option<Graticule>,
}

/// The grid a graticule is drawn and labelled in.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraticuleCrs {
    /// Rwanda's NIX/ITRF 2005 Transverse Mercator grid, metres — the planar
    /// default a field sheet is read against.
    #[default]
    TmRwanda,
    /// Geographic degrees, labelled in degrees-minutes-seconds.
    Wgs84,
}

/// A graticule as a layout authors it. The renderer keeps the grid
/// subordinate to the map: a thin light neutral line, a round interval that
/// puts about six lines across the frame when none is authored, coordinates
/// read at the frame edges inside the neatline.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Graticule {
    #[serde(default)]
    pub crs: GraticuleCrs,
    /// Line spacing in the grid's units (metres, or degrees for WGS84);
    /// omitted picks a round value from the frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<f64>,
    /// Draw the grid lines across the map; ticks and labels stay at the edges.
    #[serde(default = "yes")]
    pub lines: bool,
    #[serde(default = "yes")]
    pub labels: bool,
    #[serde(default = "graticule_color")]
    pub color: String,
    /// Coordinate label ink, independent of the grid lines and edge ticks.
    /// Omitted retains the grid's historical shared colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_color: Option<String>,
    #[serde(default = "graticule_width")]
    pub width_mm: f64,
    #[serde(default = "graticule_font")]
    pub font_pt: f64,
}
impl Default for Graticule {
    fn default() -> Self {
        Self {
            crs: GraticuleCrs::TmRwanda,
            interval: None,
            lines: true,
            labels: true,
            color: graticule_color(),
            label_color: None,
            width_mm: graticule_width(),
            font_pt: graticule_font(),
        }
    }
}
fn graticule_color() -> String {
    "#6B7280".into()
}
fn graticule_width() -> f64 {
    0.12
}
fn graticule_font() -> f64 {
    6.
}

/// One reproducible geographic view bound to a layout viewport at render time.
/// The layout owns physical millimetres; this value owns map pan, zoom and
/// rotation. It can therefore vary by transformer without cloning the paper.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeographicView {
    /// Fit these WGS84 bounds into the physical viewport. `padding_percent`
    /// expands the bounds before fitting and is applied on all four sides.
    Fit {
        extent_wgs84: [f64; 4],
        #[serde(default)]
        padding_percent: f64,
        #[serde(default)]
        rotation_deg: f64,
    },
    /// Pin the exact map camera. Scale denominator is ground millimetres per
    /// paper millimetre, so 1000.0 means 1:1000.
    Camera {
        center_wgs84: [f64; 2],
        scale_denominator: f64,
        #[serde(default)]
        rotation_deg: f64,
    },
}

impl GeographicView {
    pub fn validate(self) -> Result<(), String> {
        let valid_point =
            |point: [f64; 2]| bounded(point[0], -180., 180.) && bounded(point[1], -85., 85.);
        match self {
            Self::Fit {
                extent_wgs84,
                padding_percent,
                rotation_deg,
            } => {
                if !valid_point([extent_wgs84[0], extent_wgs84[1]])
                    || !valid_point([extent_wgs84[2], extent_wgs84[3]])
                    || extent_wgs84[0] >= extent_wgs84[2]
                    || extent_wgs84[1] >= extent_wgs84[3]
                    || !bounded(padding_percent, 0., 100.)
                    || !bounded(rotation_deg, -180., 180.)
                {
                    return Err("invalid fitted geographic print view".into());
                }
            }
            Self::Camera {
                center_wgs84,
                scale_denominator,
                rotation_deg,
            } => {
                if !valid_point(center_wgs84)
                    || !bounded(scale_denominator, 10., 100_000_000.)
                    || !bounded(rotation_deg, -180., 180.)
                {
                    return Err("invalid geographic print camera".into());
                }
            }
        }
        Ok(())
    }
}

fn yes() -> bool {
    true
}
fn is_left(align: &TextAlign) -> bool {
    *align == TextAlign::Left
}

/// Where a text element's lines sit inside its frame.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A bundled typeface family. Hosts measure and draw only faces every
/// renderer ships, so a PDF, a raster and a preview agree glyph for glyph.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FontFamily {
    #[default]
    Sans,
    Serif,
}

/// Authored typography of one text element: family, weight, slant and
/// letter spacing. Omission keeps the renderer's regular sans face.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    #[serde(default, skip_serializing_if = "is_sans")]
    pub family: FontFamily,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// Extra space after each letter, in em of the element's font size.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tracking_em: f64,
    /// Physical outward halo for authored title text. Omission preserves
    /// historical typography; project/purpose documents own colour and width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halo: Option<TextHalo>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextHalo {
    pub color: String,
    pub width_mm: f64,
}
fn is_sans(family: &FontFamily) -> bool {
    *family == FontFamily::Sans
}
fn is_zero(value: &f64) -> bool {
    *value == 0.
}
impl TextStyle {
    pub fn validate(&self) -> Result<(), String> {
        if !self.tracking_em.is_finite() || !(0.0..=0.5).contains(&self.tracking_em) {
            return Err("text tracking_em must be between 0 and 0.5".into());
        }
        if self.family == FontFamily::Serif && self.italic {
            return Err("no bundled serif italic face; use sans italic or upright serif".into());
        }
        if self
            .halo
            .as_ref()
            .is_some_and(|halo| !color(&halo.color) || !bounded(halo.width_mm, 0., 5.))
        {
            return Err("text halo requires a colour and finite width from 0 through 5 mm".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub id: String,
    pub kind: ElementKind,
    pub rect: [f64; 4],
    #[serde(default)]
    pub text: String,
    /// Horizontal alignment of a text element inside its frame; other kinds
    /// ignore it.
    #[serde(default, skip_serializing_if = "is_left")]
    pub align: TextAlign,
    /// Typeface, weight, slant and letter spacing of a text element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
    #[serde(default)]
    pub asset: String,
    #[serde(default = "font")]
    pub font_pt: f64,
    #[serde(default = "ink")]
    pub color: String,
    #[serde(default = "white")]
    pub fill: String,
    /// Whole-element opacity. Opaque frames use 1.0 and therefore fully mask
    /// a map placed below them in the canonical sheet order.
    #[serde(default = "opaque")]
    pub opacity: f64,
    /// Governed physical rectangle pen; omitted elements inherit renderer policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<LegendBinding>,
    /// Present only for map elements. The element rectangle is the physical
    /// print viewport; geographic bounds are supplied by the runtime binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<PrintViewport>,
    /// Place this content-sized table/legend below an earlier table/legend,
    /// sharing its right edge. Authored rect dimensions remain upper bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<ContentFlow>,
    /// Measured furniture anchoring; auto hugs the sheet at 5 mm. Flow takes precedence.
    #[serde(default, skip_serializing_if = "sheet_margin::is_auto")]
    pub sheet_anchor: sheet_margin::SheetAnchor,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContentFlow {
    pub after: String,
    /// Attach to the measured predecessor, not its unused frame space.
    #[serde(default)]
    pub placement: ContentPlacement,
    #[serde(default = "content_flow_gap")]
    pub gap_mm: f64,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentPlacement {
    /// Historical flow: below the predecessor, sharing the frame's right edge.
    #[default]
    BelowRight,
    /// Stack content below the predecessor with a common left edge.
    BelowLeft,
    /// Dock a schedule directly to the predecessor's right, sharing its top edge.
    RightTop,
}
impl ContentFlow {
    pub fn origin(&self, prior: [f64; 4], width: f64) -> [f64; 2] {
        match self.placement {
            ContentPlacement::BelowRight => [
                prior[0] + prior[2] - width,
                prior[1] + prior[3] + self.gap_mm,
            ],
            ContentPlacement::BelowLeft => [prior[0], prior[1] + prior[3] + self.gap_mm],
            ContentPlacement::RightTop => [prior[0] + prior[2] + self.gap_mm, prior[1]],
        }
    }
}
/// Physical gap between measured content frames.
fn content_flow_gap() -> f64 {
    5.
}
fn font() -> f64 {
    10.
}
fn ink() -> String {
    "#253747".into()
}
fn white() -> String {
    "#ffffff".into()
}
fn opaque() -> f64 {
    1.
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub label: String,
    pub png_data_url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintStyle {
    /// Physical polygon hatching, optionally restricted by an exact property vocabulary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hatch: Option<PrintHatch>,
    pub stroke: String,
    pub width_mm: f64,
    /// Opacity of strokes, casings and point outlines in physical output.
    #[serde(default = "opaque", skip_serializing_if = "is_opaque")]
    pub stroke_opacity: f64,
    #[serde(default = "white")]
    pub fill: String,
    /// Opacity of polygon and point fills in physical output.
    #[serde(default = "opaque", skip_serializing_if = "is_opaque")]
    pub fill_opacity: f64,
    #[serde(default)]
    pub casing: String,
    #[serde(default)]
    pub casing_mm: f64,
    #[serde(default)]
    pub dash_mm: Vec<f64>,
    #[serde(default = "radius")]
    pub radius_mm: f64,
    #[serde(default)]
    pub label_field: String,
    #[serde(default = "font")]
    pub label_pt: f64,
    #[serde(default)]
    pub color_field: String,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub width_field: String,
    #[serde(default)]
    pub widths: BTreeMap<String, f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintHatch {
    pub color: String,
    pub width_mm: f64,
    pub spacing_mm: f64,
    pub angle_deg: f64,
    #[serde(default)]
    pub match_field: String,
    #[serde(default)]
    pub match_values: Vec<String>,
}
impl PrintHatch {
    pub fn validate(&self) -> Result<(), String> {
        if !color(&self.color)
            || !bounded(self.width_mm, 0.05, 2.)
            || !bounded(self.spacing_mm, 0.5, 20.)
            || self.width_mm >= self.spacing_mm
            || !bounded(self.angle_deg, -180., 180.)
            || self.match_field.len() > 120
            || self.match_values.len() > 2000
            || self
                .match_values
                .iter()
                .any(|s| s.is_empty() || s.len() > 200)
            || (self.match_field.is_empty() != self.match_values.is_empty())
        {
            return Err("invalid physical print hatch or property selector".into());
        }
        Ok(())
    }
    pub fn matches(&self, properties: &serde_json::Value) -> bool {
        self.match_field.is_empty()
            || properties[&self.match_field]
                .as_str()
                .is_some_and(|s| self.match_values.iter().any(|v| v == s))
    }
}
fn radius() -> f64 {
    1.2
}
fn is_opaque(value: &f64) -> bool {
    (*value - 1.).abs() < f64::EPSILON
}
impl Default for PrintStyle {
    fn default() -> Self {
        Self {
            hatch: None,
            stroke: ink(),
            width_mm: 0.35,
            stroke_opacity: 1.,
            fill: white(),
            fill_opacity: 1.,
            casing: String::new(),
            casing_mm: 0.,
            dash_mm: vec![],
            radius_mm: radius(),
            label_field: String::new(),
            label_pt: 8.,
            color_field: String::new(),
            colors: BTreeMap::new(),
            width_field: String::new(),
            widths: BTreeMap::new(),
        }
    }
}
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|c| c.is_ascii_hexdigit())
}
fn bounded(value: f64, low: f64, high: f64) -> bool {
    value.is_finite() && (low..=high).contains(&value)
}
/// A stroke width that is either a printable width or exactly zero.
///
/// Zero is the governed *label only* state: an administrative boundary is
/// labelled on every sheet but its geometry is never drawn, and the style has
/// to be able to say so. A positive width still obeys the physical minimum,
/// so nothing is ever promoted to a sub-`low` hairline the operator did not
/// author — the 0.1 mm workaround a refused zero used to force.
fn no_stroke_or_bounded(value: f64, low: f64, high: f64) -> bool {
    value == 0. || bounded(value, low, high)
}
/// One closed physical pen contract, shared by layouts and governed style documents.
pub fn validate_physical_style(id: &str, style: &PrintStyle) -> Result<(), String> {
    if let Some(hatch) = &style.hatch {
        hatch.validate()?;
    }
    if !identifier(id)
        || !color(&style.stroke)
        || !color(&style.fill)
        || (!style.casing.is_empty() && !color(&style.casing))
        || !no_stroke_or_bounded(style.width_mm, 0.05, 10.)
        || !bounded(style.stroke_opacity, 0., 1.)
        || !bounded(style.fill_opacity, 0., 1.)
        || !bounded(style.casing_mm, 0., 10.)
        || !bounded(style.radius_mm, 0.1, 20.)
        || !bounded(style.label_pt, 4., 40.)
        || style.dash_mm.len() > 8
        || style.dash_mm.iter().any(|v| !bounded(*v, 0.1, 100.))
        || style.colors.len() > 50
        || style.widths.len() > 50
        || style.colors.values().any(|c| !color(c))
        || style
            .widths
            .values()
            .any(|v| !no_stroke_or_bounded(*v, 0.05, 10.))
    {
        return Err(format!("invalid print style {id}"));
    }
    if [&style.color_field, &style.width_field, &style.label_field]
        .iter()
        .any(|f| f.len() > 120)
        || style
            .colors
            .keys()
            .chain(style.widths.keys())
            .any(|v| v.len() > 200)
    {
        return Err("style field or category is too long".into());
    }
    Ok(())
}

pub fn validate_label_override(label: &PrintLabelOverride) -> Result<(), String> {
    if label.size_pt.is_some_and(|v| !bounded(v, 1., 60.))
        || label.halo_mm.is_some_and(|v| !bounded(v, 0., 5.))
        || label
            .offset_mm
            .is_some_and(|v| v.iter().any(|n| !bounded(*n, -20., 20.)))
        || label.priority.is_some_and(|v| !bounded(v, -1000., 1000.))
        || label.color.as_ref().is_some_and(|c| !color(c))
        || label.halo_color.as_ref().is_some_and(|c| !color(c))
        || label.field.as_ref().is_some_and(|f| !identifier(f))
    {
        return Err("invalid physical label override".into());
    }
    Ok(())
}

pub fn validate(layout: &Layout) -> Result<(), String> {
    validate_with_semantic_layers(layout, &BTreeSet::new())
}
/// Governed standard documents declare resolver layer keys before refs exist.
/// This validates their document controls without selecting a style document.
pub(super) fn validate_with_semantic_layers(
    layout: &Layout,
    semantic_layers: &BTreeSet<String>,
) -> Result<(), String> {
    if semantic_layers.iter().any(|id| !identifier(id)) {
        return Err("invalid semantic print layer key".into());
    }
    if let Some(classification) = &layout.classification {
        classification.admit()?;
    }
    if let Some(style) = &layout.attribution {
        style.validate()?;
    }
    if !layout.transformer_overrides.is_empty() {
        let mut base = layout.clone();
        base.transformer_overrides.clear();
        let layouts = BTreeMap::from([(base.id.clone(), base)]);
        let overrides = layout
            .transformer_overrides
            .iter()
            .map(|(name, patch)| {
                (
                    name.clone(),
                    BTreeMap::from([(layout.id.clone(), patch.clone())]),
                )
            })
            .collect();
        overrides::validate_inventory(&overrides, &layouts)?;
    }

    adjacent_networks::validate_selection(layout)?;
    if let Some(mv) = &layout.mv {
        mv::validate_template(layout, mv).map_err(|e| e.to_string())?;
    }
    if layout.schema == LEGACY_SCHEMA {
        return upgrade(layout.clone()).map(|_| ());
    }
    if layout.schema != SCHEMA
        || !identifier(&layout.id)
        || layout.name.trim().is_empty()
        || layout.name.len() > 160
    {
        return Err("invalid print layout identity".into());
    }
    if serde_json::to_vec(layout).map_err(|e| e.to_string())?.len() > MAX_LAYOUT_BYTES {
        return Err("print layout exceeds 750 KB including logos".into());
    }
    if layout.page_mm.iter().any(|v| !bounded(*v, 50., 2000.)) {
        return Err("invalid page dimensions".into());
    }
    if layout.elements.is_empty() || layout.elements.len() > MAX_LAYOUT_ELEMENTS {
        return Err(format!(
            "print layout needs 1..{MAX_LAYOUT_ELEMENTS} page elements"
        ));
    }
    let resolved_page = layout.paper.page_mm()?;
    if resolved_page
        .iter()
        .zip(layout.page_mm)
        .any(|(expected, actual)| (expected - actual).abs() > 1e-7)
    {
        return Err("paper properties and resolved page dimensions disagree".into());
    }
    if layout
        .scale_rounding
        .is_some_and(|step| !bounded(step, 1., 1_000_000.))
    {
        return Err("scale rounding increment must be 1..1000000".into());
    }
    if let Some(bounds) = &layout.transformer_bounds {
        if bounds.source_layers.is_empty()
            || bounds.source_layers.len() > 16
            || bounds.source_layers.iter().any(|id| !identifier(id))
            || bounds.source_layers.iter().collect::<BTreeSet<_>>().len()
                != bounds.source_layers.len()
            || !identifier(&bounds.identity_field)
            || !bounded(bounds.buffer_m, 0., 500.)
        {
            return Err("transformer polygon bounds need 1..16 unique layer IDs, an identity field and a 0..500 metre buffer".into());
        }
    }
    if let Some(policy) = &layout.network_fit {
        policy.validate()?;
    }
    if let Some(limits) = &layout.scale_limits {
        limits.validate()?;
    }
    let mut ids = BTreeSet::new();
    let mut maps = 0;
    for element in &layout.elements {
        if let Some(flow) = &element.flow {
            if !matches!(element.kind, ElementKind::Table | ElementKind::Legend)
                || !bounded(flow.gap_mm, 0., 50.)
                || !ids.contains(&flow.after)
                || !layout.elements.iter().any(|previous| {
                    previous.id == flow.after
                        && matches!(previous.kind, ElementKind::Table | ElementKind::Legend)
                })
            {
                return Err(
                    "content flow must follow an earlier table or legend with a 0..50 mm gap"
                        .into(),
                );
            }
        }
        if !identifier(&element.id) || !ids.insert(&element.id) {
            return Err("page element ids must be unique".into());
        }
        let [x, y, w, h] = element.rect;
        let minimum_dimension = if element.kind == ElementKind::Rectangle {
            0.01
        } else {
            1.
        };
        if !bounded(x, 0., layout.page_mm[0])
            || !bounded(y, 0., layout.page_mm[1])
            || !bounded(w, minimum_dimension, layout.page_mm[0])
            || !bounded(h, minimum_dimension, layout.page_mm[1])
            || x + w > layout.page_mm[0] + 1e-7
            || y + h > layout.page_mm[1] + 1e-7
        {
            return Err(format!("element {} lies outside the page", element.id));
        }
        if !bounded(element.font_pt, 3.5, 144.)
            || !color(&element.color)
            || !color(&element.fill)
            || !bounded(element.opacity, 0., 1.)
            || element
                .stroke_mm
                .is_some_and(|v| element.kind != ElementKind::Rectangle || !bounded(v, 0., 5.))
            || element.text.len() > 4000
        {
            return Err("invalid element typography or color".into());
        }
        if let Some(numbering) = element.table.as_ref().and_then(|t| t.numbering.as_ref()) {
            let table = element.table.as_ref().expect("numbering lives on a table");
            if table.layer.is_empty()
                || !identifier(&numbering.property)
                || !identifier(&numbering.order_by)
            {
                return Err(format!(
                    "table {} numbering needs its layer and identifier property and order_by",
                    element.id
                ));
            }
        }
        if let Some(style) = &element.text_style {
            if element.kind != ElementKind::Text {
                return Err(format!(
                    "element {} carries text_style but is not text",
                    element.id
                ));
            }
            style
                .validate()
                .map_err(|e| format!("element {}: {e}", element.id))?;
        }
        if element.kind == ElementKind::Map {
            maps += 1;
            let viewport = element
                .viewport
                .as_ref()
                .ok_or("a map element needs print viewport properties")?;
            if viewport.name.trim().is_empty()
                || viewport.name.len() > 160
                || !identifier(&viewport.view_slot)
                || !viewport.clip
            {
                return Err("invalid or unclipped print viewport".into());
            }
            let mut obstacles = BTreeSet::new();
            if let Some(graticule) = &viewport.graticule {
                if graticule
                    .interval
                    .is_some_and(|step| !(step.is_finite() && step > 0.))
                    || !bounded(graticule.width_mm, 0.05, 1.)
                    || !bounded(graticule.font_pt, 4., 12.)
                    || !color(&graticule.color)
                    || graticule
                        .label_color
                        .as_ref()
                        .is_some_and(|ink| !color(ink))
                {
                    return Err(
                        "graticule needs a positive interval, 0.05-1 mm lines, 4-12 pt labels and a hex colour"
                            .into(),
                    );
                }
            }
            if viewport.fit_around.len() > 32
                || viewport.fit_around.iter().any(|id| {
                    !obstacles.insert(id)
                        || !layout.elements.iter().any(|candidate| {
                            &candidate.id == id && candidate.kind != ElementKind::Map
                        })
                })
            {
                return Err("viewport fit-around must name up to 32 distinct page elements".into());
            }
        } else if element.viewport.is_some() {
            return Err("only map elements can define print viewports".into());
        }
        if element.kind == ElementKind::Table {
            let table = element.table.as_ref().ok_or("table needs a data binding")?;
            table_columns::validate_phase(table)?;
            if let Some(key) = &table.value_key {
                key.validate(table.workbook.is_none().then_some(table.columns.as_slice()))?;
            }
            if (!identifier(&table.layer) && table.workbook.is_none())
                || table.additional_layers.len() > 16
                || table.additional_layers.iter().enumerate().any(|(i, id)| {
                    !identifier(id)
                        || id == &table.layer
                        || table.additional_layers[..i].contains(id)
                })
                || (table.columns.is_empty() && table.workbook.is_none())
                || (table.workbook.is_some()
                    && (!table.columns.is_empty()
                        || !table.additional_layers.is_empty()
                        || !table.sort_by.is_empty()))
                || !(1..=8).contains(&table.panels)
                || table.panel_rows.len() > table.panels
                || table.panel_rows.iter().any(|n| *n == 0 || *n > 1_000_000)
                || !bounded(table.gap_mm, 0., 30.)
                || table.row_mm.is_some_and(|v| !bounded(v, 2., 30.))
                || table.sort_by.len() > 12
                || table.sort_by.iter().any(|v| v.is_empty() || v.len() > 160)
                || table.columns.len() > table_headings::MAX_COLUMNS
                || table.columns.len() != table.headings.len()
                || table.columns.len() != table.widths.len()
                || table.columns.iter().any(|v| v.is_empty() || v.len() > 160)
                || table.headings.iter().any(|v| v.len() > 160)
                || table.widths.iter().any(|v| !bounded(*v, 0.1, 1000.))
                || (!table.max_widths_mm.is_empty()
                    && (table.max_widths_mm.len() != table.columns.len()
                        || table.max_widths_mm.iter().any(|v| !bounded(*v, 3., 1000.))))
                || (!table.min_widths_mm.is_empty()
                    && (table.min_widths_mm.len() != table.columns.len()
                        || table.min_widths_mm.iter().enumerate().any(|(i, v)| {
                            !bounded(*v, 0., 1000.)
                                || table.max_widths_mm.get(i).is_some_and(|max| v > max)
                        })))
            {
                return Err("invalid table binding, headings or column widths".into());
            }
        } else if element.table.is_some() {
            return Err("only table elements can bind tabular data".into());
        }
        if let Some(legend) = &element.legend {
            if element.kind != ElementKind::Legend
                || legend.layers.len() > 128
                || legend.layers.iter().any(|v| v.is_empty() || v.len() > 160)
                || legend.titles.len() > 128
                || legend
                    .titles
                    .iter()
                    .any(|(k, v)| k.len() > 160 || v.len() > 160)
                || legend.category_fields.len() > 128
                || legend.category_fields.iter().any(|(layer, fields)| {
                    layer.is_empty()
                        || layer.len() > 160
                        || layer.trim() != layer
                        || layer.chars().any(char::is_control)
                        || fields.is_empty()
                        || fields.len() > 3
                        || fields.iter().any(|field| {
                            field.is_empty()
                                || field.len() > 120
                                || field.trim() != field
                                || field.chars().any(char::is_control)
                        })
                        || fields
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            != fields.len()
                        || (!legend.layers.is_empty() && !legend.layers.contains(layer))
                })
                || (!legend.categorical && !legend.category_fields.is_empty())
                || !bounded(legend.row_mm, 3., 30.)
                || !bounded(legend.indent_mm, 0., 20.)
            {
                return Err("invalid legend controls".into());
            }
        }
        if element.kind == ElementKind::Logo && !layout.assets.contains_key(&element.asset) {
            return Err(format!(
                "logo {} needs a held project/global asset",
                element.id
            ));
        }
    }
    // A geographic sheet has one map frame. Front matter (a cover or a
    // naming-convention page) has none, and so nothing a map alone honours.
    match maps {
        0 => refuse_map_features(layout)?,
        1 => {}
        _ => {
            return Err(
                "a geographic sheet must have exactly one map frame; front matter has none".into(),
            );
        }
    }
    if let Some(composition) = &layout.composition {
        let map = layout.elements.iter().find(|e| e.kind == ElementKind::Map);
        let mut seen = BTreeSet::new();
        if composition.movable.is_empty()
            || composition.movable.len() > 4
            || !bounded(composition.gap_mm, 0., 30.)
            || !bounded(composition.min_font_scale, 0.7, 1.)
            || !bounded(
                composition.collision_margin_mm,
                0.,
                diagnostics::MAX_MARGIN_MM,
            )
            || composition.movable.iter().any(|id| {
                !seen.insert(id)
                    || !layout.elements.iter().any(|e| {
                        &e.id == id
                            && matches!(e.kind, ElementKind::Table | ElementKind::Legend)
                            && e.flow.is_none()
                    })
                    || !map
                        .and_then(|e| e.viewport.as_ref())
                        .is_some_and(|v| v.fit_around.contains(id))
            })
        {
            return Err(
                "composition needs unique content roots in the map fit-around list; flowed followers move with their root"
                    .into(),
            );
        }
    }
    if layout.assets.len() > 12 || layout.styles.len() > 100 {
        return Err("too many assets or layer styles".into());
    }
    for (id, asset) in &layout.assets {
        use base64::Engine;
        if !identifier(id) || asset.label.len() > 160 {
            return Err("invalid logo identity".into());
        }
        let data = asset
            .png_data_url
            .strip_prefix("data:image/png;base64,")
            .ok_or("logos must be held PNG bytes, never remote URLs")?;
        if data.len() > 350_000 {
            return Err("logo exceeds 256 KiB; resize it before saving".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| "invalid PNG logo")?;
        if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
            return Err("invalid PNG logo header".into());
        }
        for chunk in [&bytes[16..20], &bytes[20..24]] {
            let n = u32::from_be_bytes(chunk.try_into().expect("four bytes"));
            if n == 0 || n > 4096 {
                return Err("logo dimensions exceed 4096 pixels".into());
            }
        }
    }
    for (id, style) in &layout.styles {
        validate_physical_style(id, style)?;
    }
    if layout.title_block.len() > 64
        || layout
            .title_block
            .iter()
            .any(|(key, value)| !identifier(key) || value.len() > 2000)
    {
        return Err("title block carries at most 64 identifier keys of up to 2000 bytes".into());
    }
    if layout.style_keys.len() > 100 {
        return Err("print style keys exceed 100 layer bindings".into());
    }
    for (layer, key) in &layout.style_keys {
        if !identifier(layer) || key.target != crate::style_resolution::Target::Print {
            return Err(format!(
                "print layer {layer} requires an exact print resolution key"
            ));
        }
        key.validate().map_err(|error| error.to_string())?;
    }
    if layout.style_refs.len() > 100
        || layout.style_refs.iter().any(|(layer, reference)| {
            !identifier(layer)
                || reference.len() > 500
                || reference.len() <= "_print".len()
                || !reference.ends_with("_print")
        })
    {
        return Err("invalid print style references".into());
    }
    if layout.style_overrides.len() > 100
        || layout.style_overrides.iter().any(|(id, o)| {
            !identifier(id)
                || (!layout.style_refs.contains_key(id)
                    && !layout.style_keys.contains_key(id)
                    && !semantic_layers.contains(id))
                || o.color.as_ref().is_some_and(|c| !color(c))
                || o.size.is_some_and(|v| !bounded(v, 0., 100.))
                || o.scale_weight
                    .as_ref()
                    .is_some_and(|p| o.size.is_none() || p.factor(p.reference_denominator).is_err())
                || o.opacity.is_some_and(|v| !bounded(v, 0., 1.))
                || o.dash_mm
                    .as_ref()
                    .is_some_and(|v| v.len() > 8 || v.iter().any(|n| !bounded(*n, 0.1, 100.)))
                || o.label
                    .as_ref()
                    .is_some_and(|l| validate_label_override(l).is_err())
                || o.field.as_ref().is_some_and(|field| {
                    field.is_empty() || field.len() > 160 || field.trim() != field
                })
                || (!o.size_by_value.is_empty() && o.field.is_none())
                || o.size_by_value.len() > 128
                || o.size_by_value
                    .iter()
                    .any(|(key, size)| key.len() > 160 || !bounded(*size, 0., 48.))
                || o.palette.len() > 128
                || o.palette.iter().any(|(k, v)| k.len() > 160 || !color(v))
                || o.palette_fallback
                    .as_ref()
                    .is_some_and(|c| o.palette.is_empty() || !color(c))
                || (o.color.is_some() && !o.palette.is_empty())
        })
    {
        return Err("invalid template style override; select a referenced layer and either a flat color or categorical palette".into());
    }
    if layout.layer_order.len() > 100
        || layout.layer_order.iter().any(|id| !identifier(id))
        || layout.layer_order.iter().collect::<BTreeSet<_>>().len() != layout.layer_order.len()
    {
        return Err("layer order needs up to 100 unique layer IDs".into());
    }
    if let Some(focus) = &layout.focus {
        if !bounded(focus.buffer_m, 0., 5000.)
            || !color(&focus.outside_color)
            || !color(&focus.border_color)
            || !bounded(focus.outside_opacity, 0., 1.)
            || !bounded(focus.border_width_mm, 0., 5.)
            || focus.border_opacity.is_some_and(|v| !bounded(v, 0., 1.))
        {
            return Err("invalid print-time focus parameters".into());
        }
    }
    if layout.context_layers.len() > 64 {
        return Err("a print layout cannot request more than 64 context layers".into());
    }
    let mut context_ids = BTreeSet::new();
    for context in &layout.context_layers {
        if !identifier(&context.id)
            || !context_ids.insert(&context.id)
            || context.label.trim().is_empty()
            || context.label.len() > 160
        {
            return Err("invalid or duplicate print context layer identity".into());
        }
        match &context.source {
            PrintContextSource::Survey { form } => {
                if !identifier(form) {
                    return Err("invalid survey form selector".into());
                }
            }
            PrintContextSource::LocalLayer { layer_id } => {
                if layer_id.is_empty() || layer_id.len() > 160 || layer_id.trim() != layer_id {
                    return Err("invalid local layer selector".into());
                }
            }
            PrintContextSource::Catalog { country, layer } => {
                if country.trim().is_empty() || country.len() > 80 || !identifier(layer) {
                    return Err("invalid print catalog layer selector".into());
                }
            }
            PrintContextSource::GoogleOpenBuildings {
                buffer_m,
                min_area_m2,
                min_confidence,
            } => {
                if !bounded(*buffer_m, 0., 5_000.)
                    || !bounded(*min_area_m2, 1., 100_000.)
                    || !bounded(*min_confidence, 0., 1.)
                {
                    return Err("invalid Open Buildings print context parameters".into());
                }
            }
            PrintContextSource::ElevationContours {
                buffer_m,
                minor_interval_m,
                index_interval_m,
                sample_spacing_m,
            } => {
                if !bounded(*buffer_m, 0., 5_000.)
                    || *minor_interval_m == 0
                    || *index_interval_m < *minor_interval_m
                    || *index_interval_m % *minor_interval_m != 0
                    || *index_interval_m > 1_000
                    || !bounded(*sample_spacing_m, 5., 250.)
                {
                    return Err("invalid elevation contour print context parameters".into());
                }
            }
            PrintContextSource::ProjectDsgridMv { buffer_m, corridor } => {
                if !bounded(*buffer_m, 0., 5_000.) {
                    return Err("invalid project DS Grid MV print context parameters".into());
                }
                if let Some(corridor) = corridor {
                    if !bounded(corridor.distance_m, 0.1, 1_000.)
                        || corridor.width_mm.is_some_and(|v| !bounded(v, 0.01, 5.))
                        || corridor.opacity.is_some_and(|v| !bounded(v, 0., 1.))
                        || corridor.color.as_ref().is_some_and(|v| !color(v))
                        || corridor.dash_mm.as_ref().is_some_and(|v| v.len() > 8)
                        || corridor
                            .dash_mm
                            .as_ref()
                            .is_some_and(|v| v.iter().any(|n| !bounded(*n, 0.1, 100.)))
                    {
                        return Err("invalid project DS Grid MV corridor parameters".into());
                    }
                }
            }
        }
    }
    Ok(())
}
/// A page without a map frame has no projection, bearing or scale. What its
/// layout asks of a map is refused by name, never dropped from the page; the
/// native reporter refuses the same list, in the same words, at render time.
fn refuse_map_features(layout: &Layout) -> Result<(), String> {
    let refuse = |what: String| {
        Err(format!(
            "{what} needs a map frame; layout {} has none",
            layout.id
        ))
    };
    for element in &layout.elements {
        match element.kind {
            ElementKind::NorthArrow => return refuse(format!("north arrow {}", element.id)),
            ElementKind::ScaleBar => return refuse(format!("scale bar {}", element.id)),
            ElementKind::Text if element.text.contains("{scale}") => {
                return refuse(format!("text {} binding {{scale}}", element.id));
            }
            _ => {}
        }
    }
    for (present, what) in [
        (layout.focus.is_some(), "a focus area"),
        (
            layout.composition.is_some(),
            "automatic furniture composition",
        ),
        (layout.network_fit.is_some(), "network fitting"),
        (layout.scale_limits.is_some(), "a map scale limit"),
        (layout.scale_rounding.is_some(), "map scale rounding"),
        (layout.transformer_bounds.is_some(), "transformer bounds"),
    ] {
        if present {
            return refuse(what.into());
        }
    }
    Ok(())
}
/// A layout printed as a geographic sheet: valid, with its one map frame.
/// Hosts that always project (custom-area and district MV maps) admit no
/// front matter.
/// Resolve every declared print choice once through the shared Kernel resolver.
pub fn resolve_style_documents(
    mut layout: Layout,
    resolution: &crate::style_resolution::Snapshot,
) -> Result<(Layout, Value), String> {
    if layout.style_keys.is_empty() && !layout.style_refs.is_empty() {
        return Err("style_resolution_required: migrate the governed printing template to exact style_keys through the API".into());
    }
    // A selected source may expand into a print-only layer (for example the
    // MV route corridor). Resolve the source's declared keys just as the
    // sealed report capture does; preserve every authored pen or binding.
    layout
        .style_keys
        .extend(context::missing_context_style_keys(&layout));
    if layout.ink_mode == crate::style_resolution::Ink::Monochrome {
        for key in layout.style_keys.values_mut() {
            key.ink = crate::style_resolution::Ink::Monochrome;
        }
    }
    let mut captured = crate::style_resolution::prepare_print(resolution, &layout.style_keys)
        .map_err(|e| e.to_string())?;
    captured["renderer_defaults"] =
        serde_json::to_value(renderer_defaults::resolve(resolution)?).map_err(|e| e.to_string())?;
    for (layer, reference) in captured["style_refs"].as_object().unwrap() {
        layout.styles.remove(layer);
        layout
            .style_refs
            .insert(layer.clone(), reference.as_str().unwrap().into());
        let document = &captured["print_styles"][reference.as_str().unwrap()];
        style_overrides::physical_style(document)?;
        if document["metadata"].get("scale_weight").is_some() {
            let authored = layout
                .style_overrides
                .get(layer)
                .cloned()
                .unwrap_or_default();
            let inherited = style_overrides::inherit_scale_policy(document, &authored)
                .map_err(|e| format!("layer {layer}: {e}"))?;
            layout.style_overrides.insert(layer.clone(), inherited);
        }
    }
    validate(&layout)?;
    Ok((layout, captured))
}

pub fn validate_geographic(layout: &Layout) -> Result<(), String> {
    validate(layout)?;
    if !layout.elements.iter().any(|e| e.kind == ElementKind::Map) {
        return Err(format!(
            "a geographic sheet needs a map frame; layout {} has none",
            layout.id
        ));
    }
    Ok(())
}
pub fn default_layout() -> Layout {
    let element = |id: &str, kind, rect, text: &str, font_pt| Element {
        table: None,
        viewport: None,
        flow: None,
        sheet_anchor: Default::default(),
        legend: None,
        id: id.into(),
        kind,
        rect,
        text: text.into(),
        align: TextAlign::Left,
        text_style: None,
        asset: String::new(),
        font_pt,
        color: ink(),
        fill: white(),
        opacity: opaque(),
        stroke_mm: None,
    };
    complete(Layout {
        classification: None,
        ink_mode: crate::style_resolution::Ink::Colour,
        attribution: None,
        mv: None,
        composition: None,
        transformer_bounds: None,
        project_overview: false,
        scale_rounding: None,
        network_fit: None,
        scale_limits: None,
        layer_order: Vec::new(),
        style_overrides: BTreeMap::new(),
        focus: None,
        title_block: BTreeMap::new(),
        transformer_overrides: BTreeMap::new(),
        schema: SCHEMA.into(),
        id: "network-map".into(),
        name: "Network map · A3 landscape".into(),
        paper: PaperSpec::default(),
        page_mm: [420., 297.],
        assets: BTreeMap::new(),
        styles: BTreeMap::new(),
        style_refs: BTreeMap::new(),
        style_keys: BTreeMap::new(),
        context_layers: Vec::new(),
        elements: vec![
            Element {
                viewport: Some(PrintViewport {
                    name: "Primary map".into(),
                    view_slot: "primary_map".into(),
                    clip: true,
                    fit_around: Vec::new(),
                    graticule: None,
                }),
                ..element("map", ElementKind::Map, [12., 32., 300., 235.], "", 10.)
            },
            element(
                "title",
                ElementKind::Text,
                [12., 10., 396., 15.],
                "{title}",
                22.,
            ),
            element(
                "legend",
                ElementKind::Legend,
                [321., 46., 87., 120.],
                "LEGEND",
                10.,
            ),
            element(
                "north",
                ElementKind::NorthArrow,
                [377., 183., 22., 30.],
                "",
                10.,
            ),
            element(
                "scale",
                ElementKind::ScaleBar,
                [20., 250., 78., 12.],
                "",
                8.,
            ),
            element(
                "project",
                ElementKind::Text,
                [321., 223., 87., 36.],
                "{project}\n{drawing}\n{date}",
                10.,
            ),
            element(
                "footer",
                ElementKind::Text,
                [12., 276., 396., 9.],
                "{source}",
                8.,
            ),
        ],
    })
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub enum Paper {
    A0,
    A1,
    A2,
    #[default]
    A3,
    A4,
    A5,
    Custom {
        short_mm: f64,
        long_mm: f64,
    },
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Portrait,
    #[default]
    Landscape,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaperSpec {
    pub size: Paper,
    pub orientation: Orientation,
}

impl Default for PaperSpec {
    fn default() -> Self {
        Self {
            size: Paper::A3,
            orientation: Orientation::Landscape,
        }
    }
}

impl PaperSpec {
    pub fn expression_value(self) -> &'static str {
        match self.size {
            Paper::A0 => "print_a0",
            Paper::A1 => "print_a1",
            Paper::A2 => "print_a2",
            Paper::A3 => "print_a3",
            Paper::A4 => "print_a4",
            Paper::A5 => "print_a5",
            Paper::Custom { .. } => "print_custom",
        }
    }

    pub fn page_mm(self) -> Result<[f64; 2], String> {
        let [short, long] = match self.size {
            Paper::A0 => [841., 1189.],
            Paper::A1 => [594., 841.],
            Paper::A2 => [420., 594.],
            Paper::A3 => [297., 420.],
            Paper::A4 => [210., 297.],
            Paper::A5 => [148., 210.],
            Paper::Custom { short_mm, long_mm } => {
                if !bounded(short_mm, 50., 2000.)
                    || !bounded(long_mm, 50., 2000.)
                    || short_mm > long_mm
                {
                    return Err("custom paper needs bounded short/long dimensions".into());
                }
                [short_mm, long_mm]
            }
        };
        Ok(match self.orientation {
            Orientation::Portrait => [short, long],
            Orientation::Landscape => [long, short],
        })
    }
}

fn infer_paper(page: [f64; 2]) -> Result<PaperSpec, String> {
    if page.iter().any(|value| !bounded(*value, 50., 2000.)) {
        return Err("invalid legacy page dimensions".into());
    }
    let orientation = if page[0] <= page[1] {
        Orientation::Portrait
    } else {
        Orientation::Landscape
    };
    let short = page[0].min(page[1]);
    let long = page[0].max(page[1]);
    let known = [
        (Paper::A0, [841., 1189.]),
        (Paper::A1, [594., 841.]),
        (Paper::A2, [420., 594.]),
        (Paper::A3, [297., 420.]),
        (Paper::A4, [210., 297.]),
        (Paper::A5, [148., 210.]),
    ];
    let size = known
        .into_iter()
        .find_map(|(paper, dimensions)| {
            ((short - dimensions[0]).abs() < 1e-7 && (long - dimensions[1]).abs() < 1e-7)
                .then_some(paper)
        })
        .unwrap_or(Paper::Custom {
            short_mm: short,
            long_mm: long,
        });
    Ok(PaperSpec { size, orientation })
}

pub fn upgrade(mut layout: Layout) -> Result<Layout, String> {
    match layout.schema.as_str() {
        SCHEMA => return Ok(complete(layout)),
        LEGACY_SCHEMA => {}
        _ => return Err("unsupported print layout schema".into()),
    }
    layout.paper = infer_paper(layout.page_mm)?;
    for element in &mut layout.elements {
        if element.kind == ElementKind::Map && element.viewport.is_none() {
            element.viewport = Some(PrintViewport {
                name: "Primary map".into(),
                view_slot: "primary_map".into(),
                clip: true,
                fit_around: Vec::new(),
                graticule: None,
            });
        }
    }
    layout.schema = SCHEMA.into();
    let layout = complete(layout);
    validate(&layout)?;
    Ok(layout)
}

/// A legend element always carries its binding, so every host reads the
/// document's values instead of guessing the defaults an absent binding
/// would have meant.
fn complete(mut layout: Layout) -> Layout {
    for element in &mut layout.elements {
        if element.kind == ElementKind::Legend && element.legend.is_none() {
            element.legend = elements::new_element(ElementKind::Legend, String::new(), "").legend;
        }
    }
    layout
}

pub fn fit_to_paper(mut layout: Layout, paper: PaperSpec) -> Result<Layout, String> {
    let page = paper.page_mm()?;
    let old_page = layout.page_mm;
    if old_page.iter().any(|value| !bounded(*value, 50., 2000.)) {
        return Err("cannot fit a layout with invalid page dimensions".into());
    }
    // Fit uniformly. Changing paper never stretches symbols, logos or type.
    let scale = f64::min(page[0] / old_page[0], page[1] / old_page[1]);
    let dx = (page[0] - old_page[0] * scale) / 2.;
    let dy = (page[1] - old_page[1] * scale) / 2.;
    for element in &mut layout.elements {
        element.rect = [
            dx + element.rect[0] * scale,
            dy + element.rect[1] * scale,
            element.rect[2] * scale,
            element.rect[3] * scale,
        ];
        element.font_pt = (element.font_pt * scale).clamp(4., 144.);
        // Frames and their content share one physical scale. Keep the existing
        // readable dimension floors, without changing weights or row inventory.
        if let Some(table) = &mut element.table {
            for width in &mut table.min_widths_mm {
                *width *= scale;
            }
            for width in &mut table.max_widths_mm {
                *width = (*width * scale).max(3.);
            }
            table.row_mm = table.row_mm.map(|height| (height * scale).max(2.));
            table.gap_mm *= scale;
        }
        if let Some(legend) = &mut element.legend {
            legend.row_mm = (legend.row_mm * scale).max(3.);
            legend.indent_mm *= scale;
        }
        if let Some(flow) = &mut element.flow {
            flow.gap_mm *= scale;
        }
    }
    if let Some(composition) = &mut layout.composition {
        composition.gap_mm *= scale;
        composition.collision_margin_mm *= scale;
    }
    layout.paper = paper;
    layout.page_mm = page;
    validate(&layout)?;
    Ok(layout)
}

pub fn element_index(layout: &Layout, id: &str) -> Result<usize, String> {
    layout
        .elements
        .iter()
        .position(|element| element.id == id)
        .ok_or_else(|| "page element was not found".into())
}

pub fn move_to_edge(mut layout: Layout, id: &str, front: bool) -> Result<Layout, String> {
    let index = element_index(&layout, id)?;
    let element = layout.elements.remove(index);
    if front {
        layout.elements.push(element);
    } else {
        layout.elements.insert(0, element);
    }
    validate(&layout)?;
    Ok(layout)
}

pub fn move_relative(
    mut layout: Layout,
    id: &str,
    relative_to: &str,
    above: bool,
) -> Result<Layout, String> {
    if id == relative_to {
        return Err("an element cannot be ordered relative to itself".into());
    }
    let index = element_index(&layout, id)?;
    let element = layout.elements.remove(index);
    let relative = element_index(&layout, relative_to)?;
    layout
        .elements
        .insert(if above { relative + 1 } else { relative }, element);
    validate(&layout)?;
    Ok(layout)
}

pub fn named_layout(
    id: Option<String>,
    name: Option<String>,
    paper: Paper,
    orientation: Orientation,
) -> Result<Layout, String> {
    let paper = PaperSpec {
        size: paper,
        orientation,
    };
    let mut layout = fit_to_paper(default_layout(), paper)?;
    if let Some(id) = id {
        layout.id = id;
    }
    if let Some(name) = name {
        layout.name = name;
    }
    validate(&layout)?;
    Ok(layout)
}

pub fn layout_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Layout)).expect("layout schema serializes")
}
