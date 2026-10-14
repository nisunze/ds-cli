//! Packed adapter over the retained Gantt UI index. Geometry crosses once per
//! scene; pointer probes return ordinals and never transmit authored model state.
use ds_canvas::gantt_hit_index::{Bounds, HitIndex, Region};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct GanttHitIndex {
    inner: HitIndex,
}
#[wasm_bindgen]
impl GanttHitIndex {
    #[wasm_bindgen(constructor)]
    pub fn new(
        metadata_json: &str,
        packed: &[f64],
        polylines: &[f64],
        cell_size: f64,
        edge_padding: f64,
    ) -> Result<Self, String> {
        let metadata: Vec<[String; 2]> =
            serde_json::from_str(metadata_json).map_err(|error| error.to_string())?;
        if packed.len() != metadata.len() * 8 {
            return Err("GanttHitIndex: each region requires eight packed values".into());
        }
        let mut regions = Vec::with_capacity(metadata.len());
        for ([id, target], values) in metadata.into_iter().zip(packed.chunks_exact(8)) {
            let polyline = if values[6] == -1. {
                None
            } else {
                let offset = values[6];
                let count = values[7];
                if !offset.is_finite()
                    || !count.is_finite()
                    || offset < 0.
                    || count < 0.
                    || offset.fract() != 0.
                    || count.fract() != 0.
                    || offset + count > polylines.len() as f64
                {
                    return Err("GanttHitIndex: polyline slice is outside the packed input".into());
                }
                Some(polylines[offset as usize..(offset + count) as usize].to_vec())
            };
            regions.push(Region {
                id,
                target,
                bounds: Bounds {
                    left: values[0],
                    top: values[1],
                    right: values[2],
                    bottom: values[3],
                },
                z: values[4],
                tolerance: values[5],
                polyline,
            });
        }
        Ok(Self {
            inner: HitIndex::new(regions, cell_size, edge_padding),
        })
    }
    #[wasm_bindgen(getter)]
    pub fn size(&self) -> usize {
        self.inner.size()
    }
    #[wasm_bindgen(getter, js_name=cellCount)]
    pub fn cell_count(&self) -> usize {
        self.inner.cell_count()
    }
    #[wasm_bindgen(js_name=pickAll)]
    pub fn pick_all(&self, x: f64, y: f64, targets_json: &str) -> Result<Vec<u32>, String> {
        let targets: Option<Vec<String>> = if targets_json.is_empty() {
            None
        } else {
            Some(serde_json::from_str(targets_json).map_err(|error| error.to_string())?)
        };
        Ok(self
            .inner
            .pick_all(x, y, targets.as_deref())
            .into_iter()
            .map(|index| index as u32)
            .collect())
    }
    pub fn query(&self, bounds: &[f64]) -> Result<Vec<u32>, String> {
        if bounds.len() != 4 {
            return Err("GanttHitIndex: query requires four bounds values".into());
        }
        Ok(self
            .inner
            .query(Bounds {
                left: bounds[0],
                top: bounds[1],
                right: bounds[2],
                bottom: bounds[3],
            })
            .into_iter()
            .map(|index| index as u32)
            .collect())
    }
}
#[wasm_bindgen(js_name=ganttPointToSegmentDistance)]
pub fn gantt_point_to_segment_distance(x: f64, y: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    ds_canvas::gantt_hit_index::point_to_segment_distance(x, y, ax, ay, bx, by)
}
#[wasm_bindgen(js_name=ganttPointToPolylineDistance)]
pub fn gantt_point_to_polyline_distance(points: &[f64], x: f64, y: f64) -> f64 {
    ds_canvas::gantt_hit_index::point_to_polyline_distance(points, x, y)
}
