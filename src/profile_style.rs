//! Transient Profile appearance. Governed pens remain the base; overrides are
//! scoped to the held native view and never change engineering or global styles.
use crate::profile_display::{DisplayOption, DisplayState};
use crate::profile_encoding::{DisplayRow, Encoding};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineType {
    Solid,
    Dashed,
    Dotted,
    DashDot,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_type: Option<LineType>,
    /// One Unicode symbol; rendered identically in drawing and legend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}
impl Style {
    pub fn validate(&self, target: &str) -> Result<(), String> {
        let option = serde_json::from_value::<DisplayOption>(json!(target)).is_ok();
        let symbol = matches!(target, "strain" | "suspension" | "junction" | "unknown");
        if !option && !symbol
            || self.symbol.is_some()
                && !symbol
                && !matches!(target, "structures" | "terrain_points")
        {
            return Err(
                "invalid_profile_view: unsupported Profile style target or symbol role".into(),
            );
        }
        if self.color.as_ref().is_some_and(|v| {
            v.len() != 7 || !v.starts_with('#') || !v[1..].chars().all(|c| c.is_ascii_hexdigit())
        }) || self
            .width_mm
            .is_some_and(|v| !v.is_finite() || !(0.05..=3.0).contains(&v))
            || self.symbol.as_ref().is_some_and(|v| {
                v.chars().count() != 1 || v.chars().any(|c| c.is_control() || c.is_whitespace())
            })
        {
            return Err("invalid_profile_view: style requires #rrggbb, width_mm 0.05..3 and one visible symbol".into());
        }
        Ok(())
    }
    pub fn merge(&mut self, patch: &Self) {
        if let Some(v) = &patch.color {
            self.color = Some(v.to_ascii_lowercase());
        }
        if let Some(v) = patch.width_mm {
            self.width_mm = Some(v);
        }
        if let Some(v) = patch.line_type {
            self.line_type = Some(v);
        }
        if let Some(v) = &patch.symbol {
            self.symbol = Some(v.clone());
        }
    }
    pub fn apply(&self, encoding: &mut Encoding) {
        if let Some(v) = &self.color {
            encoding.color = v.clone();
            encoding.label_color = v.clone();
        }
        if let Some(v) = self.width_mm {
            encoding.width_px = v * 96.0 / 25.4;
        }
        if let Some(v) = self.line_type {
            encoding.dash_px = match v {
                LineType::Solid => vec![],
                LineType::Dashed => vec![3.0, 1.5],
                LineType::Dotted => vec![0.5, 1.0],
                LineType::DashDot => vec![3.0, 1.0, 0.5, 1.0],
            }
            .into_iter()
            .map(|v| v * 96.0 / 25.4)
            .collect();
        }
        if let Some(v) = &self.symbol {
            encoding.symbol = Some(v.clone());
        }
    }
}
pub fn apply(state: &DisplayState, target: &str, encoding: &mut Encoding) {
    if let Some(style) = state.styles.get(target) {
        style.apply(encoding);
    }
}
/// A JSON editor submits this exact request. Vocabulary comes from Rust's schema.
pub fn controls(state: &DisplayState, rows: &[DisplayRow]) -> Value {
    let targets = rows
        .iter()
        .map(|row| json!({"target":row.option,"label_key":row.label_key}))
        .chain(
            ["strain", "suspension", "junction", "unknown"]
                .into_iter()
                .map(|target| json!({"target":target})),
        )
        .collect::<Vec<_>>();
    json!({"targets":targets, "schema":schemars::schema_for!(Style), "request":{"styles":state.styles}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_display::DisplayPatch;
    #[test]
    fn styles_merge_reset_and_refuse_atomically() {
        let state = DisplayState::default();
        let patch: DisplayPatch = serde_json::from_value(json!({"styles":{"ground":{"color":"#ABCDEF","width_mm":0.8,"line_type":"dash_dot"},"strain":{"symbol":"▲"}}})).unwrap();
        let next = state.patched(&patch).unwrap();
        assert_eq!(next.styles["ground"].color.as_deref(), Some("#abcdef"));
        let changed = next
            .patched(
                &serde_json::from_value(json!({"styles":{"ground":{"color":"#123456"}}})).unwrap(),
            )
            .unwrap();
        assert_eq!(changed.styles["ground"].width_mm, Some(0.8));
        for styles in [
            json!({"ground":{"width_mm":0}}),
            json!({"bogus":{"color":"#123456"}}),
            json!({"ground":{"symbol":"x"}}),
            json!({"strain":{"symbol":"many"}}),
        ] {
            assert!(changed
                .patched(
                    &serde_json::from_value(json!({"visibility":{"ground":false},"styles":styles}))
                        .unwrap()
                )
                .is_err());
            assert!(changed.visibility[&DisplayOption::Ground]);
        }
        let reset = changed
            .patched(&serde_json::from_value(json!({"styles":{"ground":null}})).unwrap())
            .unwrap();
        assert!(!reset.styles.contains_key("ground"));
        assert_eq!(reset.styles["strain"].symbol.as_deref(), Some("▲"));
    }
}
