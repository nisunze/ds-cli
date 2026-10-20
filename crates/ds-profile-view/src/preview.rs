//! Only an exact native geometry-only patch may replace the held view facts.
use crate::model::*;
use serde::Deserialize;
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct GeometryPreview {
    pub patch: Option<ScenePatch>,
    pub created_structure_id: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ScenePatch {
    pub outcome: String,
    pub schema_version: u32,
    pub model_revision: String,
    pub axis_pin: Axis,
    pub provisional: bool,
    pub geometry_only: bool,
    pub structures: Vec<Replacement>,
    pub spans: Vec<Span>,
    pub evidence: Evidence,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Replacement {
    pub alignment_id: String,
    pub structure: Structure,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Evidence {
    pub curve_unavailable: Vec<serde_json::Value>,
    pub section_clearance: Vec<serde_json::Value>,
    pub section_basis: Vec<serde_json::Value>,
}
pub fn project(scene: &Scene, preview: Option<&GeometryPreview>) -> Option<Scene> {
    let preview = preview?;
    let patch = preview.patch.as_ref()?;
    if patch.outcome != "patched"
        || !patch.provisional
        || !patch.geometry_only
        || patch.schema_version != scene.schema_version
        || patch.model_revision != scene.model_revision
        || patch.axis_pin.digest != scene.axis_digest()
        || !patch.spans.is_empty()
        || !patch.evidence.curve_unavailable.is_empty()
        || !patch.evidence.section_clearance.is_empty()
        || !patch.evidence.section_basis.is_empty()
    {
        return None;
    }
    if preview.created_structure_id.as_ref().is_some_and(|id| {
        !patch.structures.iter().any(|r| {
            &r.structure.entity_id == id
                && scene.bands.iter().any(|b| b.alignment_id == r.alignment_id)
        })
    }) {
        return None;
    }
    let mut next = scene.clone();
    next.provisional_geometry = true;
    for replacement in &patch.structures {
        let Some(band) = next
            .bands
            .iter_mut()
            .find(|b| b.alignment_id == replacement.alignment_id)
        else {
            continue;
        };
        if let Some(structure) = band
            .structures
            .iter_mut()
            .find(|s| s.entity_id == replacement.structure.entity_id)
        {
            *structure = replacement.structure.clone();
        } else if preview.created_structure_id.as_ref() == Some(&replacement.structure.entity_id) {
            band.structures.push(replacement.structure.clone());
        }
    }
    next.prepare_render();
    Some(next)
}
