//! Immutable presentation captured from the layout that produced an artifact.
use super::{Layout, Orientation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Engines write this beside an artifact; additive members from a newer engine
/// are accepted (owner 22 Oct) and every member below stays required.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ArtifactPresentation {
    pub layout_id: String,
    pub layout_name: String,
    pub paper_name: String,
    pub orientation: Orientation,
    pub page_mm: [f64; 2],
}

impl ArtifactPresentation {
    pub fn from_layout(layout: &Layout) -> Result<Self, String> {
        super::validate(layout)?;
        // Only the paper authority knows the supported catalogue. Consumers
        // carry its serialized identity without another list of paper names.
        let paper = serde_json::to_value(layout.paper.size).map_err(|e| e.to_string())?;
        let paper_name = paper
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                paper
                    .as_object()
                    .and_then(|value| value.keys().next().cloned())
            })
            .ok_or("paper has no name")?;
        Ok(Self {
            layout_id: layout.id.clone(),
            layout_name: layout.name.clone(),
            paper_name,
            orientation: layout.paper.orientation,
            page_mm: layout.page_mm,
        })
    }
}

pub fn validate_artifact_presentation(
    format: &str,
    paper_name: Option<&str>,
    presentation: Option<&ArtifactPresentation>,
) -> Result<(), String> {
    let named = crate::report_formats::named_print_output(format).map(|(_, id)| id);
    let is_print = named.is_some() || matches!(format, "pdf_a0" | "pdf_a3" | "voltage_drop_pdf");
    let text = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            && value.trim() == value
            && !value.chars().any(char::is_control)
    };
    if let Some(name) = paper_name {
        if !is_print
            || !text(name, 40)
            || (format == "pdf_a0" && name != "A0")
            || (format == "pdf_a3" && name != "A3")
            || (format == "voltage_drop_pdf" && name != "A4")
        {
            return Err("invalid artifact paper identity".into());
        }
    }
    if format == "voltage_drop_pdf" && paper_name != Some("A4") {
        return Err("voltage-drop PDF must declare A4 paper".into());
    }
    if let Some(value) = presentation {
        let [width, height] = value.page_mm;
        if !is_print
            || !text(&value.layout_id, 80)
            || !text(&value.layout_name, 240)
            || !text(&value.paper_name, 40)
            || paper_name != Some(value.paper_name.as_str())
            || named.is_some_and(|id| id != value.layout_id)
            || !width.is_finite()
            || !height.is_finite()
            || !(50.0..=2000.0).contains(&width)
            || !(50.0..=2000.0).contains(&height)
            || (value.orientation == Orientation::Portrait && width > height)
            || (value.orientation == Orientation::Landscape && width < height)
        {
            return Err("invalid artifact layout presentation".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_papers_are_not_a_consumer_catalogue_and_layout_identity_is_checked() {
        let mut value = ArtifactPresentation {
            layout_id: "detail".into(),
            layout_name: "Pole details".into(),
            paper_name: "ANSI B".into(),
            orientation: Orientation::Portrait,
            page_mm: [279.4, 431.8],
        };
        assert!(
            validate_artifact_presentation("pdf__detail", Some("ANSI B"), Some(&value)).is_ok()
        );
        assert!(
            validate_artifact_presentation("png__detail", Some("ANSI B"), Some(&value)).is_ok()
        );
        assert!(
            validate_artifact_presentation("pdf__other", Some("ANSI B"), Some(&value)).is_err()
        );
        assert!(validate_artifact_presentation("xlsx", Some("ANSI B"), Some(&value)).is_err());
        value.orientation = Orientation::Landscape;
        assert!(
            validate_artifact_presentation("pdf__detail", Some("ANSI B"), Some(&value)).is_err()
        );
        value.page_mm.reverse();
        assert!(
            validate_artifact_presentation("pdf__detail", Some("ANSI B"), Some(&value)).is_ok()
        );
    }

    #[test]
    fn analytical_voltage_drop_pdf_has_exact_a4_paper_identity() {
        assert!(validate_artifact_presentation("voltage_drop_pdf", Some("A4"), None).is_ok());
        assert!(validate_artifact_presentation("voltage_drop_pdf", None, None).is_err());
        assert!(validate_artifact_presentation("voltage_drop_pdf", Some("A3"), None).is_err());
    }
}
