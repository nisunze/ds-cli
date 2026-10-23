//! Cooperative cancellation.
//!
//! A token is a shared flag, nothing more. The engine polls it between stages;
//! nobody can use it to interrupt a computation mid-number, which is what keeps
//! a cancelled run from producing a half-computed result.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A shared cancellation flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    /// A token that has not been cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
#[path = "cancel_tests.rs"]
mod tests;
