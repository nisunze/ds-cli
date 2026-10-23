//! Core keeps an installed Network page engine unit on its channel's active
//! release with no person's action (philosophy §8, DECOUPLING-007), on the same
//! cadence as the UI units. A loaded page keeps the unit it captured until it
//! loads again. An absent or removed unit is retained and reads no catalog; a
//! local-dev selection is a developer's own and never follows a channel.
use crate::ui_updates::{FIRST_PASS_DELAY, PASS_INTERVAL, wait};
use ds_command_kernel::module_install::ActivationGuard;
use ds_command_kernel::module_release::{ModuleComponent, ui::UiScope};
use ds_update_runtime::modules::engine_unit::{
    EngineConvergence, EngineConvergenceOutcome, EngineUnitStore, retained_convergence,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const COMPONENT: ModuleComponent = ModuleComponent::Network;

pub struct EngineUpdates {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl EngineUpdates {
    pub fn start(lane: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::Builder::new()
            .name("ds-engine-updates".into())
            .spawn(move || {
                let cancelled = || flag.load(Ordering::Acquire);
                run(
                    &flag,
                    FIRST_PASS_DELAY,
                    PASS_INTERVAL,
                    &mut || converge(&lane, &cancelled),
                    &mut |message| eprintln!("DS server: {message}"),
                );
            })
            .ok();
        Self { stop, worker }
    }
}

impl Drop for EngineUpdates {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

type Converge<'a> = &'a mut dyn FnMut() -> Result<EngineConvergence, String>;

fn run(
    stop: &AtomicBool,
    first: Duration,
    interval: Duration,
    converge: Converge<'_>,
    log: &mut dyn FnMut(&str),
) {
    let mut said = None;
    let mut next = first;
    while wait(stop, next) {
        pass(converge, &mut said, log);
        next = interval;
    }
}

/// An update is said once; a failure is said once until it changes or clears.
fn pass(converge: Converge<'_>, said: &mut Option<String>, log: &mut dyn FnMut(&str)) {
    match converge() {
        Ok(done) => {
            *said = None;
            if done.outcome == EngineConvergenceOutcome::Updated {
                log(&format!(
                    "Network page unit is now {} (generation {}); the Desktop uses it when it next loads it",
                    done.release_version.as_deref().unwrap_or("the active release"),
                    done.generation
                ));
            }
        }
        Err(message) if said.as_deref() != Some(message.as_str()) => {
            log(&format!(
                "Network page unit update did not complete ({message}); the installed unit is retained"
            ));
            *said = Some(message);
        }
        Err(_) => {}
    }
}

/// Only this installed Core's packaged release authority decides the target.
fn converge(lane: &str, cancelled: &dyn Fn() -> bool) -> Result<EngineConvergence, String> {
    #[cfg(windows)]
    ds_cli_auth::windows_host::register_ui_store_owner()?;
    ds_update_runtime::modules::ui::ui_platform()?;
    let selection = EngineUnitStore::selection_managed(COMPONENT, lane, UiScope::Release)?;
    if let Some(retained) = retained_convergence(COMPONENT, lane, selection.as_ref()) {
        return Ok(retained);
    }
    let lane_id = ds_cli_auth::Lane::parse(lane).map_err(|failure| failure.to_string())?;
    let authority = ds_cli_auth::module_updates::capture_release_authority(lane_id)
        .map_err(|failure| failure.to_string())?;
    let grant = authority.acquire_engine_unit(COMPONENT, None, cancelled)?;
    EngineUnitStore::managed(COMPONENT, lane, UiScope::Release)?.converge(
        grant,
        ActivationGuard::default(),
        |_| {},
        cancelled,
    )
}

#[cfg(test)]
#[path = "engine_updates_tests.rs"]
mod tests;
