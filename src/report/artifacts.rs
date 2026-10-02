//! Report bridge declarations use the renderer's format and paper authorities.
//! Hosts retain locators and execute IO; they do not keep another format table.
use crate::{printing::ArtifactPresentation, report_formats};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub output_id: String,
    pub locator: String,
    pub filename: String,
    pub format: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ArtifactPresentation>,
}

fn lower_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Canonical output metadata, including GeoJSON's distinct ID and wire format.
pub fn output(output_id: &str) -> Option<report_formats::Output> {
    report_formats::output(output_id)
        .ok()
        .filter(|o| o.output_id == output_id)
}

pub fn validate(
    artifacts: Vec<Artifact>,
    combined: bool,
    inventory: bool,
    require_combined_base: bool,
) -> Result<Vec<Artifact>, String> {
    if artifacts.is_empty() || artifacts.len() > 32 || (require_combined_base && !combined) {
        return Err("invalid report artifact inventory".into());
    }
    let ordered: Vec<&str> = if combined {
        report_formats::COMBINED
            .iter()
            .chain(report_formats::OPTIONAL_COMBINED)
            .copied()
            .collect()
    } else {
        report_formats::INDIVIDUAL.to_vec()
    };
    let mut seen = BTreeSet::new();
    let mut previous: Option<(usize, &str)> = None;
    for artifact in &artifacts {
        let expected = output(&artifact.output_id).ok_or("unsupported report artifact output")?;
        if expected.format != artifact.format
            || expected.content_type != artifact.content_type
            || !seen.insert(&artifact.output_id)
            || !artifact
                .locator
                .strip_prefix("ds-reporter:v1:")
                .is_some_and(lower_hex)
            || !lower_hex(&artifact.sha256)
            || artifact.size_bytes == 0
            || artifact.size_bytes > 9_007_199_254_740_991
            || artifact.filename.is_empty()
            || artifact.filename.len() > 255
            || artifact.filename.trim() != artifact.filename
            || matches!(artifact.filename.as_str(), "." | "..")
            || artifact.filename.contains(['/', '\\'])
            || artifact.filename.chars().any(char::is_control)
        {
            return Err("invalid report artifact declaration".into());
        }
        crate::printing::validate_artifact_presentation(
            &artifact.format,
            artifact.paper_size.as_deref(),
            artifact.presentation.as_ref(),
        )?;
        if !inventory {
            let rank = ordered
                .iter()
                .position(|id| *id == artifact.output_id)
                .or_else(|| {
                    report_formats::named_print_output(&artifact.output_id).map(|_| ordered.len())
                })
                .ok_or("report artifact is not supported for this scope")?;
            let key = (rank, artifact.output_id.as_str());
            if previous.is_some_and(|previous| key <= previous) {
                return Err("report artifacts are not in canonical order".into());
            }
            previous = Some(key);
        }
    }
    if require_combined_base
        && !report_formats::COMBINED
            .iter()
            .enumerate()
            .all(|(index, id)| {
                artifacts
                    .get(index)
                    .is_some_and(|artifact| artifact.output_id == *id)
            })
    {
        return Err("combined report artifacts are missing required outputs".into());
    }
    Ok(artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(id: &str) -> Artifact {
        let output = output(id).unwrap();
        Artifact {
            output_id: output.output_id,
            format: output.format,
            content_type: output.content_type.into(),
            filename: if id == "voltage_drop_pdf" {
                report_formats::voltage_drop_pdf_filename("tx_1")
            } else {
                format!("tx_1{}", output.suffix)
            },
            locator: format!("ds-reporter:v1:{}", "a".repeat(64)),
            sha256: "b".repeat(64),
            size_bytes: 123,
            paper_size: (id == "voltage_drop_pdf").then(|| "A4".into()),
            presentation: None,
        }
    }

    #[test]
    fn every_renderer_output_reaches_the_bridge_in_canonical_order() {
        let mut ids = report_formats::INDIVIDUAL.to_vec();
        ids.extend(["jpeg__layout", "pdf__layout", "png__layout"]);
        let accepted = validate(
            ids.iter().map(|id| artifact(id)).collect(),
            false,
            false,
            false,
        )
        .unwrap();
        let geojson = accepted.iter().find(|a| a.output_id == "geojson").unwrap();
        assert_eq!(geojson.format, "geojsonzip");
        assert_eq!(geojson.content_type, "application/zip");
        assert!(output("jpg__layout").is_none());
        assert!(output("pdf__../layout").is_none());
    }

    #[test]
    fn local_voltage_drop_pdf_requires_a4_at_the_artifact_bridge() {
        assert!(validate(vec![artifact("voltage_drop_pdf")], false, false, false).is_ok());
        let mut missing_paper = artifact("voltage_drop_pdf");
        missing_paper.paper_size = None;
        assert!(validate(vec![missing_paper], false, false, false).is_err());
        let mut wrong_paper = artifact("voltage_drop_pdf");
        wrong_paper.paper_size = Some("A3".into());
        assert!(validate(vec![wrong_paper], false, false, false).is_err());
    }

    #[test]
    fn recovery_keeps_scope_order_and_duplicate_refusals() {
        assert!(validate(vec![artifact("xlsx"), artifact("shp")], false, false, false).is_err());
        assert!(validate(vec![artifact("xlsx"), artifact("xlsx")], false, true, false).is_err());
        assert!(validate(vec![artifact("geojsonseq")], false, false, false).is_err());
        assert!(validate(vec![artifact("pdf__layout")], true, false, true).is_err());
        let ids = report_formats::COMBINED
            .iter()
            .chain(report_formats::OPTIONAL_COMBINED);
        assert!(validate(ids.map(|id| artifact(id)).collect(), true, false, true).is_ok());
        assert!(validate(vec![artifact("zip")], true, true, false).is_ok());
        assert!(validate(vec![artifact("zip")], true, false, false).is_err());
    }

    #[test]
    fn refuses_mislabeled_bytes_paths_and_wrong_layout_presentation() {
        let mut bad = artifact("geojson");
        bad.format = "geojson".into();
        assert!(validate(vec![bad], false, false, false).is_err());
        let mut bad = artifact("pdf__layout");
        bad.content_type = "image/png".into();
        assert!(validate(vec![bad], false, false, false).is_err());
        let mut bad = artifact("pdf__layout");
        bad.filename = "../print.pdf".into();
        assert!(validate(vec![bad], false, false, false).is_err());
        let mut bad = artifact("pdf__layout");
        bad.paper_size = Some("A3".into());
        bad.presentation = Some(
            serde_json::from_value(serde_json::json!({
                "layout_id":"another", "layout_name":"Another", "paper_name":"A3",
                "orientation":"landscape", "page_mm":[420,297]
            }))
            .unwrap(),
        );
        assert!(validate(vec![bad], false, false, false).is_err());
    }
}
