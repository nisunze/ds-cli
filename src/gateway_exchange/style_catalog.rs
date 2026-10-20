//! A project's style catalogue (the Style Center's metadata), held per
//! account and project so the next open reads no Brain. A publication or a
//! push invalidates it without a read; the next ask reads Brain again. A layer
//! document read refreshes the held catalogue from the metadata it carries.
//! An answer read while the catalogue was invalidated, or for another account,
//! is never held, and a catalogue read that raced one is refused as
//! superseded.

use serde_json::{Map, Value};

use super::layers::LayerAction;
use super::retained::{self, Edge};
use super::Refusal;
use crate::layer_document::{self, Action};
use crate::retained_reads::Kind;

pub const SUPERSEDED: &str = "style_catalog_superseded";
const KIND: Kind = Kind::StyleCatalog;
/// The metadata a layer document shares with the catalogue.
const METADATA: [&str; 13] = [
    "project_id",
    "resolved_at",
    "styles",
    "style_editors",
    "style_properties",
    "style_presets",
    "style_resolution",
    "label_editor",
    "fill_pattern",
    "platform_config",
    "symbol_catalog",
    "design_layer_catalog",
    "required_images",
];

fn superseded() -> Refusal {
    Refusal::new(
        SUPERSEDED,
        "Style catalogue read was superseded; retry in the current account.",
    )
}

fn call(project: &str, action: Action) -> LayerAction {
    LayerAction {
        action,
        project: project.to_owned(),
        orders: Vec::new(),
    }
}

/// The project's style catalogue: the held one while it is current, else
/// Brain's, held for the next read. `refresh` invalidates it first.
pub async fn style_catalog(
    edge: &impl Edge,
    project: &str,
    refresh: bool,
) -> Result<Value, Refusal> {
    if refresh {
        invalidate(edge, Some(project)).await;
    }
    let observed = retained::read::<Value>(edge, KIND, Some(project), "").await.ok();
    if let Some(held) = observed.as_ref().and_then(|observed| observed.held.as_ref()) {
        if !held.invalidated && held.value.get("project_id").and_then(Value::as_str) == Some(project) {
            return Ok(held.value.clone());
        }
    }
    let fresh = edge.fetch(call(project, Action::GetStyleCatalog)).await?;
    if let Some(observed) = observed {
        let owner = observed.owner.as_deref();
        let generation = Some(observed.generation);
        retained::write(edge, owner, KIND, Some(project), "", &fresh, generation)
            .await
            .map_err(|_| superseded())?;
    }
    Ok(fresh)
}

/// The project's layer document from Brain. Its metadata becomes the held
/// catalogue when it admits as one and nothing invalidated it meanwhile.
pub async fn layer_document(
    edge: &impl Edge,
    project: &str,
    refresh: bool,
) -> Result<Value, Refusal> {
    if refresh {
        invalidate(edge, Some(project)).await;
    }
    let observed = retained::read::<Value>(edge, KIND, Some(project), "").await.ok();
    let action = if refresh { Action::Refresh } else { Action::GetConfig };
    let document = edge.fetch(call(project, action)).await?;
    if let Some(observed) = observed {
        let mut metadata: Map<String, Value> = METADATA
            .iter()
            .filter_map(|key| Some(((*key).to_owned(), document.get(*key)?.clone())))
            .collect();
        let warnings = document.get("warnings").cloned().unwrap_or(Value::Array(Vec::new()));
        metadata.insert("warnings".into(), warnings);
        let answer = serde_json::json!({"success": true, "data": metadata});
        if let Ok(catalogue) = layer_document::admit_style_catalog(project, &answer) {
            let owner = observed.owner.as_deref();
            let generation = Some(observed.generation);
            let _ =
                retained::write(edge, owner, KIND, Some(project), "", &catalogue, generation).await;
        }
    }
    Ok(document)
}

/// Whether a map's styles are the held, current catalogue's: the same
/// resolution revision when both name one, else the same styles. No read.
pub async fn is_current(edge: &impl Edge, project: &str, state: &Value) -> bool {
    let Ok(observed) = retained::read::<Value>(edge, KIND, Some(project), "").await else {
        return false;
    };
    let Some(held) = observed.held.filter(|held| !held.invalidated) else {
        return false;
    };
    let revision_of = |value: &Value| {
        value
            .pointer("/style_resolution/revision_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match revision_of(state) {
        Some(revision) => revision_of(&held.value).as_deref() == Some(revision.as_str()),
        None => state.get("styles") == held.value.get("styles"),
    }
}

/// Invalidate one project's catalogue, or every project's when none is named
/// (a shared style can reach any project). Best effort: a store that cannot
/// mark it keeps nothing newer than Brain.
pub async fn invalidate(edge: &impl Edge, project: Option<&str>) {
    let _ = retained::invalidate(edge, None, KIND, project, None).await;
}

/// The typed browser exports Svelte calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    /// The project's style catalogue: held while current, else Brain's.
    #[wasm_bindgen(js_name = styleCatalogRead, unchecked_return_type = "Record<string, unknown>")]
    pub async fn style_catalog(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        refresh: bool,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let catalogue = super::style_catalog(&edge, &project, refresh).await;
        typed(&catalogue.map_err(|refusal| rejection(&refusal))?)
    }

    /// The project's layer document, its metadata held as the catalogue.
    #[wasm_bindgen(js_name = styleCatalogLayerDocument, unchecked_return_type = "Record<string, unknown>")]
    pub async fn layer_document(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        refresh: bool,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let document = super::layer_document(&edge, &project, refresh).await;
        typed(&document.map_err(|refusal| rejection(&refusal))?)
    }

    /// Whether a map's styles are the held, current catalogue's.
    #[wasm_bindgen(js_name = styleCatalogCurrent)]
    pub async fn is_current(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] state: JsValue,
    ) -> Result<bool, JsValue> {
        let state = tsify::serde_wasm_bindgen::from_value(state)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        Ok(super::is_current(&BrowserEdge::new(host, store), &project, &state).await)
    }

    /// Invalidate one project's catalogue, or every project's.
    #[wasm_bindgen(js_name = styleCatalogInvalidate)]
    pub async fn invalidate(host: GatewayHost, store: StoreHost, project: Option<String>) {
        super::invalidate(&BrowserEdge::new(host, store), project.as_deref()).await;
    }
}

#[cfg(test)]
#[path = "style_catalog_tests.rs"]
mod tests;
