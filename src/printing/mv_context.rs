//! Source-pinned context independent of MV revision and pagination.
use super::mv_booklet::sha256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema: String,
    pub project_id: String,
    /// Stable native alignment identity, never sheet number or array position.
    pub alignment_id: String,
    /// Exact held render input bytes, including source layers and style recipe.
    pub context_sha256: String,
    pub render: Value,
}

impl Capture {
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if self.schema != "ds.mv-alignment-context/v1"
            || self.project_id != project
            || self.alignment_id.is_empty()
            || self.alignment_id.len() > 200
            || self.context_sha256
                != sha256(&serde_json::to_vec(&self.render).map_err(|e| e.to_string())?)
        {
            return Err("mv_context_invalid: project, alignment or source digest differs".into());
        }
        // Current model linework belongs to the engine plan, never an old
        // captured map. The independent capture contains geographic/LV context.
        if self.render["layers"].as_array().is_some_and(|layers| {
            layers.iter().any(|layer| {
                layer["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("dsgrid_mv_"))
            })
        }) {
            return Err("mv_context_invalid: captured MV geometry must be removed; the exact native plan owns MV".into());
        }
        Ok(())
    }
}

pub fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Capture)).expect("context schema")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_is_pinned_to_its_sources_and_alignment_not_model_revision_or_sheet_position() {
        let render =
            serde_json::json!({"layers":[{"id":"lv_lines","geojson":{"features":[]}}],"layout":{}});
        let mut capture = Capture {
            schema: "ds.mv-alignment-context/v1".into(),
            project_id: "p".into(),
            alignment_id: "al-22".into(),
            context_sha256: sha256(&serde_json::to_vec(&render).unwrap()),
            render,
        };
        capture.validate("p").unwrap();
        assert!(capture.validate("other").is_err());
        capture.render["layers"][0]["geojson"]["features"] = serde_json::json!([{"id":"changed"}]);
        assert!(capture.validate("p").is_err());
    }
}
