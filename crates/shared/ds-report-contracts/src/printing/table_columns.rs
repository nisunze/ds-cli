//! The schedule phase a table binding authors, and its admission. Which
//! columns a schedule can show is the Reporter's (`print_table_columns`).
use super::{TableBinding, TablePresentation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SchedulePhase {
    #[default]
    Design,
    AsBuilt,
}

pub fn available_in_phase(key: &str, phase: SchedulePhase) -> bool {
    let key = key.to_ascii_lowercase();
    phase == SchedulePhase::AsBuilt
        || (!matches!(
            key.as_str(),
            "meter_number" | "meter_numbers" | "meter_numb" | "meter_type" | "nid"
        ) && !key.contains("phone"))
}

/// Rendering and authoring enforce the same optional schedule phase.
pub fn validate_phase(binding: &TableBinding) -> Result<(), String> {
    if let Some(phase) = binding.phase {
        if binding.workbook.is_some() || binding.presentation != TablePresentation::Plain {
            return Err("Schedule phase requires a plain engineering table".into());
        }
        if binding
            .columns
            .iter()
            .any(|key| !available_in_phase(key, phase))
        {
            return Err("Installed-connection columns require an as-built schedule".into());
        }
    }
    Ok(())
}
