//! Bounded evidence from the reporter's held-fact qualification pass.
//!
//! The Reporter writes these records; additive members from a Reporter
//! released after this Kernel are accepted (owner 22 Oct). Required members,
//! types and the bounds in [`EngineeringPreflight::valid`] still decide.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineeringPreflight {
    pub schema: String,
    pub source_layers: Vec<SourceLayer>,
    pub field_lineage: Vec<FieldLineage>,
    pub blockers: usize,
    pub finding_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub coverage_counts: BTreeMap<String, usize>,
    pub findings: Vec<Finding>,
    pub omitted_details: usize,
    pub quantity_rows_total: usize,
    pub quantity_lineage: Vec<QuantityLineage>,
    pub omitted_quantity_details: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_engineering: Vec<Value>,
    /// Missing or invalid facts the workbook itemizes as Dirty Categories
    /// (Reporter 36765d4). Absent on older Reporters, whose `blockers`
    /// counted the same facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_facts: Option<DirtyFacts>,
}
/// Facts a report publishes as Dirty Categories instead of quantities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirtyFacts {
    /// One row per transformer, layer, field and code, every feature counted.
    pub rows: Vec<DirtyFactRow>,
    /// The first features with their identity; the rest are counted.
    pub findings: Vec<Finding>,
    pub omitted_details: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirtyFactRow {
    pub transformer: Option<String>,
    pub layer: String,
    pub field: String,
    pub code: String,
    /// The InfoTable section the fact feeds.
    pub section: String,
    pub features: usize,
}
/// What a receipt states about the Dirty Categories: every counted feature,
/// and per transformer its count and the `layer.field` names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirtyFactsSummary {
    pub features: usize,
    pub transformers: Vec<DirtyFactsTransformer>,
    /// Transformers counted in `features` but not listed.
    pub omitted_transformers: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirtyFactsTransformer {
    pub transformer: String,
    pub features: usize,
    pub fields: Vec<String>,
}
/// The most transformers and fields one summary names; counts stay exact.
const MAX_SUMMARY_TRANSFORMERS: usize = 50;
const MAX_SUMMARY_FIELDS: usize = 16;
const MAX_DIRTY_ROWS: usize = 65_536;

