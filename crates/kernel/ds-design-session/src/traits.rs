//! Domain display/pick policy seam for Kernel-owned design state.
//!
//! Domain crates (`ds-network`, future `ds-temp-domain`, etc.)
//! implement these traits to teach the foundation about their layers
//! without session state ever matching on layer-id strings.
//!
//! Contract: `docs/contracts/geojson-foundation-crate.md` §5.

use serde::{Deserialize, Serialize};

/// What every layer source has to tell the foundation about itself.
///
/// `ds-geo` calls `pick_priority()` on tie-breaks at the R-tree and
/// `column_policy()` when rendering attribute tables. Domains compose
/// these from constants — no per-call allocation expected.
pub trait LayerDomain: Send + Sync + 'static {
    /// Stable identifier — `"lv"`, `"mv"`, `"temp"`, `"sketch"`.
    /// Surfaces as `SelectionResolution::domain` for highlight tinting
    /// in ds-web.
    fn id(&self) -> &str;

    /// Tie-break when multiple features sit on the same click point.
    fn pick_priority(&self) -> &dyn PickPriority;

    /// Which columns the attribute table tab renders, in what order.
    fn column_policy(&self) -> &dyn ColumnPolicy;

    /// Optional binding to a Form Factory form. `None` for layers that
    /// don't have a survey/data-entry form.
    fn form_binding(&self) -> Option<&dyn FormBinding> {
        None
    }
}

#[derive(Clone, Debug)]
pub struct PickContext<'a> {
    pub layer_id: &'a str,
    pub geometry_kind: GeometryKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryKind {
    Point,
    LineString,
    Polygon,
}

/// Tie-break score. Higher wins. Foundation guarantees deterministic
/// fallback when scores tie (lex-ordered layer id).
pub trait PickPriority: Send + Sync {
    fn score(&self, props: &serde_json::Value, ctx: &PickContext) -> i32;
}

/// What the attribute table tab renders + how it filters by default.
pub trait ColumnPolicy: Send + Sync {
    fn columns_for(&self, layer_id: &str) -> Vec<ColumnSpec>;
    fn default_sort(&self, layer_id: &str) -> Option<SortSpec> {
        let _ = layer_id;
        None
    }
    fn default_filter(&self, layer_id: &str) -> Option<FilterSpec> {
        let _ = layer_id;
        None
    }
    /// Tab grouping hint: `"lv"`, `"mv"`, `"design"`, `"survey"`. Lets
    /// ds-web group sibling layers under a single header in the table.
    fn group_for_tab(&self, layer_id: &str) -> TabGroup;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub key: String,
    pub label: String,
    pub kind: ColumnKind,
    pub width_px: Option<u32>,
    #[serde(default)]
    pub editable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    String,
    Integer,
    Float,
    Boolean,
    Geometry,
    Reference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SortSpec {
    pub column: String,
    pub descending: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilterSpec {
    pub column: String,
    pub expression: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabGroup(pub String);

impl TabGroup {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Optional — only used by domains that bind features to a Form Factory form.
pub trait FormBinding: Send + Sync {
    fn form_slug_for(&self, layer_id: &str) -> Option<&str>;
}
