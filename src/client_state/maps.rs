//! Map provider state: the active project's Google dark basemap, read
//! through [`crate::gateway_exchange::maps`] for the current scope while
//! something shows it.

use std::rc::Rc;

use reactive_graph::signal::ArcRwSignal;

use super::{Effects, OnDemand, Scope, ScopedRead};
use crate::gateway_exchange::maps::{Basemap, GoogleDarkSession};

pub struct Maps {
    /// The dark roadmap basemap the active project offers, if any. A refused
    /// read leaves it out.
    pub dark_basemap: OnDemand<Option<Basemap>>,
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
        Self {
            dark_basemap: dark_read.derive(|read| {
                read.value()
                    .and_then(Option::as_ref)
                    .map(|session| session.basemap.clone())
            }),
        }
    }
}
