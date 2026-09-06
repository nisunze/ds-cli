//! Print-layout authority shared by CLI, Canvas2D preview and vector delivery.
//! Flat page elements in physical millimetres; no tile, HTML or remote-image IO.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.print-layout/v1";
pub const MAX_LAYOUT_BYTES: usize = 750_000;
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub page_mm: [f64; 2],
    pub elements: Vec<Element>,
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
    #[serde(default)]
    pub styles: BTreeMap<String, PrintStyle>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    Map,
    Text,
    Legend,
    ScaleBar,
    NorthArrow,
    Logo,
    Rectangle,
    Table,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableBinding {
    pub layer: String,
    /// Ordered property keys; headings and widths follow the same order.
    pub columns: Vec<String>,
    pub headings: Vec<String>,
    pub widths: Vec<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub id: String,
    pub kind: ElementKind,
    pub rect: [f64; 4],
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub asset: String,
    #[serde(default = "font")]
    pub font_pt: f64,
    #[serde(default = "ink")]
    pub color: String,
    #[serde(default = "white")]
    pub fill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableBinding>,
}
fn font() -> f64 {
    10.
}
fn ink() -> String {
    "#253747".into()
}
fn white() -> String {
    "#ffffff".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub label: String,
    pub png_data_url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintStyle {
    pub stroke: String,
    pub width_mm: f64,
    #[serde(default = "white")]
    pub fill: String,
    #[serde(default)]
    pub casing: String,
    #[serde(default)]
    pub casing_mm: f64,
    #[serde(default)]
    pub dash_mm: Vec<f64>,
    #[serde(default = "radius")]
    pub radius_mm: f64,
    #[serde(default)]
    pub label_field: String,
    #[serde(default = "font")]
    pub label_pt: f64,
    #[serde(default)]
    pub color_field: String,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub width_field: String,
    #[serde(default)]
    pub widths: BTreeMap<String, f64>,
}
fn radius() -> f64 {
    1.2
}
impl Default for PrintStyle {
    fn default() -> Self {
        Self {
            stroke: ink(),
            width_mm: 0.35,
            fill: white(),
            casing: String::new(),
            casing_mm: 0.,
            dash_mm: vec![],
            radius_mm: radius(),
            label_field: String::new(),
            label_pt: 8.,
            color_field: String::new(),
            colors: BTreeMap::new(),
            width_field: String::new(),
            widths: BTreeMap::new(),
        }
    }
}
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|c| c.is_ascii_hexdigit())
}
fn bounded(value: f64, low: f64, high: f64) -> bool {
    value.is_finite() && (low..=high).contains(&value)
}
pub fn validate(layout: &Layout) -> Result<(), String> {
    if layout.schema != SCHEMA
        || !identifier(&layout.id)
        || layout.name.trim().is_empty()
        || layout.name.len() > 160
    {
        return Err("invalid print layout identity".into());
    }
    if serde_json::to_vec(layout).map_err(|e| e.to_string())?.len() > MAX_LAYOUT_BYTES {
        return Err("print layout exceeds 750 KB including logos".into());
    }
    if layout.page_mm.iter().any(|v| !bounded(*v, 50., 2000.))
        || layout.elements.is_empty()
        || layout.elements.len() > 64
    {
        return Err("invalid page dimensions or element count".into());
    }
    let mut ids = BTreeSet::new();
    let mut maps = 0;
    for element in &layout.elements {
        if !identifier(&element.id) || !ids.insert(&element.id) {
            return Err("page element ids must be unique".into());
        }
        let [x, y, w, h] = element.rect;
        if !bounded(x, 0., layout.page_mm[0])
            || !bounded(y, 0., layout.page_mm[1])
            || !bounded(w, 1., layout.page_mm[0])
            || !bounded(h, 1., layout.page_mm[1])
            || x + w > layout.page_mm[0] + 1e-7
            || y + h > layout.page_mm[1] + 1e-7
        {
            return Err(format!("element {} lies outside the page", element.id));
        }
        if !bounded(element.font_pt, 4., 144.)
            || !color(&element.color)
            || !color(&element.fill)
            || element.text.len() > 4000
        {
            return Err("invalid element typography or color".into());
        }
        if element.kind == ElementKind::Map {
            maps += 1;
        }
        if element.kind == ElementKind::Table {
            let table = element.table.as_ref().ok_or("table needs a data binding")?;
            if !identifier(&table.layer)
                || table.columns.is_empty()
                || table.columns.len() > 12
                || table.columns.len() != table.headings.len()
                || table.columns.len() != table.widths.len()
                || table.columns.iter().any(|v| v.is_empty() || v.len() > 160)
                || table.headings.iter().any(|v| v.len() > 160)
                || table.widths.iter().any(|v| !bounded(*v, 0.1, 1000.))
            {
                return Err("invalid table binding, headings or column widths".into());
            }
        } else if element.table.is_some() {
            return Err("only table elements can bind tabular data".into());
        }
        if element.kind == ElementKind::Logo && !layout.assets.contains_key(&element.asset) {
            return Err(format!(
                "logo {} needs a held project/global asset",
                element.id
            ));
        }
    }
    if maps != 1 {
        return Err("a layout must have exactly one map frame".into());
    }
    if layout.assets.len() > 12 || layout.styles.len() > 100 {
        return Err("too many assets or layer styles".into());
    }
    for (id, asset) in &layout.assets {
        use base64::Engine;
        if !identifier(id) || asset.label.len() > 160 {
            return Err("invalid logo identity".into());
        }
        let data = asset
            .png_data_url
            .strip_prefix("data:image/png;base64,")
            .ok_or("logos must be held PNG bytes, never remote URLs")?;
        if data.len() > 350_000 {
            return Err("logo exceeds 256 KiB; resize it before saving".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| "invalid PNG logo")?;
        if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
            return Err("invalid PNG logo header".into());
        }
        for chunk in [&bytes[16..20], &bytes[20..24]] {
            let n = u32::from_be_bytes(chunk.try_into().expect("four bytes"));
            if n == 0 || n > 4096 {
                return Err("logo dimensions exceed 4096 pixels".into());
            }
        }
    }
    for (id, style) in &layout.styles {
        if !identifier(id)
            || !color(&style.stroke)
            || !color(&style.fill)
            || (!style.casing.is_empty() && !color(&style.casing))
            || !bounded(style.width_mm, 0.05, 10.)
            || !bounded(style.casing_mm, 0., 10.)
            || !bounded(style.radius_mm, 0.1, 20.)
            || !bounded(style.label_pt, 4., 40.)
            || style.dash_mm.len() > 8
            || style.dash_mm.iter().any(|v| !bounded(*v, 0.1, 100.))
            || style.colors.len() > 50
            || style.widths.len() > 50
            || style.colors.values().any(|c| !color(c))
            || style.widths.values().any(|v| !bounded(*v, 0.05, 10.))
        {
            return Err(format!("invalid print style {id}"));
        }
        if [&style.color_field, &style.width_field, &style.label_field]
            .iter()
            .any(|f| f.len() > 120)
            || style
                .colors
                .keys()
                .chain(style.widths.keys())
                .any(|v| v.len() > 200)
        {
            return Err("style field or category is too long".into());
        }
    }
    Ok(())
}
pub fn default_layout() -> Layout {
    let element = |id: &str, kind, rect, text: &str, font_pt| Element {
        table: None,
        id: id.into(),
        kind,
        rect,
        text: text.into(),
        asset: String::new(),
        font_pt,
        color: ink(),
        fill: white(),
    };
    Layout {
        schema: SCHEMA.into(),
        id: "network-map".into(),
        name: "Network map · A3 landscape".into(),
        page_mm: [420., 297.],
        assets: BTreeMap::new(),
        styles: BTreeMap::new(),
        elements: vec![
            element("map", ElementKind::Map, [12., 32., 300., 235.], "", 10.),
            element(
                "title",
                ElementKind::Text,
                [12., 10., 396., 15.],
                "{title}",
                22.,
            ),
            element(
                "legend",
                ElementKind::Legend,
                [321., 46., 87., 120.],
                "LEGEND",
                10.,
            ),
            element(
                "north",
                ElementKind::NorthArrow,
                [377., 183., 22., 30.],
                "",
                10.,
            ),
            element(
                "scale",
                ElementKind::ScaleBar,
                [20., 250., 78., 12.],
                "",
                8.,
            ),
            element(
                "project",
                ElementKind::Text,
                [321., 223., 87., 36.],
                "{project}\n{drawing}\n{date}",
                10.,
            ),
            element(
                "footer",
                ElementKind::Text,
                [12., 276., 396., 9.],
                "{source}",
                8.,
            ),
        ],
    }
}
#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
pub enum Paper {
    A0,
    A1,
    A2,
    #[default]
    A3,
    A4,
    A5,
}
#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Portrait,
    #[default]
    Landscape,
}

