//! Presentation of verified render diagnostics; shared by CLI and UI.
use super::table_rows::PrintWarning;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
pub struct LabelNote {
    pub layer_id: String,
    pub reason: String,
    pub count: u64,
    pub message: String,
}
#[derive(Serialize)]
pub struct Status {
    pub warnings: Vec<String>,
    pub label_notes: Vec<LabelNote>,
}
pub fn summarize(assessments: &[Value], warnings: &[PrintWarning]) -> Result<Status, String> {
    if assessments.len() > 64 || warnings.len() > crate::report_export::MAX_PRINT_WARNINGS {
        return Err("unbounded sheet status facts".into());
    }
    let mut labels = BTreeMap::<String, BTreeMap<String, u64>>::new();
    let mut finding_messages = BTreeSet::new();
    let mut has_findings = false;
    let mut all_warnings = warnings.to_vec();
    for assessment in assessments {
        if let Some(findings) = assessment["findings"].as_array() {
            has_findings |= !findings.is_empty() || assessment["truncated"] == true;
            finding_messages.extend(
                findings
                    .iter()
                    .filter_map(|f| f["message"].as_str())
                    .map(str::to_owned),
            );
        }
        if let Some(table_warnings) = assessment.get("table_warnings") {
            all_warnings.extend(
                serde_json::from_value::<Vec<PrintWarning>>(table_warnings.clone())
                    .map_err(|_| "invalid sheet table warnings")?,
            );
        }
        if all_warnings.len() > crate::report_export::MAX_PRINT_WARNINGS {
            return Err("unbounded sheet table warnings".into());
        }
        let Some(counts) = assessment.get("label_omissions_by_layer") else {
            continue;
        };
        let counts: BTreeMap<String, BTreeMap<String, u64>> =
            serde_json::from_value(counts.clone()).map_err(|_| "invalid label placement counts")?;
        if counts.len() > 100 {
            return Err("unbounded label placement layers".into());
        }
        for (layer, reasons) in counts {
            if layer.len() > 256 || reasons.len() > 4 {
                return Err("invalid label placement layer".into());
            }
            for (reason, count) in reasons {
                if !matches!(
                    reason.as_str(),
                    "spacing" | "collision" | "frame_clipping" | "polygon_containment"
                ) || count > 1_000_000
                {
                    return Err("invalid label placement reason or count".into());
                }
                // PDF/raster assessments can repeat the same sheet. They do
                // not multiply the number of labels omitted on that sheet.
                let entry = labels
                    .entry(layer.clone())
                    .or_default()
                    .entry(reason)
                    .or_default();
                *entry = (*entry).max(count);
            }
        }
    }
    let mut messages = BTreeSet::new();
    for warning in all_warnings {
        if warning.message.len() > 2048 {
            return Err("unbounded sheet warning".into());
        }
        if warning.code == "print_label_omissions"
            || (warning.code == "print_diagnostics_summary" && !has_findings)
            || finding_messages.contains(&warning.message)
        {
            continue;
        }
        messages.insert(warning.message);
    }
    let mut notes = Vec::new();
    for (layer, reasons) in labels {
        for (reason, count) in reasons {
            if count == 0 {
                continue;
            }
            let why = match reason.as_str() {
                "spacing" => "avoid repeating nearby text",
                "collision" => "avoid overlapping other text or map furniture",
                "frame_clipping" => "keep text inside the map frame",
                _ => "keep text inside its area",
            };
            let label = if count == 1 { "label" } else { "labels" };
            notes.push(LabelNote {
                message: format!(
                    "{}: {count} {label} hidden to {why}.",
                    layer.replace('_', " ")
                ),
                layer_id: layer.clone(),
                reason,
                count,
            });
        }
    }
    Ok(Status {
        warnings: messages.into_iter().collect(),
        label_notes: notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn omitted_text_is_informational_while_rows_and_furniture_stay_warnings() {
        let assessment = json!({"findings":[],"table_warnings":[],"label_omissions_by_layer":{
            "cell_boundaries":{"polygon_containment":1},"customers":{"collision":5}}});
        let warning = |code: &str, message: &str| PrintWarning {
            code: code.into(),
            message: message.into(),
            transformer: "test".into(),
            output_id: "pdf__detail".into(),
            layout_id: "detail".into(),
            element_id: String::new(),
            total_rows: 0,
            omitted_rows: 0,
        };
        let status = summarize(
            &[assessment.clone(), assessment],
            &[
                warning("print_label_omissions", "technical label diagnostic"),
                warning("table_truncated", "2 schedule rows missing"),
            ],
        )
        .unwrap();
        assert_eq!(status.warnings, vec!["2 schedule rows missing"]);
        assert_eq!(status.label_notes.len(), 2);
        assert_eq!(status.label_notes[1].count, 5);
        assert!(status.label_notes[0]
            .message
            .contains("keep text inside its area"));
    }
}
