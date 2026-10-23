//! The cartography defaults every client inherits: the relief intensity, the
//! scale bar, the measure tool's styling and the GPS-accuracy and zoom
//! badges. Brain keeps one document for the platform; reading it is public,
//! publishing it needs `map.defaults.edit`. The basemap catalogue is the
//! client's own and is neither read nor written here, and the retired 3D
//! camera section is carried through a publish unchanged.
//!
//! A document read from Brain or from this edge's store is admitted section
//! by section: a missing field takes its default, a section that does not
//! fit takes the default section, a colour or size that draws nothing takes
//! its default, and relief never starts enabled.

use ds_gateway_client::Operation;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const READ_TIMEOUT_MS: u64 = 120_000;
pub const INVALID: &str = "map_defaults_answer_invalid";

/// The relief intensities, subtle to dramatic; the relief control cycles
/// through them in this order.
pub const RELIEF_PRESETS: [f64; 5] = [0.3, 0.6, 1.0, 1.5, 2.5];

/// The preset nearest `value`, the first of two equally near.
pub fn nearest_preset(value: f64) -> f64 {
    RELIEF_PRESETS
        .into_iter()
        .fold(RELIEF_PRESETS[0], |best, preset| {
            if (value - preset).abs() < (value - best).abs() {
                preset
            } else {
                best
            }
        })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct HillshadeDefaults {
    /// Kept for the wire; relief always starts off.
    pub enabled: bool,
    pub exaggeration: f64,
}

impl Default for HillshadeDefaults {
    fn default() -> Self {
        Self {
            enabled: false,
            exaggeration: 0.6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ScaleUnit {
    #[default]
    Metric,
    Imperial,
    Nautical,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ScaleBarDefaults {
    pub enabled: bool,
    /// Pixels.
    pub max_width: u32,
    pub unit: ScaleUnit,
}

impl Default for ScaleBarDefaults {
    fn default() -> Self {
        Self {
            enabled: false,
            max_width: 120,
            unit: ScaleUnit::Metric,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct MeasureDefaults {
    pub line_color: String,
    pub point_color: String,
    pub label_color: String,
    pub halo_color: String,
    pub line_width: f64,
    pub label_text_size: f64,
    pub show_segment_labels: bool,
    pub show_running_total: bool,
}

impl Default for MeasureDefaults {
    fn default() -> Self {
        Self {
            line_color: "#facc15".into(),
            point_color: "#facc15".into(),
            label_color: "#facc15".into(),
            halo_color: "#000000".into(),
            line_width: 3.0,
            label_text_size: 13.0,
            show_segment_labels: true,
            show_running_total: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GpsAccuracyDefaults {
    pub enabled: bool,
    pub survey_only: bool,
    pub warn_above_meters: f64,
}

impl Default for GpsAccuracyDefaults {
    fn default() -> Self {
        Self {
            enabled: true,
            survey_only: true,
            warn_above_meters: 10.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ZoomLevelDefaults {
    pub enabled: bool,
    pub decimals: u8,
}

impl Default for ZoomLevelDefaults {
    fn default() -> Self {
        Self {
            enabled: false,
            decimals: 1,
        }
    }
}

/// The platform's cartography defaults.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct MapDefaults {
    pub hillshade: HillshadeDefaults,
    pub scale_bar: ScaleBarDefaults,
    pub measure: MeasureDefaults,
    pub gps_accuracy: GpsAccuracyDefaults,
    pub zoom_level: ZoomLevelDefaults,
    /// The retired 3D camera section, carried through a publish unchanged.
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub camera: Map<String, Value>,
}

impl Default for MapDefaults {
    fn default() -> Self {
        let camera = json!({"default_pitch_3d": 20, "max_pitch": 85,
            "experimental_range_warning": true});
        Self {
            hillshade: HillshadeDefaults::default(),
            scale_bar: ScaleBarDefaults::default(),
            measure: MeasureDefaults::default(),
            gps_accuracy: GpsAccuracyDefaults::default(),
            zoom_level: ZoomLevelDefaults::default(),
            camera: camera.as_object().cloned().unwrap_or_default(),
        }
    }
}

/// A section as written over its defaults, field by field: a field it does
/// not write keeps its default, a field this Kernel does not know is dropped,
/// and a section that does not fit is the default section.
fn section<T>(document: &Value, name: &str) -> T
where
    T: Serialize + for<'de> Deserialize<'de> + Default,
{
    let mut merged = serde_json::to_value(T::default()).unwrap_or(Value::Null);
    if let (Some(fields), Some(written)) = (
        merged.as_object_mut(),
        document.get(name).and_then(Value::as_object),
    ) {
        for (key, value) in written {
            if let Some(field) = fields.get_mut(key) {
                *field = value.clone();
            }
        }
    }
    T::deserialize(&merged).unwrap_or_default()
}

/// A colour that names nothing takes its default.
fn filled(color: &mut String, default: String) {
    if color.trim().is_empty() {
        *color = default;
    }
}

/// A size that draws nothing takes its default.
fn drawn(size: &mut f64, default: f64) {
    if !(*size > 0.0) {
        *size = default;
    }
}

impl MapDefaults {
    /// A stored or answered document as the defaults it carries.
    pub fn admit(document: &Value) -> Self {
        let fallback = Self::default();
        let mut camera = fallback.camera;
        if let Some(written) = document.get("camera").and_then(Value::as_object) {
            camera.extend(written.clone());
        }
        let mut defaults = Self {
            hillshade: section(document, "hillshade"),
            scale_bar: section(document, "scale_bar"),
            measure: section(document, "measure"),
            gps_accuracy: section(document, "gps_accuracy"),
            zoom_level: section(document, "zoom_level"),
            camera,
        };
        defaults.hillshade.enabled = false;
        drawn(
            &mut defaults.hillshade.exaggeration,
            HillshadeDefaults::default().exaggeration,
        );
        let fallback = MeasureDefaults::default();
        let measure = &mut defaults.measure;
        filled(&mut measure.line_color, fallback.line_color);
        filled(&mut measure.point_color, fallback.point_color);
        filled(&mut measure.label_color, fallback.label_color);
        filled(&mut measure.halo_color, fallback.halo_color);
        drawn(&mut measure.line_width, fallback.line_width);
        drawn(&mut measure.label_text_size, fallback.label_text_size);
        defaults
    }
}

fn answered(body: &Value) -> Result<MapDefaults, Refusal> {
    let document = data(body);
    if !document.is_object() {
        return Err(Refusal::new(
            INVALID,
            "The map defaults service answered no document.",
        ));
    }
    Ok(MapDefaults::admit(document))
}

/// The platform's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Read;

impl Exchange for Read {
    type Output = MapDefaults;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::generated(
            Operation::MapDefaultsAction,
            Some(json!({"action": "get_config"})),
        )?
        .timeout_ms(READ_TIMEOUT_MS)
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        answered(&body).map(Admitted::Done)
    }
}

/// Publish new defaults for every client. Brain keeps its basemap catalogue,
/// which this document never carries.
#[derive(Clone, Debug, PartialEq)]
pub struct Publish {
    pub defaults: MapDefaults,
}

impl Exchange for Publish {
    type Output = MapDefaults;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut defaults = self.defaults.clone();
        defaults.hillshade.enabled = false;
        Ok(Request::generated(
            Operation::MapDefaultsAction,
            Some(json!({"action": "update", "data": defaults})),
        )?
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        answered(&body).map(Admitted::Done)
    }
}

#[cfg(test)]
#[path = "map_defaults_tests.rs"]
mod tests;
