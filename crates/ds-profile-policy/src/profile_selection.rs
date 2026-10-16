//! Display-governed Profile selection families, without selection state or admission.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Structures,
    TerrainPoints,
    Alignments,
    TensionSections,
    AttachmentPoints,
}
pub fn profile_families(state: &crate::profile_display::DisplayState) -> Vec<Family> {
    use crate::profile_display::DisplayOption as O;
    let mut families = Vec::new();
    if state.visibility[&O::Structures] {
        families.extend([Family::Structures, Family::AttachmentPoints]);
    }
    if state.visibility[&O::TerrainPoints] {
        families.push(Family::TerrainPoints);
    }
    if state.visibility[&O::Ground] {
        families.push(Family::Alignments);
    }
    if state.visibility[&O::Wire] {
        families.push(Family::TensionSections);
    }
    families
}
