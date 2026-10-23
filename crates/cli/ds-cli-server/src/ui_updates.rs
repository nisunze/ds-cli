//! Core keeps every installed UI unit on its channel's active release with no
//! person's action (philosophy §8, DECOUPLING-007). The background host runs a
//! pass shortly after it starts and then every interval. Activation affects
//! later captures only: a running Desktop keeps the unit it loaded until it
//! loads again. Absent or removed units are retained and read no catalog.
use ds_command_kernel::module_install::ActivationGuard;
use ds_command_kernel::module_release::ui::UiComponent;
use ds_update_runtime::modules::ui::{
    self as ui, UiConvergence, UiConvergenceOutcome, UiModuleStore,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Leaves a Desktop opened at sign-in time to capture its units first.
pub const FIRST_PASS_DELAY: Duration = Duration::from_secs(120);
pub const PASS_INTERVAL: Duration = Duration::from_secs(60 * 60);
const SLICE: Duration = Duration::from_millis(100);
const COMPONENTS: [UiComponent; 2] = [UiComponent::Interactions, UiComponent::CommandKernel];

pub struct UiUpdates {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl UiUpdates {
    pub fn start(lane: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::Builder::new()
            .name("ds-ui-updates".into())
            .spawn(move || {
                let cancelled = || flag.load(Ordering::Acquire);
                run(
                    &flag,
                    FIRST_PASS_DELAY,
                    PASS_INTERVAL,
                    &mut |component| converge(component, &lane, &cancelled),
                    &mut |message| eprintln!("DS server: {message}"),
                );
            })
            .ok();
        Self { stop, worker }
    }
}

impl Drop for UiUpdates {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

type Converge<'a> = &'a mut dyn FnMut(UiComponent) -> Result<UiConvergence, String>;

fn run(
    stop: &AtomicBool,
    first: Duration,
    interval: Duration,
    converge: Converge<'_>,
    log: &mut dyn FnMut(&str),
) {
    let mut said = [None, None];
    let mut next = first;
    while wait(stop, next) {
        pass(converge, &mut said, log);
        next = interval;
    }
}

/// An update is said once; a failure is said once until it changes or clears.
fn pass(converge: Converge<'_>, said: &mut [Option<String>; 2], log: &mut dyn FnMut(&str)) {
    for (index, component) in COMPONENTS.into_iter().enumerate() {
        match converge(component) {
            Ok(done) => {
                said[index] = None;
                if done.outcome == UiConvergenceOutcome::Updated {
                    log(&format!(
                        "{} UI unit is now {} (generation {}); the Desktop uses it when it next loads",
                        component.as_str(),
                        done.release_version
                            .as_deref()
                            .unwrap_or("the active release"),
                        done.generation
                    ));
                }
            }
            Err(message) if said[index].as_deref() != Some(message.as_str()) => {
                log(&format!(
                    "{} UI unit update did not complete ({message}); the installed unit is retained",
                    component.as_str()
                ));
                said[index] = Some(message);
            }
            Err(_) => {}
        }
    }
}

/// Sliced so stopping the host never waits out an interval; false once stopped.
pub(crate) fn wait(stop: &AtomicBool, duration: Duration) -> bool {
    let mut waited = Duration::ZERO;
    while waited < duration && !stop.load(Ordering::Acquire) {
        let slice = SLICE.min(duration - waited);
        thread::sleep(slice);
        waited += slice;
    }
    !stop.load(Ordering::Acquire)
}

/// Only this installed Core's packaged release authority decides the target.
fn converge(
    component: UiComponent,
    lane: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<UiConvergence, String> {
    #[cfg(windows)]
    ds_cli_auth::windows_host::register_ui_store_owner()?;
    ui::ui_platform()?;
    let selection = UiModuleStore::selection_managed(component, lane)?;
    if let Some(retained) = ui::retained_convergence(component, lane, selection.as_ref()) {
        return Ok(retained);
    }
    let lane_id = ds_cli_auth::Lane::parse(lane).map_err(|failure| failure.to_string())?;
    let authority = ds_cli_auth::module_updates::capture_release_authority(lane_id)
        .map_err(|failure| failure.to_string())?;
    let grant = authority.acquire_active_ui(component, cancelled)?;
    UiModuleStore::managed(component, lane)?.converge(
        grant,
        ActivationGuard::default(),
        |_| {},
        cancelled,
    )
}

#[cfg(test)]
#[path = "ui_updates_tests.rs"]
mod tests;
