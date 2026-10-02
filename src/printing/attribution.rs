//! Mandatory licensing content with typography supplied by a governed printing
//! document. Legacy hosts retain their existing stamp when no capture is held.
use base64::Engine;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CREDIT: &str = "DS GridDesign by datasolutions.rw";
pub const SVG_ID: &str = "ds-grid-credit-v1";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub font_pt: f64,
    pub color: String,
    pub svg_font_family: String,
    pub pdf_font_family: String,
    pub right_inset_pt: f64,
    pub baseline_pt: f64,
}
impl Style {
    pub fn validate(&self) -> Result<(), String> {
        if !self.font_pt.is_finite()
            || !(7.0..=48.0).contains(&self.font_pt)
            || !self.right_inset_pt.is_finite()
            || !(40.0..=1000.0).contains(&self.right_inset_pt)
            || !self.baseline_pt.is_finite()
            || !(7.0..=1000.0).contains(&self.baseline_pt)
            || self.color.len() != 7
            || !self.color.starts_with('#')
            || !self.color[1..].bytes().all(|b| b.is_ascii_hexdigit())
            || self.svg_font_family.is_empty()
            || self.svg_font_family.len() > 100
            || !self
                .svg_font_family
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" -_".contains(&b))
            || ![
                "Helvetica",
                "Helvetica-Bold",
                "Helvetica-Oblique",
                "Helvetica-BoldOblique",
                "Times-Roman",
                "Times-Bold",
                "Times-Italic",
                "Times-BoldItalic",
                "Courier",
                "Courier-Bold",
                "Courier-Oblique",
                "Courier-BoldOblique",
            ]
            .contains(&self.pdf_font_family.as_str())
        {
            return Err(
                "print_attribution_invalid: invalid governed type, ink or placement".into(),
            );
        }
        let lightness = (0..3)
            .map(|i| u8::from_str_radix(&self.color[1 + i * 2..3 + i * 2], 16).unwrap() as f64)
            .sum::<f64>()
            / (3. * 255.);
        if lightness > 0.6 {
            return Err(
                "print_attribution_invalid: licensing ink must remain visible on paper".into(),
            );
        }
        Ok(())
    }
}
pub fn svg(page_mm: [f64; 2], style: &Style) -> String {
    let held = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(style).unwrap());
    format!("<g id=\"{SVG_ID}\" data-credit-style=\"{held}\"><text x=\"{}\" y=\"{}\" text-anchor=\"start\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\">{CREDIT}</text></g>",
        page_mm[0] - style.right_inset_pt * 25.4 / 72., page_mm[1] - style.baseline_pt * 25.4 / 72.,
        style.svg_font_family, style.font_pt * 25.4 / 72., style.color)
}
/// Hold the same document policy through SVG conversion into accessible PDF
/// text. This decodes our own exact SVG group, never performs style selection.
pub fn from_svg(svg: &str) -> Result<Option<Style>, String> {
    let start = format!("<g id=\"{SVG_ID}\" data-credit-style=\"");
    let Some((_, tail)) = svg.split_once(&start) else {
        return Ok(None);
    };
    let held = tail
        .split_once('"')
        .ok_or("print_attribution_invalid: incomplete capture")?
        .0;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(held)
        .map_err(|_| "print_attribution_invalid: malformed capture")?;
    let style: Style = serde_json::from_slice(&bytes)
        .map_err(|_| "print_attribution_invalid: malformed document")?;
    style.validate()?;
    Ok(Some(style))
}
/// Remove only the generated group before replacing it with native PDF text.
pub fn without_svg_credit(svg: &str) -> String {
    let start = format!("<g id=\"{SVG_ID}\"");
    let Some(begin) = svg.find(&start) else {
        return svg.into();
    };
    let Some(end) = svg[begin..].find("</g>") else {
        return svg.into();
    };
    format!("{}{}", &svg[..begin], &svg[begin + end + 4..])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn governed_caption_policy_survives_svg_transport_without_defaults() {
        let style: Style = serde_json::from_value(serde_json::json!({
            "font_pt":9,"color":"#123456","svg_font_family":"DejaVu Serif",
            "pdf_font_family":"Times-Roman","right_inset_pt":180,"baseline_pt":11
        }))
        .unwrap();
        style.validate().unwrap();
        let svg = svg([420., 297.], &style);
        assert_eq!(from_svg(&svg).unwrap(), Some(style));
        assert_eq!(svg.matches(CREDIT).count(), 1);
        assert!(!without_svg_credit(&svg).contains(CREDIT));
        let mut invalid = from_svg(&svg).unwrap().unwrap();
        invalid.color = "#ffffff".into();
        assert!(invalid.validate().is_err());
        invalid.color = "#123456".into();
        invalid.font_pt = 0.;
        assert!(invalid.validate().is_err());
    }
}
