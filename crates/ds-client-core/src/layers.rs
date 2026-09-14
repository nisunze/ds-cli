//! Governed layer metadata and ordering. The server owns assembly and scope;
//! the request bodies and every admission rule are the kernel's
//! (`ds_command_kernel::layer_document`), shared with the browser.
use crate::{ClientError, ErrorKind, TransportResponse};
use ds_command_kernel::layer_document::{self as wire, Action};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LAYERS_RESPONSE_LIMIT: usize = wire::DOCUMENT_RESPONSE_LIMIT;
pub const STYLE_CATALOG_RESPONSE_LIMIT: usize = wire::STYLE_CATALOG_RESPONSE_LIMIT;
#[derive(Clone)]
pub struct LayerSnapshot {
    document: Value,
}
impl LayerSnapshot {
    /// The bounded backend-authored map/style document, never browser state.
    pub fn document(&self) -> &Value {
        &self.document
    }
    pub fn project_id(&self) -> &str {
        self.document["project_id"]
            .as_str()
            .expect("validated project")
    }
    pub fn canonical_ids(&self) -> std::collections::BTreeSet<&str> {
        self.document["layers"]
            .as_array()
            .expect("validated layers")
            .iter()
            .filter_map(|layer| layer["metadata"]["config_layer_id"].as_str())
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct StyleSnapshot {
    document: Value,
    warnings: Vec<String>,
}
impl StyleSnapshot {
    /// The bounded backend-authored Style Center document, with no renderer
    /// sources, layers, sidebar, viewport, or basemap state.
    pub fn document(&self) -> &Value {
        &self.document
    }
    pub fn project_id(&self) -> &str {
        self.document["project_id"]
            .as_str()
            .expect("validated project")
    }
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

/// One canonical layer's requested order; the kernel owns its bounds.
pub type LayerOrder = wire::Order;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerOrderReceipt {
    pub project_id: String,
    pub reordered: usize,
}

pub(crate) enum LayerOperation<'a> {
    Read { refresh: bool },
    StyleCatalogRead,
    Reorder(&'a [LayerOrder]),
}
impl LayerOperation<'_> {
    const fn action(&self) -> Action {
        match self {
            Self::Read { refresh: true } => Action::Refresh,
            Self::Read { refresh: false } => Action::GetConfig,
            Self::StyleCatalogRead => Action::GetStyleCatalog,
            Self::Reorder(_) => Action::Reorder,
        }
    }
    pub(crate) const fn response_limit(&self) -> usize {
        self.action().response_limit()
    }

    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
        let orders: &[LayerOrder] = match self {
            Self::Reorder(orders) => orders,
            _ => &[],
        };
        let body = wire::request_body(self.action(), project, orders, "web")
            .expect("closed layer request: project and orders were validated");
        serde_json::to_vec(&body).expect("closed layer request")
    }
}

pub(crate) fn validate_orders(orders: &[LayerOrder]) -> Result<(), ClientError> {
    wire::validate_orders(orders)
        .map_err(|_| ClientError::new(ErrorKind::InvalidInput, "invalid canonical layer order"))
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "invalid governed layer response",
    )
}
/// The kernel's refusal, carried as the route's bounded refusal so callers
/// read the same words `ds` and the browser show.
fn refused(refusal: wire::Refusal) -> ClientError {
    ClientError::with_service_refusal(
        ErrorKind::UnreadableResponse,
        "invalid governed layer response",
        crate::ServiceRefusal::new(0, Some(refusal.code), Some(&refusal.message)),
    )
}
fn wire_value(response: TransportResponse, limit: usize) -> Result<Value, ClientError> {
    if response.body.is_empty() || response.body.len() > limit {
        return Err(bad());
    }
    serde_json::from_slice(&response.body).map_err(|_| bad())
}
pub(crate) fn data(response: TransportResponse) -> Result<Value, ClientError> {
    let wire = wire_value(response, LAYERS_RESPONSE_LIMIT)?;
    if wire["success"] != true || !wire["data"].is_object() {
        return Err(bad());
    }
    Ok(wire["data"].clone())
}
pub(crate) fn decode_style_snapshot(
    response: TransportResponse,
    project: &str,
) -> Result<StyleSnapshot, ClientError> {
    let wire = wire_value(response, STYLE_CATALOG_RESPONSE_LIMIT)?;
    let document = wire::admit_style_catalog(project, &wire).map_err(refused)?;
    let warnings = document["warnings"]
        .as_array()
        .expect("admitted catalogue carries warnings")
        .iter()
        .map(|warning| warning.as_str().expect("admitted warning text").to_owned())
        .collect();
    Ok(StyleSnapshot { document, warnings })
}
pub(crate) fn decode_snapshot(
    response: TransportResponse,
    project: &str,
) -> Result<LayerSnapshot, ClientError> {
    let wire = wire_value(response, LAYERS_RESPONSE_LIMIT)?;
    let document = wire::admit_document(project, &wire).map_err(refused)?;
    Ok(LayerSnapshot { document })
}
pub(crate) fn decode_order(
    response: TransportResponse,
    project: &str,
    count: usize,
) -> Result<LayerOrderReceipt, ClientError> {
    let wire = wire_value(response, LAYERS_RESPONSE_LIMIT)?;
    let receipt = wire::admit_order_receipt(project, count, &wire).map_err(refused)?;
    serde_json::from_value(receipt).map_err(|_| bad())
}
