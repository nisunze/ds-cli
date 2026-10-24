//! The capabilities an account holds, kept on this edge so the next start
//! gates the interface at once and offline. A fresh read replaces the held
//! snapshot unless it looks like a transient collapse to the baseline role
//! for the same person: then the held snapshot stays and is what shows.

use serde_json::{Map, Value};

use super::account::Capabilities;
use super::retained::{self, Edge};
use super::Refusal;
use crate::retained_reads::Kind;

const KIND: Kind = Kind::AccountCapabilities;
/// What an elevated platform role loses when a read collapses it.
const PLATFORM_EDITS: [&str; 7] = [
    "styles.edit",
    "forms.edit",
    "network.template.edit",
    "network.template.propagate",
    "ui.styles",
    "ui.forms",
    "ui.templates",
];
/// What a project role loses when a read drops it.
const PROJECT_EDITS: [&str; 5] = [
    "design.edit",
    "pipeline.run",
    "entries.create",
    "reports.export",
    "project_data.edit",
];

fn key(project: Option<&str>) -> String {
    project.map_or_else(|| "platform".to_owned(), |project| format!("project:{project}"))
}

fn text<'a>(snapshot: &'a Map<String, Value>, section: &str, field: &str) -> &'a str {
    snapshot
        .get(section)
        .and_then(|section| section.get(field))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn roles(snapshot: &Map<String, Value>, section: &str) -> Vec<String> {
    snapshot
        .get(section)
        .and_then(|section| section.get("roles"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_owned)
        .collect()
}

fn granted(snapshot: &Map<String, Value>, capability: &str) -> bool {
    match snapshot.get("capabilities").and_then(|all| all.get(capability)) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
        Some(Value::Null) | None => false,
    }
}

fn granted_count(snapshot: &Map<String, Value>) -> usize {
    snapshot
        .get("capabilities")
        .and_then(Value::as_object)
        .map_or(0, |all| all.keys().filter(|key| granted(snapshot, key)).count())
}

fn loses_any(previous: &Map<String, Value>, incoming: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter()
        .any(|key| granted(previous, key) && !granted(incoming, key))
}

/// Whether a fresh snapshot looks like a transient collapse of the held one:
/// the same person, fewer grants, and either an elevated platform role gone
/// to the baseline with an edit right lost, or the same project's roles gone
/// with a project edit right lost.
pub fn suspicious_downgrade(
    previous: &Map<String, Value>,
    incoming: &Map<String, Value>,
    project: Option<&str>,
) -> bool {
    let email = |snapshot: &Map<String, Value>| text(snapshot, "user", "email").trim().to_lowercase();
    if email(previous) != email(incoming) || granted_count(incoming) >= granted_count(previous) {
        return false;
    }
    let previous_app = roles(previous, "app");
    let incoming_app = roles(incoming, "app");
    let elevated = previous_app.iter().any(|role| role != "standard_user");
    let baseline = incoming_app.is_empty() || incoming_app == ["standard_user"];
    if elevated && baseline && loses_any(previous, incoming, &PLATFORM_EDITS) {
        return true;
    }
    let project_of = |snapshot: &Map<String, Value>| text(snapshot, "active_project", "project_id").to_owned();
    let expected = project.map(str::to_owned).unwrap_or_else(|| {
        Some(project_of(previous))
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| project_of(incoming))
    });
    let same_project = expected == project_of(previous) && expected == project_of(incoming);
    same_project
        && !roles(previous, "active_project").is_empty()
        && roles(incoming, "active_project").is_empty()
        && loses_any(previous, incoming, &PROJECT_EDITS)
}

/// The snapshot this edge holds for the platform or one project, if any.
pub async fn held(edge: &impl Edge, project: Option<&str>) -> Result<Option<Map<String, Value>>, Refusal> {
    let observed = retained::read(edge, KIND, None, &key(project)).await?;
    Ok(observed.held.map(|held| held.value))
}

/// A fresh snapshot from Brain, held for the next start; a transient
/// collapse keeps and answers the held one.
pub async fn refresh(edge: &impl Edge, project: Option<&str>) -> Result<Map<String, Value>, Refusal> {
    let key = key(project);
    let observed = retained::read::<Map<String, Value>>(edge, KIND, None, &key)
        .await
        .ok();
    let fresh = edge
        .fetch(Capabilities {
            project: project.map(str::to_owned),
        })
        .await?;
    if let Some(observed) = observed {
        if let Some(previous) = observed.held.map(|held| held.value) {
            if suspicious_downgrade(&previous, &fresh, project) {
                return Ok(previous);
            }
        }
        let owner = observed.owner.as_deref();
        let _ = retained::write(edge, owner, KIND, None, &key, &fresh, None).await;
    }
    Ok(fresh)
}

/// The typed browser exports Svelte calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod exports {
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    /// The capabilities this edge holds for the platform or one project.
    #[wasm_bindgen(js_name = accountCapabilitiesHeld, unchecked_return_type = "Record<string, unknown> | null")]
    pub async fn held(host: GatewayHost, store: StoreHost, project: Option<String>) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let held = super::held(&edge, project.as_deref()).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// Fresh capabilities from Brain, held for the next start.
    #[wasm_bindgen(js_name = accountCapabilitiesRefresh, unchecked_return_type = "Record<string, unknown>")]
    pub async fn refresh(host: GatewayHost, store: StoreHost, project: Option<String>) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let fresh = super::refresh(&edge, project.as_deref()).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }
}

#[cfg(test)]
#[path = "account_capabilities_tests.rs"]
mod tests;
