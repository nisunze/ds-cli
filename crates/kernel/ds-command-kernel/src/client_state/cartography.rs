//! The cartography a client shows: the platform's map defaults and the relief
//! control. The defaults show at once from what this edge holds, follow
//! Brain's document while a signed-in page shows them, and are held again
//! after every read and publish. Relief starts off every session; its
//! intensity is the account's held choice, else the defaults' intensity at
//! the nearest preset.

use std::future::Future;
use std::rc::Rc;

use reactive_graph::computed::ArcMemo;
use reactive_graph::signal::ArcRwSignal;
use reactive_graph::traits::{Get, GetUntracked, Set, Update, With, WithUntracked};
use serde::Serialize;
use serde_json::Value;

use super::{Demand, Effects, OnDemand, Scope, ScopedRead};
use crate::durable_store::Request;
use crate::gateway_exchange::map_defaults::{self, nearest_preset, MapDefaults, Publish, RELIEF_PRESETS};
use crate::gateway_exchange::Refusal;
use crate::ui_preferences::{Key, Operation};

/// The relief control: whether it shows and how strongly.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Relief {
    /// On for this session only; every start begins with relief off.
    pub hillshade: bool,
    /// One of `presets`.
    pub exaggeration: f64,
    /// The intensities the control steps through, subtle to dramatic.
    pub presets: Vec<f64>,
}

pub struct Cartography {
    /// The platform's defaults: what this edge holds until Brain answers,
    /// the built-in defaults until either does.
    pub defaults: OnDemand<MapDefaults>,
    pub relief: OnDemand<Relief>,
    scope: ArcRwSignal<Scope>,
    /// The last defaults this account published, until it changes.
    published: ArcRwSignal<Option<(Scope, MapDefaults)>>,
    /// The intensity this account chose this session.
    chosen: ArcRwSignal<Option<(Scope, f64)>>,
    shown: ArcRwSignal<bool>,
}

