//! The installed page decides which UI unit may serve it (owner 22 Oct 2026).
use super::*;
use serde_json::json;

/// The shape wasm-bindgen writes, with the exports a UI Kernel of
/// 21 October carried (sign-in included).
const OLDER_UNIT: &str = "let wasm;\n\
export function firebaseSignIn(api_key, email, password) {\n\
export function firebaseRefresh(api_key, token) {\n\
export function layerState(value) {\n\
export class DesignSession {\n\
export async function warm() {\n\
function private_helper() {}\n\
const local = 1;\n\
export { initSync, __wbg_init as default };\n";

/// A later unit: sign-in moved out (removed), one operation added.
const NEWER_UNIT_WITHOUT_SIGN_IN: &str = "export function layerState(value) {\n\
export function vectorTool(op) {\n\
export class DesignSession {\n\
export async function warm() {\n\
export { initSync, __wbg_init as default };\n";

/// A later unit that only adds.
const ADDITIVE_UNIT: &str = "export function firebaseSignIn(api_key, email, password) {\n\
export function firebaseRefresh(api_key, token) {\n\
export function layerState(value) {\n\
export function vectorTool(op) {\n\
export class DesignSession {\n\
export async function warm() {\n\
export { initSync, __wbg_init as default };\n";

fn exports(glue: &str) -> BTreeSet<String> {
    unit_exports(glue.as_bytes()).unwrap()
}

#[test]
fn a_units_runtime_exports_are_read_from_its_javascript_member() {
    assert_eq!(
        exports(OLDER_UNIT),
        [
            "DesignSession",
            "default",
            "firebaseRefresh",
            "firebaseSignIn",
            "initSync",
            "layerState",
            "warm"
        ]
        .into_iter()
        .map(String::from)
        .collect()
    );
    assert_eq!(
        unit_exports(b"function only_private() {}\n")
            .unwrap_err()
            .code,
        "module_artifact_invalid"
    );
    assert_eq!(
        unit_exports(&[0xff, 0xfe]).unwrap_err().code,
        "module_artifact_invalid"
    );
}

#[test]
fn an_older_page_keeps_its_unit_when_a_newer_unit_drops_an_export_it_calls() {
    // A page installed before pages recorded requirements: the unit it loads
    // today bounds what it may call.
    let installed = exports(OLDER_UNIT);
    let required =
        installed_page_requirement(None, UiComponent::CommandKernel, Some(&installed)).unwrap();
    let refused = admit_for_installed_page(
        UiComponent::CommandKernel,
        &required,
        &exports(NEWER_UNIT_WITHOUT_SIGN_IN),
    )
    .unwrap_err();
    assert_eq!(refused.code, INCOMPATIBLE_WITH_INSTALLED_PAGE);
    assert!(refused.message.contains(
        "lacks 2 export(s) the installed Desktop page calls: firebaseRefresh, firebaseSignIn"
    ));
    assert_eq!(
        missing_exports(&required, &exports(NEWER_UNIT_WITHOUT_SIGN_IN)),
        ["firebaseRefresh", "firebaseSignIn"]
    );
    // A unit that only adds serves it.
    admit_for_installed_page(
        UiComponent::CommandKernel,
        &required,
        &exports(ADDITIVE_UNIT),
    )
    .unwrap();
}

#[test]
fn a_page_that_recorded_its_requirements_admits_a_unit_that_dropped_what_it_no_longer_calls() {
    let declared = UiPageRequirements::admit(&json!({
        "schema": PAGE_REQUIREMENTS_SCHEMA,
        "components": {
            "command-kernel": ["layerState", "vectorTool", "DesignSession", "default"],
            "interactions": ["pick"],
            "future-component": ["anything"]
        },
        "built_at": "2026-10-22T12:00:00Z"
    }))
    .unwrap();
    let installed = exports(OLDER_UNIT);
    let required = installed_page_requirement(
        Some(&declared),
        UiComponent::CommandKernel,
        Some(&installed),
    )
    .unwrap();
    admit_for_installed_page(
        UiComponent::CommandKernel,
        &required,
        &exports(NEWER_UNIT_WITHOUT_SIGN_IN),
    )
    .unwrap();
    // The same newer page on the older unit: the unit lacks vectorTool.
    let refused =
        admit_for_installed_page(UiComponent::CommandKernel, &required, &installed).unwrap_err();
    assert_eq!(refused.code, INCOMPATIBLE_WITH_INSTALLED_PAGE);
    assert!(refused.message.contains("vectorTool"));
    // Nothing installed and nothing declared: nothing to protect.
    assert!(installed_page_requirement(None, UiComponent::Interactions, None).is_none());
    // A component the page declares nothing of requires nothing.
    let none = UiPageRequirements::admit(&json!({
        "schema": "ds.ui-page-requirements/v2", "components": {}
    }))
    .unwrap();
    assert!(none.exports(UiComponent::Interactions).is_empty());
}

#[test]
fn malformed_or_unbounded_page_requirements_refuse_typed() {
    for value in [
        json!({"components": {}}),
        json!({"schema": PAGE_REQUIREMENTS_SCHEMA}),
        json!({"schema": PAGE_REQUIREMENTS_SCHEMA, "components": []}),
        json!({"schema": "ds.desktop-ui-inventory/v1", "components": {}}),
        json!({"schema": PAGE_REQUIREMENTS_SCHEMA, "components": {"command-kernel": ["not a name"]}}),
        json!({"schema": PAGE_REQUIREMENTS_SCHEMA, "components": {"command-kernel": [7]}}),
        json!({"schema": PAGE_REQUIREMENTS_SCHEMA, "components": {"command-kernel":
            (0..=MAX_REQUIRED_EXPORTS).map(|i| format!("op{i}")).collect::<Vec<_>>()}}),
    ] {
        assert_eq!(
            UiPageRequirements::admit(&value).unwrap_err().code,
            "ui_page_requirements_invalid",
            "{value}"
        );
    }
    assert!(valid_export_name("default") && valid_export_name("$x_1"));
    assert!(!valid_export_name("1x") && !valid_export_name("") && !valid_export_name("a-b"));
}

#[test]
fn a_refusal_names_a_bounded_list_of_missing_exports() {
    let required = (0..40)
        .map(|i| format!("op{i:02}"))
        .collect::<BTreeSet<_>>();
    let refused = admit_for_installed_page(UiComponent::Interactions, &required, &BTreeSet::new())
        .unwrap_err();
    assert!(refused.message.contains("lacks 40 export(s)"));
    assert!(refused.message.contains("op31 and 8 more"));
    assert!(!refused.message.contains("op32"));
}
