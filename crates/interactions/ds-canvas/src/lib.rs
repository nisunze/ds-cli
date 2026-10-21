//! Reusable Canvas2D primitives extracted from the Profile interaction contract.
//! No graph, page, station or engineering model: hosts pass window facts and
//! intent; native/WASM consumers share the same camera and spatial results.
pub mod free_layout;
pub mod gantt_hit_index;
pub mod measured_path;
pub mod polygon_edge_hit;
pub mod profile_label_layout;
pub mod selection;
pub mod viewport;
pub use ds_geo_lite::window::Point;
use serde::{Deserialize, Serialize};
pub use viewport::{HostSurface, Projection, Viewport};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn max_x(self) -> f64 {
        self.x + self.width
    }
    pub fn max_y(self) -> f64 {
        self.y + self.height
    }
    pub fn center(self) -> Point {
        Point::new(self.x + self.width / 2., self.y + self.height / 2.)
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.max_x() && p.y >= self.y && p.y <= self.max_y()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl Bounds {
    pub fn width(self) -> f64 {
        self.max_x - self.min_x
    }
    pub fn height(self) -> f64 {
        self.max_y - self.min_y
    }
}
/// Profile's armed press remains a click until it travels three CSS pixels.
pub fn drag_started(start: Point, current: Point) -> bool {
    start.is_finite() && current.is_finite() && start.distance_to(current) >= 3.
}

pub mod packed;

pub mod layout_session;
