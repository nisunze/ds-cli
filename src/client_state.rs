//! Client application state as Rust signals: Leptos's reactive core
//! (`reactive_graph`), never its UI (architecture sentence 4).
//!
//! The Kernel holds the application state a client shows as signals and
//! memos, fed by its own decisions: gateway answers through
//! [`crate::gateway_exchange`] and what this edge holds in the Kernel's
//! durable store. The graph has the same semantics in the browser's WASM and
//! natively; only the [`Effects`] a host performs differ. Svelte subscribes to
//! each memo through one thin binding (`ds-web/src/lib/runtime/rust-state.ts`),
//! sends intents through typed commands and keeps only presentation state. A
//! memo notifies only when its value changes. The README's "Client state on
//! Rust signals" section is the pattern for moving a store here.
//!
//! Async work runs on any_spawner's executor. The browser adapter starts
//! wasm-bindgen-futures; a native host initializes its own runtime first.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use reactive_graph::computed::{ArcAsyncDerived, ArcMemo};
use reactive_graph::signal::ArcRwSignal;
use reactive_graph::traits::{Get, GetUntracked, Update, With};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::durable_store::Request;
use crate::gateway_exchange::{Exchange, Refusal};

pub mod capture;
pub mod cartography;
pub mod maps;

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser;

/// Who is signed in and which project is active, as the host reports them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Scope {
    #[serde(default)]
    pub principal: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
}

impl Scope {
    /// Trimmed; an empty principal or project names none.
    pub fn new(principal: Option<&str>, project: Option<&str>) -> Self {
        let named = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        Self {
            principal: named(principal),
            project: named(project),
        }
    }

    /// A signed-in principal with an active project.
    pub fn is_complete(&self) -> bool {
        self.principal.is_some() && self.project.is_some()
    }

    /// What a read of the signed-in account is for: the principal alone.
    fn signed_in(&self) -> Option<Scope> {
        self.principal.as_ref().map(|_| self.account())
    }

    /// What this edge holds for whoever is here, signed in or not.
    fn account(&self) -> Scope {
        Scope {
            principal: self.principal.clone(),
            project: None,
        }
    }
}

/// The effects a host performs for the client state.
pub trait Effects: 'static {
    /// Run one gateway exchange to its typed outcome.
    fn exchange<C: Exchange + 'static>(
        &self,
        call: C,
    ) -> impl Future<Output = Result<C::Output, Refusal>> + use<Self, C>;
    /// One call of the Kernel's durable store under the identity the host
    /// binds: its answer, or the store's refusal.
    fn store(&self, request: Request) -> impl Future<Output = Result<Value, Refusal>> + use<Self>;
    /// The host's clock.
    fn now_ms(&self) -> u64;
    /// A fresh random token: 32 lowercase hex digits.
    fn nonce(&self) -> String;
}

/// One client's state: its scope and each domain's signals.
pub struct ClientState<E: Effects> {
    effects: Rc<E>,
    scope: ArcRwSignal<Scope>,
    pub maps: maps::Maps,
    pub cartography: cartography::Cartography,
    pub capture: capture::Capture,
}

impl<E: Effects> ClientState<E> {
    pub fn new(effects: E) -> Self {
        let effects = Rc::new(effects);
        let scope = ArcRwSignal::new(Scope::default());
        Self {
            maps: maps::Maps::new(&scope, &effects),
            cartography: cartography::Cartography::new(&scope, &effects),
            capture: capture::Capture::new(&scope),
            effects,
            scope,
        }
    }

    /// Command: the host reports who is signed in and which project is
    /// active. An unchanged scope notifies nothing.
    pub fn set_scope(&self, scope: Scope) {
        let scope = Scope::new(scope.principal.as_deref(), scope.project.as_deref());
        self.scope.maybe_update(|current| {
            let changed = *current != scope;
            if changed {
                *current = scope;
            }
            changed
        });
    }

    pub fn scope(&self) -> Scope {
        self.scope.get_untracked()
    }

    /// Command: publish map defaults for every client.
    pub fn publish_map_defaults(
        &self,
        defaults: crate::gateway_exchange::map_defaults::MapDefaults,
    ) -> impl Future<Output = Result<crate::gateway_exchange::map_defaults::MapDefaults, Refusal>> + use<E>
    {
        self.cartography.publish(&self.effects, defaults)
    }

    /// Command: show or hide relief for this session.
    pub fn toggle_relief(&self) {
        self.cartography.toggle_relief();
    }

