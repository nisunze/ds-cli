//! Typed projection into the existing native command contracts. No execution.
use crate::model::Point;
use serde::{Deserialize, Serialize};
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    RetypeSummary {
        outcome: crate::retype::Outcome,
        #[serde(default)]
        translations: std::collections::BTreeMap<String, String>,
    },
    Station {
        expected_revision: String,
        structure_id: String,
        station_m: f64,
    },
    Ground {
        command_id: String,
        expected_revision: String,
        entity_id: String,
        elevation_m: f64,
        sheet: PropertySheet,
    },
    Create {
        pointer: CreatePointer,
        id: String,
        mode: CreateMode,
        choice: String,
        first_support_id: Option<String>,
        first_alignment_id: Option<String>,
    },
    Lasso {
        model_id: String,
        expected_revision: String,
        polygon: Vec<Point>,
        mode: Option<String>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreatePointer {
    pub model_revision: String,
    pub axis_pin_digest: String,
    pub scene_x: f64,
    pub scene_y: f64,
    pub tolerance_scene_units: f64,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreateMode {
    Structure,
    GroundPoint,
    TensionSection,
}
#[derive(Debug, Deserialize)]
pub struct PropertySheet {
    pub revision: String,
    pub kind: String,
    pub command_schema_version: u32,
    pub groups: Vec<PropertyGroup>,
}
#[derive(Debug, Deserialize)]
pub struct PropertyGroup {
    pub fields: Vec<PropertyField>,
}
#[derive(Debug, Deserialize)]
pub struct PropertyField {
    pub id: String,
    pub can_edit: bool,
    pub editor: PropertyEditor,
}
#[derive(Debug, Deserialize)]
pub struct PropertyEditor {
    pub command_kind: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Payload {
    RetypeSummary(crate::retype::Summary),
    Station {
        expected_revision: String,
        structure_id: String,
        station_m: f64,
    },
    Ground {
        command_id: String,
        command_schema_version: u32,
        expected_revision: String,
        command: GroundCommand,
    },
    Create {
        #[serde(flatten)]
        pointer: CreatePointer,
        operation: CreateOperation,
    },
    Lasso {
        model_id: String,
        expected_revision: String,
        space: &'static str,
        polygon: Vec<[f64; 2]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<&'static str>,
    },
}
#[derive(Debug, Serialize)]
pub struct GroundCommand {
    pub command_kind: &'static str,
    pub edits: Vec<GroundEdit>,
}
#[derive(Debug, Serialize)]
pub struct GroundEdit {
    pub entity_id: String,
    pub field_id: &'static str,
    pub value: f64,
}
#[derive(Debug, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CreateOperation {
    Structure {
        id: String,
        structure_type_id: String,
    },
    GroundPoint {
        id: String,
        feature_class: String,
    },
    TensionSection {
        id: String,
        template_section_id: String,
        first_support_id: Option<String>,
        first_alignment_id: Option<String>,
    },
}
pub fn project(request: Request) -> Result<Payload, String> {
    let invalid = || {
        "profile_payload_invalid: native revision, identity and finite coordinates are required"
            .to_string()
    };
    match request {
        Request::RetypeSummary {
            outcome,
            translations,
        } => Ok(Payload::RetypeSummary(crate::retype::summary(
            &outcome,
            &translations,
        ))),
        Request::Station {
            expected_revision,
            structure_id,
            station_m,
        } => {
            if expected_revision.is_empty() || structure_id.is_empty() || !station_m.is_finite() {
                return Err(invalid());
            }
            Ok(Payload::Station {
                expected_revision,
                structure_id,
                station_m,
            })
        }
        Request::Ground {
            command_id,
            expected_revision,
            entity_id,
            elevation_m,
            sheet,
        } => {
            if !elevation_m.is_finite()
                || command_id.is_empty()
                || entity_id.is_empty()
                || expected_revision.is_empty()
            {
                return Err(invalid());
            }
            if sheet.revision != expected_revision
                || sheet.kind != "terrain_point"
                || !sheet.groups.iter().flat_map(|g| &g.fields).any(|f| {
                    f.id == "z_m"
                        && f.can_edit
                        && f.editor.command_kind.as_deref() == Some("edit_profile_properties")
                })
            {
                return Err("profile_ground_not_editable: ground elevation is not editable at this model revision".into());
            }
            Ok(Payload::Ground {
                command_id,
                command_schema_version: sheet.command_schema_version,
                expected_revision,
                command: GroundCommand {
                    command_kind: "edit_profile_properties",
                    edits: vec![GroundEdit {
                        entity_id,
                        field_id: "z_m",
                        value: elevation_m,
                    }],
                },
            })
        }
        Request::Create {
            pointer,
            id,
            mode,
            choice,
            first_support_id,
            first_alignment_id,
        } => {
            if id.is_empty()
                || choice.is_empty()
                || pointer.model_revision.is_empty()
                || pointer.axis_pin_digest.is_empty()
                || ![
                    pointer.scene_x,
                    pointer.scene_y,
                    pointer.tolerance_scene_units,
                ]
                .iter()
                .all(|n| n.is_finite())
                || pointer.tolerance_scene_units < 0.
                || pointer.tolerance_scene_units > 32.
            {
                return Err(invalid());
            }
            let operation = match mode {
                CreateMode::Structure => CreateOperation::Structure {
                    id,
                    structure_type_id: choice,
                },
                CreateMode::GroundPoint => CreateOperation::GroundPoint {
                    id,
                    feature_class: choice,
                },
                CreateMode::TensionSection => CreateOperation::TensionSection {
                    id,
                    template_section_id: choice,
                    first_support_id,
                    first_alignment_id,
                },
            };
            Ok(Payload::Create { pointer, operation })
        }
        Request::Lasso {
            model_id,
            expected_revision,
            polygon,
            mode,
        } => {
            if model_id.is_empty()
                || expected_revision.is_empty()
                || polygon.len() < 3
                || polygon.len() > 100_000
                || polygon
                    .iter()
                    .any(|p| !p.scene_x.is_finite() || !p.scene_y.is_finite())
            {
                return Err(invalid());
            }
            let polygon = polygon.iter().map(|p| [p.scene_x, p.scene_y]).collect();
            Ok(Payload::Lasso {
                model_id,
                expected_revision,
                space: "profile",
                polygon,
                mode: if mode.as_deref() == Some("add") {
                    Some("add")
                } else {
                    None
                },
            })
        }
    }
}
pub fn evaluate(json: &str) -> Result<String, String> {
    let request =
        serde_json::from_str(json).map_err(|e| format!("profile_payload_invalid: {e}"))?;
    serde_json::to_string(&project(request)?).map_err(|e| e.to_string())
}
