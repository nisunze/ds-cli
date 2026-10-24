//! The one table of preference keys: each key's wire name, its home (the
//! account, or one project), its byte bound and the contract that admits its
//! value. `Key` answers all four from here, so a new key does not compile
//! until it is placed in every column.
use super::design::{self, SourceKind};
use super::{map, project_view, window, workbench};
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
        Key::PropertiesPreferDocked => "properties_prefer_docked",
        Key::StyleCenterLayout => "style_center_layout",
        Key::MessagingNotificationDismissals => "messaging_notification_dismissals",
        Key::LocalClassStyles => "local_class_styles",
        Key::ProcessSetup => "process_setup",
        Key::LayerVisibility => "layer_visibility",
        Key::PopupLayersDisabled => "popup_layers_disabled",
        Key::WorkingArea => "working_area",
        Key::WorkingAreaForms => "working_area_forms",
        Key::ProcessPreset => "process_preset",
        Key::ActiveLvTransformer => "active_lv_transformer",
        Key::StatusWarningTypes => "status_warning_types",
        Key::StatusSort => "status_sort",
        Key::MapViewport => "map_viewport",
        Key::PinnedTransformers => "pinned_transformers",
        Key::FastModeCustomerSource => "fast_mode_customer_source",
        Key::FastModePoleSource => "fast_mode_pole_source",
        Key::Theme => "theme",
        Key::Language => "language",
        Key::OfflineMode => "offline_mode",
        Key::ActivityHints => "activity_hints",
        Key::DesktopRecommendationShownAt => "desktop_recommendation_shown_at",
        Key::LayerGroupSettings => "layer_group_settings",
        Key::CommandShortcutsVisible => "command_shortcuts_visible",
        Key::DrawerWidth => "drawer_width",
        Key::ConnectorStyle => "connector_style",
        Key::ToolsPanelSize => "tools_panel_size",
        Key::PdfScreenScale => "pdf_screen_scale",
        Key::ProfileWorkspace => "profile_workspace",
        Key::ProfileViewports => "profile_viewports",
        Key::LayerWorkspace => "layer_workspace",
        Key::ProjectWorkspace => "project_workspace",
        Key::BoardView => "board_view",
        Key::PaperPlacements => "paper_placements",
        Key::PaperNoteStyle => "paper_note_style",
        Key::PaperSnap => "paper_snap",
        Key::PaperCollapsedGroups => "paper_collapsed_groups",
        Key::PaperToolDock => "paper_tool_dock",
    }
}

/// Whether a key belongs to one project rather than to the account.
pub(in crate::ui_preferences) fn project_scoped(key: Key) -> bool {
    match key {
        Key::LayerVisibility
        | Key::PopupLayersDisabled
        | Key::WorkingArea
        | Key::WorkingAreaForms
        | Key::ProcessPreset
        | Key::ProcessSetup
        | Key::ActiveLvTransformer
        | Key::StatusWarningTypes
        | Key::StatusSort
        | Key::MapViewport
        | Key::PinnedTransformers
        | Key::FastModeCustomerSource
        | Key::FastModePoleSource
        | Key::LayerWorkspace
        | Key::ProjectWorkspace
        | Key::BoardView
        | Key::PaperPlacements
        | Key::PaperNoteStyle
        | Key::PaperSnap
        | Key::PaperCollapsedGroups
        | Key::PaperToolDock => true,
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
        | Key::PropertiesPreferDocked
        | Key::StyleCenterLayout
        | Key::MessagingNotificationDismissals
        | Key::LocalClassStyles
        | Key::Theme
        | Key::Language
        | Key::OfflineMode
        | Key::ActivityHints
        | Key::DesktopRecommendationShownAt
        | Key::LayerGroupSettings
        | Key::CommandShortcutsVisible
        | Key::DrawerWidth
        | Key::ConnectorStyle
        | Key::ToolsPanelSize
        | Key::PdfScreenScale
        | Key::ProfileWorkspace
        | Key::ProfileViewports => false,
    }
}

/// The largest stored value, in bytes of its JSON text.
pub(in crate::ui_preferences) fn max_bytes(key: Key) -> usize {
    match key {
        Key::LayerGroupCollapse | Key::LayerVisibility | Key::TileLayers => 512 * 1024,
        Key::PopupLayersDisabled | Key::PinnedTransformers => 256 * 1024,
        Key::ApplicationSettings => 128 * 1024,
        Key::LocalClassStyles => map::MAX_LOCAL_CLASS_STYLES_BYTES,
        Key::WorkingArea => workbench::MAX_BOUNDARY_BYTES + 64 * 1024,
        Key::WorkingAreaForms => crate::survey_working_area_forms::MAX_SELECTION_BYTES,
        Key::ProfileOccupancy => workbench::PROFILE_OCCUPANCY_MAX_BYTES,
        Key::PaperPlacements => 512 * 1024,
        Key::ProfileViewports | Key::PaperCollapsedGroups => 256 * 1024,
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
        Key::ToolLayout => workbench::tool_layout(value),
        Key::PropertiesDockWidth => workbench::properties_dock_width(value),
        Key::UploadDialogLayout => workbench::upload_dialog_layout(value),
        Key::StyleCenterLayout => workbench::style_center_layout(value),
        Key::ApplicationSettings => workbench::application_settings(value),
        Key::WorkingArea => workbench::working_area(value),
        Key::WorkingAreaForms => crate::survey_working_area_forms::admit_selection(value),
        Key::ProfileOccupancy => workbench::profile_occupancy(value),
        Key::PinnedTransformers => design::pinned_transformers(value),
        Key::PinnedGroupVisible | Key::PropertiesPreferDocked => design::switch(value),
        Key::ActiveLvTransformer => design::active_lv_transformer(value),
        Key::ProcessPreset => design::process_preset(value),
        Key::ProcessSetup => design::process_setup(value),
        Key::MessagingNotificationDismissals => workbench::notification_dismissals(value),
        Key::LocalClassStyles => map::local_class_styles(value),
        Key::StatusSort => design::status_sort(value),
        Key::StatusWarningTypes => design::status_warning_types(value),
        Key::CreateTransformerMapSources => design::create_transformer_map_sources(value),
        Key::FastModeCustomerSource => design::fast_mode_sources(SourceKind::Customers, value),
        Key::FastModePoleSource => design::fast_mode_sources(SourceKind::Poles, value),
        Key::Theme => window::theme(value),
        Key::Language => window::language(value),
        Key::OfflineMode => design::switch(value),
        Key::ActivityHints => window::activity_hints(value),
        Key::DesktopRecommendationShownAt => window::instant(value),
        Key::LayerGroupSettings => window::layer_group_settings(value),
        Key::CommandShortcutsVisible => window::command_shortcuts(value),
        Key::DrawerWidth => window::drawer_width(value),
        Key::ConnectorStyle => window::connector_style(value),
        Key::ToolsPanelSize => window::tools_panel_size(value),
        Key::PdfScreenScale => window::pdf_screen_scale(value),
        Key::ProfileWorkspace => window::profile_workspace(value),
        Key::ProfileViewports => window::profile_viewports(value),
        Key::LayerWorkspace => project_view::layer_workspace(value),
        Key::ProjectWorkspace => project_view::project_workspace(value),
        Key::BoardView => project_view::board_view(value),
        Key::PaperPlacements => project_view::paper_placements(value),
        Key::PaperNoteStyle => project_view::paper_note_style(value),
        Key::PaperSnap => design::switch(value),
        Key::PaperCollapsedGroups => project_view::paper_collapsed_groups(value),
        Key::PaperToolDock => project_view::paper_tool_dock(value),
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