    /// Command: step relief to its next intensity and show it.
    pub fn cycle_relief(&self) {
        self.cartography.cycle_relief(&self.effects);
    }

    /// Command: open a survey form for the signed-in account.
    pub fn capture_open(
        &self,
        context: crate::survey_capture::Context,
    ) -> impl Future<Output = Result<(), Refusal>> + use<E> {
        self.capture.open(&self.effects, context)
    }

    /// Command: one field's value in the form being filled.
    pub fn capture_set(&self, key: &str, value: Value) {
        self.capture.set_value(&self.effects, key, value);
    }

    /// Command: the device's location (`gps`) or a point picked on the map.
    pub fn capture_locate(&self, location: Value, source: &str) {
        self.capture.locate(&self.effects, location, source);
    }

    /// Command: the geometry a drawing or an edge chain gives the entry.
    pub fn capture_geometry(&self, geometry: Value) {
        self.capture.set_geometry(&self.effects, geometry);
    }

    /// Command: the entry's connectivity and its picked precedent.
    pub fn capture_connect(&self, connectivity: Value, precedent: Value) {
        self.capture.connect(&self.effects, connectivity, precedent);
    }

    /// Command: drop the held draft and start from the form's defaults.
    pub fn capture_discard_draft(&self) {
        self.capture.discard_draft(&self.effects);
    }

    /// Command: close the form; its draft stays.
    pub fn capture_close(&self) {
        self.capture.close();
    }

    /// Command: submit the form; `keep` turns each planned upload's picked
    /// file into held bytes.
    pub fn capture_submit<K, Fut>(&self, keep: K) -> impl Future<Output = Result<Value, Refusal>> + use<E, K, Fut>
    where
        K: Fn(crate::survey_capture::Upload) -> Fut + 'static,
        Fut: Future<Output = Result<(), Refusal>>,
    {
        self.capture.submit(&self.effects, keep)
    }

    /// Command: queue one correction, deletion or restore of an entry,
    /// planned against the row this device shows of it.
    pub fn survey_mutate(&self, payload: Value, prior: Value) -> impl Future<Output = Result<Value, Refusal>> + use<E> {
        self.capture.mutate(&self.effects, payload, prior)
    }

    /// Command: delete a node and the edges it cascades to.
    pub fn survey_delete_cascade(&self, request: Value) -> impl Future<Output = Result<Value, Refusal>> + use<E> {
        self.capture.delete_cascade(&self.effects, request)
    }
}

/// How many observers want a read. A read runs only while it is wanted, so
/// a value nobody shows is never fetched.
#[derive(Clone, Default)]
pub struct Demand(ArcRwSignal<usize>);

impl Demand {
    /// Wants the read until the returned guard drops.
    pub fn want(&self) -> Want {
        self.0.update(|wanted| *wanted += 1);
        Want(self.0.clone())
    }

    fn wanted(&self) -> bool {
        self.0.with(|wanted| *wanted > 0)
    }
}

/// Keeps a read wanted while it lives.
pub struct Want(ArcRwSignal<usize>);

impl Drop for Want {
    fn drop(&mut self) {
        self.0.update(|wanted| *wanted = wanted.saturating_sub(1));
    }
}

/// A memo whose source reads only while someone wants it.
#[derive(Clone)]
pub struct OnDemand<T: Send + Sync + 'static> {
    pub value: ArcMemo<T>,
    pub demand: Demand,
}

/// Where one scoped read stands for the current scope.
#[derive(Clone, Debug, PartialEq)]
pub enum Read<T> {
    /// Nothing to read for this scope, such as a project read without a
    /// signed-in principal or an active project.
    Idle,
    /// This scope has no answer yet. `previous` is the same principal's last
    /// answer, kept so a project switch does not flicker; another
    /// principal's answer never shows.
    Pending { previous: Option<T> },
    /// This scope's answer.
    Done(Result<T, Refusal>),
}

impl<T> Read<T> {
    /// The value to show: this scope's answer, or the kept previous one.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Pending { previous } => previous.as_ref(),
            Self::Done(Ok(value)) => Some(value),
            Self::Idle | Self::Done(Err(_)) => None,
        }
    }
}

/// One answer, for the key it was read for.
#[derive(Clone, Debug, PartialEq)]
struct Answer<T> {
    key: Scope,
    outcome: Result<T, Refusal>,
}

