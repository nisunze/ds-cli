//! Canvas2D numeric interface over ds-canvas; no command-kernel dependency.
use wasm_bindgen::prelude::*;
#[wasm_bindgen(js_name=canvasCamera)]
pub fn canvas_camera(operation: u8, input: &[f64]) -> Result<Vec<f64>, String> {
    ds_canvas::packed::camera_buffer(operation, input)
}
#[wasm_bindgen(js_name=canvasSelection)]
pub fn canvas_selection(operation: u8, a: &[f64], b: &[f64]) -> Result<Vec<f64>, String> {
    ds_canvas::packed::selection_buffer(operation, a, b)
}
#[wasm_bindgen(js_name=canvasGesture)]
pub fn canvas_gesture(operation: u8, input: &[f64]) -> Result<Vec<f64>, String> {
    ds_canvas::packed::gesture_buffer(operation, input)
}
/// Retained spatial index built once per immutable scene revision. Pointer
/// queries contain only coordinates; geometry never crosses on mousemove.
#[wasm_bindgen::prelude::wasm_bindgen]
pub struct CanvasMeasuredPath {
    inner: ds_canvas::measured_path::MeasuredPath,
}
#[wasm_bindgen::prelude::wasm_bindgen]
impl CanvasMeasuredPath {
    #[wasm_bindgen::prelude::wasm_bindgen(constructor)]
    pub fn new(segments: &[f64], ordinates: &[f64]) -> Result<CanvasMeasuredPath, String> {
        Ok(Self {
            inner: ds_canvas::measured_path::MeasuredPath::new(segments, ordinates)?,
        })
    }
    pub fn at(&self, x: f64, y: f64) -> Vec<f64> {
        self.inner.at(x, y)
    }
    pub fn snap(&self, group: f64, x: f64, tolerance: f64) -> Vec<f64> {
        self.inner.snap(group, x, tolerance)
    }
}

/// Numeric page/paper layout session. The host retains domain element IDs;
/// commands separately validate and persist the authored document.
#[wasm_bindgen]
pub struct CanvasLayoutSession {
    inner: ds_canvas::layout_session::LayoutSession,
}
#[wasm_bindgen]
impl CanvasLayoutSession {
    #[wasm_bindgen(constructor)]
    pub fn new(bounds: &[f64], rects: &[f64], grid: f64, minimum: f64) -> Result<Self, String> {
        Ok(Self {
            inner: ds_canvas::layout_session::LayoutSession::new(bounds, rects, grid, minimum)?,
        })
    }
    pub fn project(&mut self, width: f64, height: f64, dpr: f64) -> Result<Vec<f64>, String> {
        self.inner.project(width, height, dpr)
    }
    pub fn camera(&self) -> Vec<f64> {
        self.inner.camera()
    }
    pub fn fit(&mut self) -> Vec<f64> {
        self.inner.fit()
    }
    pub fn pan(&mut self, dx: f64, dy: f64) -> Result<Vec<f64>, String> {
        self.inner.pan(dx, dy)
    }
    pub fn zoom(&mut self, x: f64, y: f64, factor: f64) -> Result<Vec<f64>, String> {
        self.inner.zoom(x, y, factor)
    }
    pub fn rectangles(&self) -> Vec<f64> {
        self.inner.rectangles()
    }
    pub fn handles(&self, index: usize) -> Result<Vec<f64>, String> {
        self.inner.handles(index)
    }
    pub fn hit(&self, x: f64, y: f64, selected: i32) -> i32 {
        self.inner.hit(x, y, selected)
    }
    pub fn drag(
        &self,
        index: usize,
        handle: u8,
        pointer: &[f64],
        snap: bool,
    ) -> Result<Vec<f64>, String> {
        self.inner.drag(index, handle, pointer, snap)
    }
    #[wasm_bindgen(js_name=applyRect)]
    pub fn apply_rect(&mut self, index: usize, rect: &[f64]) -> Result<Vec<f64>, String> {
        self.inner.apply_rect(index, rect)
    }
}
