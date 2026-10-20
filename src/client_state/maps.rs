//! Map provider state: the project's Google dark basemap and the 3D provider
//! configuration, read through [`crate::gateway_exchange::maps`] for the
//! current scope while something shows them.

use std::rc::Rc;

use reactive_graph::signal::ArcRwSignal;
use reactive_graph::traits::GetUntracked;
use serde::Serialize;

use super::{Effects, OnDemand, Read, Scope, ScopedRead};
use crate::gateway_exchange::maps::{Basemap, Config3d, GoogleDarkSession, Maps3dConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "lowercase")]
pub enum Maps3dLoadStatus {
    Idle,
    Loading,
    Ready,
    Unavailable,
    Error,
}

/// Where the 3D provider configuration stands, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Maps3dLoadState {
    pub status: Maps3dLoadStatus,
    pub detail: String,
}

pub struct Maps {
    config_read: ScopedRead<Config3d>,
    /// The dark roadmap basemap the active project offers, if any. A refused
    /// read leaves it out.
    pub dark_basemap: OnDemand<Option<Basemap>>,
    /// The 3D provider configuration for the current scope.
    pub config_3d: OnDemand<Option<Config3d>>,
    pub load_3d: OnDemand<Maps3dLoadState>,
    /// Google Photorealistic 3D can be offered: a complete scope and a keyed
    /// root URL.
    pub google_3d: OnDemand<bool>,
}

impl Maps {
    pub(super) fn new(scope: &ArcRwSignal<Scope>, effects: &Rc<impl Effects>) -> Self {
        let dark_read = ScopedRead::new(scope, {
            let effects = Rc::clone(effects);
            move |scope: Scope| {
                effects.exchange(GoogleDarkSession {
                    project: scope.project.unwrap_or_default(),
                })
            }
        });
        // The configuration is global; a complete scope gates reading it.
        let config_read = ScopedRead::new(scope, {
            let effects = Rc::clone(effects);
            move |_: Scope| effects.exchange(Maps3dConfig)
        });
        Self {
            dark_basemap: dark_read.derive(|read| {
                read.value()
                    .and_then(Option::as_ref)
                    .map(|session| session.basemap.clone())
            }),
            config_3d: config_read.derive(|read| read.value().cloned()),
            load_3d: config_read.derive(load_state),
            google_3d: config_read.derive(|read| read.value().is_some_and(offers_google)),
            config_read,
        }
    }

    /// Command: read the 3D provider configuration again.
    pub fn retry_3d(&self) {
        self.config_read.retry();
    }

    /// Resolves with the 3D load state once this scope's read has answered.
    pub fn settled_3d(&self) -> impl std::future::Future<Output = Maps3dLoadState> + 'static {
        let (settled, load) = (self.config_read.settled(), self.load_3d.value.clone());
        async move {
            settled.await;
            load.get_untracked()
        }
    }
}

fn offers_google(config: &Config3d) -> bool {
    config.google.available && !config.google.root_url.trim().is_empty()
}

fn load_state(read: &Read<Config3d>) -> Maps3dLoadState {
    let (status, detail) = match read {
        Read::Idle => (Maps3dLoadStatus::Idle, String::new()),
        Read::Pending { .. } => (Maps3dLoadStatus::Loading, "Loading Maps3D API config.".into()),
        Read::Done(Ok(config)) if offers_google(config) => (
            Maps3dLoadStatus::Ready,
            "Google 3D Tiles API root URL loaded.".into(),
        ),
        Read::Done(Ok(config)) if config.google.available => (
            Maps3dLoadStatus::Unavailable,
            "Maps3D API did not return a Google 3D root URL.".into(),
        ),
        Read::Done(Ok(_)) => (
            Maps3dLoadStatus::Unavailable,
            "Maps3D API reports Google 3D unavailable.".into(),
        ),
        Read::Done(Err(refusal)) if refusal.message.trim().is_empty() => (
            Maps3dLoadStatus::Error,
            "Maps3D API config request failed.".into(),
        ),
        Read::Done(Err(refusal)) => (Maps3dLoadStatus::Error, refusal.message.clone()),
    };
    Maps3dLoadState { status, detail }
}
