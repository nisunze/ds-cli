//! Measured furniture positioning, shared by print hosts and public schemas.
use super::{Element, ElementKind, Layout, TableAlignment};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MARGIN_MM: f64 = 5.;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SheetAnchor {
    /// Use table/legend alignment and the authored frame’s upper/lower page half.
    #[default]
    Auto,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// Explicit free composition, retaining authored coordinates.
    Authored,
}

pub fn is_auto(anchor: &SheetAnchor) -> bool {
    *anchor == SheetAnchor::Auto
}

pub fn place(element: &Element, measured: [f64; 4], page: [f64; 2]) -> Result<[f64; 4], String> {
    place_in(element, measured, [0., 0., page[0], page[1]])
}

/// The canonical drawn sheet border owns the printable domain. Layouts without
/// it retain the physical page as their domain (titles outside a map are legal).
pub fn boundary(layout: &Layout) -> [f64; 4] {
    layout
        .elements
        .iter()
        .find(|e| e.id == "page-border" && e.kind == ElementKind::Rectangle)
        .map(|e| e.rect)
        .unwrap_or([0., 0., layout.page_mm[0], layout.page_mm[1]])
}

pub fn inset(boundary: [f64; 4], margin: f64) -> [f64; 4] {
    [
        boundary[0] + margin,
        boundary[1] + margin,
        boundary[2] - 2. * margin,
        boundary[3] - 2. * margin,
    ]
}

pub fn place_in(
    element: &Element,
    measured: [f64; 4],
    boundary: [f64; 4],
) -> Result<[f64; 4], String> {
    if element.flow.is_some()
        || element.sheet_anchor == SheetAnchor::Authored
        || !matches!(element.kind, ElementKind::Table | ElementKind::Legend)
    {
        return Ok(measured);
    }
    let [_, _, width, height] = measured;
    let area = inset(boundary, MARGIN_MM);
    if width > area[2] || height > area[3] {
        return Err(format!(
            "{} cannot fit inside the 5 mm sheet margin; adjust panels, frame or type size",
            element.id
        ));
    }
    let (right, bottom) = match element.sheet_anchor {
        SheetAnchor::TopLeft => (false, false),
        SheetAnchor::TopRight => (true, false),
        SheetAnchor::BottomLeft => (false, true),
        SheetAnchor::BottomRight => (true, true),
        SheetAnchor::Auto => {
            let right = element
                .table
                .as_ref()
                .map(|t| &t.align)
                .or_else(|| element.legend.as_ref().map(|l| &l.align))
                .is_none_or(|align| *align == TableAlignment::Right);
            // The authored origin chooses the vertical side. Reshaping or
            // changing row counts cannot flip a tall schedule to the bottom.
            (right, element.rect[1] >= boundary[1] + boundary[3] / 2.)
        }
        SheetAnchor::Authored => unreachable!(),
    };
    Ok([
        if right {
            area[0] + area[2] - width
        } else {
            area[0]
        },
        if bottom {
            area[1] + area[3] - height
        } else {
            area[1]
        },
        width,
        height,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn measured_edge_ignores_unused_frame_and_stays_stable_as_rows_change() {
        let mut e: Element = serde_json::from_value(json!({"id":"schedule", "kind":"table", "rect":[250,20,160,100], "table":{"layer":"poles","columns":["id"],"headings":["Id"],"widths":[1]}})).unwrap();
        assert_eq!(
            place(&e, [340., 20., 70., 40.], [420., 297.]).unwrap(),
            [345., 5., 70., 40.]
        );
        assert_eq!(
            place(&e, [340., 20., 70., 80.], [420., 297.]).unwrap(),
            [345., 5., 70., 80.]
        );
        e.sheet_anchor = SheetAnchor::BottomLeft;
        assert_eq!(
            place(&e, [340., 20., 70., 40.], [420., 297.]).unwrap(),
            [5., 252., 70., 40.]
        );
        e.sheet_anchor = SheetAnchor::Authored;
        assert_eq!(
            place(&e, [340., 20., 70., 40.], [420., 297.]).unwrap(),
            [340., 20., 70., 40.]
        );
    }
}
