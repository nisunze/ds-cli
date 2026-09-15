//! Read-only consistency findings. Conflicting saved values never authorize
//! changing engineering input or choosing one source as the approved design.
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
type Row = BTreeMap<String, Value>;
#[derive(Debug, Serialize)]
pub struct CableFinding {
    pub code: &'static str,
    pub transformer: String,
    pub line: String,
    pub pole: String,
    pub pole_cable: String,
    pub line_cables: Vec<String>,
    pub status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct CableReview {
    pub total: usize,
    pub truncated: bool,
    pub findings: Vec<CableFinding>,
}
fn text<'a>(row: &'a Row, name: &str) -> &'a str {
    row.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
}
fn transformer<'a>(row: &'a Row, fallback: &'a str) -> &'a str {
    let name = text(row, "transfo");
    if name.is_empty() {
        fallback
    } else {
        name
    }
}
fn cable(row: &Row) -> &str {
    for name in ["computed_properties", "desirable_properties"] {
        if let Some(value) = row
            .get(name)
            .and_then(|v| v.get("cable_size"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return value;
        }
    }
    text(row, "cable_size")
}
fn normalized(value: &str) -> String {
    value
        .to_lowercase()
        .replace('×', "x")
        .replace("mm²", "")
        .replace("mm2", "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}
fn bounded(value: &str) -> String {
    value.chars().take(240).collect()
}
/// Compare labels within exact transformer/line identities. Normalizes only
/// typography; differing conductor/neutral descriptions remain for review.
pub fn lv_cables(fallback: &str, lines: &[Row], poles: &[Row]) -> CableReview {
    let mut expected: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    for row in lines {
        let line = text(row, "line_number");
        let size = cable(row);
        if !line.is_empty() && !size.is_empty() {
            expected
                .entry((transformer(row, fallback), line))
                .or_default()
                .insert(size);
        }
    }
    let mut findings = Vec::new();
    let mut total = 0;
    for row in poles {
        let transfo = transformer(row, fallback);
        let line = text(row, "line_number");
        // Pole schedules display their saved cable label. Do not replace it
        // with a line-derived value to make the comparison appear consistent.
        let size = text(row, "cable_size");
        let Some(sizes) = expected.get(&(transfo, line)) else {
            continue;
        };
        if size.is_empty()
            || sizes
                .iter()
                .any(|value| normalized(value) == normalized(size))
        {
            continue;
        }
        total += 1;
        if findings.len() >= 100 {
            continue;
        }
        let pole = ["pole_number", "pole_id", "id"]
            .iter()
            .map(|key| text(row, key))
            .find(|s| !s.is_empty())
            .unwrap_or("");
        findings.push(CableFinding {
            code: "lv_pole_line_cable_conflict",
            transformer: bounded(transfo),
            line: bounded(line),
            pole: bounded(pole),
            pole_cable: bounded(size),
            line_cables: sizes.iter().take(20).map(|s| bounded(s)).collect(),
            status: "pending_engineering_review",
        });
    }
    CableReview {
        truncated: total > findings.len(),
        total,
        findings,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn row(v: Value) -> Row {
        serde_json::from_value(v).unwrap()
    }
    #[test]
    fn conflict_keeps_both_values_and_never_crosses_transformers() {
        let lines = vec![
            row(json!({"transfo":"a","line_number":"LV_Line_01","cable_size":"3x35"})),
            row(json!({"transfo":"b","line_number":"LV_Line_01","cable_size":"3x50"})),
        ];
        let poles = vec![
            row(
                json!({"transfo":"a","line_number":"LV_Line_01","pole_number":"P01","cable_size":"3x50"}),
            ),
            row(json!({"transfo":"b","line_number":"LV_Line_01","cable_size":"3x50"})),
        ];
        let before = poles.clone();
        let result = lv_cables("", &lines, &poles);
        assert_eq!(result.total, 1);
        assert_eq!(result.findings[0].pole_cable, "3x50");
        assert_eq!(result.findings[0].line_cables, ["3x35"]);
        assert_eq!(result.findings[0].status, "pending_engineering_review");
        assert_eq!(before, poles);
    }
    #[test]
    fn displayed_computed_line_cable_and_typographic_equivalence_are_respected() {
        let lines = vec![row(
            json!({"line_number":"L1","cable_size":"old","computed_properties":{"cable_size":"3 × 50 mm²"}}),
        )];
        let poles = vec![row(json!({"line_number":"L1","cable_size":"3x50mm2"}))];
        assert_eq!(lv_cables("a", &lines, &poles).total, 0);
    }
}
