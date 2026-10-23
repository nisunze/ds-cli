//! Native host protection primitives; no CLI dispatch or argument parsing.

#[cfg(windows)]
pub mod state_windows;
#[cfg(any(windows, test))]
mod state_windows_error;
#[cfg(windows)]
pub mod windows_host;
