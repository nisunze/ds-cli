//! What a newly added page element is, and which governed print styles a
//! print layer may bind — decided once for every host.
//!
//! The Printing setup page used to build a new text, frame, table, legend or
//! logo element from literals of its own, and to decide which `_print` style
//! documents its layer picker offered by a filename suffix. Both are document
//! defaults: the kernel already owns every other default of an element, and
//! `ds report layout new|edit` could add one only by retyping every field.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    Element, ElementKind, Layout, LegendBinding, PrintViewport, TableBinding, TableOverflow,
};

/// The rectangle every new element starts in, in page millimetres.
pub const NEW_ELEMENT_RECT: [f64; 4] = [15., 15., 60., 18.];
/// A logo starts smaller: it is an image, not a text frame.
pub const NEW_LOGO_RECT: [f64; 4] = [15., 15., 35., 20.];
/// The table a new table element binds to until the author changes it.
pub const NEW_TABLE_LAYER: &str = "lv_print_info";
pub const NEW_TABLE_COLUMNS: [&str; 3] = ["Description", "Unit", "Quantity"];
pub const NEW_TABLE_WIDTHS: [f64; 3] = [6., 1., 2.];

/// The canonical new element of a kind. `asset` names the layout asset a
/// logo shows; other kinds ignore it.
pub fn new_element(kind: ElementKind, id: String, asset: &str) -> Element {
    let is_logo = kind == ElementKind::Logo;
    Element {
        id,
        kind,
        align: super::TextAlign::Left,
        text_style: None,
        rect: if is_logo {
            NEW_LOGO_RECT
        } else {
            NEW_ELEMENT_RECT
        },
        text: if kind == ElementKind::Text {
            "Text".into()
        } else {
            String::new()
        },
        asset: if is_logo { asset.into() } else { String::new() },
        font_pt: super::font(),
        color: super::ink(),
        fill: super::white(),
        opacity: super::opaque(),
        stroke_mm: None,
        table: (kind == ElementKind::Table).then(|| TableBinding {
            phase: None,
            workbook: None,
            layer: NEW_TABLE_LAYER.into(),
            additional_layers: Vec::new(),
            columns: NEW_TABLE_COLUMNS.iter().map(|c| c.to_string()).collect(),
            headings: NEW_TABLE_COLUMNS.iter().map(|c| c.to_string()).collect(),
            widths: NEW_TABLE_WIDTHS.to_vec(),
            column_sizing: super::ColumnSizing::Content,
            max_widths_mm: Vec::new(),
            min_widths_mm: Vec::new(),
            measure_headings: super::table_measure_headings_default(),
            heading_mode: Default::default(),
            value_key: None,
            overflow: TableOverflow::TruncateWarn,
            panels: super::one_panel(),
            panel_rows: Vec::new(),
            gap_mm: super::panel_gap(),
            sort_by: Vec::new(),
            row_mm: None,
            align: Default::default(),
            presentation: Default::default(),
        }),
        // A legend carries its binding from the start, so an inspector reads
        // the values the document holds rather than guessing them.
        legend: (kind == ElementKind::Legend).then(|| LegendBinding {
            align: Default::default(),
            indent_mm: super::legend_indent(),
            layers: Vec::new(),
            titles: Default::default(),
            categorical: true,
            row_mm: super::legend_row(),
        }),
        // A map element is only valid with its print viewport, so a new one
        // carries the primary slot the page's own default layout uses.
        viewport: (kind == ElementKind::Map).then(|| PrintViewport {
            name: "Primary map".into(),
            view_slot: "primary_map".into(),
            clip: true,
            fit_around: Vec::new(),
            graticule: None,
        }),
        flow: None,
        sheet_anchor: Default::default(),
    }
}

fn kind_token(kind: ElementKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "element".into())
}

