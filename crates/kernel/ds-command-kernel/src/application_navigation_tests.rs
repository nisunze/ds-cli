use super::*;

fn context(family: ApplicationNavigationFamily) -> ApplicationNavigationContext {
    ApplicationNavigationContext {
        surface: ApplicationNavigationSurface::Page,
        family,
        grants: ["ui.forms", "ui.solar", "ui.styles", "ui.governance"]
            .into_iter()
            .map(|key| (key.to_owned(), true))
            .collect(),
        app_all: true,
        map_component: true,
        solar_component: true,
        active_project: Some("project/a b".into()),
        current: None,
        saved_visibility: "{}".into(),
    }
}
#[test]
fn menu_families_share_one_projection_and_tools_stays_in_commands() {
    use ApplicationDestinationId as Id;
    for surface in [
        ApplicationNavigationSurface::Map,
        ApplicationNavigationSurface::Page,
    ] {
        let mut context = context(ApplicationNavigationFamily::Context);
        context.surface = surface;
        assert_eq!(
            destinations(&context)
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            [Id::Projects, Id::Work, Id::Solar]
        );
        context.family = ApplicationNavigationFamily::Commands;
        assert_eq!(
            destinations(&context)
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            [
                Id::Tools,
                Id::Factory,
                Id::Styles,
                Id::Governance,
                Id::Reliability,
                Id::Server
            ]
        );
        assert!(destinations(&context)
            .iter()
            .all(|entry| !entry.label_key.contains("backups")
                && !entry.label_key.contains("printing")));
        context.current = Some(Id::Tools);
        assert!(!destinations(&context)
            .iter()
            .any(|entry| entry.id == Id::Tools));
    }
}
#[test]
fn held_grants_components_and_preferences_still_gate_actual_destinations() {
    use ApplicationDestinationId as Id;
    let mut context = context(ApplicationNavigationFamily::Commands);
    context.grants.clear();
    context.app_all = false;
    assert_eq!(
        destinations(&context)
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [Id::Tools]
    );
    context.grants.insert("ui.styles".into(), true);
    context.map_component = false;
    assert!(!destinations(&context)
        .iter()
        .any(|entry| entry.id == Id::Styles));
    context.map_component = true;
    context.saved_visibility = r#"{"styles":false}"#.into();
    assert!(!destinations(&context)
        .iter()
        .any(|entry| entry.id == Id::Styles));
    context.family = ApplicationNavigationFamily::Context;
    context.grants.insert("ui.solar".into(), true);
    let solar = destinations(&context)
        .into_iter()
        .find(|entry| entry.id == Id::Solar)
        .unwrap();
    assert_eq!(solar.project.as_deref(), Some("project/a b"));
    context.active_project = None;
    assert!(!destinations(&context)
        .iter()
        .any(|entry| entry.id == Id::Solar));
}
#[test]
fn preferences_drop_retired_keys_and_preserve_authored_visibility() {
    let preferences = shortcut_preferences(
        r#"{"printing":true,"backups":true,"factory":false,"styles":"invalid","new_addition":false}"#,
        None,
    );
    assert_eq!(preferences.entries.len(), 6);
    assert!(
        !preferences
            .entries
            .iter()
            .find(|entry| entry.id == CommandShortcutId::Factory)
            .unwrap()
            .visible
    );
    assert!(
        preferences
            .entries
            .iter()
            .find(|entry| entry.id == CommandShortcutId::Styles)
            .unwrap()
            .visible
    );
    let saved: Value = serde_json::from_str(&preferences.serialized).unwrap();
    assert!(saved.get("printing").is_none() && saved.get("backups").is_none());
    let changed = shortcut_preferences(
        &preferences.serialized,
        Some(CommandShortcutChange {
            id: CommandShortcutId::Styles,
            visible: false,
        }),
    );
    assert!(
        !changed
            .entries
            .iter()
            .find(|entry| entry.id == CommandShortcutId::Factory)
            .unwrap()
            .visible
    );
    assert!(
        !changed
            .entries
            .iter()
            .find(|entry| entry.id == CommandShortcutId::Styles)
            .unwrap()
            .visible
    );
    assert!(shortcut_preferences("malformed", None)
        .entries
        .iter()
        .all(|entry| entry.visible));
}
#[test]
fn governance_tabs_keep_their_parent_destination_and_template_routes_are_not_reclassified() {
    assert_eq!(
        current_destination("/governance"),
        Some(ApplicationDestinationId::Governance)
    );
    assert_eq!(
        current_destination("/projects/template/data/"),
        Some(ApplicationDestinationId::Project)
    );
    assert_eq!(current_destination("/printing"), None);
    assert_eq!(current_destination("/backups"), None);
    assert_eq!(current_destination("/projects/p/data/extra"), None);
}

#[test]
fn ledger_only_reader_retains_one_governance_doorway() {
    let mut context = context(ApplicationNavigationFamily::Commands);
    context.grants.clear();
    let governance = destinations(&context)
        .into_iter()
        .find(|entry| entry.id == ApplicationDestinationId::Governance)
        .unwrap();
    assert_eq!(
        governance.target,
        Some(ApplicationNavigationTarget::Governance)
    );
    assert_eq!(governance.view.as_deref(), Some("backups"));
    context.app_all = false;
    assert!(!destinations(&context)
        .iter()
        .any(|entry| entry.id == ApplicationDestinationId::Governance));
}
