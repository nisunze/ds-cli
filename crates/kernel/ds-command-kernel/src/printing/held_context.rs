//! Whether a capture of held local data is complete enough to print.
//!
//! A survey form's synchronised entries and a local (map.draw or imported)
//! layer print from what this computer holds — map visibility is irrelevant,
//! and nothing is fetched. The browser decided the capture's integrity in
//! `held-print-context.ts`; now the host hands the raw capture — the feature
//! collection it read, the count the store declared beside it, how the layer
//! is stored and which project it came from — and the kernel decides whether
//! the capture is the complete held payload or must be refused by key. The
//! admitted value is the FeatureCollection the print context carries.
use super::preview::{refusal, Refusal};
use super::PrintContextSource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most features one held source may print as complete data. Beyond it
/// a capture is a display preview, and a preview is not printable data.
pub const MAX_HELD_FEATURES: usize = 50_000;

/// One capture as the host read it from its stores, before any decision.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeldCapture {
    /// The context layer id the capture answers (`PrintContextLayer::id`).
    pub context_id: String,
    /// The captured `FeatureCollection`.
    pub features: Value,
    /// How many features the store declared for the source, read
    /// independently of the capture.
    #[serde(default)]
    pub declared_count: u64,
    /// How a local layer's features are stored; `preview` holds a display
    /// subset, never the complete data.
    #[serde(default)]
    pub render_mode: Option<String>,
    /// The project a local GIS-folder layer was imported for, when it says.
    #[serde(default)]
    pub source_project: Option<String>,
    /// Whether the local layer's features were completely loaded.
    #[serde(default)]
    pub features_loaded: bool,
}

/// The captured features, when the value is a `FeatureCollection` whose
/// `features` is an array.
fn collection_features(capture: &HeldCapture) -> Option<&Vec<Value>> {
    let collection = capture.features.as_object()?;
    if collection.get("type").and_then(Value::as_str) != Some("FeatureCollection") {
        return None;
    }
    collection.get("features")?.as_array()
}

