//! Governed Profile pens from the API style document, in physical units.
//! Screen conversion shares the printing convention: 96 px/in, 72 pt/in.
use crate::profile_display::{DisplayOption, DisplayState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pen {
    pub color: String,
    pub width_mm: f64,
    pub dash_mm: Vec<f64>,
    pub label_color: String,
    pub label_size_pt: f64,
    pub label_bold: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Encoding {
    pub color: String,
    pub width_px: f64,
    pub dash_px: Vec<f64>,
    pub label_color: String,
    pub font: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct DisplayRow {
    pub option: DisplayOption,
    pub label_key: String,
    pub checked: bool,
    pub encoding: Encoding,
}
/// No host palette and no guessed missing pen. The API owns the complete document.
pub fn resolve(
    state: &DisplayState,
    document: &serde_json::Value,
) -> Result<Vec<DisplayRow>, String> {
    let pens: BTreeMap<String, Pen> =
        serde_json::from_value(document["metadata"]["profile_pens"].clone())
            .map_err(|error| format!("profile_styles_unavailable: {error}"))?;
    if pens.len() != DisplayOption::ALL.len() + 1 {
        return Err("profile_styles_unavailable: the governed Profile document must contain every display pen and the clearance witness pen".into());
    }
    clearance_violation_encoding(document)?;
    DisplayOption::ALL
        .into_iter()
        .map(|option| {
            let key = serde_json::to_value(option).unwrap();
            let key = key.as_str().unwrap();
            let pen = pens
                .get(key)
                .ok_or_else(|| format!("profile_styles_unavailable: missing {key}"))?;
            Ok(DisplayRow {
                option,
                label_key: option.label_key().into(),
                checked: state.visibility[&option],
                encoding: encode_pen(pen, key)?,
            })
        })
        .collect()
}
/// Located witnesses share the Clearance visibility option and retain their own governed ink.
pub fn clearance_violation_encoding(document: &serde_json::Value) -> Result<Encoding, String> {
    let pen: Pen = serde_json::from_value(
        document["metadata"]["profile_pens"]["clearance_violations"].clone(),
    )
    .map_err(|error| format!("profile_styles_unavailable: clearance_violations: {error}"))?;
    encode_pen(&pen, "clearance_violations")
}
fn encode_pen(pen: &Pen, key: &str) -> Result<Encoding, String> {
    if !pen.width_mm.is_finite()
        || !(0.05..=3.0).contains(&pen.width_mm)
        || !pen.label_size_pt.is_finite()
        || !(6.0..=24.0).contains(&pen.label_size_pt)
        || pen.dash_mm.len() > 8
        || pen
            .dash_mm
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0 || *v > 20.0)
        || !color(&pen.color)
        || !color(&pen.label_color)
    {
        return Err(format!("profile_styles_invalid: {key}"));
    }
    Ok(Encoding {
        color: pen.color.clone(),
        width_px: pen.width_mm * 96.0 / 25.4,
        dash_px: pen.dash_mm.iter().map(|v| v * 96.0 / 25.4).collect(),
        label_color: pen.label_color.clone(),
        font: format!(
            "{}{}px monospace",
            if pen.label_bold { "bold " } else { "" },
            pen.label_size_pt * 96.0 / 72.0
        ),
    })
}
pub fn review_status_colors(
    document: &serde_json::Value,
) -> Result<BTreeMap<String, String>, String> {
    let colors: BTreeMap<String, String> =
        serde_json::from_value(document["metadata"]["profile_review_status_colors"].clone())
            .map_err(|error| format!("profile_styles_unavailable: {error}"))?;
    if colors.len() != 3
        || ["pass", "fail", "unknown"]
            .iter()
            .any(|key| !colors.get(*key).is_some_and(|value| color(value)))
    {
        return Err(
            "profile_styles_invalid: review status colors require pass, fail and unknown".into(),
        );
    }
    Ok(colors)
}
pub fn review_box_ink(document: &serde_json::Value) -> Result<String, String> {
    document["metadata"]["profile_review_box_ink"]
        .as_str()
        .filter(|value| color(value))
        .map(str::to_owned)
        .ok_or_else(|| "profile_styles_unavailable: governed review box ink is required".into())
}
fn color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn governed_pens_are_complete_distinct_and_scaled() {
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/profile-display-style.json"))
                .unwrap();
        let rows = resolve(&DisplayState::default(), &document).unwrap();
        assert_eq!(rows.len(), 16);
        assert!(rows[0].checked);
        assert_eq!(rows.len(), 16);
        assert_ne!(
            serde_json::to_value(clearance_violation_encoding(&document).unwrap()).unwrap(),
            serde_json::to_value(
                &rows
                    .iter()
                    .find(|r| r.option == DisplayOption::ClearanceLine)
                    .unwrap()
                    .encoding
            )
            .unwrap()
        );
        let guide = &rows
            .iter()
            .find(|row| row.option == DisplayOption::ClearanceLine)
            .unwrap()
            .encoding;
        let witness = clearance_violation_encoding(&document).unwrap();
        assert_eq!(guide.width_px, 0.4 * 96.0 / 25.4);
        assert_eq!(guide.dash_px, vec![2.0 * 96.0 / 25.4, 96.0 / 25.4]);
        assert_eq!(guide.label_color, "#fb7185");
        assert_eq!(guide.font, "11px monospace");
        assert_eq!(witness.width_px, 0.5 * 96.0 / 25.4);
        assert!(witness.dash_px.is_empty());
        assert_eq!(witness.label_color, "#a78bfa");
        assert_eq!(witness.font, "bold 12px monospace");
        let encodings: std::collections::BTreeSet<_> = rows
            .iter()
            .map(|row| {
                format!(
                    "{}:{}:{:?}",
                    row.encoding.color, row.encoding.width_px, row.encoding.dash_px
                )
            })
            .collect();
        assert_eq!(encodings.len(), rows.len());
        assert!(resolve(&DisplayState::default(), &serde_json::json!({})).is_err());
    }
}
