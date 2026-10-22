//! Independent profile drawing and transient interaction surface. The application
//! supplies projected facts and handles typed intents; this crate cannot call Kernel.
#[cfg(target_arch = "wasm32")]
pub mod browser;
pub mod camera;
pub mod draw_geometry;
pub mod hit;
pub mod labels;
pub mod model;
pub mod navigation;
#[cfg(target_arch = "wasm32")]
mod paint;
#[cfg(target_arch = "wasm32")]
mod panel;
pub mod panel_model;
pub mod payload;
pub mod preview;
#[cfg(target_arch = "wasm32")]
mod readiness;
pub mod selection;

#[cfg(target_arch = "wasm32")]
mod display;
pub mod filter;
#[cfg(target_arch = "wasm32")]
mod legend;
pub mod retype;
pub mod retype_search;
#[cfg(target_arch = "wasm32")]
mod weather;
