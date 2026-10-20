//! The page's client state: the one [`ClientState`] ds-web makes with its
//! gateway and store hosts, the typed commands it calls, and each memo as a
//! [`StateSignal`] its Svelte stores subscribe to.

use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use js_sys::Function;
use reactive_graph::effect::ImmediateEffect;
use reactive_graph::traits::With;
use send_wrapper::SendWrapper;
use serde::Serialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;

use super::{Demand, Effects, OnDemand, Scope, Want};
use crate::durable_store::port::{PageStore, StoreHost, StorePort};
use crate::durable_store::Request;
use crate::gateway_exchange::browser::{attempt, rejection, typed, GatewayHost};
use crate::gateway_exchange::map_defaults::MapDefaults;
use crate::gateway_exchange::retained::store_refusal;
use crate::gateway_exchange::{Exchange, Refusal};

/// The page could not perform a request at all (its credentials or facts
/// failed), as distinct from a refusal of the request.
pub const HOST_FAILED: &str = "client_state_host_failed";

#[wasm_bindgen(typescript_custom_section)]
const STATE_SIGNAL: &'static str = r#"
/** A Rust signal. `$lib/runtime/rust-state` reads it as a Svelte store. */
export interface StateSignal<T> {
    /** Calls `callback` at once with the current value, then after every change, until disposed. Its read is wanted meanwhile. */
    subscribe(callback: (value: T) => void): StateSubscription;
}

/** One subscription to a StateSignal. */
export interface StateSubscription {
    /** Stops the callbacks and frees the subscription; call it once. */
    dispose(): void;
}
"#;

/// The page's effects: each gateway exchange through its `fetch`, each
/// store call through its StoreHost.
struct Page {
    gateway: Rc<GatewayHost>,
    store: Rc<PageStore>,
}

impl Effects for Page {
    fn exchange<C: Exchange + 'static>(
        &self,
        call: C,
    ) -> impl Future<Output = Result<C::Output, Refusal>> + use<C> {
        let host = Rc::clone(&self.gateway);
        async move {
            attempt(&host, call)
                .await
                .unwrap_or_else(|failure| Err(host_failure(&failure)))
        }
    }

    fn store(&self, request: Request) -> impl Future<Output = Result<Value, Refusal>> + use<> {
        let store = Rc::clone(&self.store);
        async move {
            store
                .call(request, None, None)
                .await
                .map(|reply| reply.answer)
                .map_err(store_refusal)
        }
    }

    fn now_ms(&self) -> u64 {
        js_sys::Date::now().max(0.0) as u64
    }
}

fn host_failure(failure: &JsValue) -> Refusal {
    let message = failure
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .or_else(|| failure.as_string())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| "The page could not make the request.".into());
    Refusal::new(HOST_FAILED, message)
}

/// A value as the plain object its generated interface describes; `None`
/// is `null`.
fn plain<T: Serialize>(value: &T) -> JsValue {
    let serializer = tsify::serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_missing_as_null(true);
    value.serialize(&serializer).unwrap_or(JsValue::UNDEFINED)
}

/// One memo as the page reads it; each subscription wants its read.
#[wasm_bindgen(skip_typescript)]
pub struct StateSignal {
    read: Arc<dyn Fn() -> JsValue + Send + Sync>,
    demand: Demand,
}

impl StateSignal {
    fn of<T>(state: &OnDemand<T>) -> Self
    where
        T: Serialize + PartialEq + Send + Sync + 'static,
    {
        let value = state.value.clone();
        Self {
            read: Arc::new(move || value.with(plain)),
            demand: state.demand.clone(),
        }
    }
}

#[wasm_bindgen]
impl StateSignal {
    /// Calls `callback` at once with the current value, then synchronously
    /// after every change, until the subscription is disposed.
    pub fn subscribe(&self, callback: Function) -> StateSubscription {
        let want = self.demand.want();
        let read = Arc::clone(&self.read);
        let callback = SendWrapper::new(callback);
        let effect = ImmediateEffect::new(move || {
            // The binding reports a subscriber's own failure outside this effect.
            let _ = callback.call1(&JsValue::UNDEFINED, &read());
        });
        StateSubscription {
            _effect: effect,
            _want: want,
        }
    }
}

/// One subscription: its effect calls back while its read stays wanted.
#[wasm_bindgen(skip_typescript)]
pub struct StateSubscription {
    _effect: ImmediateEffect,
    _want: Want,
}

#[wasm_bindgen]
impl StateSubscription {
    pub fn dispose(self) {}
}

/// The page's client state. ds-web makes one, with its gateway and store
/// hosts.
#[wasm_bindgen(js_name = ClientState)]
pub struct PageState(super::ClientState<Page>);

#[wasm_bindgen(js_class = ClientState)]
impl PageState {
    #[wasm_bindgen(constructor)]
    pub fn new(gateway: GatewayHost, store: StoreHost) -> PageState {
        // A second state on the page keeps the executor the first started.
        let _ = any_spawner::Executor::init_wasm_bindgen();
        PageState(super::ClientState::new(Page {
            gateway: Rc::new(gateway),
            store: Rc::new(PageStore::new(store)),
        }))
    }

    /// Command: who is signed in and which project is active.
    #[wasm_bindgen(js_name = setScope)]
    pub fn set_scope(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Scope")] scope: JsValue,
    ) -> Result<(), JsValue> {
        let scope: Scope = tsify::serde_wasm_bindgen::from_value(scope)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        self.0.set_scope(scope);
        Ok(())
    }

    /// The dark roadmap basemap the active project offers.
    #[wasm_bindgen(js_name = darkBasemap, unchecked_return_type = "StateSignal<Basemap | null>")]
    pub fn dark_basemap(&self) -> StateSignal {
        StateSignal::of(&self.0.maps.dark_basemap)
    }

    /// The platform's map defaults.
    #[wasm_bindgen(js_name = mapDefaults, unchecked_return_type = "StateSignal<MapDefaults>")]
    pub fn map_defaults(&self) -> StateSignal {
        StateSignal::of(&self.0.cartography.defaults)
    }

    /// Command: publish map defaults for every client; answers what Brain
    /// kept, or rejects with the refusal.
    #[wasm_bindgen(js_name = publishMapDefaults, unchecked_return_type = "Promise<MapDefaults>")]
    pub fn publish_map_defaults(
        &self,
        #[wasm_bindgen(unchecked_param_type = "MapDefaults")] defaults: JsValue,
    ) -> Result<js_sys::Promise, JsValue> {
        let defaults: MapDefaults = tsify::serde_wasm_bindgen::from_value(defaults)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let publishing = self.0.publish_map_defaults(defaults);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let published = publishing.await.map_err(|refusal| rejection(&refusal))?;
            typed(&published)
        }))
    }

    /// The relief control.
    #[wasm_bindgen(unchecked_return_type = "StateSignal<Relief>")]
    pub fn relief(&self) -> StateSignal {
        StateSignal::of(&self.0.cartography.relief)
    }

    /// Command: show or hide relief for this session.
    #[wasm_bindgen(js_name = toggleRelief)]
    pub fn toggle_relief(&self) {
        self.0.toggle_relief();
    }

    /// Command: step relief to its next intensity and show it.
    #[wasm_bindgen(js_name = cycleRelief)]
    pub fn cycle_relief(&self) {
        self.0.cycle_relief();
    }
}
