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
}