pub fn named_layout(
    id: Option<String>,
    name: Option<String>,
    paper: Paper,
    orientation: Orientation,
) -> Result<Layout, String> {
    let [short, long] = match paper {
        Paper::A0 => [841., 1189.],
        Paper::A1 => [594., 841.],
        Paper::A2 => [420., 594.],
        Paper::A3 => [297., 420.],
        Paper::A4 => [210., 297.],
        Paper::A5 => [148., 210.],
    };
    let page = match orientation {
        Orientation::Portrait => [short, long],
        Orientation::Landscape => [long, short],
    };
    let mut layout = default_layout();
    // One authored composition is fitted uniformly; paper choices never stretch symbols or type.
    let scale = f64::min(page[0] / layout.page_mm[0], page[1] / layout.page_mm[1]);
    let dx = (page[0] - layout.page_mm[0] * scale) / 2.;
    let dy = (page[1] - layout.page_mm[1] * scale) / 2.;
    for e in &mut layout.elements {
        e.rect = [
            dx + e.rect[0] * scale,
            dy + e.rect[1] * scale,
            e.rect[2] * scale,
            e.rect[3] * scale,
        ];
        e.font_pt = (e.font_pt * scale).clamp(4., 144.);
    }
    layout.page_mm = page;
    if let Some(id) = id {
        layout.id = id;
    }
    if let Some(name) = name {
        layout.name = name;
    }
    validate(&layout)?;
    Ok(layout)
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    New {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        paper: Paper,
        #[serde(default)]
        orientation: Orientation,
    },
    Rename {
        layout: Layout,
        name: String,
    },
    Duplicate {
        layout: Layout,
        id: String,
        name: String,
    },
    Validate {
        layout: Layout,
    },
    Edit {
        layout: Layout,
        element: Element,
    },
    Remove {
        layout: Layout,
        id: String,
    },
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("print input exceeds 32 MiB".into());
    }
    let command: Command =
        serde_json::from_slice(input).map_err(|e| format!("invalid printing request: {e}"))?;
    let value = match command {
        Command::New {
            id,
            name,
            paper,
            orientation,
        } => json!({"layout":named_layout(id,name,paper,orientation)?}),
        Command::Rename { mut layout, name } => {
            layout.name = name;
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::Duplicate {
            mut layout,
            id,
            name,
        } => {
            if layout.id == id {
                return Err("A duplicate requires a new template id".into());
            }
            layout.id = id;
            layout.name = name;
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::Validate { layout } => {
            validate(&layout)?;
            json!({"layout":layout,"valid":true})
        }
        Command::Edit {
            mut layout,
            element,
        } => {
            if let Some(existing) = layout.elements.iter_mut().find(|e| e.id == element.id) {
                *existing = element
            } else {
                layout.elements.push(element)
            }
            validate(&layout)?;
            json!({"layout":layout})
        }
        Command::Remove { mut layout, id } => {
            if !layout.elements.iter().any(|e| e.id == id) {
                return Err("page element was not found".into());
            }
            layout.elements.retain(|e| e.id != id);
            validate(&layout)?;
            json!({"layout":layout})
        }
    };
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

pub fn layout_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Layout)).expect("layout schema serializes")
}
pub fn command_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Command)).expect("command schema serializes")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_and_roundtrip_are_valid_and_unknown_fields_refuse() {
        let layout = default_layout();
        validate(&layout).unwrap();
        let encoded =
            serde_json::to_vec(&serde_json::json!({"op":"validate","layout":layout})).unwrap();
        assert!(evaluate(&encoded).unwrap().contains("\"valid\":true"));
        assert!(evaluate(br#"{"op":"new","unknown":true}"#).is_err());
        assert_eq!(layout_schema()["additionalProperties"], false);
    }
    #[test]
    fn named_paper_templates_and_copies_keep_identity_explicit() {
        for paper in [
            Paper::A0,
            Paper::A1,
            Paper::A2,
            Paper::A3,
            Paper::A4,
            Paper::A5,
        ] {
            for orientation in [Orientation::Portrait, Orientation::Landscape] {
                let layout = named_layout(
                    Some("sample".into()),
                    Some("My template".into()),
                    paper,
                    orientation,
                )
                .unwrap();
                validate(&layout).unwrap();
                assert_eq!(layout.name, "My template");
            }
        }
        let layout = default_layout();
        assert!(evaluate(
            &serde_json::to_vec(
                &json!({"op":"duplicate","layout":layout,"id":layout.id,"name":"Copy"})
            )
            .unwrap()
        )
        .is_err());
        let result = evaluate(
            &serde_json::to_vec(&json!({"op":"rename","layout":layout,"name":"Renamed"})).unwrap(),
        )
        .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["layout"]["id"], layout.id);
        assert_eq!(result["layout"]["name"], "Renamed");
    }
    #[test]
    fn invalid_frames_and_unheld_logos_refuse() {
        let mut layout = default_layout();
        layout.elements[0].rect[2] = 1000.;
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.elements[0].rect[0] = f64::NAN;
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.elements[1].kind = ElementKind::Logo;
        layout.elements[1].asset = "missing".into();
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.assets.insert(
            "remote".into(),
            Asset {
                label: "Remote".into(),
                png_data_url: "https://example.test/logo.png".into(),
            },
        );
        assert!(validate(&layout).is_err());
    }
    #[test]
    fn edit_validates_whole_document_and_map_cannot_be_removed() {
        let layout = default_layout();
        let mut element = layout.elements[1].clone();
        element.text = "New title".into();
        let result = evaluate(
            &serde_json::to_vec(
                &serde_json::json!({"op":"edit","layout":layout,"element":element}),
            )
            .unwrap(),
        )
        .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["layout"]["elements"][1]["text"], "New title");
        assert_eq!(
            result["layout"]["elements"][0]["rect"],
            serde_json::json!([12., 32., 300., 235.])
        );
        assert!(evaluate(
            &serde_json::to_vec(&serde_json::json!({"op":"remove","layout":layout,"id":"map"}))
                .unwrap()
        )
        .is_err());
    }
}
