//! Numeric transport preserves nonfinite host text metrics as numeric facts.
use ds_canvas::profile_label_layout::{
    self as labels, Align, Baseline, MeasuredLabel, Options, OrderRequest, Orientation, Placement,
    Point,
};
use wasm_bindgen::prelude::*;

fn orientation(code: f64) -> Result<Orientation, JsError> {
    match code {
        0.0 => Ok(Orientation::Auto),
        1.0 => Ok(Orientation::Right),
        2.0 => Ok(Orientation::Left),
        3.0 => Ok(Orientation::Above),
        4.0 => Ok(Orientation::Below),
        5.0 => Ok(Orientation::Vertical),
        _ => Err(JsError::new("canvasProfileLabel: unknown orientation code")),
    }
}
fn placement_values(p: Placement) -> [f64; 5] {
    [
        p.x,
        p.y,
        match p.align {
            Align::Left => 0.0,
            Align::Right => 1.0,
            Align::Center => 2.0,
        },
        if p.baseline == Baseline::Bottom {
            0.0
        } else {
            1.0
        },
        p.rotation_rad,
    ]
}
/// Records [anchor x, anchor y, priority, locale collation rank, nonempty].
#[wasm_bindgen(js_name=canvasProfileLabelOrder)]
pub fn canvas_profile_label_order(
    records: &[f64],
    width: f64,
    height: f64,
) -> Result<Vec<u32>, JsError> {
    if !records.len().is_multiple_of(5) {
        return Err(JsError::new(
            "canvasProfileLabelOrder: expected five numbers per request",
        ));
    }
    let requests: Vec<_> = records
        .chunks_exact(5)
        .map(|r| OrderRequest {
            anchor: Point { x: r[0], y: r[1] },
            priority: r[2],
            collation_rank: r[3] as usize,
            enabled: r[4] != 0.0,
        })
        .collect();
    Ok(labels::label_order(&requests, width, height)
        .into_iter()
        .map(|index| index as u32)
        .collect())
}
#[wasm_bindgen(js_name=canvasProfileLabelPlacement)]
pub fn canvas_profile_label_placement(
    x: f64,
    y: f64,
    orientation_code: f64,
) -> Result<Vec<f64>, JsError> {
    Ok(placement_values(labels::placement(
        Point { x, y },
        orientation(orientation_code)?,
    ))
    .to_vec())
}
/// Ordered records [input index, anchor x/y, orientation, required, width, lines].
/// Options [viewport width/height, line/font height, padding, margin, cell size].
#[wasm_bindgen(js_name=canvasProfileLabelLayout)]
pub fn canvas_profile_label_layout(records: &[f64], options: &[f64]) -> Result<Vec<f64>, JsError> {
    if !records.len().is_multiple_of(7) || options.len() != 7 {
        return Err(JsError::new(
            "canvasProfileLabelLayout: expected seven-number records and options",
        ));
    }
    let requests: Result<Vec<_>, JsError> = records
        .chunks_exact(7)
        .map(|r| {
            Ok(MeasuredLabel {
                input_index: r[0] as usize,
                anchor: Point { x: r[1], y: r[2] },
                orientation: orientation(r[3])?,
                required: r[4] != 0.0,
                text_width: r[5],
                line_count: r[6] as usize,
            })
        })
        .collect();
    let options = Options {
        width: options[0],
        height: options[1],
        line_height: options[2],
        font_height: options[3],
        padding: options[4],
        margin: options[5],
        cell_size: options[6],
    };
    let mut out = Vec::new();
    for label in labels::layout_labels(&requests?, options) {
        out.push(label.input_index as f64);
        out.extend(placement_values(label.placement));
        out.extend([
            label.bounds.left,
            label.bounds.top,
            label.bounds.right,
            label.bounds.bottom,
        ]);
    }
    Ok(out)
}