/// Add the canonical new element of `kind` to the layout. Without an `id`
/// the kernel mints `<kind>-<n>`, the first such id the layout does not
/// already hold. Returns the layout and the new element's index.
pub fn add(
    mut layout: Layout,
    kind: ElementKind,
    id: Option<String>,
    asset: &str,
) -> Result<(Layout, usize), String> {
    let id = match id {
        Some(id) if !id.trim().is_empty() => {
            if layout.elements.iter().any(|e| e.id == id) {
                return Err(format!("page element `{id}` already exists"));
            }
            id
        }
        _ => {
            let token = kind_token(kind);
            let held: BTreeSet<&str> = layout.elements.iter().map(|e| e.id.as_str()).collect();
            (1..)
                .map(|n| format!("{token}-{n}"))
                .find(|candidate| !held.contains(candidate.as_str()))
                .expect("an unused ordinal exists")
        }
    };
    if kind == ElementKind::Logo && !layout.assets.contains_key(asset) {
        return Err("a logo needs an asset the layout holds".into());
    }
    layout.elements.push(new_element(kind, id, asset));
    super::validate(&layout)?;
    let index = layout.elements.len() - 1;
    Ok((layout, index))
}

/// One governed style document as the host's style catalogue describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StyleFacts {
    pub style_ref: String,
    #[serde(default)]
    pub style_target: String,
}

