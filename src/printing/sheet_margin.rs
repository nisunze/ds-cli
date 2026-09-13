//! Measured furniture positioning, shared by print hosts and public schemas.
use super::{Element, ElementKind, TableAlignment};
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
    if element.flow.is_some()
        || element.sheet_anchor == SheetAnchor::Authored
        || !matches!(element.kind, ElementKind::Table | ElementKind::Legend)
    {
        return Ok(measured);
    }
    let [_, _, width, height] = measured;
    if width > page[0] - 2. * MARGIN_MM || height > page[1] - 2. * MARGIN_MM {
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
            (right, element.rect[1] >= page[1] / 2.)
        }
        SheetAnchor::Authored => unreachable!(),
    };
    Ok([
        if right {
            page[0] - MARGIN_MM - width
        } else {
            MARGIN_MM
        },
        if bottom {
            page[1] - MARGIN_MM - height
        } else {
            MARGIN_MM
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
