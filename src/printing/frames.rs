//! Parent-relative print hierarchy. Flat V2 coordinates are adapted only here.
use super::{ElementKind, Layout};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TableFrame {
    pub rect_mm: [f64; 4],
    #[serde(default)]
    pub panels_mm: Vec<[f64; 4]>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TableContainer {
    pub element_id: String,
    /// Relative to the printable border, never to the paper or another table.
    pub rect_mm: [f64; 4],
    pub table: TableFrame,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct BorderFrame {
    /// Relative to map_space. The border clips all descendants.
    pub rect_mm: [f64; 4],
    pub tables: Vec<TableContainer>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MapSpace {
    pub rect_mm: [f64; 4],
    pub border: BorderFrame,
}

pub fn hierarchy(layout: &Layout) -> MapSpace {
    let boundary = super::sheet_margin::boundary(layout);
    MapSpace {
        rect_mm: [0., 0., layout.page_mm[0], layout.page_mm[1]],
        border: BorderFrame {
            rect_mm: boundary,
            tables: layout
                .elements
                .iter()
                .filter(|e| e.kind == ElementKind::Table)
                .map(|e| TableContainer {
                    element_id: e.id.clone(),
                    rect_mm: [
                        e.rect[0] - boundary[0],
                        e.rect[1] - boundary[1],
                        e.rect[2],
                        e.rect[3],
                    ],
                    table: TableFrame {
                        rect_mm: [0., 0., e.rect[2], e.rect[3]],
                        panels_mm: vec![],
                    },
                })
                .collect(),
        },
    }
}

/// A compact schematic before the native renderer has measured real rows.
/// These estimates are never print/export geometry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DraftTable {
    pub rect_mm: [f64; 4],
    pub column_widths_mm: Vec<f64>,
    pub row_mm: f64,
    pub font_mm: f64,
    pub sample_rows: usize,
}
pub fn drafts(layout: &Layout) -> Result<BTreeMap<String, DraftTable>, String> {
    let mut drafts = BTreeMap::new();
    for e in &layout.elements {
        let Some(table) = &e.table else { continue };
        let font_mm = e.font_pt * 25.4 / 72.;
        let Ok(rows) = super::table_rows::plan(
            &e.id,
            6,
            e.rect[3],
            e.font_pt,
            table.row_mm,
            table.panels,
            false,
            super::TableOverflow::TruncateWarn,
        ) else {
            continue;
        };
        let widths = table
            .headings
            .iter()
            .enumerate()
            .map(|(i, heading)| {
                // Font metrics belong to Reporter. The editor schematic reserves
                // approximate heading width and never stretches to a max container.
                let natural = (heading.chars().count() as f64 * font_mm * 0.55 + 2.).max(8.);
                let min = table.min_widths_mm.get(i).copied().unwrap_or(0.);
                let max = table
                    .max_widths_mm
                    .get(i)
                    .copied()
                    .filter(|v| *v > 0.)
                    .unwrap_or(f64::INFINITY);
                natural.max(min).min(max)
            })
            .collect::<Vec<_>>();
        let total = widths.iter().sum::<f64>();
        let factor = (e.rect[2] / total).min(1.);
        let widths = widths.into_iter().map(|w| w * factor).collect::<Vec<_>>();
        let height = (rows.visible_rows + 2) as f64 * rows.step_mm;
        let Ok(rect) = super::sheet_margin::place_in(
            e,
            [e.rect[0], e.rect[1], total * factor, height],
            super::sheet_margin::boundary(layout),
        ) else {
            continue;
        };
        drafts.insert(
            e.id.clone(),
            DraftTable {
                rect_mm: rect,
                column_widths_mm: widths,
                row_mm: rows.step_mm,
                font_mm,
                sample_rows: rows.visible_rows,
            },
        );
    }
    Ok(drafts)
}

/// Pick real stepped furniture, without making its empty envelope selectable.
pub fn hit(
    layout: &Layout,
    measured: &BTreeMap<String, Vec<[f64; 4]>>,
    point: [f64; 2],
) -> Result<Option<usize>, String> {
    if point.iter().any(|v| !v.is_finite())
        || measured.len() > 500
        || measured.values().map(Vec::len).sum::<usize>() > 2000
        || measured
            .values()
            .flatten()
            .any(|r| r.iter().any(|v| !v.is_finite()) || r[2] < 0. || r[3] < 0.)
    {
        return Err("invalid or unbounded measured furniture frames".into());
    }
    let inside = |r: &[f64; 4]| {
        point[0] >= r[0] && point[1] >= r[1] && point[0] <= r[0] + r[2] && point[1] <= r[1] + r[3]
    };
    if !inside(&super::sheet_margin::boundary(layout)) {
        return Ok(None);
    }
    Ok(layout.elements.iter().enumerate().rev().find_map(|(i, e)| {
        if e.id == "page-border" {
            return None;
        }
        let found = measured
            .get(&e.id)
            .map(|parts| parts.iter().any(inside))
            .unwrap_or_else(|| inside(&e.rect));
        found.then_some(i)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn tables_have_independent_parent_relative_containers() {
        let mut layout = super::super::default_layout();
        for document in [
            json!({"id":"page-border","kind":"rectangle","rect":[10,10,400,277]}),
            json!({"id":"poles","kind":"table","rect":[15,15,120,267]}),
            json!({"id":"customers","kind":"table","rect":[140,15,140,267]}),
        ] {
            layout
                .elements
                .push(serde_json::from_value(document).unwrap());
        }
        let before = hierarchy(&layout);
        assert_eq!(before.border.tables[0].rect_mm, [5., 5., 120., 267.]);
        assert_eq!(before.border.tables[0].table.rect_mm, [0., 0., 120., 267.]);
        layout
            .elements
            .iter_mut()
            .find(|e| e.id == "customers")
            .unwrap()
            .rect = [150., 20., 130., 262.];
        let after = hierarchy(&layout);
        assert_eq!(before.border.tables[0], after.border.tables[0]);
        assert_eq!(after.border.tables[1].rect_mm, [140., 10., 130., 262.]);
    }
    #[test]
    fn hit_uses_stepped_parts_and_cannot_escape_the_border() {
        let mut layout = super::super::default_layout();
        layout.elements.clear();
        for document in [
            json!({"id":"page-border","kind":"rectangle","rect":[10,10,400,277]}),
            json!({"id":"poles","kind":"table","rect":[5,5,120,267]}),
        ] {
            layout
                .elements
                .push(serde_json::from_value(document).unwrap());
        }
        let measured = BTreeMap::from([(
            "poles".into(),
            vec![[15., 15., 30., 250.], [50., 15., 30., 100.]],
        )]);
        assert_eq!(hit(&layout, &measured, [20., 200.]).unwrap(), Some(1));
        assert_eq!(hit(&layout, &measured, [60., 200.]).unwrap(), None);
        assert_eq!(hit(&layout, &BTreeMap::new(), [7., 7.]).unwrap(), None);
        assert!(hit(&layout, &measured, [f64::NAN, 0.]).is_err());
    }
    #[test]
    fn draft_tables_do_not_stretch_to_maximum_container_size() {
        let mut layout = super::super::default_layout();
        layout.elements.push(
            serde_json::from_value(json!({
                "id":"poles","kind":"table","rect":[15,15,420,811],"font_pt":5.5,
                "table":{"layer":"poles","columns":["id","x","y"],"headings":["Pole","X","Y"],
                "widths":[1,1,1],"panels":3,"row_mm":3.5,"align":"left"}
            }))
            .unwrap(),
        );
        let draft = drafts(&layout).unwrap().remove("poles").unwrap();
        assert!(draft.rect_mm[2] < 50.);
        assert_eq!(draft.rect_mm[3], 28.);
        assert_eq!(draft.row_mm, 3.5);
        assert_eq!(layout.elements.last().unwrap().rect, [15., 15., 420., 811.]);
        // A temporarily too-small container remains editable. Only actual
        // printing refuses unreadable geometry; the schematic is optional.
        layout.elements.last_mut().unwrap().rect[3] = 1.;
        assert!(!drafts(&layout).unwrap().contains_key("poles"));
    }
}