/// The wire name of a preference key.
fn name(key: Key) -> String {
    serde_json::to_value(key)
        .ok()
        .and_then(|name| name.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The value this edge holds for an account key, if any.
fn held<E: Effects>(
    effects: &E,
    key: Key,
) -> impl Future<Output = Result<Option<Value>, Refusal>> + use<E> {
    let reading = effects.store(Request::UiPreferences {
        operation: Operation::Get {
            project: None,
            keys: vec![key],
        },
    });
    async move {
        let answer = reading.await?;
        Ok(answer
            .get("values")
            .and_then(|values| values.get(name(key)))
            .filter(|value| !value.is_null())
            .cloned())
    }
}

/// Hold a value for an account key, best effort: a refusal keeps the
/// previous one.
fn hold<E: Effects>(effects: &E, key: Key, value: Value) -> impl Future<Output = ()> + use<E> {
    let writing = effects.store(Request::UiPreferences {
        operation: Operation::Set {
            project: None,
            key,
            value,
            now: effects.now_ms(),
        },
    });
    async move {
        let _ = writing.await;
    }
}

fn document(defaults: &MapDefaults) -> Value {
    serde_json::to_value(defaults).unwrap_or(Value::Null)
}

impl Cartography {
    pub(super) fn new<E: Effects>(scope: &ArcRwSignal<Scope>, effects: &Rc<E>) -> Self {
        // Showing the defaults or the relief control wants every read here.
        let demand = Demand::default();
        let held_defaults = ScopedRead::held(scope, &demand, {
            let effects = Rc::clone(effects);
            move |_| {
                let reading = held(&*effects, Key::MapDefaults);
                async move { Ok(reading.await?.map(|held| MapDefaults::admit(&held))) }
            }
        });
        let fresh = ScopedRead::signed_in(scope, &demand, {
            let effects = Rc::clone(effects);
            move |_| {
                let reading = effects.exchange(map_defaults::Read);
                let effects = Rc::clone(&effects);
                async move {
                    let fresh = reading.await?;
                    hold(&*effects, Key::MapDefaults, document(&fresh)).await;
                    Ok(fresh)
                }
            }
        });
        let held_relief = ScopedRead::held(scope, &demand, {
            let effects = Rc::clone(effects);
            move |_| {
                let reading = held(&*effects, Key::HillshadeExaggeration);
                async move { Ok(reading.await?.and_then(|held| held.as_f64())) }
            }
        });

        let published = ArcRwSignal::new(None::<(Scope, MapDefaults)>);
        let defaults = ArcMemo::new({
            let (scope, published) = (scope.clone(), published.clone());
            let (fresh, held) = (fresh.state.value, held_defaults.state.value);
            move |_: Option<&MapDefaults>| {
                let account = scope.with(Scope::account);
                let published = published.with(|published| {
                    published
                        .as_ref()
                        .filter(|(by, _)| *by == account)
                        .map(|(_, defaults)| defaults.clone())
                });
                published
                    .or_else(|| fresh.with(|fresh| fresh.value().cloned()))
                    .or_else(|| held.with(|held| held.value().cloned().flatten()))
                    .unwrap_or_default()
            }
        });

        let chosen = ArcRwSignal::new(None::<(Scope, f64)>);
        let shown = ArcRwSignal::new(false);
        let relief = ArcMemo::new({
            let (scope, chosen, shown) = (scope.clone(), chosen.clone(), shown.clone());
            let (held, defaults) = (held_relief.state.value, defaults.clone());
            move |_: Option<&Relief>| {
                let account = scope.with(Scope::account);
                let chosen = chosen.with(|chosen| {
                    chosen
                        .as_ref()
                        .filter(|(by, _)| *by == account)
                        .map(|(_, exaggeration)| *exaggeration)
                });
                let exaggeration = chosen
                    .or_else(|| held.with(|held| held.value().copied().flatten()))
                    .unwrap_or_else(|| {
                        defaults.with(|defaults| nearest_preset(defaults.hillshade.exaggeration))
                    });
                Relief {
                    hillshade: shown.get(),
                    exaggeration,
                    presets: RELIEF_PRESETS.to_vec(),
                }
            }
        });

        Self {
            defaults: OnDemand {
                value: defaults,
                demand: demand.clone(),
            },
            relief: OnDemand {
                value: relief,
                demand,
            },
            scope: scope.clone(),
            published,
            chosen,
            shown,
        }
    }

    /// Publish defaults for every client; they show here at once and this
    /// edge holds them.
    pub(super) fn publish<E: Effects>(
        &self,
        effects: &Rc<E>,
        defaults: MapDefaults,
    ) -> impl Future<Output = Result<MapDefaults, Refusal>> + use<E> {
        let publishing = effects.exchange(Publish { defaults });
        let account = self.scope.with_untracked(Scope::account);
        let (published, effects) = (self.published.clone(), Rc::clone(effects));
        async move {
            let defaults = publishing.await?;
            published.set(Some((account, defaults.clone())));
            hold(&*effects, Key::MapDefaults, document(&defaults)).await;
            Ok(defaults)
        }
    }

    /// Show or hide relief for this session.
    pub(super) fn toggle_relief(&self) {
        self.shown.update(|shown| *shown = !*shown);
    }

    /// Step to the next intensity, showing relief so the change is visible.
    /// The account keeps the intensity.
    pub(super) fn cycle_relief<E: Effects>(&self, effects: &Rc<E>) {
        let current = self.relief.value.get_untracked().exaggeration;
        let next = RELIEF_PRESETS
            .iter()
            .position(|preset| *preset == current)
            .map_or(RELIEF_PRESETS[0], |index| {
                RELIEF_PRESETS[(index + 1) % RELIEF_PRESETS.len()]
            });
        let account = self.scope.with_untracked(Scope::account);
        self.chosen.set(Some((account, next)));
        self.shown.set(true);
        any_spawner::Executor::spawn_local(hold(
            &**effects,
            Key::HillshadeExaggeration,
            next.into(),
        ));
    }
}

#[cfg(test)]
#[path = "cartography_tests.rs"]
mod tests;
