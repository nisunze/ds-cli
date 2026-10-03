//! Print-layout authority shared by CLI, Canvas2D preview and vector delivery.
//! Flat page elements in physical millimetres; no tile, HTML or remote-image IO.
pub mod adjacent_networks;
pub mod attribution;
pub mod context;
pub mod custom;
pub mod diagnostics;
pub mod elements;
pub mod frames;
pub mod held_context;
pub mod inventory;
pub mod lifecycle;
pub mod map;
pub mod mv;
pub mod mv_booklet;
pub mod mv_context;
pub mod mv_placement;
pub mod mv_settings;
pub mod names;
pub mod network_fit;
pub mod overrides;
pub mod page_role;
pub mod pens;
pub mod placement;
pub mod preview;
pub mod project_sources;
pub mod renderer_defaults;
pub mod resolved_styles;
pub mod session;
pub mod sheet_margin;
pub mod sheet_status;
pub mod standards;
pub mod structure_descriptions;
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
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
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
    #[serde(default = "graticule_width")]
    pub width_mm: f64,
    #[serde(default = "graticule_font")]
    pub font_pt: f64,
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
    if layout.page_mm.iter().any(|v| !bounded(*v, 50., 2000.))
        || layout.elements.is_empty()
        || layout.elements.len() > 200
    {
        return Err("invalid page dimensions or element count".into());
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
        if !bounded(x, 0., layout.page_mm[0])
            || !bounded(y, 0., layout.page_mm[1])
            || !bounded(w, 1., layout.page_mm[0])
            || !bounded(h, 1., layout.page_mm[1])
            || x + w > layout.page_mm[0] + 1e-7
            || y + h > layout.page_mm[1] + 1e-7
        {
            return Err(format!("element {} lies outside the page", element.id));
        }
        if !bounded(element.font_pt, 4., 144.)
            || !color(&element.color)
            || !color(&element.fill)
            || !bounded(element.opacity, 0., 1.)
            || element.text.len() > 4000
        {
            return Err("invalid element typography or color".into());
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
        asset: String::new(),
        font_pt,
        color: ink(),
        fill: white(),
        opacity: opaque(),
    };
    complete(Layout {
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

fn fit_to_paper(mut layout: Layout, paper: PaperSpec) -> Result<Layout, String> {
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
    }
    layout.paper = paper;
    layout.page_mm = page;
    validate(&layout)?;
    Ok(layout)
}

fn element_index(layout: &Layout, id: &str) -> Result<usize, String> {
    layout
        .elements
        .iter()
        .position(|element| element.id == id)
        .ok_or_else(|| "page element was not found".into())
}

fn move_to_edge(mut layout: Layout, id: &str, front: bool) -> Result<Layout, String> {
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

fn move_relative(
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

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    SetTransformerOverride {
        layout: Layout,
        transformer: String,
        #[serde(default)]
        patch: Option<overrides::PrintOverride>,
    },
    StandardRegistry,
    ConsumeResolvedStyles {
        capture: Value,
        keys: BTreeMap<String, Value>,
        project: String,
    },
    StandardLayout {
        templates: standards::Templates,
        kind: standards::Kind,
        role: standards::PageRole,
        identity: standards::Identity,
        #[serde(default)]
        overrides: Option<standards::Overrides>,
    },
    /// Native and WASM hosts pass measured occupied furniture after layout.
    AssessFurniture {
        page_mm: [f64; 2],
        occupied: Vec<diagnostics::Occupied>,
        /// The paper edge furniture keeps inside of; default 1 mm.
        #[serde(default)]
        margin_mm: Option<f64>,
    },
    /// The collision role of every element of a layout (see `diagnostics`):
    /// which rectangles are furniture, and how movable each is.
    FurnitureRoles {
        layout: Layout,
    },
    /// The free surface of the sheet around measured fixed furniture (see
    /// `placement`): the maximal free rectangles, and the stepping
    /// rectangles from each corner for panels of `panel_w` mm.
    PlacementAreas {
        page_mm: [f64; 2],
        #[serde(default)]
        occupied: Vec<diagnostics::Occupied>,
        #[serde(default)]
        margin_mm: Option<f64>,
        #[serde(default)]
        gap_mm: Option<f64>,
        #[serde(default)]
        panel_w: Option<f64>,
        #[serde(default)]
        max_panels: Option<usize>,
    },
    ProjectSettings {
        sheets: serde_json::Value,
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        mode: Option<crate::report_formats::ReadinessMode>,
    },
    MvResolve {
        sheets: Value,
        project_id: String,
        #[serde(default)]
        model_fields: BTreeMap<mv::ModelField, String>,
    },
    MvSelectionPatch {
        current: Vec<Value>,
        selection: mv::Selection,
    },
    MvBookletPlan {
        setup: Box<mv::Resolved>,
        model_revision: String,
        pages: Vec<mv_booklet::Page>,
    },
    MvStandardize {
        layout: Layout,
    },
    MvBookletPreview {
        plan: mv_booklet::Plan,
        previews: Vec<mv_booklet::Preview>,
    },
    OutputSelection {
        #[serde(default)]
        selection: Option<crate::report_formats::DesignOutputSelection>,
        #[serde(default)]
        legacy_formats: Option<Vec<String>>,
        /// Whatever the project has actually stored, in any shape it has ever
        /// been stored in. A host passes the row value through and never
        /// decides which of the other two inputs it resembles.
        #[serde(default, deserialize_with = "deserialize_present")]
        stored: Option<serde_json::Value>,
        #[serde(default)]
        execution: Option<BTreeMap<String, Vec<crate::report_formats::Placement>>>,
        #[serde(default)]
        required_layout: Option<String>,
    },
    /// Patch a project's settings sheet with the outputs it should produce and
    /// the per-transformer print views one layout was authored with. The
    /// export row is resolved through every alias the planner reads, and
    /// created when the sheet has none.
    ProjectOutputPatch {
        current: Vec<serde_json::Value>,
        selection: crate::report_formats::DesignOutputSelection,
        #[serde(default)]
        layout_id: Option<String>,
        #[serde(default)]
        views: BTreeMap<String, BTreeMap<String, GeographicView>>,
    },
    /// What a host may do to a named setup right now — read live or from a
    /// held copy, create or update, copy, delete, or refuse — decided from
    /// the facts the host reports (see `lifecycle`).
    LifecyclePlan {
        facts: lifecycle::Facts,
    },
    /// Which transformers may be printed, in what order, and which of them
    /// this device holds (see `inventory`).
    PrintableTransformers {
        rows: Vec<serde_json::Value>,
        #[serde(default)]
        rooms: Option<Vec<inventory::Room>>,
        #[serde(default)]
        current_user: String,
        limit: usize,
    },
    /// Available plain-schedule columns from this transformer's actual engineering data.
    TableColumns {
        layout: Layout,
        element_id: String,
        layers: BTreeMap<String, Value>,
    },
    /// Show selected source columns, preserving headings and physical column widths.
    TableColumnSelection {
        layout: Layout,
        element_id: String,
        columns: Vec<String>,
        layers: BTreeMap<String, Value>,
    },
    /// Author a design/as-built schedule; design removes installed-connection columns.
    TablePhase {
        layout: Layout,
        element_id: String,
        phase: table_columns::SchedulePhase,
    },
    /// May a PDF preview of this layout be produced right now, and from what:
    /// every bound print style must be available and there must be map data
    /// to draw. The host assembles the render request from the answer.
    RenderPlan {
        layout: Layout,
        #[serde(default)]
        available_style_refs: Vec<String>,
        #[serde(default)]
        layer_count: usize,
        #[serde(default)]
        text: serde_json::Value,
        #[serde(default)]
        date: String,
    },
    /// The context sources a template starts with, and the styles their
    /// layers print with when none are authored (see `context`).
    ContextDefaults {},
    /// Every catalogue dataset as a selectable row with its default decision.
    ContextCatalog {
        resources: Vec<context::CatalogResource>,
    },
    /// The selection a new project template starts with.
    ContextDefaultSelection {
        layout: Layout,
        resources: Vec<context::CatalogResource>,
    },
    /// The canonical new page element of a kind (see `elements`); the id is
    /// minted when absent, a logo names the layout asset it shows.
    AddElement {
        layout: Layout,
        kind: ElementKind,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        asset: String,
    },
    /// Which governed print styles a print layer may bind, from the host's
    /// style catalogue facts.
    StyleRefChoices {
        catalogue: Vec<elements::StyleFacts>,
    },
    /// Bind (or, with an empty reference, unbind) a print layer's style.
    SetLayerStyleRef {
        layout: Layout,
        layer_id: String,
        style_ref: String,
        catalogue: Vec<elements::StyleFacts>,
    },
    /// Every pen of the layout — each bound print layer with what it prints
    /// with, from the governed documents the host holds (see `pens`).
    PrintPens {
        layout: Layout,
        #[serde(default)]
        documents: Vec<pens::PenDocument>,
    },
    /// Set, or with `null` clear, one property of a layer's template-local
    /// pen; an override left with nothing to say is removed.
    PenEdit {
        layout: Layout,
        layer_id: String,
        property: pens::PenProperty,
        #[serde(default)]
        value: serde_json::Value,
    },
    /// Replace, or with `null` remove, a layer's whole template-local pen.
    SetStyleOverride {
        layout: Layout,
        layer_id: String,
        #[serde(default)]
        change: Option<PrintStyleOverride>,
    },
    /// May a live transformer preview be produced right now, and as what
    /// (see `preview`). Rendering remains a host effect.
    PreviewPlan {
        facts: preview::Facts,
    },
    /// The DPI and paper a local map print may ask for, with the defaults.
    MapRequestChoices {},
    /// The print editor's zero state and its history cap (see `session`).
    SessionDefaults {},
    /// One event against the editor's workflow state: the next state and the
    /// operations the host performs on the documents it holds.
    SessionStep {
        state: session::State,
        event: session::Event,
    },
    /// Switch one context layer on or off, seeding fallback styles.
    ContextToggle {
        layout: Layout,
        layer: PrintContextLayer,
        enabled: bool,
        #[serde(default)]
        resource: Option<context::CatalogResource>,
    },
    TransformerInstructions {
        layout: Layout,
        current: Vec<serde_json::Value>,
        instructions: BTreeMap<String, Option<overrides::PrintOverride>>,
    },
    TransformerOverride {
        layout: Layout,
        instructions: overrides::PrintOverride,
    },
    TransformerViews {
        views: BTreeMap<String, BTreeMap<String, GeographicView>>,
    },
    New {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        paper: Paper,
        #[serde(default)]
        orientation: Orientation,
    },
    Rename {
        layout: Layout,
        name: String,
    },
    Duplicate {
        layout: Layout,
        id: String,
        name: String,
    },
    Validate {
        layout: Layout,
    },
    Edit {
        layout: Layout,
        element: Box<Element>,
    },
    Remove {
        layout: Layout,
        id: String,
    },
    SetPaper {
        layout: Layout,
        paper: Paper,
        orientation: Orientation,
    },
    SetViewport {
        layout: Layout,
        id: String,
        name: String,
        view_slot: String,
    },
    /// Set the project template's opt-in footprint and physical print-scale policy.
    SetFitPolicy {
        layout: Layout,
        network_fit: Option<network_fit::NetworkFit>,
        scale_limits: Option<network_fit::ScaleLimits>,
    },
    /// Configure print-only neighbouring circuits beneath the focused network,
    /// preserving each role's governed symbol and explicit lifecycle palette.
    SetAdjacentNetworks {
        layout: Layout,
        enabled: bool,
        #[serde(default)]
        style_refs: BTreeMap<String, String>,
        #[serde(default)]
        policy: Option<adjacent_networks::Policy>,
    },
    /// Resolve an exact held API table into the renderer's held document map.
    /// Style documents own all pens; this operation selects no style by name.
    ResolveStyles {
        layout: Layout,
        resolution: crate::style_resolution::Snapshot,
        #[serde(default)]
        style_keys: BTreeMap<String, crate::style_resolution::Key>,
    },
    /// Toggle the configured project transformer shade without changing its style.
    SetFocusVisibility {
        layout: Layout,
        visible: bool,
    },
    /// Before rendering a draft preview, prepare its selected context. Rendering itself reads held state.
    PreviewContextPreparation {
        layout: Layout,
        online: bool,
    },
    /// Map space -> printable border -> independent container -> local table.
    SheetStatus {
        assessments: Vec<Value>,
        warnings: Vec<table_rows::PrintWarning>,
    },
    ContextStatus {
        omitted: Vec<sheet_status::ContextOmission>,
    },
    TableDrafts {
        layout: Layout,
    },
    SheetFrames {
        layout: Layout,
    },
    TableContainerRect {
        layout: Layout,
        element_id: String,
        rect_mm: [f64; 4],
    },
    FurnitureHit {
        layout: Layout,
        measured: BTreeMap<String, Vec<[f64; 4]>>,
        point_mm: [f64; 2],
    },
    SetContextLayers {
        layout: Layout,
        context_layers: Vec<PrintContextLayer>,
    },
    BringToFront {
        layout: Layout,
        id: String,
    },
    SendToBack {
        layout: Layout,
        id: String,
    },
    MoveAbove {
        layout: Layout,
        id: String,
        relative_to: String,
    },
    MoveBelow {
        layout: Layout,
        id: String,
        relative_to: String,
    },
}
/// Tell "the key is absent" apart from "the key is present and null". Serde
/// collapses both onto `None`, and for a stored settings value that difference
/// is the whole question: a project's export row may hold `null`, and `null` is
/// something it stored — an empty selection — not a caller that passed no
/// argument at all.
fn deserialize_present<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// The per-transformer print view map, bounded and validated once. The native
/// reporter reads this exact nesting at export time, so the rule lives beside
/// the document grammar rather than in whichever host writes it.
fn validate_transformer_views(
    views: &BTreeMap<String, BTreeMap<String, GeographicView>>,
) -> Result<(), String> {
    if views.len() > 10_000 {
        return Err("transformer print views exceed 10,000 transformers".into());
    }
    for (transformer, slots) in views {
        let bytes = transformer.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 121
            || !((bytes[0].is_ascii_lowercase())
                || (bytes[0] == b'_' && bytes.get(1).is_some_and(u8::is_ascii_digit)))
            || !bytes[1..]
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
            || slots.len() > 64
        {
            return Err("invalid transformer print view map".into());
        }
        for (slot, view) in slots {
            if slot.is_empty()
                || slot.len() > 80
                || !slot
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err("invalid print viewport slot".into());
            }
            view.validate()?;
        }
    }
    Ok(())
}

/// The whole project-settings patch a printing preparation performs: the
/// export selection into its aliased row, and one layout's authored views into
/// the `printing_views` row the native reporter reads
/// (transformer → layout id → slot → view, bounded 10 000 / 32 / 8).
fn project_output_patch(
    current: Vec<serde_json::Value>,
    selection: &crate::report_formats::DesignOutputSelection,
    layout_id: Option<&str>,
    views: &BTreeMap<String, BTreeMap<String, GeographicView>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut rows = current;
    selection.require_deliverable()?;
    crate::report_formats::apply_output_selection(&mut rows, selection)?;
    if views.is_empty() {
        return Ok(rows);
    }
    let layout_id = layout_id.ok_or("print views require the layout they were authored on")?;
    if !identifier(layout_id) {
        return Err("invalid printing setup id".into());
    }
    validate_transformer_views(views)?;
    let index = rows
        .iter()
        .position(|row| {
            crate::report_formats::setting_parameter_key(row)
                .is_some_and(|key| key == PRINTING_VIEWS_SETTING)
        })
        .unwrap_or_else(|| {
            rows.push(json!({"parameter":PRINTING_VIEWS_SETTING,"value":{}}));
            rows.len() - 1
        });
    let mut current = match rows[index]["value"].as_object() {
        Some(existing) => existing.clone(),
        None => serde_json::Map::new(),
    };
    for (transformer, slots) in views {
        let layouts = current
            .entry(transformer.clone())
            .or_insert_with(|| json!({}));
        if !layouts.is_object() {
            *layouts = json!({});
        }
        layouts[layout_id] = json!(slots);
    }
    if current.len() > 10_000 {
        return Err("transformer print views exceed 10,000 transformers".into());
    }
    for layouts in current.values() {
        let layouts = layouts
            .as_object()
            .ok_or("invalid transformer print views")?;
        if layouts.len() > 32 {
            return Err("a transformer carries print views for more than 32 layouts".into());
        }
        for slots in layouts.values() {
            if slots.as_object().is_none_or(|slots| slots.len() > 8) {
                return Err("a print layout carries more than 8 viewport slots".into());
            }
        }
    }
    rows[index]["value"] = Value::Object(current);
    Ok(rows)
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("print input exceeds 32 MiB".into());
    }
    let command: Command =
        serde_json::from_slice(input).map_err(|e| format!("invalid printing request: {e}"))?;
    let value = match command {
        Command::SetTransformerOverride {
            mut layout,
            transformer,
            patch,
        } => {
            if let Some(patch) = patch {
                layout.transformer_overrides.insert(transformer, patch);
            } else {
                layout.transformer_overrides.remove(&transformer);
            }
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::StandardRegistry => standards::registry(),
        Command::ConsumeResolvedStyles {
            capture,
            keys,
            project,
        } => resolved_styles::consume(&capture, &keys, &project)?,
        Command::StandardLayout {
            templates,
            kind,
            role,
            identity,
            overrides,
        } => {
            json!({"layout":standards::layout(&templates, kind, role, &identity, overrides.as_ref())?})
        }
        Command::TableColumns {
            layout,
            element_id,
            layers,
        } => serde_json::to_value(table_columns::catalog(&layout, &element_id, &layers)?)
            .map_err(|e| e.to_string())?,
        Command::TableColumnSelection {
            layout,
            element_id,
            columns,
            layers,
        } => {
            json!({ "layout": table_columns::select_columns(&layout, &element_id, &columns, &layers)? })
        }
        Command::TablePhase {
            layout,
            element_id,
            phase,
        } => json!({ "layout": table_columns::set_phase(&layout, &element_id, phase)? }),
        Command::ProjectSettings {
            sheets,
            mode,
            project_id,
        } => {
            let mut result = crate::report_formats::inspect_project_settings(&sheets, mode)?;
            result["mv"] = mv::inspect(&sheets, project_id.as_deref());
            result["transformer_instructions"] = sheets["project_settings"]
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row["parameter"] == "printing_overrides")
                })
                .map(|row| row["value"].clone())
                .unwrap_or_else(|| json!({}));
            result
        }
        Command::MvResolve {
            sheets,
            project_id,
            model_fields,
        } => match mv::resolve(&sheets, &project_id, model_fields) {
            Ok(resolved) => json!({"resolved":resolved,"refusal":Value::Null}),
            Err(refusal) => json!({"resolved":Value::Null,"refusal":refusal}),
        },
        Command::MvSelectionPatch { current, selection } => {
            match mv::patch_selection(&current, &selection) {
                Ok(rows) => json!({"project_settings":rows,"refusal":Value::Null}),
                Err(refusal) => json!({"project_settings":Value::Null,"refusal":refusal}),
            }
        }
        Command::MvBookletPlan {
            setup,
            model_revision,
            pages,
        } => serde_json::to_value(mv_booklet::plan(&setup, &model_revision, pages)?)
            .map_err(|e| e.to_string())?,
        Command::MvStandardize { layout } => {
            json!({"standard":mv::STANDARD,"layout":mv::standardize(layout)?})
        }
        Command::MvBookletPreview { plan, previews } => {
            mv_booklet::preview_document(&plan, &previews)?
        }
        Command::OutputSelection {
            selection,
            legacy_formats,
            stored,
            execution,
            required_layout,
        } => {
            let selection = match stored {
                Some(stored) => {
                    if selection.is_some() || legacy_formats.is_some() || execution.is_some() {
                        return Err(
                            "pass a stored settings value alone, without a selection, legacy formats or placements".into(),
                        );
                    }
                    crate::report_formats::stored_output_selection(&stored)?
                }
                None => crate::report_formats::normalize_output_selection(
                    selection,
                    legacy_formats,
                    execution,
                )?,
            };
            if let Some(layout) = required_layout {
                if !identifier(&layout)
                    || !selection.prints.iter().any(|print| {
                        print.enabled && print.layout_id == layout && !print.formats.is_empty()
                    })
                {
                    return Err(
                        "the selected outputs must include the required named layout".into(),
                    );
                }
            }
            let placements = selection.placement_map()?;
            // The selection omits the default layout; the reply states it, so
            // a host renders the choice without re-stating `extended`.
            let voltage_drop_report = selection.voltage_drop_report_format();
            json!({"selection":selection,"placements":placements,
                "voltage_drop_report":voltage_drop_report})
        }
        Command::ProjectOutputPatch {
            current,
            selection,
            layout_id,
            views,
        } => {
            json!({"project_settings":project_output_patch(current, &selection, layout_id.as_deref(), &views)?})
        }
        Command::LifecyclePlan { facts } => {
            serde_json::to_value(lifecycle::plan(&facts)?).map_err(|e| e.to_string())?
        }
        Command::PrintableTransformers {
            rows,
            rooms,
            current_user,
            limit,
        } => {
            let mut result = serde_json::to_value(inventory::printable(
                &rows,
                rooms.as_deref().unwrap_or_default(),
                &current_user,
                limit,
            )?)
            .map_err(|e| e.to_string())?;
            // Absence of a host room inventory is unknown, not proof of no cache.
            result["local_rooms_known"] = json!(rooms.is_some());
            if rooms.is_none() {
                for row in result["transformers"]
                    .as_array_mut()
                    .expect("inventory rows")
                {
                    row["cached"] = serde_json::Value::Null;
                    row["dirty"] = serde_json::Value::Null;
                }
            }
            result
        }
        Command::RenderPlan {
            layout,
            available_style_refs,
            layer_count,
            text,
            date,
        } => {
            validate(&layout)?;
            let mut refusals = Vec::new();
            let mut refs: Vec<&String> = Vec::new();
            for reference in layout.style_refs.values() {
                if !refs.contains(&reference) {
                    refs.push(reference);
                }
                if !available_style_refs.contains(reference) {
                    refusals.push(json!({
                        "code": "print_style_unavailable",
                        "message_key": "printing_style_unavailable",
                        "params": {"reference": reference},
                    }));
                }
            }
            if layer_count == 0 {
                refusals.push(json!({
                    "code": "print_preview_needs_layers",
                    "message_key": "printing_preview_needs_layers",
                    "params": {},
                }));
            }
            let mut composed = text.as_object().cloned().unwrap_or_default();
            composed.insert("date".into(), serde_json::Value::String(date));
            let mut names: BTreeMap<String, String> = composed
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_owned())))
                .collect();
            names::plan_text(&mut names);
            for (key, value) in names {
                composed.insert(key, json!(value));
            }
            json!({
                "ready": refusals.is_empty(),
                "refusals": refusals,
                "print_style_refs": refs,
                "text": composed,
            })
        }
        Command::ContextDefaults {} => json!({
            "derived": context::derived_layers(),
            "corridor_default": context::default_corridor(),
            "style_keys": context::derived_style_keys(),
            "excluded_catalog_layers": context::DEFAULT_CATALOG_EXCLUSIONS,
        }),
        Command::ContextCatalog { resources } => json!({"rows": context::catalog(&resources)?}),
        Command::ContextDefaultSelection { layout, resources } => {
            let layout = upgrade(layout)?;
            json!({"layout": context::default_selection(layout, &resources)?})
        }
        Command::AddElement {
            layout,
            kind,
            id,
            asset,
        } => {
            let (layout, index) = elements::add(upgrade(layout)?, kind, id, &asset)?;
            json!({"layout": layout, "index": index})
        }
        Command::StyleRefChoices { catalogue } => {
            json!({"refs": elements::style_ref_choices(&catalogue)})
        }
        Command::SetLayerStyleRef {
            layout,
            layer_id,
            style_ref,
            catalogue,
        } => {
            json!({"layout": elements::set_layer_style_ref(upgrade(layout)?, &layer_id, &style_ref, &catalogue)?})
        }
        Command::PrintPens { layout, documents } => {
            json!({"pens": pens::pens(&upgrade(layout)?, &documents)?})
        }
        Command::PenEdit {
            layout,
            layer_id,
            property,
            value,
        } => json!({"layout": pens::edit(upgrade(layout)?, &layer_id, property, &value)?}),
        Command::SetStyleOverride {
            layout,
            layer_id,
            change,
        } => json!({"layout": pens::set_override(upgrade(layout)?, &layer_id, change)?}),
        Command::PreviewPlan { facts } => preview::evaluate(preview::Facts {
            layout: upgrade(facts.layout)?,
            ..facts
        })?,
        Command::MapRequestChoices {} => map::request_choices(),
        Command::SessionDefaults {} => json!({
            "state": session::State::default(),
            "history_cap": session::HISTORY_CAP,
        }),
        Command::SessionStep { state, event } => {
            serde_json::to_value(session::step(&state, &event)?).map_err(|e| e.to_string())?
        }
        Command::ContextToggle {
            layout,
            layer,
            enabled,
            resource,
        } => {
            let layout = upgrade(layout)?;
            json!({"layout": context::toggle(layout, layer, enabled, resource.as_ref())?})
        }
        Command::TransformerInstructions {
            layout,
            current,
            instructions,
        } => {
            let layout = upgrade(layout)?;
            json!({"project_settings": overrides::update_settings(&layout, &current, &instructions)?})
        }
        Command::TransformerOverride {
            layout,
            instructions,
        } => {
            let layout = overrides::apply(&upgrade(layout)?, &instructions)?;
            json!({"layout": layout, "instructions": instructions})
        }
        Command::TransformerViews { views } => {
            validate_transformer_views(&views)?;
            json!({"views":views})
        }
        Command::New {
            id,
            name,
            paper,
            orientation,
        } => json!({"layout":named_layout(id,name,paper,orientation)?}),
        Command::Rename { layout, name } => {
            let mut layout = upgrade(layout)?;
            layout.name = name;
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::Duplicate { layout, id, name } => {
            let mut layout = upgrade(layout)?;
            if layout.id == id {
                return Err("A duplicate requires a new template id".into());
            }
            layout.id = id;
            layout.name = name;
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::AssessFurniture {
            page_mm,
            occupied,
            margin_mm,
        } => serde_json::to_value(diagnostics::assess_sheet(
            page_mm,
            margin_mm.unwrap_or(diagnostics::DEFAULT_MARGIN_MM),
            &occupied,
        )?)
        .map_err(|e| e.to_string())?,
        Command::PlacementAreas {
            page_mm,
            occupied,
            margin_mm,
            gap_mm,
            panel_w,
            max_panels,
        } => {
            let margin = margin_mm.unwrap_or(diagnostics::DEFAULT_MARGIN_MM);
            if !margin.is_finite() || !(0. ..=diagnostics::MAX_MARGIN_MM).contains(&margin) {
                return Err(format!(
                    "the collision margin is 0 through {} mm",
                    diagnostics::MAX_MARGIN_MM
                ));
            }
            let area = [
                margin,
                margin,
                page_mm[0] - 2. * margin,
                page_mm[1] - 2. * margin,
            ];
            let obstacles: Vec<placement::Rect> = occupied.iter().map(|o| o.rect_mm).collect();
            let gap = gap_mm.unwrap_or(sheet_margin::MARGIN_MM);
            let free = placement::free_rectangles(area, &obstacles, gap)?;
            let mut skylines = serde_json::Map::new();
            if let Some(panel_w) = panel_w {
                for corner in placement::CORNERS {
                    let steps = placement::skyline(
                        area,
                        &obstacles,
                        gap,
                        corner,
                        panel_w,
                        max_panels.unwrap_or(8),
                    )?;
                    let key = serde_json::to_value(corner)
                        .map_err(|e| e.to_string())?
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    skylines.insert(key, json!(steps));
                }
            }
            json!({"area_mm": area, "margin_mm": margin, "gap_mm": gap, "free": free, "skylines": skylines})
        }
        Command::FurnitureRoles { layout } => {
            let layout = upgrade(layout)?;
            json!({
                "margin_mm": layout.collision_margin_mm(),
                "roles": layout.elements.iter().map(|e| json!({"id": e.id, "kind": e.kind, "role": diagnostics::role_of(e)})).collect::<Vec<_>>(),
            })
        }
        Command::Validate { layout } => {
            let layout = upgrade(layout)?;
            validate(&layout)?;
            json!({"layout":layout,"valid":true})
        }
        Command::Edit { layout, element } => {
            let mut layout = upgrade(layout)?;
            let element = *element;
            if let Some(existing) = layout.elements.iter_mut().find(|e| e.id == element.id) {
                *existing = element
            } else {
                layout.elements.push(element)
            }
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::Remove { layout, id } => {
            let mut layout = upgrade(layout)?;
            if !layout.elements.iter().any(|e| e.id == id) {
                return Err("page element was not found".into());
            }
            layout.elements.retain(|e| e.id != id);
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::SetPaper {
            layout,
            paper,
            orientation,
        } => {
            json!({"layout":fit_to_paper(upgrade(layout)?, PaperSpec { size: paper, orientation })?})
        }
        Command::SetViewport {
            layout,
            id,
            name,
            view_slot,
        } => {
            let mut layout = upgrade(layout)?;
            let index = element_index(&layout, &id)?;
            if layout.elements[index].kind != ElementKind::Map {
                return Err("print viewport properties require a map element".into());
            }
            layout.elements[index].viewport = Some(PrintViewport {
                name,
                view_slot,
                clip: true,
                fit_around: layout.elements[index]
                    .viewport
                    .as_ref()
                    .map(|v| v.fit_around.clone())
                    .unwrap_or_default(),
                graticule: layout.elements[index]
                    .viewport
                    .as_ref()
                    .and_then(|v| v.graticule.clone()),
            });
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::ContextStatus { omitted } => json!(sheet_status::context_status(omitted)?),
        Command::PreviewContextPreparation { layout, online } => {
            let layout = upgrade(layout)?;
            let (shown, hidden): (Vec<_>, Vec<_>) = layout
                .context_layers
                .iter()
                .partition(|layer| context::prints_in(&layout, layer));
            let hidden: Vec<Value> = hidden
                .into_iter()
                .map(|layer| context::hidden_context(&layout.id, layer))
                .collect();
            json!({"contexts":shown,"acquire_missing":online,"hidden":hidden})
        }
        Command::SetFocusVisibility { layout, visible } => {
            let mut layout = upgrade(layout)?;
            if let Some(focus) = &mut layout.focus {
                focus.visible = visible;
            } else if visible {
                return Err(
                    "configure the template's focus styling before enabling its shade".into(),
                );
            }
            validate(&layout)?;
            json!({"layout": layout})
        }
        Command::SetFitPolicy {
            layout,
            network_fit,
            scale_limits,
        } => {
            let mut layout = upgrade(layout)?;
            layout.network_fit = network_fit;
            layout.scale_limits = scale_limits;
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::TableContainerRect {
            layout,
            element_id,
            rect_mm,
        } => {
            let mut layout = upgrade(layout)?;
            let border = sheet_margin::boundary(&layout);
            let element = layout
                .elements
                .iter_mut()
                .find(|e| e.id == element_id && e.kind == ElementKind::Table)
                .ok_or("table container does not exist")?;
            element.rect = [
                border[0] + rect_mm[0],
                border[1] + rect_mm[1],
                rect_mm[2],
                rect_mm[3],
            ];
            element.sheet_anchor = sheet_margin::SheetAnchor::Authored;
            // Explicit placement makes this container independent of a sibling.
            element.flow = None;
            validate(&layout)?;
            json!({"layout": layout})
        }
        Command::FurnitureHit {
            layout,
            measured,
            point_mm,
        } => {
            let layout = upgrade(layout)?;
            json!({"index": frames::hit(&layout, &measured, point_mm)?})
        }
        Command::SheetStatus {
            assessments,
            warnings,
        } => serde_json::to_value(sheet_status::summarize(&assessments, &warnings)?)
            .map_err(|e| e.to_string())?,
        Command::TableDrafts { layout } => {
            let layout = upgrade(layout)?;
            json!({"tables": frames::drafts(&layout)?})
        }
        Command::SheetFrames { layout } => {
            let layout = upgrade(layout)?;
            json!({"map_space": frames::hierarchy(&layout)})
        }
        Command::SetAdjacentNetworks {
            layout,
            enabled,
            style_refs,
            policy,
        } => {
            json!({"layout": adjacent_networks::configure(upgrade(layout)?, enabled, style_refs, policy)?})
        }
        Command::ResolveStyles {
            layout,
            resolution,
            style_keys,
        } => {
            let mut layout = upgrade(layout)?;
            if !style_keys.is_empty() {
                layout.style_keys = style_keys;
            }
            let (layout, captured) = resolve_style_documents(layout, &resolution)?;
            json!({"layout":layout,"capture":captured})
        }
        Command::SetContextLayers {
            mut layout,
            context_layers,
        } => {
            layout = upgrade(layout)?;
            layout.context_layers = context_layers;
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::BringToFront { layout, id } => {
            json!({"layout":move_to_edge(upgrade(layout)?, &id, true)?})
        }
        Command::SendToBack { layout, id } => {
            json!({"layout":move_to_edge(upgrade(layout)?, &id, false)?})
        }
        Command::MoveAbove {
            layout,
            id,
            relative_to,
        } => json!({"layout":move_relative(upgrade(layout)?, &id, &relative_to, true)?}),
        Command::MoveBelow {
            layout,
            id,
            relative_to,
        } => json!({"layout":move_relative(upgrade(layout)?, &id, &relative_to, false)?}),
    };
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

pub fn layout_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Layout)).expect("layout schema serializes")
}
pub fn command_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Command)).expect("command schema serializes")
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_ink_identity_is_closed_and_preserves_authored_pens() {
        let colour = default_layout();
        let mut value = serde_json::to_value(&colour).unwrap();
        assert!(value.get("ink_mode").is_none());
        value["ink_mode"] = json!("monochrome");
        let monochrome: Layout = serde_json::from_value(value.clone()).unwrap();
        validate(&monochrome).unwrap();
        assert_eq!(
            monochrome.ink_mode,
            crate::style_resolution::Ink::Monochrome
        );
        assert_eq!(monochrome.styles, colour.styles);
        assert_eq!(monochrome.elements, colour.elements);
        assert_eq!(
            serde_json::to_value(&monochrome).unwrap()["ink_mode"],
            "monochrome"
        );
        value["ink_mode"] = json!("gray");
        assert!(serde_json::from_value::<Layout>(value.clone()).is_err());
        value["ink_mode"] = json!("colour");
        value["ink"] = json!("monochrome");
        assert!(serde_json::from_value::<Layout>(value)
            .unwrap_err()
            .to_string()
            .contains("unknown field"));
    }

    #[test]
    fn metric_corridor_roundtrips_and_rejects_invalid_distance() {
        let mut layout = default_layout();
        let source = serde_json::json!({"kind":"project_dsgrid_mv","buffer_m":200.,
            "corridor":{"distance_m":6.,"color":"#957197","width_mm":0.15,"opacity":0.3,"dash_mm":[2.,1.]}});
        layout.context_layers.push(PrintContextLayer {
            id: "mv".into(),
            label: "MV".into(),
            source: serde_json::from_value(source.clone()).unwrap(),
        });
        validate(&layout).unwrap();
        assert_eq!(
            serde_json::to_value(&layout.context_layers[0].source).unwrap(),
            source
        );
        for invalid in [0., -6., 1001.] {
            if let PrintContextSource::ProjectDsgridMv {
                corridor: Some(c), ..
            } = &mut layout.context_layers[0].source
            {
                c.distance_m = invalid;
            }
            assert!(validate(&layout).unwrap_err().contains("corridor"));
        }
    }

    #[test]
    fn viewport_fit_around_roundtrips_and_rejects_missing_or_duplicate_elements() {
        let mut layout = default_layout();
        let anchor = layout.elements[1].id.clone();
        layout.elements[0].viewport.as_mut().unwrap().fit_around = vec![anchor.clone()];
        validate(&layout).unwrap();
        let decoded: Layout =
            serde_json::from_value(serde_json::to_value(&layout).unwrap()).unwrap();
        assert_eq!(decoded, layout);
        for ids in [
            vec!["missing".into()],
            vec!["map".into()],
            vec![anchor.clone(), anchor],
        ] {
            layout.elements[0].viewport.as_mut().unwrap().fit_around = ids;
            assert!(validate(&layout).unwrap_err().contains("fit-around"));
        }
    }

    #[test]
    fn content_flow_requires_an_earlier_content_frame() {
        let mut layout = default_layout();
        let legend = layout
            .elements
            .iter()
            .position(|e| e.kind == ElementKind::Legend)
            .unwrap();
        let mut table = layout.elements[legend].clone();
        table.id = "quantities".into();
        table.kind = ElementKind::Table;
        table.legend = None;
        table.table = Some(TableBinding {
            layer: "info".into(),
            additional_layers: vec!["tapping_poles".into()],
            columns: vec!["quantity".into()],
            headings: vec!["Quantity".into()],
            widths: vec![10.],
            panels: 1,
            gap_mm: 3.,
            ..Default::default()
        });
        layout.elements.insert(legend, table);
        layout.elements[legend + 1].flow = Some(ContentFlow {
            after: "quantities".into(),
            placement: ContentPlacement::default(),
            gap_mm: 5.,
        });
        validate(&layout).unwrap();
        let encoded = serde_json::to_vec(&layout).unwrap();
        assert_eq!(serde_json::from_slice::<Layout>(&encoded).unwrap(), layout);
        for invalid in [
            vec!["info".into()],
            vec!["a".into(), "a".into()],
            vec!["../bad".into()],
        ] {
            let mut invalid_layout = layout.clone();
            invalid_layout.elements[legend]
                .table
                .as_mut()
                .unwrap()
                .additional_layers = invalid;
            assert!(validate(&invalid_layout).is_err());
        }
        layout.elements[legend + 1].flow.as_mut().unwrap().after = "legend".into();
        assert!(validate(&layout).unwrap_err().contains("earlier"));
        layout.elements[legend + 1].flow.as_mut().unwrap().after = "map".into();
        assert!(validate(&layout).is_err());
        layout.elements[legend + 1].flow.as_mut().unwrap().after = "quantities".into();
        layout.elements[legend + 1].flow.as_mut().unwrap().gap_mm = -1.;
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn measured_schedule_and_overview_controls_roundtrip_and_refuse_conflicts() {
        let mut layout = default_layout();
        let mut table = layout
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Legend)
            .unwrap()
            .clone();
        table.id = "schedule".into();
        table.kind = ElementKind::Table;
        table.legend = None;
        table.table = Some(TableBinding {
            layer: "lv_poles".into(),
            columns: vec!["pole_number".into(), "struct_type".into()],
            headings: vec!["Pole".into(), "Type".into()],
            widths: vec![20., 40.],
            panels: 3,
            row_mm: Some(3.5),
            sort_by: vec!["pole_number".into()],
            ..Default::default()
        });
        layout.elements.push(table);
        layout
            .elements
            .iter_mut()
            .find(|e| e.kind == ElementKind::Map)
            .unwrap()
            .viewport
            .as_mut()
            .unwrap()
            .fit_around
            .push("schedule".into());
        layout.composition = Some(Composition {
            movable: vec!["schedule".into()],
            gap_mm: 5.,
            min_font_scale: 1.,
            collision_margin_mm: 1.,
        });
        validate(&layout).unwrap();
        assert_eq!(
            serde_json::from_slice::<Layout>(&serde_json::to_vec(&layout).unwrap()).unwrap(),
            layout
        );
        layout
            .composition
            .as_mut()
            .unwrap()
            .movable
            .push("schedule".into());
        assert!(validate(&layout).is_err());
        layout.composition.as_mut().unwrap().movable.pop();
        layout.composition.as_mut().unwrap().min_font_scale = 0.69;
        assert!(validate(&layout).is_err());
        layout.composition.as_mut().unwrap().min_font_scale = 0.7;
        validate(&layout).unwrap();
        layout
            .elements
            .last_mut()
            .unwrap()
            .table
            .as_mut()
            .unwrap()
            .panels = 0;
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn focus_visibility_retains_saved_style_and_defaults_old_templates_visible() {
        let mut layout = default_layout();
        layout.focus = Some(serde_json::from_value(json!({"buffer_m":100.,"outside_color":"#eeeeee","outside_opacity":0.4,"border_color":"#888888","border_width_mm":0.2})).unwrap());
        assert!(layout.focus.as_ref().unwrap().visible);
        let off =
            evaluated_layout(json!({"op":"set_focus_visibility","layout":layout,"visible":false}));
        assert!(!off.focus.as_ref().unwrap().visible);
        let restored =
            evaluated_layout(json!({"op":"set_focus_visibility","layout":off,"visible":true}));
        assert_eq!(restored.focus, layout.focus);
    }

    #[test]
    fn print_sources_focus_and_order_are_closed_and_bounded() {
        let mut l = default_layout();
        l.context_layers = vec![
            PrintContextLayer {
                id: "survey_poles".into(),
                label: "Surveyed poles".into(),
                source: PrintContextSource::Survey {
                    form: "lv_poles_as_built".into(),
                },
            },
            PrintContextLayer {
                id: "local".into(),
                label: "Local geometry".into(),
                source: PrintContextSource::LocalLayer {
                    layer_id: "local-123".into(),
                },
            },
        ];
        l.layer_order = vec!["contours".into(), "roads".into(), "lv_lines".into()];
        l.focus = Some(PrintFocus {
            visible: true,
            buffer_m: 35.,
            outside_color: "#E4E7E9".into(),
            outside_opacity: 0.6,
            border_color: "#7C8792".into(),
            border_width_mm: 0.25,
            border_opacity: Some(0.2),
        });
        validate(&l).unwrap();
        assert_eq!(
            serde_json::from_slice::<Layout>(&serde_json::to_vec(&l).unwrap()).unwrap(),
            l
        );
        l.layer_order.push("roads".into());
        assert!(validate(&l).is_err());
        l.layer_order.pop();
        l.focus.as_mut().unwrap().border_opacity = Some(1.1);
        assert!(validate(&l).is_err());
        l.focus.as_mut().unwrap().border_opacity = None;
        l.focus.as_mut().unwrap().buffer_m = -1.;
        assert!(validate(&l).is_err());
    }

    fn evaluated_layout(request: serde_json::Value) -> Layout {
        let result = evaluate(&serde_json::to_vec(&request).unwrap()).unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        serde_json::from_value(result["layout"].clone()).unwrap()
    }

    #[test]
    fn default_and_roundtrip_are_valid_and_unknown_fields_refuse() {
        let layout = default_layout();
        validate(&layout).unwrap();
        let encoded =
            serde_json::to_vec(&serde_json::json!({"op":"validate","layout":layout})).unwrap();
        assert!(evaluate(&encoded).unwrap().contains("\"valid\":true"));
        assert!(evaluate(br#"{"op":"new","unknown":true}"#).is_err());
        assert_eq!(layout_schema()["additionalProperties"], false);
    }

    #[test]
    fn transformer_views_validate_in_the_shared_printing_kernel() {
        let result = evaluate(br#"{"op":"transformer_views","views":{"tx_1":{"primary_map":{"mode":"camera","center_wgs84":[29.7,-2.5],"scale_denominator":1000}}}}"#).unwrap();
        assert!(result.contains("\"scale_denominator\":1000.0"));
        assert!(evaluate(br#"{"op":"transformer_views","views":{"TX 1":{}}}"#).is_err());
        assert!(evaluate(br#"{"op":"transformer_views","views":{"TX_1":{}}}"#).is_err());
        assert!(evaluate(br#"{"op":"transformer_views","views":{"tx_1":{"map":{"mode":"fit","extent_wgs84":[30,-2,29,-1]}}}}"#).is_err());
    }

    #[test]
    fn a_render_plan_admits_a_preview_only_with_its_styles_and_some_map_data() {
        let mut layout: serde_json::Value =
            serde_json::from_str(&evaluate(br#"{"op":"new","paper":"A3"}"#).expect("a new layout"))
                .expect("layout json");
        let layout = layout["layout"].take();
        let mut with_style = layout.clone();
        with_style["style_refs"] =
            serde_json::json!({"lv_lines": "lv_lines_print", "poles": "poles_print"});
        let plan = |layout: &serde_json::Value,
                    refs: serde_json::Value,
                    count: usize|
         -> serde_json::Value {
            let request = serde_json::json!({"op":"render_plan","layout":layout,"available_style_refs":refs,"layer_count":count,"text":{"title":"T","project":"P","drawing":"D","source":"S"},"date":"2026-09-11"});
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).expect("plan"))
                .unwrap()
        };
        let ready = plan(
            &with_style,
            serde_json::json!(["lv_lines_print", "poles_print", "other"]),
            2,
        );
        assert_eq!(ready["ready"], true);
        assert_eq!(
            ready["print_style_refs"],
            serde_json::json!(["lv_lines_print", "poles_print"])
        );
        assert_eq!(ready["text"]["date"], "2026-09-11");
        assert_eq!(ready["text"]["title"], "T");
        let missing = plan(&with_style, serde_json::json!(["lv_lines_print"]), 2);
        assert_eq!(missing["ready"], false);
        assert_eq!(missing["refusals"][0]["code"], "print_style_unavailable");
        assert_eq!(missing["refusals"][0]["params"]["reference"], "poles_print");
        let empty = plan(&layout, serde_json::json!([]), 0);
        assert_eq!(empty["refusals"][0]["code"], "print_preview_needs_layers");
        assert_eq!(empty["print_style_refs"], serde_json::json!([]));
    }

    #[test]
    fn native_inventory_excludes_reserved_names_and_marks_unobserved_rooms_unknown() {
        let request = json!({"op":"printable_transformers","rows":[
            {"name":"combined_transformer"},{"name":"MV_DATA","kind":"individual"},
            {"name":"collisions"},{"name":"compounded_report"},{"name":"  "},
            {"name":"tx_a","metadata":{"version":"0x10"}}
        ],"limit":10});
        let out: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["local_rooms_known"], false);
        assert_eq!(out["transformers"][0]["server_version"], 16.0);
        assert!(out["transformers"][0]["cached"].is_null());
        assert!(out["transformers"][0]["dirty"].is_null());
    }

    #[test]
    fn a_printable_inventory_is_one_command() {
        let request = serde_json::json!({"op":"printable_transformers","rows":[{"name":"b"},{"name":"a","kind":"individual"}],"rooms":[{"name":"A","owner_email":null,"local_dirty":true}],"current_user":"","limit":1});
        let out: serde_json::Value = serde_json::from_str(
            &evaluate(&serde_json::to_vec(&request).unwrap()).expect("inventory"),
        )
        .unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["transformers"][0]["name"], "a");
        assert_eq!(out["transformers"][0]["dirty"], true);
        assert_eq!(out["more"]["omitted"], 1);
        assert!(evaluate(br#"{"op":"printable_transformers","rows":[],"limit":0}"#).is_err());
    }

    #[test]
    fn a_project_output_patch_writes_the_export_row_and_the_reporter_view_nesting() {
        let request = serde_json::json!({"op":"project_output_patch",
            "current":[{"parameter":"tr-export-formats","value":"shp"},
                       {"parameter":"printing_views","value":{"tx_1":{"other":{"held":true}}}}],
            "selection":{"schema":"ds.design-output-selection/v1","prints":[
                {"layout_id":"detail","formats":["pdf"]}],"geospatial":["gpkg"],"tabular":[],
                "execution":{"pdf__detail":["desktop"]}},
            "layout_id":"detail",
            "views":{"tx_1":{"primary_map":{"mode":"camera","center_wgs84":[29.7,-2.5],"scale_denominator":1000}}}});
        let result: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        let rows = result["project_settings"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "no row is added when both already exist");
        assert_eq!(rows[0]["parameter"], "tr-export-formats");
        assert_eq!(
            rows[0]["value"]["execution"],
            serde_json::json!({"pdf__detail":["desktop"]})
        );
        assert_eq!(rows[0]["value"]["geospatial"], serde_json::json!(["gpkg"]));
        // The layout is nested under the transformer, beside what was held.
        assert_eq!(
            rows[1]["value"]["tx_1"]["other"],
            serde_json::json!({"held":true})
        );
        assert_eq!(
            rows[1]["value"]["tx_1"]["detail"]["primary_map"]["scale_denominator"],
            serde_json::json!(1000.0)
        );

        // A sheet with neither row gets both, under the first alias.
        let bare = serde_json::json!({"op":"project_output_patch","current":[],
            "selection":{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"]},
            "layout_id":"detail",
            "views":{"tx_1":{"primary_map":{"mode":"camera","center_wgs84":[29.7,-2.5],"scale_denominator":1000}}}});
        let created: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&bare).unwrap()).unwrap()).unwrap();
        let rows = created["project_settings"].as_array().unwrap();
        assert_eq!(rows[0]["parameter"], "design_export_format");
        assert_eq!(rows[1]["parameter"], "printing_views");

        // Views need the layout they were authored on, and are validated by
        // the same rule `transformer_views` applies.
        let mut missing = request.clone();
        missing["layout_id"] = serde_json::Value::Null;
        assert!(evaluate(&serde_json::to_vec(&missing).unwrap()).is_err());
        let mut invalid = request;
        invalid["views"] = serde_json::json!({"TX 1":{}});
        assert!(evaluate(&serde_json::to_vec(&invalid).unwrap()).is_err());
        assert!(evaluate(br#"{"op":"project_output_patch","current":[],"selection":{"schema":"ds.design-output-selection/v1"},"unknown":1}"#).is_err());
    }

    /// The host passes the stored value through; deciding which legacy shape
    /// it is was the TypeScript half this op replaces.
    #[test]
    fn context_ops_answer_defaults_catalogue_rows_and_toggles_over_host_facts() {
        let defaults: serde_json::Value =
            serde_json::from_str(&evaluate(br#"{"op":"context_defaults"}"#).unwrap()).unwrap();
        assert_eq!(defaults["derived"].as_array().unwrap().len(), 3);
        assert_eq!(
            defaults["style_keys"]["contours_index"]["entity_class"],
            "contours_index"
        );
        assert_eq!(defaults["excluded_catalog_layers"][0], "rwanda_upi_parcels");
        let rows: serde_json::Value = serde_json::from_str(
            &evaluate(
                br#"{"op":"context_catalog","resources":[{"layer":"roads","label":"Roads","country":"Rwanda","ready":true},{"layer":"wetlands","label":"Wetlands","country":"Rwanda"}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(rows["rows"][0]["default_on"], true);
        assert_eq!(rows["rows"][1]["refusal"], "printing_context_no_dataset");
        let layout = serde_json::to_string(&default_layout()).unwrap();
        let selected: serde_json::Value = serde_json::from_str(
            &evaluate(
                format!(
                    r#"{{"op":"context_default_selection","layout":{layout},"resources":[{{"layer":"roads","label":"Roads","country":"Rwanda","ready":true}}]}}"#
                )
                .as_bytes(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            selected["layout"]["context_layers"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert!(selected["layout"]["style_keys"]["dsgrid_mv_lines"].is_object());
        assert!(evaluate(
            format!(
                r#"{{"op":"context_toggle","layout":{layout},"layer":{{"id":"wetlands","label":"Wetlands","source":{{"kind":"catalog","country":"Rwanda","layer":"wetlands"}}}},"enabled":true,"resource":{{"layer":"wetlands","label":"Wetlands","country":"Rwanda"}}}}"#
            )
            .as_bytes()
        )
        .unwrap_err()
        .contains("printing_context_no_dataset"));
    }

    #[test]
    fn lifecycle_plan_is_one_printing_op_over_host_facts() {
        let plan: serde_json::Value = serde_json::from_str(
            &evaluate(
                br#"{"op":"lifecycle_plan","facts":{"intent":"save","library":"project","online":true,"editing":true,"loaded_matches":true,"held_revision":"abc"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(plan["decision"], "update");
        assert_eq!(plan["expected_revision"], "abc");
        let refused: serde_json::Value = serde_json::from_str(
            &evaluate(
                br#"{"op":"lifecycle_plan","facts":{"intent":"read","library":"global","online":false}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(refused["decision"], "refuse");
        assert_eq!(refused["refusal"]["message_key"], "printing_no_cached_copy");
        assert!(evaluate(
            br#"{"op":"lifecycle_plan","facts":{"intent":"read","library":"global","surprise":1}}"#
        )
        .is_err());
    }

    #[test]
    fn output_selection_reads_whatever_the_project_stored_and_answers_placements() {
        let result: serde_json::Value = serde_json::from_str(
            &evaluate(br#"{"op":"output_selection","stored":"png__detail,xlsx"}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(result["selection"]["prints"][0]["layout_id"], "detail");
        assert_eq!(
            result["selection"]["prints"][0]["formats"],
            serde_json::json!(["png"])
        );
        assert_eq!(
            result["placements"],
            serde_json::json!({"xlsx":["desktop","web"],"png__detail":["desktop"]})
        );
        let restricted: serde_json::Value = serde_json::from_str(&evaluate(
            br#"{"op":"output_selection","stored":{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"],"execution":{"geospatial":["web"]}}}"#,
        ).unwrap()).unwrap();
        assert_eq!(
            restricted["placements"],
            serde_json::json!({"gpkg":["web"]})
        );
        assert_eq!(
            evaluate(br#"{"op":"output_selection","stored":"gpkg","legacy_formats":["xlsx"]}"#),
            Err("pass a stored settings value alone, without a selection, legacy formats or placements".into())
        );
        // A row that holds `null` is a project that stored nothing, and reads
        // as an empty selection. Passing no `stored` at all is a different
        // request, and without an authoring input it is still refused.
        for empty in [
            br#"{"op":"output_selection","stored":null}"#.as_slice(),
            br#"{"op":"output_selection","stored":""}"#.as_slice(),
        ] {
            let read: serde_json::Value = serde_json::from_str(&evaluate(empty).unwrap()).unwrap();
            assert_eq!(read["selection"]["geospatial"], serde_json::json!([]));
            assert_eq!(read["placements"], serde_json::json!({}));
        }
        assert!(evaluate(br#"{"op":"output_selection"}"#).is_err());
    }

    /// The reply states the voltage-drop layout whether or not the selection
    /// carries it, and the settings patch writes a chosen `brief` through.
    #[test]
    fn output_selection_answers_the_voltage_drop_layout_and_the_patch_keeps_it() {
        let read = |request: &[u8]| -> serde_json::Value {
            serde_json::from_str(&evaluate(request).unwrap()).unwrap()
        };
        let default = read(br#"{"op":"output_selection","stored":"shp,voltage_drop"}"#);
        assert_eq!(default["voltage_drop_report"], "extended");
        assert!(default["selection"]["voltage_drop_report"].is_null());
        let brief = read(
            br#"{"op":"output_selection","selection":{"schema":"ds.design-output-selection/v1","tabular":["voltage_drop"],"voltage_drop_report":"brief"}}"#,
        );
        assert_eq!(brief["voltage_drop_report"], "brief");
        assert_eq!(brief["selection"]["voltage_drop_report"], "brief");
        let refused = evaluate(
            br#"{"op":"output_selection","stored":{"schema":"ds.design-output-selection/v1","voltage_drop_report":"summary"}}"#,
        )
        .unwrap_err();
        assert!(
            refused.contains("unknown voltage-drop report format `summary`"),
            "{refused}"
        );

        let patched = read(
            br#"{"op":"project_output_patch","current":[{"parameter":"design_export_format","value":"shp"}],"selection":{"schema":"ds.design-output-selection/v1","tabular":["voltage_drop"],"voltage_drop_report":"brief"}}"#,
        );
        assert_eq!(
            patched["project_settings"][0]["value"]["voltage_drop_report"],
            "brief"
        );
    }

    #[test]
    fn output_selection_can_require_the_layout_being_authored() {
        assert!(evaluate(
            br#"{"op":"output_selection","legacy_formats":["pdf__a3"],"required_layout":"a3"}"#
        )
        .is_ok());
        assert!(evaluate(
            br#"{"op":"output_selection","legacy_formats":["xlsx"],"required_layout":"a3"}"#
        )
        .is_err());
    }
    #[test]
    fn minimum_column_widths_are_ordered_and_bounded_by_caps() {
        let mut layout = default_layout();
        layout.elements.push(serde_json::from_value(serde_json::json!({"id":"info","kind":"table","rect":[20,20,100,100],
            "table":{"layer":"info","columns":["quantity"],"headings":["Quantity"],"widths":[1],"min_widths_mm":[6],"max_widths_mm":[10]}})).unwrap());
        validate(&layout).unwrap();
        for invalid in [vec![11.], vec![-1.], vec![f64::NAN], vec![6., 6.]] {
            layout
                .elements
                .last_mut()
                .unwrap()
                .table
                .as_mut()
                .unwrap()
                .min_widths_mm = invalid;
            assert!(validate(&layout).is_err());
        }
    }

    #[test]
    fn table_heading_measurement_is_optional_and_roundtrips() {
        let mut value = serde_json::json!({"layer":"customers","columns":["service_length"],"headings":["Service length"],"widths":[1]});
        let default: TableBinding = serde_json::from_value(value.clone()).unwrap();
        assert!(default.measure_headings);
        value["measure_headings"] = serde_json::json!(false);
        let data_only: TableBinding = serde_json::from_value(value).unwrap();
        assert!(!data_only.measure_headings);
        assert_eq!(
            serde_json::to_value(data_only).unwrap()["measure_headings"],
            false
        );
    }

    #[test]
    fn label_offsets_are_physical_bounded_and_roundtrip() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("spans".into(), "master/spans_print".into());
        layout.style_overrides.insert(
            "spans".into(),
            serde_json::from_value(json!({"label":{"offset_mm":[0,-1.4]}})).unwrap(),
        );
        validate(&layout).unwrap();
        assert_eq!(
            serde_json::from_value::<Layout>(serde_json::to_value(&layout).unwrap()).unwrap(),
            layout
        );
        layout
            .style_overrides
            .get_mut("spans")
            .unwrap()
            .label
            .as_mut()
            .unwrap()
            .offset_mm = Some([0., 21.]);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn context_layers_are_explicit_and_parameter_bounded() {
        let layout = default_layout();
        let result = evaluated_layout(json!({
            "op":"set_context_layers",
            "layout":layout,
            "context_layers":[
                {"id":"roads","label":"Roads","source":{"kind":"catalog","country":"Rwanda","layer":"roads"}},
                {"id":"buildings_context","label":"Building footprints","source":{"kind":"google_open_buildings","buffer_m":200.0,"min_area_m2":25.0,"min_confidence":0.7}},
                {"id":"elevation_contours","label":"10 m contours","source":{"kind":"elevation_contours","buffer_m":200.0,"minor_interval_m":10,"index_interval_m":50,"sample_spacing_m":10.0}},
                {"id":"project_dsgrid_mv","label":"Project MV models","source":{"kind":"project_dsgrid_mv","buffer_m":200.0}}
            ]
        }));
        assert_eq!(result.context_layers.len(), 4);
        let mut invalid = result;
        invalid.context_layers[2].source = PrintContextSource::ElevationContours {
            buffer_m: 200.,
            minor_interval_m: 10,
            index_interval_m: 55,
            sample_spacing_m: 10.,
        };
        assert!(validate(&invalid).is_err());
    }
    #[test]
    fn named_paper_templates_and_copies_keep_identity_explicit() {
        for paper in [
            Paper::A0,
            Paper::A1,
            Paper::A2,
            Paper::A3,
            Paper::A4,
            Paper::A5,
        ] {
            for orientation in [Orientation::Portrait, Orientation::Landscape] {
                let layout = named_layout(
                    Some("sample".into()),
                    Some("My template".into()),
                    paper,
                    orientation,
                )
                .unwrap();
                validate(&layout).unwrap();
                assert_eq!(layout.name, "My template");
            }
        }
        let layout = default_layout();
        assert!(evaluate(
            &serde_json::to_vec(
                &json!({"op":"duplicate","layout":layout,"id":layout.id,"name":"Copy"})
            )
            .unwrap()
        )
        .is_err());
        let result = evaluate(
            &serde_json::to_vec(&json!({"op":"rename","layout":layout,"name":"Renamed"})).unwrap(),
        )
        .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["layout"]["id"], layout.id);
        assert_eq!(result["layout"]["name"], "Renamed");
    }
    #[test]
    fn invalid_frames_and_unheld_logos_refuse() {
        let mut layout = default_layout();
        layout.elements[0].rect[2] = 1000.;
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.elements[0].rect[0] = f64::NAN;
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.elements[1].kind = ElementKind::Logo;
        layout.elements[1].asset = "missing".into();
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.assets.insert(
            "remote".into(),
            Asset {
                label: "Remote".into(),
                png_data_url: "https://example.test/logo.png".into(),
            },
        );
        assert!(validate(&layout).is_err());
    }
    #[test]
    fn edit_validates_whole_document_and_map_cannot_be_removed() {
        let layout = default_layout();
        let mut element = layout.elements[1].clone();
        element.text = "New title".into();
        let result = evaluate(
            &serde_json::to_vec(
                &serde_json::json!({"op":"edit","layout":layout,"element":element}),
            )
            .unwrap(),
        )
        .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["layout"]["elements"][1]["text"], "New title");
        assert_eq!(
            result["layout"]["elements"][0]["rect"],
            serde_json::json!([12., 32., 300., 235.])
        );
        assert!(evaluate(
            &serde_json::to_vec(&serde_json::json!({"op":"remove","layout":layout,"id":"map"}))
                .unwrap()
        )
        .is_err());
    }

    /// Front matter: the default sheet with every map-bound element removed.
    fn map_free_page() -> Layout {
        let mut layout = default_layout();
        layout.id = "atlas-front-matter".into();
        layout.elements.retain(|e| {
            !matches!(
                e.kind,
                ElementKind::Map
                    | ElementKind::Legend
                    | ElementKind::NorthArrow
                    | ElementKind::ScaleBar
            )
        });
        layout
    }

    #[test]
    fn map_free_cover_and_convention_pages_are_admitted_as_front_matter() {
        let mut cover = map_free_page();
        cover.assets.insert(
            "logo-a".into(),
            Asset {
                label: "a.png".into(),
                png_data_url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ"
                    .into(),
            },
        );
        cover.elements.extend([
            serde_json::from_value(
                json!({"id":"cover_frame","kind":"rectangle","rect":[12,40,396,200]}),
            )
            .unwrap(),
            serde_json::from_value(
                json!({"id":"logo","kind":"logo","rect":[20,50,40,20],"asset":"logo-a"}),
            )
            .unwrap(),
        ]);
        let mut convention = map_free_page();
        convention.elements.push(
            serde_json::from_value(json!({
                "id":"names","kind":"table","rect":[20,50,200,180],"font_pt":8,
                "table":{"layer":"names","columns":["code","meaning"],"headings":["Code","Meaning"],
                    "widths":[1,3],"panels":1,"row_mm":5,"align":"left"}
            }))
            .unwrap(),
        );
        for page in [cover, convention] {
            assert!(page.elements.iter().all(|e| e.kind != ElementKind::Map));
            validate(&page).unwrap();
            // The command surface admits it unchanged: no stand-in map added.
            assert_eq!(
                evaluated_layout(json!({"op":"validate","layout":page})),
                page
            );
            // A host that always projects still refuses it.
            assert_eq!(
                validate_geographic(&page).unwrap_err(),
                "a geographic sheet needs a map frame; layout atlas-front-matter has none"
            );
        }
    }

    #[test]
    fn map_free_page_refuses_what_only_a_map_can_honour() {
        let refusal = |layout: &Layout| validate(layout).unwrap_err();
        let donor = default_layout();
        let with_element = |kind: ElementKind, id: &str| {
            let mut page = map_free_page();
            page.elements.push(Element {
                id: id.into(),
                ..donor
                    .elements
                    .iter()
                    .find(|e| e.kind == kind)
                    .unwrap()
                    .clone()
            });
            page
        };
        assert_eq!(
            refusal(&with_element(ElementKind::NorthArrow, "north")),
            "north arrow north needs a map frame; layout atlas-front-matter has none"
        );
        assert_eq!(
            refusal(&with_element(ElementKind::ScaleBar, "scale")),
            "scale bar scale needs a map frame; layout atlas-front-matter has none"
        );
        let mut scale_text = map_free_page();
        scale_text.elements[0].text = "{title} {scale}".into();
        assert_eq!(
            refusal(&scale_text),
            "text title binding {scale} needs a map frame; layout atlas-front-matter has none"
        );
        type LayoutMutation = (&'static str, fn(&mut Layout));
        let options: [LayoutMutation; 6] = [
            ("a focus area", |l| {
                l.focus = Some(
                    serde_json::from_value(json!({"buffer_m":50.,"outside_color":"#ffffff",
                        "outside_opacity":0.5,"border_color":"#000000","border_width_mm":0.3}))
                    .unwrap(),
                )
            }),
            ("automatic furniture composition", |l| {
                l.composition = Some(Composition {
                    movable: vec!["title".into()],
                    gap_mm: 5.,
                    min_font_scale: 1.,
                    collision_margin_mm: 1.,
                })
            }),
            ("network fitting", |l| {
                l.network_fit = Some(
                    serde_json::from_value(json!({"source_layers":["roads"],"clearance_mm":2.}))
                        .unwrap(),
                )
            }),
            ("a map scale limit", |l| {
                l.scale_limits = Some(
                    serde_json::from_value(json!({"min_denominator":500.,"max_denominator":5000.}))
                        .unwrap(),
                )
            }),
            ("map scale rounding", |l| l.scale_rounding = Some(500.)),
            ("transformer bounds", |l| {
                l.transformer_bounds = Some(TransformerBounds {
                    source_layers: vec!["poles".into()],
                    identity_field: "transfo".into(),
                    buffer_m: 10.,
                })
            }),
        ];
        for (what, set) in options {
            let mut page = map_free_page();
            set(&mut page);
            assert_eq!(
                refusal(&page),
                format!("{what} needs a map frame; layout atlas-front-matter has none")
            );
        }
    }

    #[test]
    fn a_geographic_sheet_keeps_exactly_one_map_frame() {
        let layout = default_layout();
        validate(&layout).unwrap();
        validate_geographic(&layout).unwrap();
        let mut two = layout.clone();
        two.elements.push(Element {
            id: "map-2".into(),
            ..layout.elements[0].clone()
        });
        assert!(refused(&two, "exactly one map frame"));
        // Removing the map leaves a sheet whose north arrow has nothing to point at.
        assert_eq!(
            evaluate(
                &serde_json::to_vec(&json!({"op":"remove","layout":layout,"id":"map"})).unwrap()
            )
            .unwrap_err(),
            "north arrow north needs a map frame; layout network-map has none"
        );
    }

    fn refused(layout: &Layout, reason: &str) -> bool {
        validate(layout).is_err_and(|e| e.contains(reason))
    }

    #[test]
    fn legacy_layout_upgrades_to_explicit_paper_and_clipped_viewport() {
        let mut legacy = serde_json::to_value(default_layout()).unwrap();
        legacy["schema"] = json!(LEGACY_SCHEMA);
        legacy.as_object_mut().unwrap().remove("paper");
        legacy["elements"][0]
            .as_object_mut()
            .unwrap()
            .remove("viewport");
        let layout = evaluated_layout(json!({"op":"validate","layout":legacy}));
        assert_eq!(layout.schema, SCHEMA);
        assert_eq!(layout.paper.size, Paper::A3);
        assert_eq!(layout.paper.orientation, Orientation::Landscape);
        assert_eq!(
            layout.elements[0].viewport.as_ref().unwrap().view_slot,
            "primary_map"
        );
        assert!(layout.elements[0].viewport.as_ref().unwrap().clip);

        let mut custom = layout;
        custom.schema = LEGACY_SCHEMA.into();
        custom.page_mm = [1000., 700.];
        custom.paper = PaperSpec::default();
        custom.elements[0].viewport = None;
        let upgraded = upgrade(custom).unwrap();
        assert_eq!(
            upgraded.paper,
            PaperSpec {
                size: Paper::Custom {
                    short_mm: 700.,
                    long_mm: 1000.
                },
                orientation: Orientation::Landscape
            }
        );
    }

    #[test]
    fn paper_viewport_and_stack_commands_have_one_canonical_result() {
        let a0 = evaluated_layout(json!({
            "op":"set_paper",
            "layout":default_layout(),
            "paper":"A0",
            "orientation":"landscape"
        }));
        assert_eq!(a0.paper.size, Paper::A0);
        assert_eq!(a0.paper.orientation, Orientation::Landscape);
        assert_eq!(a0.page_mm, [1189., 841.]);

        let viewport = evaluated_layout(json!({
            "op":"set_viewport",
            "layout":a0,
            "id":"map",
            "name":"Transformer map",
            "view_slot":"transformer_primary"
        }));
        let binding = viewport.elements[0].viewport.as_ref().unwrap();
        assert_eq!(binding.name, "Transformer map");
        assert_eq!(binding.view_slot, "transformer_primary");
        assert!(binding.clip);

        let front = evaluated_layout(json!({
            "op":"bring_to_front",
            "layout":viewport,
            "id":"map"
        }));
        assert_eq!(front.elements.last().unwrap().id, "map");
        let back = evaluated_layout(json!({
            "op":"send_to_back",
            "layout":front,
            "id":"legend"
        }));
        assert_eq!(back.elements.first().unwrap().id, "legend");
        let above = evaluated_layout(json!({
            "op":"move_above",
            "layout":back,
            "id":"footer",
            "relative_to":"map"
        }));
        let map = element_index(&above, "map").unwrap();
        assert_eq!(above.elements[map + 1].id, "footer");
        let below = evaluated_layout(json!({
            "op":"move_below",
            "layout":above,
            "id":"footer",
            "relative_to":"map"
        }));
        let map = element_index(&below, "map").unwrap();
        assert_eq!(below.elements[map - 1].id, "footer");
    }

    #[test]
    fn viewport_metadata_cannot_be_attached_to_non_map_elements() {
        let mut layout = default_layout();
        layout.elements[1].viewport = Some(PrintViewport {
            name: "Wrong".into(),
            view_slot: "wrong".into(),
            clip: true,
            fit_around: Vec::new(),
            graticule: None,
        });
        assert!(validate(&layout).is_err());
        assert!(evaluate(
            &serde_json::to_vec(&json!({
                "op":"set_viewport",
                "layout":default_layout(),
                "id":"title",
                "name":"Wrong",
                "view_slot":"wrong"
            }))
            .unwrap()
        )
        .is_err());
    }
    #[test]
    fn category_sizes_need_a_field_and_scale_rounding_is_bounded() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("roads".into(), "gt/roads_print".into());
        let mut change = PrintStyleOverride::default();
        change.size_by_value.insert("National Road".into(), 4.4);
        layout.style_overrides.insert("roads".into(), change);
        assert!(validate(&layout).is_err());
        layout.style_overrides.get_mut("roads").unwrap().field = Some("type".into());
        layout.scale_rounding = Some(100.);
        validate(&layout).unwrap();
        layout.scale_rounding = Some(0.);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn template_label_overrides_validate_physical_limits() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("roads".into(), "gt/roads_print".into());
        let mut change = PrintStyleOverride {
            label: Some(PrintLabelOverride {
                size_pt: Some(8.),
                ..Default::default()
            }),
            ..Default::default()
        };
        layout
            .style_overrides
            .insert("roads".into(), change.clone());
        validate(&layout).unwrap();
        change.label.as_mut().unwrap().size_pt = Some(0.);
        layout.style_overrides.insert("roads".into(), change);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn a_label_only_administrative_boundary_is_a_valid_print_style() {
        let mut layout = default_layout();
        let mut style = PrintStyle {
            width_mm: 0.,
            stroke_opacity: 0.,
            label_field: "village".into(),
            ..Default::default()
        };
        layout
            .styles
            .insert("village_boundaries".into(), style.clone());
        // Zero width is the governed "labelled but never drawn" state, not a
        // refusal the operator has to work around with a 0.1 mm hairline.
        validate(&layout).expect("label-only administrative boundary");
        // A positive width still obeys the physical minimum.
        for refused in [0.01, -1., 10.5, f64::NAN] {
            style.width_mm = refused;
            layout
                .styles
                .insert("village_boundaries".into(), style.clone());
            assert!(
                validate(&layout).is_err(),
                "width {refused} must stay refused"
            );
        }
        style.width_mm = 0.;
        style.widths = BTreeMap::from([("built".into(), 0.), ("planned".into(), 0.4)]);
        layout
            .styles
            .insert("village_boundaries".into(), style.clone());
        validate(&layout).expect("a per-category width may also suppress its stroke");
        style.widths.insert("planned".into(), 0.01);
        layout.styles.insert("village_boundaries".into(), style);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn phase_selector_and_physical_hatch_bounds_are_explicit() {
        let mut hatch = PrintHatch {
            color: "#996633".into(),
            width_mm: 0.2,
            spacing_mm: 2.,
            angle_deg: 45.,
            match_field: "phase".into(),
            match_values: vec!["ii".into()],
        };
        assert!(hatch.validate().is_ok());
        assert!(hatch.matches(&serde_json::json!({"phase":"ii"})));
        assert!(!hatch.matches(&serde_json::json!({"phase":"i"})));
        assert!(!hatch.matches(&serde_json::json!({})));
        hatch.spacing_mm = 0.1;
        assert!(hatch.validate().is_err());
        let mut layout = default_layout();
        layout.transformer_bounds = Some(TransformerBounds {
            source_layers: vec!["tr".into(), "tr".into()],
            identity_field: "transfo".into(),
            buffer_m: 10.,
        });
        assert!(validate(&layout).is_err());
    }
    #[test]
    fn attribution_is_one_closed_layout_field_and_validated_by_every_reader() {
        let mut document = serde_json::to_value(default_layout()).unwrap();
        document["attribution"] = json!({"font_pt":7.,"color":"#595959","svg_font_family":"DejaVu Sans","pdf_font_family":"Helvetica","right_inset_pt":146.,"baseline_pt":7.});
        let layout: Layout = serde_json::from_value(document.clone()).unwrap();
        validate(&layout).unwrap();
        let canonical = serde_json::to_value(upgrade(layout).unwrap()).unwrap();
        assert_eq!(canonical["attribution"], document["attribution"]);
        let schema = layout_schema();
        assert!(schema["properties"]["attribution"].is_object());
        document["attribution"]["font_pts"] = json!(8.);
        assert!(serde_json::from_value::<Layout>(document.clone()).is_err());
        document["attribution"]
            .as_object_mut()
            .unwrap()
            .remove("font_pts");
        document["attribution"]["font_pt"] = json!(0.);
        assert!(validate(&serde_json::from_value(document).unwrap())
            .unwrap_err()
            .contains("print_attribution_invalid"));
    }
}
#[test]
fn monochrome_layout_cannot_resolve_a_colour_only_key() {
    let document = json!({"type":"line","paint":{"line-color":"#123456","line-width":1}});
    let key: crate::style_resolution::Key = serde_json::from_value(json!({"entity_class":"test_line","source_kind":"project_model","target":"print","role":"project"})).unwrap();
    let table: crate::style_resolution::Snapshot = serde_json::from_value(json!({
            "schema":"ds.style-resolution/v1","project_id":"test","revision_id":"a".repeat(64),
            "bindings":[{"key":key,"style_ref":"test/line_print","revision_id":"b".repeat(64),
                "content_sha256":crate::style_plan::style_digest(&document),"scope":"global","document":document}]
        })).unwrap();
    let mut layout = default_layout();
    layout.style_keys.insert("line".into(), key);
    layout.ink_mode = crate::style_resolution::Ink::Monochrome;
    let refusal = resolve_style_documents(layout, &table).unwrap_err();
    assert!(refusal.contains("style_resolution_unknown"));
    assert!(refusal.contains("monochrome"));
}