/// Admit one held capture for a survey or local-layer source of `project`.
///
/// A survey capture needs synchronised geometry (`declared_count > 0`),
/// within the bound, and exactly as many features as the store declared —
/// fewer or more means the store changed during the capture. A local layer
/// needs its complete payload: within the bound and not a display preview
/// (`print_context_too_large`), from this project when it names one
/// (`print_context_foreign_project`), and loaded with every declared feature
/// (`print_context_capture_incomplete`). Any other source kind is not held
/// data and is refused. The admitted value is the FeatureCollection itself.
pub fn admit(
    project: &str,
    source: &PrintContextSource,
    capture: &HeldCapture,
) -> Result<Value, Refusal> {
    let refuse = |code: &str, key: &str, name: &str| {
        Err(refusal(
            code,
            key,
            &[("name", name), ("bound", &MAX_HELD_FEATURES.to_string())],
        ))
    };
    match source {
        PrintContextSource::Survey { form } => {
            let Some(features) = collection_features(capture) else {
                return refuse(
                    "print_context_capture_invalid",
                    "printing_context_capture_invalid",
                    form,
                );
            };
            if capture.declared_count == 0 {
                return refuse(
                    "print_context_survey_empty",
                    "printing_context_survey_empty",
                    form,
                );
            }
            if capture.declared_count > MAX_HELD_FEATURES as u64 {
                return refuse(
                    "print_context_too_large",
                    "printing_context_too_large",
                    form,
                );
            }
            if features.len() as u64 != capture.declared_count {
                return refuse(
                    "print_context_capture_changed",
                    "printing_context_capture_changed",
                    form,
                );
            }
        }
        PrintContextSource::LocalLayer { layer_id } => {
            let Some(features) = collection_features(capture) else {
                return refuse(
                    "print_context_capture_invalid",
                    "printing_context_capture_invalid",
                    layer_id,
                );
            };
            if capture.declared_count > MAX_HELD_FEATURES as u64
                || capture.render_mode.as_deref() == Some("preview")
            {
                return refuse(
                    "print_context_too_large",
                    "printing_context_too_large",
                    layer_id,
                );
            }
            if capture
                .source_project
                .as_deref()
                .is_some_and(|owner| owner != project)
            {
                return refuse(
                    "print_context_foreign_project",
                    "printing_context_foreign_project",
                    layer_id,
                );
            }
            if !capture.features_loaded || features.len() as u64 != capture.declared_count {
                return refuse(
                    "print_context_capture_incomplete",
                    "printing_context_capture_incomplete",
                    layer_id,
                );
            }
        }
        _ => {
            return refuse(
                "print_context_not_held_source",
                "printing_context_not_held_source",
                &capture.context_id,
            );
        }
    }
    Ok(capture.features.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn collection(count: usize) -> Value {
        let features: Vec<Value> = (0..count)
            .map(|i| {
                json!({"type": "Feature", "id": i, "geometry": {"type": "Point", "coordinates": [30.1, -1.9]}, "properties": {}})
            })
            .collect();
        json!({"type": "FeatureCollection", "features": features})
    }

    fn survey() -> PrintContextSource {
        PrintContextSource::Survey {
            form: "poles".into(),
        }
    }

    fn local() -> PrintContextSource {
        PrintContextSource::LocalLayer {
            layer_id: "sketch_1".into(),
        }
    }

    fn capture(count: usize) -> HeldCapture {
        HeldCapture {
            context_id: "poles".into(),
            features: collection(count),
            declared_count: count as u64,
            render_mode: Some("rows".into()),
            source_project: Some("p1".into()),
            features_loaded: true,
        }
    }

    #[test]
    fn a_complete_capture_is_admitted() {
        let admitted = admit("p1", &survey(), &capture(3)).unwrap();
        assert_eq!(admitted, collection(3));
        let admitted = admit("p1", &local(), &capture(2)).unwrap();
        assert_eq!(admitted["features"].as_array().unwrap().len(), 2);
        // A local layer that names no project belongs to whoever holds it.
        let mut unnamed = capture(1);
        unnamed.source_project = None;
        assert!(admit("p1", &local(), &unnamed).is_ok());
        // At the bound is within the bound.
        let mut bound = capture(0);
        bound.features = collection(MAX_HELD_FEATURES);
        bound.declared_count = MAX_HELD_FEATURES as u64;
        assert!(admit("p1", &survey(), &bound).is_ok());
    }

    #[test]
    fn a_changed_oversized_preview_or_foreign_capture_is_refused_by_key() {
        let code = |source: &PrintContextSource, capture: &HeldCapture| {
            admit("p1", source, capture).unwrap_err().code
        };
        let mut empty = capture(0);
        empty.declared_count = 0;
        assert_eq!(code(&survey(), &empty), "print_context_survey_empty");
        let refused = admit("p1", &survey(), &empty).unwrap_err();
        assert_eq!(refused.message_key, "printing_context_survey_empty");
        assert_eq!(refused.params["name"], "poles");
        assert_eq!(refused.params["bound"], "50000");

        let mut changed = capture(3);
        changed.declared_count = 4;
        assert_eq!(code(&survey(), &changed), "print_context_capture_changed");

        let mut oversized = capture(1);
        oversized.declared_count = MAX_HELD_FEATURES as u64 + 1;
        assert_eq!(code(&survey(), &oversized), "print_context_too_large");
        assert_eq!(code(&local(), &oversized), "print_context_too_large");

        let mut preview = capture(2);
        preview.render_mode = Some("preview".into());
        assert_eq!(code(&local(), &preview), "print_context_too_large");

        let mut foreign = capture(2);
        foreign.source_project = Some("p2".into());
        assert_eq!(code(&local(), &foreign), "print_context_foreign_project");

        let mut unloaded = capture(2);
        unloaded.features_loaded = false;
        assert_eq!(
            code(&local(), &unloaded),
            "print_context_capture_incomplete"
        );
        let mut short = capture(2);
        short.declared_count = 3;
        assert_eq!(code(&local(), &short), "print_context_capture_incomplete");

        let mut invalid = capture(1);
        invalid.features = json!({"type": "Feature"});
        assert_eq!(code(&survey(), &invalid), "print_context_capture_invalid");
        assert_eq!(code(&local(), &invalid), "print_context_capture_invalid");

        let catalog = PrintContextSource::Catalog {
            country: "Rwanda".into(),
            layer: "roads".into(),
        };
        assert_eq!(code(&catalog, &capture(1)), "print_context_not_held_source");
    }
}
