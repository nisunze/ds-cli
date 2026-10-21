//! Application destinations and operator shortcut preferences.
//!
//! The screen supplies held grants and component answers; this owner projects
//! one menu family. Route URLs and translated labels remain presentation.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

macro_rules! binding_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}
binding_enum!(ApplicationDestinationId {
    Projects,
    Work,
    Factory,
    Solar,
    Governance,
    Reliability,
    Server,
    Project,
    Tools,
    Styles,
});
binding_enum!(ApplicationNavigationSurface { Map, Page });
binding_enum!(ApplicationNavigationFamily { Context, Commands });
binding_enum!(ApplicationDestinationGroup { Primary, Other });
binding_enum!(ApplicationDestinationAction { Styles });
binding_enum!(CommandShortcutId {
    Factory,
    Styles,
    Solar,
    Governance,
    Reliability,
    Server
});
binding_enum!(ApplicationNavigationTarget {
    Projects,
    Work,
    Forms,
    Solar,
    Governance,
    Reliability,
    Server,
    Tools
});

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationNavigationContext {
    pub surface: ApplicationNavigationSurface,
    pub family: ApplicationNavigationFamily,
    pub grants: BTreeMap<String, bool>,
    pub app_all: bool,
    pub map_component: bool,
    pub solar_component: bool,
    pub active_project: Option<String>,
    pub current: Option<ApplicationDestinationId>,
    /// Exact stored JSON; malformed/retired preferences take current defaults.
    pub saved_visibility: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDestination {
    pub id: ApplicationDestinationId,
    pub label_key: String,
    pub icon: String,
    pub group: ApplicationDestinationGroup,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ApplicationNavigationTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<ApplicationDestinationAction>,
    pub reload_document: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct CommandShortcutPreference {
    pub id: CommandShortcutId,
    pub label_key: String,
    pub visible: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct CommandShortcutChange {
    pub id: CommandShortcutId,
    pub visible: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct CommandShortcutPreferences {
    pub entries: Vec<CommandShortcutPreference>,
    /// The normalized persisted record, with retired keys removed.
    pub serialized: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ApplicationNavigationProjection {
    pub destinations: Vec<ApplicationDestination>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct CurrentApplicationDestination {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<ApplicationDestinationId>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct CommandShortcutPreferencesRequest {
    pub saved: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<CommandShortcutChange>,
}

const SHORTCUTS: [(CommandShortcutId, &str, &str); 6] = [
    (CommandShortcutId::Factory, "factory", "nav_factory"),
    (CommandShortcutId::Styles, "styles", "nav_styles"),
    (CommandShortcutId::Solar, "solar", "nav_solar"),
    (
        CommandShortcutId::Governance,
        "governance",
        "nav_governance",
    ),
    (
        CommandShortcutId::Reliability,
        "reliability",
        "nav_reliability",
    ),
    (CommandShortcutId::Server, "server", "nav_server"),
];

pub fn shortcut_preferences(
    saved: &str,
    change: Option<CommandShortcutChange>,
) -> CommandShortcutPreferences {
    let saved: Value = serde_json::from_str(saved).unwrap_or(Value::Null);
    let mut normalized = serde_json::Map::new();
    let entries = SHORTCUTS
        .into_iter()
        .map(|(id, key, label)| {
            let visible = change
                .as_ref()
                .filter(|change| change.id == id)
                .map(|change| change.visible)
                .unwrap_or_else(|| saved.get(key).and_then(Value::as_bool).unwrap_or(true));
            normalized.insert(key.to_owned(), Value::Bool(visible));
            CommandShortcutPreference {
                id,
                label_key: label.to_owned(),
                visible,
            }
        })
        .collect();
    CommandShortcutPreferences {
        entries,
        serialized: Value::Object(normalized).to_string(),
    }
}

struct Spec {
    id: ApplicationDestinationId,
    label: &'static str,
    icon: &'static str,
    target: Option<ApplicationNavigationTarget>,
    shortcut: Option<CommandShortcutId>,
    cap: Option<&'static str>,
    app_all: bool,
}

/// Tools belong to Commands; Backups belongs to Governance. Printing settings
/// are reached through their template project, never a standalone command.
pub fn destinations(context: &ApplicationNavigationContext) -> Vec<ApplicationDestination> {
    use ApplicationDestinationId as Id;
    use ApplicationNavigationTarget as Target;
    use CommandShortcutId as Shortcut;
    let catalog = [
        Spec {
            id: Id::Tools,
            label: "nav_tools",
            icon: "⚒",
            target: Some(Target::Tools),
            shortcut: None,
            cap: None,
            app_all: false,
        },
        Spec {
            id: Id::Projects,
            label: "nav_projects",
            icon: "■",
            target: Some(Target::Projects),
            shortcut: None,
            cap: None,
            app_all: false,
        },
        Spec {
            id: Id::Work,
            label: "nav_project_work",
            icon: "▣",
            target: Some(Target::Work),
            shortcut: None,
            cap: None,
            app_all: false,
        },
        Spec {
            id: Id::Factory,
            label: "nav_factory",
            icon: "🏭",
            target: Some(Target::Forms),
            shortcut: Some(Shortcut::Factory),
            cap: Some("ui.forms"),
            app_all: false,
        },
        Spec {
            id: Id::Solar,
            label: "nav_solar_project",
            icon: "☀",
            target: Some(Target::Solar),
            shortcut: Some(Shortcut::Solar),
            cap: Some("ui.solar"),
            app_all: false,
        },
        Spec {
            id: Id::Styles,
            label: "nav_styles",
            icon: "🎨",
            target: None,
            shortcut: Some(Shortcut::Styles),
            cap: Some("ui.styles"),
            app_all: false,
        },
        Spec {
            id: Id::Governance,
            label: "nav_governance",
            icon: "◆",
            target: Some(Target::Governance),
            shortcut: Some(Shortcut::Governance),
            cap: Some("ui.governance"),
            app_all: false,
        },
        Spec {
            id: Id::Reliability,
            label: "nav_reliability",
            icon: "▥",
            target: Some(Target::Reliability),
            shortcut: Some(Shortcut::Reliability),
            cap: None,
            app_all: true,
        },
        Spec {
            id: Id::Server,
            label: "nav_server",
            icon: "▰",
            target: Some(Target::Server),
            shortcut: Some(Shortcut::Server),
            cap: None,
            app_all: true,
        },
    ];
    let preferences = shortcut_preferences(&context.saved_visibility, None);
    catalog
        .into_iter()
        .filter_map(|spec| {
            let primary = matches!(spec.id, Id::Projects | Id::Work | Id::Solar);
            if primary != (context.family == ApplicationNavigationFamily::Context)
                || Some(spec.id) == context.current
                || spec.app_all && !context.app_all
                || spec.cap.is_some_and(|cap| {
                    !context.grants.get(cap).copied().unwrap_or(false)
                        && !(spec.id == Id::Governance && context.app_all)
                })
                || spec.shortcut.is_some_and(|id| {
                    preferences
                        .entries
                        .iter()
                        .any(|entry| entry.id == id && !entry.visible)
                })
                || spec.id == Id::Styles && !context.map_component
                || spec.id == Id::Solar
                    && (!context.solar_component
                        || context.active_project.as_deref().is_none_or(str::is_empty))
            {
                return None;
            }
            Some(ApplicationDestination {
                id: spec.id,
                label_key: spec.label.to_owned(),
                icon: spec.icon.to_owned(),
                group: if primary {
                    ApplicationDestinationGroup::Primary
                } else {
                    ApplicationDestinationGroup::Other
                },
                target: spec.target,
                project: (spec.id == Id::Solar)
                    .then(|| context.active_project.clone())
                    .flatten(),
                view: (spec.id == Id::Governance
                    && !context
                        .grants
                        .get("ui.governance")
                        .copied()
                        .unwrap_or(false))
                .then(|| "backups".to_owned()),
                action: (spec.id == Id::Styles).then_some(ApplicationDestinationAction::Styles),
                reload_document: spec.id == Id::Server,
            })
        })
        .collect()
}

/// Identify the current application destination; Governance tabs share their
/// parent destination. This does not construct backend or application URLs.
pub fn current_destination(pathname: &str) -> Option<ApplicationDestinationId> {
    use ApplicationDestinationId as Id;
    let path = pathname.trim_end_matches('/');
    match path {
        "/projects" => Some(Id::Projects),
        "/tools" => Some(Id::Tools),
        "/work" => Some(Id::Work),
        "/forms" => Some(Id::Factory),
        "/solar" => Some(Id::Solar),
        "/governance" => Some(Id::Governance),
        "/sre" => Some(Id::Reliability),
        "/server" => Some(Id::Server),
        _ if path.starts_with("/forms/") => Some(Id::Factory),
        _ => {
            let pieces: Vec<_> = path.split('/').collect();
            (pieces.len() >= 3
                && pieces[0].is_empty()
                && pieces[1] == "projects"
                && !pieces[2].is_empty()
                && (pieces.len() == 3
                    || pieces.len() == 4
                        && matches!(
                            pieces[3],
                            "assets"
                                | "data"
                                | "uploads"
                                | "tiles"
                                | "data-audit"
                                | "moments"
                                | "local-data"
                        )))
            .then_some(Id::Project)
        }
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::{Ts, Tsify};
    use wasm_bindgen::prelude::*;
    fn js(error: tsify::Error) -> JsValue {
        JsError::new(&error.to_string()).into()
    }
    #[wasm_bindgen(js_name = applicationNavigation)]
    pub fn project(
        context: Ts<ApplicationNavigationContext>,
    ) -> Result<Ts<ApplicationNavigationProjection>, JsValue> {
        ApplicationNavigationProjection {
            destinations: destinations(&context.to_rust().map_err(js)?),
        }
        .into_ts()
        .map_err(js)
    }
    #[wasm_bindgen(js_name = commandShortcutPreferences)]
    pub fn shortcuts(
        request: Ts<CommandShortcutPreferencesRequest>,
    ) -> Result<Ts<CommandShortcutPreferences>, JsValue> {
        let request = request.to_rust().map_err(js)?;
        shortcut_preferences(&request.saved, request.change)
            .into_ts()
            .map_err(js)
    }
    #[wasm_bindgen(js_name = applicationDestination)]
    pub fn current(pathname: &str) -> Result<Ts<CurrentApplicationDestination>, JsValue> {
        CurrentApplicationDestination {
            current: current_destination(pathname),
        }
        .into_ts()
        .map_err(js)
    }
}

#[cfg(test)]
#[path = "application_navigation_tests.rs"]
mod tests;
