//! Bounded numeric transport adapters for native/WASM geometry consumers.
use crate::{Bounds, HostSurface, Point, Viewport, selection, viewport};
const MAX_POINTS: usize = 16_384;
fn exact(input: &[f64], size: usize) -> Result<(), String> {
    if input.len() == size {
        Ok(())
    } else {
        Err(format!("canvas buffer requires {size} numbers"))
    }
}
fn point(input: &[f64]) -> Point {
    Point::new(input[0], input[1])
}
fn view(input: &[f64]) -> Viewport {
    Viewport {
        zoom: input[0],
        pan_x: input[1],
        pan_y: input[2],
    }
}
fn view_buffer(v: Viewport) -> Vec<f64> {
    vec![v.zoom, v.pan_x, v.pan_y]
}
fn scene(input: &[f64]) -> Result<(Bounds, HostSurface, Viewport), String> {
    let bounds = Bounds {
        min_x: input[0],
        min_y: input[1],
        max_x: input[2],
        max_y: input[3],
    };
    if !input[..4].iter().all(|v| v.is_finite()) || bounds.width() < 0. || bounds.height() < 0. {
        return Err("canvas bounds must be finite and ordered".into());
    }
    Ok((
        bounds,
        HostSurface::new(input[4], input[5], 1.)?,
        view(&input[6..9]),
    ))
}
/// ABI v1: 0 projection, 1 pan, 2 centered zoom, 3 cursor zoom,
/// 4 fit, 5 center on scene point, 6 clamp zoom, 7 constants.
pub fn camera_buffer(operation: u8, input: &[f64]) -> Result<Vec<f64>, String> {
    match operation {
        0 => {
            exact(input, 9)?;
            let (b, h, v) = scene(input)?;
            let p = viewport::projection(&b, &h, &v);
            Ok(vec![p.scale, p.offset_x, p.offset_y])
        }
        1 => {
            exact(input, 5)?;
            Ok(view_buffer(viewport::pan(&view(input), input[3], input[4])))
        }
        2 => {
            exact(input, 4)?;
            Ok(view_buffer(viewport::zoom_centered(&view(input), input[3])))
        }
        3 | 5 => {
            exact(input, if operation == 3 { 12 } else { 11 })?;
            let (b, h, v) = scene(input)?;
            let p = point(&input[9..]);
            if !p.is_finite() {
                return Err("canvas anchor must be finite".into());
            }
            Ok(view_buffer(if operation == 3 {
                viewport::zoom_at(&b, &h, &v, p, input[11])
            } else {
                viewport::center_on(&b, &h, &v, p)
            }))
        }
        4 => {
            exact(input, 0)?;
            Ok(view_buffer(Viewport::fitted()))
        }
        6 => {
            exact(input, 1)?;
            Ok(vec![viewport::clamp_zoom(input[0])])
        }
        7 => {
            exact(input, 0)?;
            Ok(vec![
                viewport::MIN_ZOOM,
                viewport::MAX_ZOOM,
                viewport::FIT_PADDING_PX,
            ])
        }
        _ => Err("unknown canvas camera operation".into()),
    }
}
fn points(input: &[f64]) -> Result<Vec<Point>, String> {
    if !input.len().is_multiple_of(2)
        || input.len() > MAX_POINTS * 2
        || !input.iter().all(|v| v.is_finite())
    {
        return Err(format!(
            "canvas geometry requires finite XY pairs, at most {MAX_POINTS} points"
        ));
    }
    Ok(input.chunks_exact(2).map(point).collect())
}
/// ABI v1: 0 rectangle, 1 polygon contains point, 2 segments intersect,
/// 3 polyline intersects polygon, 4 point/segment distance, 5 drag threshold.
pub fn selection_buffer(operation: u8, a: &[f64], b: &[f64]) -> Result<Vec<f64>, String> {
    let a = points(a)?;
    let b = points(b)?;
    let boolean = |v: bool| Ok(vec![if v { 1. } else { 0. }]);
    match operation {
        0 if a.len() == 1 && b.len() == 1 => Ok(selection::rectangle(a[0], b[0])
            .iter()
            .flat_map(|p| [p.x, p.y])
            .collect()),
        1 if b.len() == 1 => boolean(selection::point_in_polygon(&a, b[0])),
        2 if a.len() == 2 && b.len() == 2 => {
            boolean(selection::segments_intersect(a[0], a[1], b[0], b[1]))
        }
        // Bound the pairwise lasso work as well as transport size.
        3 if a.len().saturating_mul(b.len()) <= 1_048_576 => {
            boolean(selection::polyline_intersects_polygon(&a, &b))
        }
        4 if a.len() == 1 && b.len() == 2 => {
            Ok(vec![selection::distance_to_segment(a[0], b[0], b[1])])
        }
        5 if a.len() == 1 && b.len() == 1 => boolean(crate::drag_started(a[0], b[0])),
        _ => Err("invalid canvas selection operation, shape or work limit".into()),
    }
}
/// 0 constrained axis drag [start,anchor,current,units,min,max];
/// 1 drag threshold [start_x,start_y,current_x,current_y,threshold_css_px].
pub fn gesture_buffer(operation: u8, input: &[f64]) -> Result<Vec<f64>, String> {
    match operation {
        0 => {
            exact(input, 6)?;
            Ok(crate::measured_path::axis_drag(
                input[0], input[1], input[2], input[3], input[4], input[5],
            )?
            .to_vec())
        }
        1 => {
            exact(input, 5)?;
            if !input.iter().all(|v| v.is_finite()) || input[4] < 0. {
                return Err(
                    "drag threshold requires finite positions and a nonnegative tolerance".into(),
                );
            }
            Ok(vec![
                if point(input).distance_to(point(&input[2..])) >= input[4] {
                    1.
                } else {
                    0.
                },
            ])
        }
        _ => Err("unknown canvas gesture operation".into()),
    }
}
