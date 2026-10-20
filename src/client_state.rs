//! Client application state as Rust signals: Leptos's reactive core
//! (`reactive_graph`), never its UI (architecture sentence 4).
//!
//! The Kernel holds the application state a client shows as signals and
//! memos, fed by its own decisions: gateway answers through
//! [`crate::gateway_exchange`] and, as stores move here, durable reads. The
//! graph has the same semantics in the browser's WASM and natively; only the
//! [`Effects`] a host performs differ. Svelte subscribes to each memo through
//! one thin binding (`ds-web/src/lib/runtime/rust-state.ts`), sends intents
//! through typed commands and keeps only presentation state. A memo notifies
//! only when its value changes. The README's "Client state on Rust signals"
//! section is the pattern for moving a store here.
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

use crate::gateway_exchange::{Exchange, Refusal};

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
}

/// The effects a host performs for the client state.
pub trait Effects: 'static {
    /// Run one gateway exchange to its typed outcome.
    fn exchange<C: Exchange + 'static>(
        &self,
        call: C,
    ) -> impl Future<Output = Result<C::Output, Refusal>> + use<Self, C>;
}

/// One client's state: its scope and each domain's signals.
pub struct ClientState {
    scope: ArcRwSignal<Scope>,
    pub maps: maps::Maps,
}

impl ClientState {
    pub fn new(effects: impl Effects) -> Self {
        let effects = Rc::new(effects);
        let scope = ArcRwSignal::new(Scope::default());
        Self {
            maps: maps::Maps::new(&scope, &effects),
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
    /// No signed-in principal or no active project: nothing is read.
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

/// One answer, for the scope it was read for. No outcome: nothing was read
/// for that scope.
#[derive(Clone, Debug, PartialEq)]
struct Answer<T> {
    scope: Scope,
    outcome: Option<Result<T, Refusal>>,
}

/// A read that follows the scope while it is wanted. It reads when the scope
/// is complete and never reads the same scope twice. A change while a read
/// is in flight waits for it, then only the latest scope is read
/// (reactive_graph's async derived), and an answer counts only for the
/// scope it was read for.
pub struct ScopedRead<T: Send + Sync + 'static> {
    pub state: OnDemand<Read<T>>,
}

impl<T: Clone + PartialEq + Send + Sync + 'static> ScopedRead<T> {
    pub fn new<F, Fut>(scope: &ArcRwSignal<Scope>, read: F) -> Self
    where
        F: Fn(Scope) -> Fut + 'static,
        Fut: Future<Output = Result<T, Refusal>> + 'static,
    {
        let demand = Demand::default();
        // The last answer read: kept while nothing wants a newer one, and
        // reused when the same scope is wanted again.
        let last = Rc::new(RefCell::new(None::<Answer<T>>));
        let answer = ArcAsyncDerived::new_unsync({
            let (scope, demand) = (scope.clone(), demand.clone());
            move || {
                let (scope, wanted) = (scope.get(), demand.wanted());
                let kept = last.borrow().clone();
                let answered = kept.as_ref().is_some_and(|kept| kept.scope == scope);
                let pending =
                    (wanted && scope.is_complete() && !answered).then(|| read(scope.clone()));
                let last = Rc::clone(&last);
                async move {
                    if !scope.is_complete() {
                        last.replace(None);
                        return Answer {
                            scope,
                            outcome: None,
                        };
                    }
                    let Some(pending) = pending else {
                        return kept.unwrap_or(Answer {
                            scope: Scope::default(),
                            outcome: None,
                        });
                    };
                    let answer = Answer {
                        scope,
                        outcome: Some(pending.await),
                    };
                    last.replace(Some(answer.clone()));
                    answer
                }
            }
        });
        let value = ArcMemo::new({
            let scope = scope.clone();
            move |_: Option<&Read<T>>| {
                let scope = scope.get();
                if !scope.is_complete() {
                    return Read::Idle;
                }
                answer.with(|answer: &Option<Answer<T>>| match answer {
                    Some(answer) if answer.scope == scope => match &answer.outcome {
                        Some(outcome) => Read::Done(outcome.clone()),
                        None => Read::Pending { previous: None },
                    },
                    Some(answer) if answer.scope.principal == scope.principal => Read::Pending {
                        previous: answer.outcome.clone().and_then(Result::ok),
                    },
                    _ => Read::Pending { previous: None },
                })
            }
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
#[path = "client_state_tests.rs"]
mod tests;
