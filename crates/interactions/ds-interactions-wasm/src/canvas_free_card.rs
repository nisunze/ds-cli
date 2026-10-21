//! Numeric transport over the same free-card hit authority native callers use.
use crate::canvas_graph::CanvasGeometryBoundaryError;
use ds_canvas_graph::{
    Point,
    free_hit::{FreeCards, PortEnd},
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct CanvasFreeCardGeometry {
    inner: FreeCards,
}

#[wasm_bindgen]
impl CanvasFreeCardGeometry {
    #[wasm_bindgen(constructor)]
    pub fn new(rectangles: &[f64], parents: &[i32]) -> Result<Self, CanvasGeometryBoundaryError> {
        Ok(Self {
            inner: FreeCards::new(rectangles, parents)?,
        })
    }

    /// [anchor x, anchor y, world radius, visible 0/1].
    pub fn port(
        &self,
        index: usize,
        end: u8,
        zoom: f64,
    ) -> Result<Vec<f64>, CanvasGeometryBoundaryError> {
        let port = self.inner.port(index, PortEnd::try_from(end)?, zoom)?;
        Ok(vec![
            port.anchor.x,
            port.anchor.y,
            port.radius,
            if port.visible { 1. } else { 0. },
        ])
    }

    pub fn boundary(
        &self,
        index: usize,
        x: f64,
        y: f64,
    ) -> Result<Vec<f64>, CanvasGeometryBoundaryError> {
        let point = self.inner.boundary(index, Point::new(x, y))?;
        Ok(vec![point.x, point.y])
    }

    pub fn hit(&self, x: f64, y: f64) -> Result<i32, CanvasGeometryBoundaryError> {
        Ok(self
            .inner
            .hit(Point::new(x, y))?
            .map_or(-1, |index| index as i32))
    }

    /// [] for no hit; otherwise [rectangle index, end 0/1].
    #[wasm_bindgen(js_name = hitPort)]
    pub fn hit_port(
        &self,
        x: f64,
        y: f64,
        zoom: f64,
        minimum_screen_radius: Option<f64>,
    ) -> Result<Vec<i32>, CanvasGeometryBoundaryError> {
        Ok(self
            .inner
            .hit_port(
                Point::new(x, y),
                zoom,
                minimum_screen_radius.unwrap_or(ds_canvas_graph::free_hit::PORT_SCREEN_RADIUS),
            )?
            .map_or_else(Vec::new, |hit| vec![hit.index as i32, hit.end as i32]))
    }

    #[wasm_bindgen(js_name = drop)]
    pub fn drop_target(
        &self,
        x: f64,
        y: f64,
        dragged: i32,
    ) -> Result<i32, CanvasGeometryBoundaryError> {
        let dragged = if dragged == -1 {
            None
        } else if dragged >= 0 {
            Some(dragged as usize)
        } else {
            return Err(ds_canvas_graph::GeometryError::InvalidMetric {
                field: "draggedCardIndex".into(),
                value: dragged.to_string(),
                expected: "-1 or a parent-table index".into(),
            }
            .into());
        };
        Ok(self
            .inner
            .drop_target(Point::new(x, y), dragged)?
            .map_or(-1, |index| index as i32))
    }
}
