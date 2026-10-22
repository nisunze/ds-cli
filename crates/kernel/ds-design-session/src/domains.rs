//! Domain plug-ins: `LvDomain` + `MvDomain` + `TempDomain` impls of
//! [`crate::traits::LayerDomain`].
//!
//! These are the seam where domain knowledge meets the foundation.
//! `ds-geo` knows nothing about LV poles or MV lines; this module
//! teaches it just enough to tie-break picks and arrange attribute-table
//! columns per layer. `ds-web` constructs sessions with one of these
//! domains via `SessionHandle::with_domain`.
//!
//! Contract: `docs/contracts/geojson-foundation-crate.md` §5.

use crate::traits::{
    ColumnKind, ColumnPolicy, ColumnSpec, FormBinding, GeometryKind, LayerDomain, PickContext,
    PickPriority, TabGroup,
};
use serde_json::Value;
use std::sync::Arc;

// ---------------------------------------------------------------------
// LV domain
// ---------------------------------------------------------------------

pub struct LvDomain {
    columns: LvColumnPolicy,
    pick: LvPickPriority,
    forms: LvFormBinding,
}

impl LvDomain {
    pub fn new() -> Self {
        Self {
            columns: LvColumnPolicy,
            pick: LvPickPriority,
            forms: LvFormBinding,
        }
    }
    pub fn shared() -> Arc<dyn LayerDomain> {
        Arc::new(Self::new())
    }
}

impl Default for LvDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerDomain for LvDomain {
    fn id(&self) -> &str {
        "lv"
    }
    fn pick_priority(&self) -> &dyn PickPriority {
        &self.pick
    }
    fn column_policy(&self) -> &dyn ColumnPolicy {
        &self.columns
    }
    fn form_binding(&self) -> Option<&dyn FormBinding> {
        Some(&self.forms)
    }
}

pub struct LvPickPriority;

impl PickPriority for LvPickPriority {
    fn score(&self, _props: &Value, ctx: &PickContext) -> i32 {
        match ctx.geometry_kind {
            GeometryKind::Point => 100,
            GeometryKind::LineString => 50,
            GeometryKind::Polygon => 10,
        }
    }
}

pub struct LvColumnPolicy;

impl ColumnPolicy for LvColumnPolicy {
    fn columns_for(&self, layer_id: &str) -> Vec<ColumnSpec> {
        vec![
            ColumnSpec {
                key: "id".into(),
                label: "ID".into(),
                kind: ColumnKind::String,
                width_px: Some(120),
                editable: false,
            },
            ColumnSpec {
                key: "name".into(),
                label: "Name".into(),
                kind: ColumnKind::String,
                width_px: Some(160),
                editable: true,
            },
        ]
        .into_iter()
        .chain(layer_specific_extra_columns_lv(layer_id))
        .collect()
    }
    fn group_for_tab(&self, _layer_id: &str) -> TabGroup {
        TabGroup::new("lv")
    }
}

fn layer_specific_extra_columns_lv(layer_id: &str) -> Vec<ColumnSpec> {
    match layer_id {
        "customers" => vec![ColumnSpec {
            key: "load_kva".into(),
            label: "Load (kVA)".into(),
            kind: ColumnKind::Float,
            width_px: Some(100),
            editable: true,
        }],
        "lv_poles" => vec![ColumnSpec {
            key: "assembly_type".into(),
            label: "Assembly".into(),
            kind: ColumnKind::String,
            width_px: Some(120),
            editable: true,
        }],
        _ => vec![],
    }
}

pub struct LvFormBinding;

