//! Progress delivery.
//!
//! The engine emits [`ProgressEvent`]s; a sink decides what they mean. The
//! Tauri shell forwards them to a window, the CLI prints them, the Cloud Run
//! adapter traces them, and tests collect them.

use std::sync::{Arc, Mutex};

use ds_solar_contracts::ProgressEvent;

/// Somewhere progress events go.
pub trait ProgressSink: Send + Sync {
    /// Receive one event. Must not panic and must not block for long: a slow
    /// sink slows the run it is observing.
    fn emit(&self, event: ProgressEvent);
}

/// Discards every event.
impl ProgressSink for () {
    fn emit(&self, _event: ProgressEvent) {}
}

/// Logs each event through `tracing`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingProgressSink;

impl ProgressSink for TracingProgressSink {
    fn emit(&self, event: ProgressEvent) {
        tracing::info!(
            phase = ?event.phase,
            stage = event.stage.as_deref().unwrap_or("-"),
            step = event.step.unwrap_or(0),
            total = event.total_steps.unwrap_or(0),
            "{}",
            event.message
        );
    }
}

/// Keeps every event, for tests and for the CLI's end-of-run summary.
#[derive(Debug, Clone, Default)]
pub struct CollectingProgressSink {
    events: Arc<Mutex<Vec<ProgressEvent>>>,
}

impl CollectingProgressSink {
    /// A new, empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything collected so far.
    pub fn events(&self) -> Vec<ProgressEvent> {
        self.events.lock().expect("progress mutex").clone()
    }
}

impl ProgressSink for CollectingProgressSink {
    fn emit(&self, event: ProgressEvent) {
        self.events.lock().expect("progress mutex").push(event);
    }
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
