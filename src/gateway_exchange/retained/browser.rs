//! The page's edge: gateway exchanges through the browser executor, and the
//! Kernel store through the page's StoreHost (the storage worker on the Web,
//! `core.store` on the Desktop).

use js_sys::Reflect;
use serde_json::Value;
use wasm_bindgen::{JsCast, JsValue};

use super::{store_refusal, Edge};
use crate::durable_store::port::{PageStore, StoreHost, StorePort};
use crate::durable_store::Call;
use crate::gateway_exchange::browser::{attempt, facts, GatewayHost};
use crate::gateway_exchange::{Exchange, Refusal};

/// A host failure that carried no code of its own.
pub const HOST_FAILED: &str = "gateway_host_failed";

pub(crate) struct BrowserEdge {
    host: GatewayHost,
    store: PageStore,
}

impl BrowserEdge {
    pub(crate) fn new(host: GatewayHost, store: StoreHost) -> Self {
        Self {
            host,
            store: PageStore::new(store),
        }
    }
}

/// A host that threw: its own `code` when it named one.
fn host_refusal(error: JsValue) -> Refusal {
    let text = |name: &str| {
        Reflect::get(&error, &name.into())
            .ok()
            .and_then(|value| value.as_string())
    };
    let message = error
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "The page could not answer the gateway call.".into());
    let code = text("code").unwrap_or_else(|| HOST_FAILED.into());
    Refusal::new(&code, message)
}

impl Edge for BrowserEdge {
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
        attempt(&self.host, call)
            .await
            .unwrap_or_else(|error| Err(host_refusal(error)))
    }

    async fn store(&self, call: Call) -> Result<Value, Refusal> {
        self.store
            .call(call.request, call.expect_owner, None)
            .await
            .map(|reply| reply.answer)
            .map_err(store_refusal)
    }

    fn now_ms(&self) -> u64 {
        js_sys::Date::now().max(0.0) as u64
    }

    fn online(&self) -> bool {
        facts(&self.host).is_ok_and(|facts| facts.link_up && !facts.offline_mode)
    }
}
