//! The one table of preference keys: each key's wire name, its home (the
//! account, or one project), its byte bound and the contract that admits its
//! value. `Key` answers all four from here, so a new key does not compile
//! until it is placed in every column.
use super::design::{self, SourceKind};
use super::{map, workbench};
use crate::ui_preferences::{Key, DEFAULT_MAX_BYTES};
use serde_json::Value;

pub(in crate::ui_preferences) fn name(key: Key) -> &'static str {
    match key {
        Key::ActiveProject => "active_project",
        Key::LayerGroupCollapse => "layer_group_collapse",
        Key::HillshadeExaggeration => "hillshade_exaggeration",
        Key::MapDefaults => "map_defaults",
        Key::ToolLayout => "tool_layout",
        Key::TileLayers => "tile_layers",
        Key::ApplicationSettings => "application_settings",
        Key::EditProjectionChoice => "edit_projection_choice",
        Key::EditProjectionCustom => "edit_projection_custom",
        Key::PinnedGroupVisible => "pinned_group_visible",
        Key::ProfileOccupancy => "profile_occupancy",
        Key::CreateTransformerMapSources => "create_transformer_map_sources",
        Key::UploadDialogLayout => "upload_dialog_layout",
        Key::PropertiesDockWidth => "properties_dock_width",
        Key::StyleCenterLayout => "style_center_layout",
        Key::LayerVisibility => "layer_visibility",
        Key::PopupLayersDisabled => "popup_layers_disabled",
        Key::EarthEngineExplorer => "earth_engine_explorer",
        Key::WorkingArea => "working_area",
        Key::ProcessPreset => "process_preset",
        Key::ActiveLvTransformer => "active_lv_transformer",
        Key::StatusWarningTypes => "status_warning_types",
        Key::StatusSort => "status_sort",
        Key::MapViewport => "map_viewport",
        Key::PinnedTransformers => "pinned_transformers",
        Key::FastModeCustomerSource => "fast_mode_customer_source",
        Key::FastModePoleSource => "fast_mode_pole_source",
    }
}

/// Whether a key belongs to one project rather than to the account.
pub(in crate::ui_preferences) fn project_scoped(key: Key) -> bool {
    match key {
        Key::LayerVisibility
        | Key::PopupLayersDisabled
        | Key::EarthEngineExplorer
        | Key::WorkingArea
        | Key::ProcessPreset
        | Key::ActiveLvTransformer
        | Key::StatusWarningTypes
        | Key::StatusSort
        | Key::MapViewport
        | Key::PinnedTransformers
        | Key::FastModeCustomerSource
        | Key::FastModePoleSource => true,
        Key::ActiveProject
        | Key::LayerGroupCollapse
        | Key::HillshadeExaggeration
        | Key::MapDefaults
        | Key::ToolLayout
        | Key::TileLayers
        | Key::ApplicationSettings
        | Key::EditProjectionChoice
        | Key::EditProjectionCustom
        | Key::PinnedGroupVisible
        | Key::ProfileOccupancy
        | Key::CreateTransformerMapSources
        | Key::UploadDialogLayout
        | Key::PropertiesDockWidth
        | Key::StyleCenterLayout => false,
    }
}

/// The largest stored value, in bytes of its JSON text.
pub(in crate::ui_preferences) fn max_bytes(key: Key) -> usize {
    match key {
        Key::LayerGroupCollapse | Key::LayerVisibility | Key::TileLayers => 512 * 1024,
        Key::PopupLayersDisabled | Key::EarthEngineExplorer | Key::PinnedTransformers => 256 * 1024,
        Key::ApplicationSettings => 128 * 1024,
        Key::WorkingArea => workbench::MAX_BOUNDARY_BYTES + 64 * 1024,
        Key::ProfileOccupancy => workbench::PROFILE_OCCUPANCY_MAX_BYTES,
        _ => DEFAULT_MAX_BYTES,
    }
}

/// The canonical form of an offered or stored value, or `None` when the
/// key's contract refuses it.
pub(in crate::ui_preferences) fn admit(key: Key, value: &Value) -> Option<Value> {
    match key {
        Key::ActiveProject => super::active_project(value),
        Key::LayerGroupCollapse | Key::LayerVisibility => map::switches(value),
        Key::HillshadeExaggeration => map::hillshade_exaggeration(value),
        Key::MapDefaults => map::map_defaults(value),
        Key::MapViewport => map::map_viewport(value),
        Key::TileLayers => map::tile_layers(value),
        Key::EditProjectionChoice => map::edit_projection_choice(value),
        Key::EditProjectionCustom => map::edit_projection_custom(value),
        Key::PopupLayersDisabled => map::popup_layers_disabled(value),
        Key::EarthEngineExplorer => map::earth_engine_explorer(value),
        Key::ToolLayout => workbench::tool_layout(value),
        Key::PropertiesDockWidth => workbench::properties_dock_width(value),
        Key::UploadDialogLayout => workbench::upload_dialog_layout(value),
        Key::StyleCenterLayout => workbench::style_center_layout(value),
        Key::ApplicationSettings => workbench::application_settings(value),
        Key::WorkingArea => workbench::working_area(value),
        Key::ProfileOccupancy => workbench::profile_occupancy(value),
        Key::PinnedTransformers => design::pinned_transformers(value),
        Key::PinnedGroupVisible => design::switch(value),
        Key::ActiveLvTransformer => design::active_lv_transformer(value),
        Key::ProcessPreset => design::process_preset(value),
        Key::StatusSort => design::status_sort(value),
        Key::StatusWarningTypes => design::status_warning_types(value),
        Key::CreateTransformerMapSources => design::create_transformer_map_sources(value),
        Key::FastModeCustomerSource => design::fast_mode_sources(SourceKind::Customers, value),
        Key::FastModePoleSource => design::fast_mode_sources(SourceKind::Poles, value),
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