impl DirtyFacts {
    /// Every feature with a missing or invalid fact.
    pub fn features(&self) -> Option<usize> {
        self.rows
            .iter()
            .try_fold(0_usize, |sum, row| sum.checked_add(row.features))
    }
    pub(super) fn valid(&self) -> bool {
        let text = |s: &str| !s.is_empty() && s.len() <= 2048 && !s.chars().any(char::is_control);
        self.rows.len() <= MAX_DIRTY_ROWS
            && self.findings.len() <= 100
            && self.features().is_some_and(|features| {
                self.findings.len().checked_add(self.omitted_details) == Some(features)
            })
            && self.rows.iter().all(|row| {
                row.features > 0
                    && row.transformer.as_deref().is_none_or(text)
                    && [&row.layer, &row.field, &row.code, &row.section]
                        .into_iter()
                        .all(|value| text(value))
            })
            && self.findings.iter().all(|row| row.valid())
    }
    pub fn summary(&self) -> DirtyFactsSummary {
        let mut grouped: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();
        for row in &self.rows {
            let entry = grouped
                .entry(
                    row.transformer
                        .clone()
                        .unwrap_or_else(|| "unattributed".into()),
                )
                .or_default();
            entry.0 += row.features;
            entry.1.insert(format!("{}.{}", row.layer, row.field));
        }
        DirtyFactsSummary {
            features: self.features().unwrap_or(usize::MAX),
            omitted_transformers: grouped.len().saturating_sub(MAX_SUMMARY_TRANSFORMERS),
            transformers: grouped
                .into_iter()
                .take(MAX_SUMMARY_TRANSFORMERS)
                .map(|(transformer, (features, fields))| DirtyFactsTransformer {
                    transformer,
                    features,
                    fields: fields.into_iter().take(MAX_SUMMARY_FIELDS).collect(),
                })
                .collect(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceLayer {
    pub layer: String,
    pub features: usize,
    pub held_features_sha256: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldLineage {
    pub layer: String,
    pub field: String,
    #[serde(default = "required")]
    pub requirement: String,
    pub sections: Vec<String>,
    pub source_rows: usize,
    pub valid: usize,
    pub explicit_zero: usize,
    pub existing_component: usize,
    pub tapping_marker: usize,
    pub missing: usize,
    pub invalid: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    pub layer: String,
    pub field: String,
    pub transformer: Option<String>,
    pub feature_id: Option<Value>,
    pub feature_index: usize,
    pub recorded: Option<Value>,
}
impl Finding {
    fn valid(&self) -> bool {
        let text = |s: &str| !s.is_empty() && s.len() <= 2048 && !s.chars().any(char::is_control);
        text(&self.code)
            && text(&self.layer)
            && text(&self.field)
            && self.transformer.as_deref().is_none_or(text)
            && self
                .feature_id
                .as_ref()
                .is_none_or(|value| value.is_string() || value.is_number())
            && self.recorded.as_ref().is_none_or(|value| {
                serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 8192)
            })
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantityLineage {
    pub section: String,
    pub description: String,
    pub unit: String,
    pub quantity: f64,
    pub transformer: Option<String>,
}

impl EngineeringPreflight {
    pub(super) fn valid(&self) -> bool {
        let text = |s: &str| !s.is_empty() && s.len() <= 2048 && !s.chars().any(char::is_control);
        let optional = |s: &Option<String>| s.as_deref().is_none_or(text);
        self.schema == "ds.report.engineering-preflight/v1"
            && self.network_engineering.len() <= 4096
            && self
                .network_engineering
                .iter()
                .all(valid_network_engineering)
            && self.source_layers.len() <= 32
            && self.field_lineage.len() <= 128
            && self.findings.len() <= 100
            && self.quantity_lineage.len() <= 500
            && self.finding_counts.len() <= 32
            && self.coverage_counts.len() <= 1
            && self
                .coverage_counts
                .keys()
                .all(|code| code == "optional_engineering_fact_unrecorded")
            && self
                .field_lineage
                .iter()
                .filter(|row| row.requirement == "optional")
                .try_fold(0_usize, |n, row| n.checked_add(row.missing))
                == Some(
                    self.coverage_counts
                        .get("optional_engineering_fact_unrecorded")
                        .copied()
                        .unwrap_or(0),
                )
            && self.finding_counts.keys().all(|code| text(code))
            && self
                .finding_counts
                .values()
                .try_fold(0_usize, |n, count| n.checked_add(*count))
                == Some(self.blockers)
            && self.findings.len().checked_add(self.omitted_details) == Some(self.blockers)
            && self
                .quantity_lineage
                .len()
                .checked_add(self.omitted_quantity_details)
                == Some(self.quantity_rows_total)
            && self
                .source_layers
                .iter()
                .all(|row| text(&row.layer) && super::exact_lower_sha256(&row.held_features_sha256))
            && self.field_lineage.iter().all(|row| {
                text(&row.layer)
                    && text(&row.field)
                    && (row.requirement == "required"
                        || row.requirement == "optional" && row.field == "flying_stay")
                    && row.sections.len() <= 16
                    && row.sections.iter().all(|section| text(section))
                    && [
                        row.valid,
                        row.explicit_zero,
                        row.existing_component,
                        row.tapping_marker,
                        row.missing,
                        row.invalid,
                    ]
                    .into_iter()
                    .all(|n| n <= row.source_rows)
            })
            && self.findings.iter().all(Finding::valid)
            && self.dirty_facts.as_ref().is_none_or(DirtyFacts::valid)
            && self.quantity_lineage.iter().all(|row| {
                text(&row.section)
                    && text(&row.description)
                    && text(&row.unit)
                    && optional(&row.transformer)
                    && row.quantity.is_finite()
                    && row.quantity >= 0.0
            })
    }
}

fn required() -> String {
    "required".into()
}

// Network owns this provenance and its engineering semantics. The report
// consumer checks only the bounded receipt shape and retains the exact Value.
fn valid_network_engineering(value: &Value) -> bool {
    let sha = |value: &Value| value.as_str().is_some_and(super::exact_lower_sha256);
    let receipt = &value["receipt"];
    let identity = &receipt["engineIdentity"];
    let artifact = &receipt["artifact"];
    value.as_object().is_some_and(|row| row.len() == 3)
        && serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 64 * 1024)
        && sha(&value["executable_sha256"])
        && value["development_override"].is_boolean()
        && receipt.is_object()
        && receipt["schema"] == "ds.network.task-result/v1"
        && receipt["outcome"] == "completed"
        && sha(&receipt["inputSha256"])
        && identity.is_object()
        && identity["schema"] == "ds.network-engine/v1"
        && artifact.is_object()
        && sha(&artifact["sha256"])
        && matches!(
            (artifact["file"].as_str(), artifact["schema"].as_str()),
            (
                Some("engineering.json"),
                Some("ds.network.report-engineering.result/v1")
            ) | (
                Some("mv-report.json"),
                Some("ds.network.mv-report-result/v1")
            )
        )
        && artifact["byteCount"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
}

#[cfg(test)]
pub(super) mod tests;
