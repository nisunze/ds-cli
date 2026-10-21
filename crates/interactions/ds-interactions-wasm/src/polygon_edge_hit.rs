//! Lazy browser geometry and projection adapter for UI-owned polygon testing.
use ds_canvas::polygon_edge_hit::{PolygonEdgeHitError, PolygonEdgeValue, polygon_edge_hit_source};
use wasm_bindgen::prelude::*;

/// Borrowed JS references preserve exact doubles, signed zero and read order.
/// Only fields requested by the core are read; ancillary metadata/axes stay held.
struct BrowserValue(JsValue);
impl PolygonEdgeValue for BrowserValue {
    type Error = JsValue;
    fn field(&self, name: &str) -> Result<Self, Self::Error> {
        if !self.0.is_object() {
            return Ok(Self(JsValue::UNDEFINED));
        }
        js_sys::Reflect::get(&self.0, &name.into()).map(Self)
    }
    fn array_len(&self) -> Result<Option<usize>, Self::Error> {
        if !js_sys::Array::is_array(&self.0) {
            return Ok(None);
        }
        js_sys::Reflect::get(&self.0, &"length".into())
            .map(|length| length.as_f64().map(|length| length as usize))
    }
    fn item(&self, index: usize) -> Result<Self, Self::Error> {
        js_sys::Reflect::get(&self.0, &(index as f64).into()).map(Self)
    }
    fn number(&self) -> Option<f64> {
        self.0.as_f64()
    }
    fn text(&self) -> Option<String> {
        self.0.as_string()
    }
}

#[wasm_bindgen(js_name = "polygonEdgeHit")]
pub fn polygon_edge_hit(
    geometry: &JsValue,
    pointer_x: f64,
    pointer_y: f64,
    tolerance_px: &JsValue,
    projector: &js_sys::Function,
) -> Result<bool, JsValue> {
    let Some(tolerance_px) = tolerance_px.as_f64() else {
        return Ok(false);
    };
    polygon_edge_hit_source(
        &BrowserValue(geometry.clone()),
        [pointer_x, pointer_y],
        tolerance_px,
        |point| {
            let projected =
                projector.call2(&JsValue::UNDEFINED, &point[0].into(), &point[1].into())?;
            let x = js_sys::Reflect::get(&projected, &0.into())?
                .as_f64()
                .ok_or_else(|| JsError::new("polygonEdgeHit: projector must return two numbers"))?;
            let y = js_sys::Reflect::get(&projected, &1.into())?
                .as_f64()
                .ok_or_else(|| JsError::new("polygonEdgeHit: projector must return two numbers"))?;
            Ok::<_, JsValue>([x, y])
        },
    )
    .map_err(|error| match error {
        PolygonEdgeHitError::InvalidCoordinates => {
            js_sys::TypeError::new("polygonEdgeHit: polygon coordinates must be an array").into()
        }
        PolygonEdgeHitError::InvalidRing => {
            js_sys::TypeError::new("polygonEdgeHit: polygon rings must be arrays").into()
        }
        PolygonEdgeHitError::Read(error) | PolygonEdgeHitError::Project(error) => error,
    })
}