/// Which governed print styles a print layer may bind: the catalogue's
/// `print` targets, which are the governed `_print` clones, in stable order.
pub fn style_ref_choices(catalogue: &[StyleFacts]) -> Vec<String> {
    let mut refs: Vec<String> = catalogue
        .iter()
        .filter(|s| s.style_target == "print" && s.style_ref.ends_with("_print"))
        .map(|s| s.style_ref.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // The page ordered with `localeCompare`: case-insensitive first, then
    // exact, which is what a picker of `_print` names reads naturally.
    refs.sort_by_key(|r| (r.to_lowercase(), r.clone()));
    refs
}

/// Bind a print layer to a governed print style, or unbind it with an empty
/// reference. A reference the catalogue does not offer is refused.
pub fn set_layer_style_ref(
    mut layout: Layout,
    layer_id: &str,
    style_ref: &str,
    catalogue: &[StyleFacts],
) -> Result<Layout, String> {
    let layer_id = layer_id.trim();
    if layer_id.is_empty() || layer_id.len() > 128 || layer_id.chars().any(char::is_control) {
        return Err("layer_id must be one bounded identifier".into());
    }
    let style_ref = style_ref.trim();
    if style_ref.is_empty() {
        layout.style_refs.remove(layer_id);
    } else {
        if !style_ref_choices(catalogue)
            .iter()
            .any(|choice| choice == style_ref)
        {
            return Err(format!(
                "printing_style_ref_ineligible: `{style_ref}` is not a governed print style"
            ));
        }
        layout.style_refs.insert(layer_id.into(), style_ref.into());
    }
    super::validate(&layout)?;
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printing::default_layout;

    fn kinds() -> [ElementKind; 8] {
        [
            ElementKind::Map,
            ElementKind::Text,
            ElementKind::Legend,
            ElementKind::ScaleBar,
            ElementKind::NorthArrow,
            ElementKind::Logo,
            ElementKind::Rectangle,
            ElementKind::Table,
        ]
    }

    #[test]
    fn every_kind_starts_in_the_same_frame_with_the_same_ink() {
        for kind in kinds() {
            let e = new_element(kind, "x".into(), "logo-1");
            let rect = if kind == ElementKind::Logo {
                NEW_LOGO_RECT
            } else {
                NEW_ELEMENT_RECT
            };
            assert_eq!(e.rect, rect, "{kind:?}");
            assert_eq!((e.font_pt, e.opacity), (10., 1.), "{kind:?}");
            assert_eq!((e.color.as_str(), e.fill.as_str()), ("#253747", "#ffffff"));
            assert_eq!(
                e.text,
                if kind == ElementKind::Text {
                    "Text"
                } else {
                    ""
                }
            );
            assert_eq!(
                e.asset,
                if kind == ElementKind::Logo {
                    "logo-1"
                } else {
                    ""
                }
            );
            assert_eq!(e.viewport.is_some(), kind == ElementKind::Map);
            assert!(e.flow.is_none());
        }
    }

    #[test]
    fn a_table_binds_the_info_table_and_a_legend_carries_its_binding() {
        let table = new_element(ElementKind::Table, "t".into(), "")
            .table
            .unwrap();
        assert_eq!(table.layer, NEW_TABLE_LAYER);
        assert_eq!(table.columns, ["Description", "Unit", "Quantity"]);
        assert_eq!(table.headings, table.columns);
        assert_eq!(table.widths, [6., 1., 2.]);
        assert_eq!(table.overflow, TableOverflow::TruncateWarn);
        assert_eq!(table.panels, 1);
        let legend = new_element(ElementKind::Legend, "l".into(), "")
            .legend
            .unwrap();
        assert!(legend.categorical && legend.layers.is_empty());
        assert_eq!((legend.indent_mm, legend.row_mm), (4., 6.));
        assert!(new_element(ElementKind::Text, "x".into(), "")
            .table
            .is_none());
        assert!(new_element(ElementKind::Text, "x".into(), "")
            .legend
            .is_none());
    }

    #[test]
    fn add_mints_the_first_free_ordinal_id_and_refuses_a_duplicate() {
        let layout = default_layout();
        let (layout, index) = add(layout, ElementKind::Text, None, "").unwrap();
        assert_eq!(index, layout.elements.len() - 1);
        assert_eq!(layout.elements[index].id, "text-1");
        let (layout, _) = add(layout, ElementKind::Text, None, "").unwrap();
        assert_eq!(layout.elements.last().unwrap().id, "text-2");
        let (layout, _) = add(layout, ElementKind::ScaleBar, Some("bar".into()), "").unwrap();
        assert_eq!(layout.elements.last().unwrap().id, "bar");
        assert!(add(layout, ElementKind::ScaleBar, Some("bar".into()), "").is_err());
    }

    #[test]
    fn a_logo_needs_an_asset_the_layout_holds() {
        let layout = default_layout();
        assert!(add(layout.clone(), ElementKind::Logo, None, "missing").is_err());
        let mut with_asset = layout;
        with_asset.assets.insert(
            "logo-a".into(),
            super::super::Asset {
                label: "a.png".into(),
                png_data_url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ"
                    .into(),
            },
        );
        let (layout, index) = add(with_asset, ElementKind::Logo, None, "logo-a").unwrap();
        assert_eq!(layout.elements[index].asset, "logo-a");
        assert_eq!(layout.elements[index].rect, NEW_LOGO_RECT);
    }

    fn catalogue() -> Vec<StyleFacts> {
        let facts = |r: &str, t: &str| StyleFacts {
            style_ref: r.into(),
            style_target: t.into(),
        };
        vec![
            facts("lv_lines_print", "print"),
            facts("lv_lines", "design"),
            facts("customers_print", "print"),
            facts("odd_print_target", "print"),
            facts("poles_print", "design"),
            facts("customers_print", "print"),
            facts("Z_print", "print"),
            facts("a_print", "print"),
        ]
    }

    #[test]
    fn print_choices_are_the_print_targets_named_as_print_clones_in_order() {
        assert_eq!(
            style_ref_choices(&catalogue()),
            vec!["a_print", "customers_print", "lv_lines_print", "Z_print"]
        );
    }

    #[test]
    fn a_layer_binds_only_an_offered_print_style_and_unbinds_with_nothing() {
        let layout = default_layout();
        let bound = set_layer_style_ref(layout.clone(), "lv_lines", "lv_lines_print", &catalogue())
            .unwrap();
        assert_eq!(bound.style_refs["lv_lines"], "lv_lines_print");
        assert!(
            set_layer_style_ref(layout.clone(), "lv_lines", "poles_print", &catalogue())
                .unwrap_err()
                .starts_with("printing_style_ref_ineligible")
        );
        let unbound = set_layer_style_ref(bound, "lv_lines", "", &catalogue()).unwrap();
        assert!(!unbound.style_refs.contains_key("lv_lines"));
        assert!(set_layer_style_ref(layout, " ", "lv_lines_print", &catalogue()).is_err());
    }
}
