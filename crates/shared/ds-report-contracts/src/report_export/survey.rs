//! The survey-layers document a Reporter run appends: its schema, bounds,
//! layer naming and what the engine says it appended. The Kernel's held
//! copy and run planning stay in `ds-command-kernel` `report_export::survey`.
use serde::{Deserialize, Serialize};

/// The wire schema of the document a host stages beside a transformer's room
/// and the engine reads digest-bound (`survey_layers_asset`).
pub const SURVEY_LAYERS_SCHEMA: &str = "ds.survey-layers/v1";
/// The most features one staged document may carry, over every form.
pub const MAX_SURVEY_LAYER_FEATURES: usize = 100_000;
/// The most bytes one staged document may occupy: the engine's bound.
pub const MAX_SURVEY_LAYERS_BYTES: u64 = 64 * 1024 * 1024;

/// The layer one appended form becomes: `survey_` and the slug, lowercased,
/// every character outside `[a-z0-9_]` replaced by `_`, underscores trimmed;
/// `survey_form` when nothing is left. The cloud handler and the engine's
/// local path both call this, so the layer a report carries has one name.
pub fn survey_layer_name(form_slug: &str) -> String {
    let cleaned = form_slug
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if cleaned.is_empty() {
        "survey_form".into()
    } else {
        format!("survey_{cleaned}")
    }
}

/// What the engine says it appended for one form. Additive members from a
/// newer engine are accepted (owner 22 Oct).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SurveyLayerResult {
    pub form: String,
    pub layer: String,
    pub features: u64,
}
