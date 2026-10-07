//! Profile dock geometry. Opening always restores equal Map/Profile halves.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub available_height_px: f64,
    /// Omission means opening; an explicit height is a user/CLI resize.
    pub height_px: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct Layout {
    pub height_px: f64,
    pub map_height_px: f64,
}
pub fn resolve(request: Request) -> Result<Layout, String> {
    let available = request.available_height_px;
    if !available.is_finite() || !(1.0..=10000.0).contains(&available) {
        return Err(
            "invalid_profile_view: available height must be finite within 1..10000 pixels".into(),
        );
    }
    let height = request.height_px.unwrap_or(available / 2.0);
    if !height.is_finite() || height <= 0.0 || height >= available {
        return Err("invalid_profile_view: Profile height must leave both panes visible".into());
    }
    Ok(Layout {
        height_px: height,
        map_height_px: available - height,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_halves_the_available_area_and_resize_is_explicit() {
        for available in [360.0, 900.0, 1440.0] {
            let result = resolve(Request {
                available_height_px: available,
                height_px: None,
            })
            .unwrap();
            assert_eq!(result.height_px, result.map_height_px);
        }
        assert_eq!(
            resolve(Request {
                available_height_px: 900.0,
                height_px: Some(300.0)
            })
            .unwrap()
            .map_height_px,
            600.0
        );
        assert!(resolve(Request {
            available_height_px: 900.0,
            height_px: Some(900.0)
        })
        .is_err());
    }
}
