//! Which UI unit may serve the installed Desktop page.
//!
//! UI units (UI Kernel, Interactions) update independently of the Desktop page
//! that calls them (owner, 22 October 2026). No version, `ui_contract` or digest
//! equality decides whether a unit may serve a page: the exports the installed
//! page calls do. The page build records those exports
//! ([`UiPageRequirements`]); a unit declares its runtime exports in its own
//! verified JavaScript member ([`unit_exports`]). A unit that lacks one is
//! `ui_unit_incompatible_with_installed_page`, naming what is missing: Core keeps
//! the installed unit, and a page refuses to run on it.
//!
//! A page installed before pages recorded their requirements declares nothing.
//! It works with the unit it loads today, so the exports of the installed unit
//! bound what it may call ([`installed_page_requirement`]): additions flow, and
//! an export is dropped only after every installed page declares it no longer
//! needs it.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ModuleReleaseError;
use crate::module_engine::{schema_family, UiComponent};

pub const PAGE_REQUIREMENTS_SCHEMA: &str = "ds.ui-page-requirements/v1";
/// The refusal when a unit lacks an export the installed page calls.
pub const INCOMPATIBLE_WITH_INSTALLED_PAGE: &str = "ui_unit_incompatible_with_installed_page";
/// The most exports one page may require of one component.
pub const MAX_REQUIRED_EXPORTS: usize = 4096;
/// The most exports a unit's JavaScript member may declare.
pub const MAX_UNIT_EXPORTS: usize = 8192;
/// The longest export name either side may name.
pub const MAX_EXPORT_NAME: usize = 128;
/// The largest JavaScript member read for its export declarations.
pub const MAX_GLUE_BYTES: usize = 16 * 1024 * 1024;
/// The most missing exports a refusal message spells out.
const MAX_NAMED_MISSING: usize = 32;

/// What one Desktop page build calls of each UI component. Additive members
/// and components this Kernel does not know are accepted and ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiPageRequirements {
    pub schema: String,
    /// Component name (`command-kernel`, `interactions`) to the exports the
    /// page calls.
    pub components: BTreeMap<String, BTreeSet<String>>,
}

fn invalid(message: impl Into<String>) -> ModuleReleaseError {
    ModuleReleaseError {
        code: "ui_page_requirements_invalid",
        message: message.into(),
    }
}

/// An export name a page may call: a JavaScript identifier or `default`.
pub fn valid_export_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    !name.is_empty()
        && name.len() <= MAX_EXPORT_NAME
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_' || first == b'$')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$')
}

impl UiPageRequirements {
    /// Admit a page's recorded requirements: its document family, bounded
    /// component lists and valid export names. A missing required member or a
    /// wrong type is the typed `ui_page_requirements_invalid`.
    pub fn admit(value: &Value) -> Result<Self, ModuleReleaseError> {
        let requirements: Self = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("malformed page requirements: {error}")))?;
        if schema_family(&requirements.schema) != schema_family(PAGE_REQUIREMENTS_SCHEMA) {
            return Err(invalid("not a UI page requirements document"));
        }
        if requirements.components.len() > 16
            || requirements.components.iter().any(|(component, exports)| {
                component.is_empty()
                    || component.len() > 64
                    || exports.len() > MAX_REQUIRED_EXPORTS
                    || exports.iter().any(|name| !valid_export_name(name))
            })
        {
            return Err(invalid(
                "page requirements exceed their bounds or name an invalid export",
            ));
        }
        Ok(requirements)
    }

    /// The exports this page calls of one component; none when it names none.
    pub fn exports(&self, component: UiComponent) -> BTreeSet<String> {
        self.components
            .get(component.as_str())
            .cloned()
            .unwrap_or_default()
    }
}

/// The runtime exports a UI unit's verified JavaScript member declares
/// (wasm-bindgen output: `export function`, `export class`, `export const`
/// and `export { a, b as c }` lists).
pub fn unit_exports(javascript: &[u8]) -> Result<BTreeSet<String>, ModuleReleaseError> {
    let artifact = |message: &str| ModuleReleaseError {
        code: "module_artifact_invalid",
        message: message.into(),
    };
    if javascript.len() > MAX_GLUE_BYTES {
        return Err(artifact(
            "the UI unit's JavaScript member exceeds its export bound",
        ));
    }
    let text = std::str::from_utf8(javascript)
        .map_err(|_| artifact("the UI unit's JavaScript member is not UTF-8"))?;
    let mut exports = BTreeSet::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("export ") else {
            continue;
        };
        let rest = rest.trim_start();
        if let Some(list) = rest.strip_prefix('{') {
            let list = list.split('}').next().unwrap_or_default();
            for item in list.split(',') {
                let name = match item.split_once(" as ") {
                    Some((_, alias)) => alias.trim(),
                    None => item.trim(),
                };
                if !name.is_empty() {
                    exports.insert(name.to_owned());
                }
            }
            continue;
        }
        let rest = rest.strip_prefix("async ").unwrap_or(rest);
        let declared = [
            "function* ",
            "function ",
            "class ",
            "const ",
            "let ",
            "var ",
        ]
        .iter()
        .find_map(|keyword| rest.strip_prefix(keyword));
        if let Some(declared) = declared {
            let name: String = declared
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !name.is_empty() {
                exports.insert(name);
            }
        } else if rest.starts_with("default ") {
            exports.insert("default".to_owned());
        }
    }
    if exports.is_empty() || exports.len() > MAX_UNIT_EXPORTS {
        return Err(artifact(
            "the UI unit's JavaScript member declares no bounded export list",
        ));
    }
    Ok(exports)
}

/// What the installed page requires of one component: its recorded
/// requirements when it recorded them; otherwise, for a page installed before
/// pages recorded requirements, the exports of the unit it loads today. `None`
/// when there is neither (nothing installed calls this component yet).
pub fn installed_page_requirement(
    declared: Option<&UiPageRequirements>,
    component: UiComponent,
    installed_unit_exports: Option<&BTreeSet<String>>,
) -> Option<BTreeSet<String>> {
    match declared {
        Some(requirements) => Some(requirements.exports(component)),
        None => installed_unit_exports.cloned(),
    }
}

/// Does this unit provide every export the installed page calls? Additional
/// exports never refuse a unit; a missing one is the typed
/// `ui_unit_incompatible_with_installed_page` naming it.
pub fn admit_for_installed_page(
    component: UiComponent,
    required: &BTreeSet<String>,
    provided: &BTreeSet<String>,
) -> Result<(), ModuleReleaseError> {
    let missing = missing_exports(required, provided);
    if missing.is_empty() {
        return Ok(());
    }
    let named = missing
        .iter()
        .take(MAX_NAMED_MISSING)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let more = missing.len().saturating_sub(MAX_NAMED_MISSING);
    Err(ModuleReleaseError {
        code: INCOMPATIBLE_WITH_INSTALLED_PAGE,
        message: format!(
            "the {} UI unit lacks {} export(s) the installed Desktop page calls: {named}{}; \
             the installed unit is kept. Update the Desktop, or keep this unit until a release \
             provides them",
            component.as_str(),
            missing.len(),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ),
    })
}

/// The exports the page calls that the unit lacks, in name order.
pub fn missing_exports(required: &BTreeSet<String>, provided: &BTreeSet<String>) -> Vec<String> {
    required.difference(provided).cloned().collect()
}

#[cfg(test)]
#[path = "ui_page_tests.rs"]
mod tests;
