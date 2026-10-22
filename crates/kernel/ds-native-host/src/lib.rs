//! Native host protection primitives; no CLI dispatch or argument parsing.

#[cfg(windows)]
pub mod state_windows;
#[cfg(windows)]
pub mod windows_host;
