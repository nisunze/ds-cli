//! The page's client state: the one [`ClientState`] ds-web makes with its
//! gateway host, the typed commands it calls, and each memo as a
//! [`StateSignal`] its Svelte stores subscribe to.

use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use js_sys::{Function, Promise};
use reactive_graph::effect::ImmediateEffect;
use reactive_graph::traits::With;
use send_wrapper::SendWrapper;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::{Demand, Effects, OnDemand, Scope, Want};
use crate::gateway_exchange::browser::{attempt, GatewayHost};
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

/// The page's effects: each gateway exchange through its `fetch`.
struct Page(Rc<GatewayHost>);

impl Effects for Page {
    fn exchange<C: Exchange + 'static>(
        &self,
        call: C,
    ) -> impl Future<Output = Result<C::Output, Refusal>> + use<C> {
        let host = Rc::clone(&self.0);
        async move {
            attempt(&host, call)
                .await
                .unwrap_or_else(|failure| Err(host_failure(&failure)))
        }
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

/// The page's client state. ds-web makes one, with its gateway host.
#[wasm_bindgen(js_name = ClientState)]
pub struct PageState(super::ClientState);

#[wasm_bindgen(js_class = ClientState)]
impl PageState {
    #[wasm_bindgen(constructor)]
    pub fn new(gateway: GatewayHost) -> PageState {
        // A second state on the page keeps the executor the first started.
        let _ = any_spawner::Executor::init_wasm_bindgen();
        PageState(super::ClientState::new(Page(Rc::new(gateway))))
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

    /// The 3D provider configuration for the current scope.
    #[wasm_bindgen(js_name = maps3dConfig, unchecked_return_type = "StateSignal<Config3d | null>")]
    pub fn maps3d_config(&self) -> StateSignal {
        StateSignal::of(&self.0.maps.config_3d)
    }

    #[wasm_bindgen(
        js_name = maps3dLoadState,
        unchecked_return_type = "StateSignal<Maps3dLoadState>"
    )]
    pub fn maps3d_load_state(&self) -> StateSignal {
        StateSignal::of(&self.0.maps.load_3d)
    }

    /// Whether Google Photorealistic 3D can be offered.
    #[wasm_bindgen(js_name = google3dAvailable, unchecked_return_type = "StateSignal<boolean>")]
    pub fn google3d_available(&self) -> StateSignal {
        StateSignal::of(&self.0.maps.google_3d)
    }

    /// Command: read the 3D provider configuration again.
    #[wasm_bindgen(js_name = retryMaps3d)]
    pub fn retry_maps3d(&self) {
        self.0.maps.retry_3d();
    }

    /// The 3D load state once the current scope's read has answered.
    #[wasm_bindgen(js_name = maps3dSettled, unchecked_return_type = "Promise<Maps3dLoadState>")]
    pub fn maps3d_settled(&self) -> Promise {
        let settled = self.0.maps.settled_3d();
        wasm_bindgen_futures::future_to_promise(async move { Ok(plain(&settled.await)) })
    }
}
