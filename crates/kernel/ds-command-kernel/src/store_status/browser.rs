//! The page's and the storage worker's `navigator.storage`. Reached through
//! the global scope so one implementation serves a window and a worker.
use super::{web_status, Persisted, Persistence, StorageApi, StoreStatus};
use js_sys::{Function, Promise, Reflect};
use std::rc::Rc;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;

thread_local! {
    static PERSISTENCE: Rc<Persistence> = Rc::new(Persistence::default());
}

struct Navigator;
impl Navigator {
    fn storage() -> Option<JsValue> {
        let navigator = Reflect::get(&js_sys::global(), &"navigator".into()).ok()?;
        let storage = Reflect::get(&navigator, &"storage".into()).ok()?;
        (!storage.is_undefined() && !storage.is_null()).then_some(storage)
    }
    async fn call(method: &str) -> Option<JsValue> {
        let storage = Self::storage()?;
        let function: Function = Reflect::get(&storage, &method.into())
            .ok()?
            .dyn_into()
            .ok()?;
        let promise: Promise = function.call0(&storage).ok()?.dyn_into().ok()?;
        JsFuture::from(promise).await.ok()
    }
}
impl StorageApi for Navigator {
    async fn persist(&self) -> Option<bool> {
        Self::call("persist").await?.as_bool()
    }
    async fn persisted(&self) -> Option<bool> {
        Self::call("persisted").await?.as_bool()
    }
    async fn estimate(&self) -> Option<(u64, Option<u64>)> {
        let estimate = Self::call("estimate").await?;
        let number = |name: &str| Reflect::get(&estimate, &name.into()).ok()?.as_f64();
        Some((
            number("usage").unwrap_or(0.0) as u64,
            number("quota").map(|q| q as u64),
        ))
    }
}

/// Ask the browser to keep the store, once per worker, and record its answer.
pub async fn request_persistence() -> Persisted {
    let persistence = PERSISTENCE.with(Rc::clone);
    persistence.ensure(&Navigator).await
}

/// `ds.store-status/v1` for the hosted Web.
pub async fn status() -> StoreStatus {
    web_status(&Navigator, PERSISTENCE.with(|held| held.recorded())).await
}

/// `ds.store-status/v1` as JSON. In a window or the storage worker it reads
/// the browser's storage; the Desktop page hands the answer of its native
/// store call and gets the same typed record, checked.
#[wasm_bindgen(js_name = storeStatus)]
pub async fn store_status(native: Option<String>) -> Result<String, JsValue> {
    let status = match native {
        Some(answer) => super::from_native_answer(&answer),
        None => Ok(status().await),
    }
    .map_err(|e| JsValue::from_str(&e))?;
    serde_json::to_string(&status).map_err(|e| JsValue::from_str(&e.to_string()))
}