/// A read that follows the scope while it is wanted. The scope decides its
/// key: the whole scope for a project read, the principal alone for an
/// account read, and none when there is nothing to read. It never reads the
/// same key twice. A change while a read is in flight waits for it, then
/// only the latest key is read (reactive_graph's async derived), and an
/// answer counts only for the key it was read for.
pub struct ScopedRead<T: Send + Sync + 'static> {
    pub state: OnDemand<Read<T>>,
}

impl<T: Clone + PartialEq + Send + Sync + 'static> ScopedRead<T> {
    /// A read of the active project for the signed-in principal.
    pub fn project<F, Fut>(scope: &ArcRwSignal<Scope>, read: F) -> Self
    where
        F: Fn(Scope) -> Fut + 'static,
        Fut: Future<Output = Result<T, Refusal>> + 'static,
    {
        let key = |scope: &Scope| scope.is_complete().then(|| scope.clone());
        Self::keyed(scope, key, Demand::default(), read)
    }

    /// A read of the signed-in account, wanted with `demand`.
    pub fn signed_in<F, Fut>(scope: &ArcRwSignal<Scope>, demand: &Demand, read: F) -> Self
    where
        F: Fn(Scope) -> Fut + 'static,
        Fut: Future<Output = Result<T, Refusal>> + 'static,
    {
        Self::keyed(scope, Scope::signed_in, demand.clone(), read)
    }

    /// A read of what this edge holds for whoever is here, signed in or not,
    /// wanted with `demand`.
    pub fn held<F, Fut>(scope: &ArcRwSignal<Scope>, demand: &Demand, read: F) -> Self
    where
        F: Fn(Scope) -> Fut + 'static,
        Fut: Future<Output = Result<T, Refusal>> + 'static,
    {
        Self::keyed(scope, |scope| Some(scope.account()), demand.clone(), read)
    }

    fn keyed<F, Fut>(
        scope: &ArcRwSignal<Scope>,
        key: fn(&Scope) -> Option<Scope>,
        demand: Demand,
        read: F,
    ) -> Self
    where
        F: Fn(Scope) -> Fut + 'static,
        Fut: Future<Output = Result<T, Refusal>> + 'static,
    {
        let key = ArcMemo::new({
            let scope = scope.clone();
            move |_: Option<&Option<Scope>>| scope.with(key)
        });
        // The last answer read: kept while nothing wants a newer one, and
        // reused when the same key is wanted again.
        let last = Rc::new(RefCell::new(None::<Answer<T>>));
        let answer = ArcAsyncDerived::new_unsync({
            let (key, demand) = (key.clone(), demand.clone());
            move || {
                let (key, wanted) = (key.get(), demand.wanted());
                let kept = last.borrow().clone();
                let answered = kept.as_ref().is_some_and(|kept| Some(&kept.key) == key.as_ref());
                let pending = match &key {
                    Some(key) if wanted && !answered => Some(read(key.clone())),
                    _ => None,
                };
                let last = Rc::clone(&last);
                async move {
                    let Some(key) = key else {
                        last.replace(None);
                        return None;
                    };
                    let Some(pending) = pending else {
                        return kept;
                    };
                    let answer = Answer {
                        key,
                        outcome: pending.await,
                    };
                    last.replace(Some(answer.clone()));
                    Some(answer)
                }
            }
        });
        let value = ArcMemo::new(move |_: Option<&Read<T>>| {
            let Some(key) = key.get() else {
                return Read::Idle;
            };
            answer.with(|answer: &Option<Option<Answer<T>>>| {
                match answer.as_ref().and_then(Option::as_ref) {
                    Some(answer) if answer.key == key => Read::Done(answer.outcome.clone()),
                    Some(answer) if answer.key.principal == key.principal => Read::Pending {
                        previous: answer.outcome.clone().ok(),
                    },
                    _ => Read::Pending { previous: None },
                }
            })
        });
        Self {
            state: OnDemand { value, demand },
        }
    }

    /// A value derived from this read, wanted together with it.
    pub fn derive<U>(&self, derive: impl Fn(&Read<T>) -> U + Send + Sync + 'static) -> OnDemand<U>
    where
        U: PartialEq + Send + Sync + 'static,
    {
        let state = self.state.value.clone();
        OnDemand {
            value: ArcMemo::new(move |_: Option<&U>| state.with(&derive)),
            demand: self.state.demand.clone(),
        }
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
#[path = "client_state_tests.rs"]
mod tests;