impl FormBinding for LvFormBinding {
    fn form_slug_for(&self, layer_id: &str) -> Option<&str> {
        match layer_id {
            "customers" => Some("customers"),
            "lv_poles" => Some("lv_poles"),
            "lv_lines" => Some("lv_lines"),
            "drop_cables" => Some("drop_cables"),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------
// MV domain
// ---------------------------------------------------------------------

pub struct MvDomain {
    columns: MvColumnPolicy,
    pick: MvPickPriority,
    forms: MvFormBinding,
}

impl MvDomain {
    pub fn new() -> Self {
        Self {
            columns: MvColumnPolicy,
            pick: MvPickPriority,
            forms: MvFormBinding,
        }
    }
    pub fn shared() -> Arc<dyn LayerDomain> {
        Arc::new(Self::new())
    }
}

impl Default for MvDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerDomain for MvDomain {
    fn id(&self) -> &str {
        "mv"
    }
    fn pick_priority(&self) -> &dyn PickPriority {
        &self.pick
    }
    fn column_policy(&self) -> &dyn ColumnPolicy {
        &self.columns
    }
    fn form_binding(&self) -> Option<&dyn FormBinding> {
        Some(&self.forms)
    }
}

pub struct MvPickPriority;

impl PickPriority for MvPickPriority {
    fn score(&self, _props: &Value, ctx: &PickContext) -> i32 {
        match ctx.geometry_kind {
            GeometryKind::Point => 100,
            GeometryKind::LineString => 60,
            GeometryKind::Polygon => 20,
        }
    }
}

pub struct MvColumnPolicy;

impl ColumnPolicy for MvColumnPolicy {
    fn columns_for(&self, layer_id: &str) -> Vec<ColumnSpec> {
        vec![
            ColumnSpec {
                key: "id".into(),
                label: "ID".into(),
                kind: ColumnKind::String,
                width_px: Some(120),
                editable: false,
            },
            ColumnSpec {
                key: "voltage_kv".into(),
                label: "Voltage (kV)".into(),
                kind: ColumnKind::Float,
                width_px: Some(110),
                editable: true,
            },
        ]
        .into_iter()
        .chain(layer_specific_extra_columns_mv(layer_id))
        .collect()
    }
    fn group_for_tab(&self, _layer_id: &str) -> TabGroup {
        TabGroup::new("mv")
    }
}

fn layer_specific_extra_columns_mv(layer_id: &str) -> Vec<ColumnSpec> {
    match layer_id {
        "mv_lines" | "mv_structures" => vec![ColumnSpec {
            key: "import_key".into(),
            label: "Import key".into(),
            kind: ColumnKind::String,
            width_px: Some(140),
            editable: false,
        }],
        _ => vec![],
    }
}

pub struct MvFormBinding;

impl FormBinding for MvFormBinding {
    fn form_slug_for(&self, layer_id: &str) -> Option<&str> {
        match layer_id {
            "mv_lines" => Some("mv_lines"),
            "mv_structures" => Some("mv_structures"),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------
// Temp domain (sketches, ad-hoc imports, drop-in user data)
// ---------------------------------------------------------------------

pub struct TempDomain {
    columns: TempColumnPolicy,
    pick: TempPickPriority,
}

impl TempDomain {
    pub fn new() -> Self {
        Self {
            columns: TempColumnPolicy,
            pick: TempPickPriority,
        }
    }
    pub fn shared() -> Arc<dyn LayerDomain> {
        Arc::new(Self::new())
    }
}

impl Default for TempDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerDomain for TempDomain {
    fn id(&self) -> &str {
        "temp"
    }
    fn pick_priority(&self) -> &dyn PickPriority {
        &self.pick
    }
    fn column_policy(&self) -> &dyn ColumnPolicy {
        &self.columns
    }
}

pub struct TempPickPriority;

impl PickPriority for TempPickPriority {
    fn score(&self, _props: &Value, ctx: &PickContext) -> i32 {
        match ctx.geometry_kind {
            GeometryKind::Point => 30,
            GeometryKind::LineString => 15,
            GeometryKind::Polygon => 5,
        }
    }
}

pub struct TempColumnPolicy;

impl ColumnPolicy for TempColumnPolicy {
    fn columns_for(&self, _layer_id: &str) -> Vec<ColumnSpec> {
        vec![ColumnSpec {
            key: "id".into(),
            label: "ID".into(),
            kind: ColumnKind::String,
            width_px: Some(140),
            editable: false,
        }]
    }
    fn group_for_tab(&self, _layer_id: &str) -> TabGroup {
        TabGroup::new("temp")
    }
}

#[cfg(test)]
#[path = "domains_tests.rs"]
mod tests;
