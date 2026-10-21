//! A process-local in-memory host of the durable stores: the same
//! [`super::execute`] and the same answer as the OPFS host, for a host that
//! keeps nothing between runs (the Node test host of ds-web, which proves its
//! adapters against the Kernel's own store rules instead of re-implementing
//! them). sqlite-wasm-rs's default VFS is in memory: nothing survives the
//! process and no effect file is written.
use super::opfs::DurableStoreReply;
use super::{execute, Call, Identity, INVALID};
use rusqlite::Connection;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static MEMORY: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

fn failure(error: String) -> JsValue {
    JsValue::from_str(&serde_json::json!({ "error": error, "reset": false }).to_string())
}

/// One call in one transaction of this process's in-memory store; the same
/// arguments, answer and `{error, reset}` refusal as `durableStoreTransaction`.
#[wasm_bindgen(js_name = durableStoreMemoryTransaction)]
pub fn transaction(
    call: String,
    identity: Option<String>,
    bytes: Option<Vec<u8>>,
) -> Result<DurableStoreReply, JsValue> {
    let call: Call = serde_json::from_str(&call).map_err(|e| failure(format!("{INVALID}: {e}")))?;
    let identity = identity
        .map(|text| serde_json::from_str::<Identity>(&text))
        .transpose()
        .map_err(|e| failure(format!("{INVALID}: {e}")))?;
    MEMORY.with(|held| {
        let mut held = held.borrow_mut();
        if held.is_none() {
            *held = Some(Connection::open_in_memory().map_err(|e| failure(e.to_string()))?);
        }
        let connection = held.as_mut().expect("the in-memory store is open");
        let answer = execute(connection, identity.as_ref(), call, bytes).map_err(failure)?;
        let text = serde_json::to_string(&answer).map_err(|e| failure(e.to_string()))?;
        Ok(DurableStoreReply::new(text, answer.bytes))
    })
}

/// Forget everything this process's in-memory store holds.
#[wasm_bindgen(js_name = durableStoreMemoryReset)]
pub fn reset() {
    MEMORY.with(|held| drop(held.borrow_mut().take()));
}
