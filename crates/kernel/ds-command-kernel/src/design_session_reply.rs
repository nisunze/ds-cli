//! Admission of the `SessionHandle` JSON replies (`ds_design_session::reply`).
//!
//! A host passes the reply text it received to the decoder of its family. The
//! owning type admits or refuses it as `design_session_response_invalid`, and
//! the admitted text is returned as a value of the generated declaration, so
//! the browser parses no reply and declares no reply shape of its own.
use ds_design_session::dataframe::{BulkCellEditResult, CellEditDiff, LayerRow};
use ds_design_session::mutation::SessionDiff;
use ds_design_session::projection::ProjectionMetadata;
use ds_design_session::reply::{self, ReplyError, ReplyFamily};
use ds_design_session::session::{HoverResult, ScopeStatus, SelectionState, SnapResult};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

fn refused(error: ReplyError) -> JsError {
    JsError::new(&error.to_string())
}

/// The admitted text, exactly as written, as a JavaScript value.
fn admitted<T: Tsify>(family: ReplyFamily, reply: &str) -> Result<Ts<T>, JsError> {
    js_sys::JSON::parse(reply)
        .map(Ts::new_unchecked)
        .map_err(|_| refused(ReplyError::new(family, "is not JSON text")))
}

#[wasm_bindgen(js_name = decodeDesignSessionDiff)]
pub fn decode_diff(reply: &str) -> Result<Ts<SessionDiff>, JsError> {
    reply::decode_diff(reply).map_err(refused)?;
    admitted(ReplyFamily::Diff, reply)
}

#[wasm_bindgen(js_name = decodeDesignSessionScopeStatus)]
pub fn decode_scope_status(reply: &str) -> Result<Ts<ScopeStatus>, JsError> {
    reply::decode_scope_status(reply).map_err(refused)?;
    admitted(ReplyFamily::ScopeStatus, reply)
}

#[wasm_bindgen(js_name = decodeDesignSessionProjection)]
pub fn decode_projection(reply: &str) -> Result<Ts<ProjectionMetadata>, JsError> {
    reply::decode_projection(reply).map_err(refused)?;
    admitted(ReplyFamily::Projection, reply)
}

#[wasm_bindgen(js_name = decodeDesignSessionSnap)]
pub fn decode_snap(reply: &str) -> Result<Ts<SnapResult>, JsError> {
    reply::decode_snap(reply).map_err(refused)?;
    admitted(ReplyFamily::Snap, reply)
}

#[wasm_bindgen(js_name = decodeDesignSessionHover)]
pub fn decode_hover(reply: &str) -> Result<Ts<HoverResult>, JsError> {
    reply::decode_hover(reply).map_err(refused)?;
    admitted(ReplyFamily::Hover, reply)
}

#[wasm_bindgen(js_name = decodeDesignSessionSelection)]
pub fn decode_selection(reply: &str) -> Result<Ts<SelectionState>, JsError> {
    reply::decode_selection(reply).map_err(refused)?;
    admitted(ReplyFamily::Selection, reply)
}

/// `undefined` when the Kernel answered `null` (no such layer or feature).
#[wasm_bindgen(js_name = decodeDesignSessionRow)]
pub fn decode_row(reply: &str) -> Result<Option<Ts<LayerRow>>, JsError> {
    match reply::decode_row(reply).map_err(refused)? {
        Some(_) => admitted(ReplyFamily::Row, reply).map(Some),
        None => Ok(None),
    }
}

/// `undefined` when the Kernel answered `null` (no such feature).
#[wasm_bindgen(js_name = decodeDesignSessionCellEdit)]
pub fn decode_cell_edit(reply: &str) -> Result<Option<Ts<CellEditDiff>>, JsError> {
    match reply::decode_cell_edit(reply).map_err(refused)? {
        Some(_) => admitted(ReplyFamily::CellEdit, reply).map(Some),
        None => Ok(None),
    }
}

#[wasm_bindgen(js_name = decodeDesignSessionCellEdits)]
pub fn decode_cell_edits(reply: &str) -> Result<Ts<BulkCellEditResult>, JsError> {
    reply::decode_cell_edits(reply).map_err(refused)?;
    admitted(ReplyFamily::CellEdits, reply)
}
