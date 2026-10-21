//! How a compounded archive is grouped — read from exactly what ds-brain writes.
//!
//! Since ds-brain 8bed741 a registry entry carries `group_count`, `groups` (the
//! sorted, distinct FIRST-level folder labels the archive covers, `_unassigned`
//! dropped) and a `grouping` block pinning the applied `report_archive` plan —
//! or `{"applied": false, "detail": …}` when no plan was applied and the archive
//! is flat by contract. Archives written before that carry `district_count` /
//! `districts` only; those are read here as the legacy fallback and nowhere
//! else.
//!
//! Presence decides, not content: once ANY generic field is on the entry the
//! legacy pair is ignored, so a stale compatibility field can never become the
//! active truth. Every host (the native client, the CLI, the web through WASM)
//! reads one answer from here instead of re-deciding the fold.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One archive can name at most this many first-level groups.
pub const MAX_ARCHIVE_GROUPS: usize = 2_000;
/// A group label is a folder segment; the plan's definition ids are tokens.
const MAX_LABEL_CHARS: usize = 512;
const MAX_DEFINITION_IDS: usize = 16;
const MAX_TOKEN_CHARS: usize = 128;
const MAX_DETAIL_CHARS: usize = 1_024;

/// The archive-entry fields grouping is read from. Every other field of the
/// entry is ignored, so a host may hand the whole registry row over.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ArchiveGroupingFields {
    #[serde(default)]
    pub group_count: Option<serde_json::Value>,
    #[serde(default)]
    pub groups: Option<serde_json::Value>,
    #[serde(default)]
    pub grouping: Option<serde_json::Value>,
    #[serde(default)]
    pub district_count: Option<serde_json::Value>,
    #[serde(default)]
    pub districts: Option<serde_json::Value>,
}

/// Where the archive's grouping came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveGroupingKind {
    /// An applied `report_archive` plan filed the archive.
    Plan,
    /// No plan was applied; the archive is flat by contract, not collapsed.
    Flat,
    /// A generic count/label list with no plan block (never written by a
    /// current brain, but read rather than refused).
    Recorded,
    /// A pre-plan archive: `district_count` / `districts` only.
    LegacyDistrict,
    /// Nothing about grouping was recorded.
    Unrecorded,
}

/// The applied plan's identity as the archive pinned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchiveGroupingPlan {
    pub purpose: Option<String>,
    /// Ordered grouping keys; the first names the level `labels` are from.
    pub definition_ids: Vec<String>,
    pub plan_digest: Option<String>,
    pub projection_sha256: Option<String>,
    pub revision: Option<u64>,
    pub member_count: Option<u64>,
    pub unassigned_count: Option<u64>,
    /// Groups in the whole plan, at every level — not the archive's
    /// first-level `count`.
    pub group_count: Option<u64>,
}

/// The decoded grouping of one archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchiveGroups {
    pub kind: ArchiveGroupingKind,
    /// First-level groups the archive covers.
    pub count: u64,
    /// Their labels, in the order recorded.
    pub labels: Vec<String>,
    /// The grouping key the labels are values of: the plan's first definition
    /// id, `district` for a legacy archive, absent otherwise.
    pub key: Option<String>,
    pub plan: Option<ArchiveGroupingPlan>,
    /// The service's own sentence for a flat archive.
    pub detail: Option<String>,
    /// The noun hosts render `count` under (`<noun_key>_one|_many`).
    pub noun_key: &'static str,
}

impl ArchiveGroups {
    /// True when the archive was meant to be filed under groups and none were
    /// resolved: everything landed in `_unassigned`. A flat archive asked for
    /// no groups, so it never filed "nothing".
    pub fn filed_no_group(&self) -> bool {
        self.count == 0 && self.kind != ArchiveGroupingKind::Flat
    }
}

/// Why a grouping record was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveGroupsError {
    /// A count is not a non-negative integer.
    CountInvalid,
    /// The label list is not a list of bounded, distinct, non-empty labels.
    LabelsInvalid,
    /// More labels than an archive may name.
    LabelsUnbounded,
    /// `group_count` disagrees with the labels it counts.
    CountMismatch,
    /// The `grouping` block is not the shape ds-brain writes.
    PlanInvalid,
    /// A flat (no-plan) archive that still names groups.
    FlatWithGroups,
}

impl ArchiveGroupsError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::CountInvalid => "archive_group_count_invalid",
            Self::LabelsInvalid => "archive_group_labels_invalid",
            Self::LabelsUnbounded => "archive_group_labels_unbounded",
            Self::CountMismatch => "archive_group_count_mismatch",
            Self::PlanInvalid => "archive_grouping_plan_invalid",
            Self::FlatWithGroups => "archive_flat_with_groups",
        }
    }
}

impl std::fmt::Display for ArchiveGroupsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for ArchiveGroupsError {}

fn text_ok(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}

fn count(value: Option<&serde_json::Value>) -> Result<Option<u64>, ArchiveGroupsError> {
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or(ArchiveGroupsError::CountInvalid),
    }
}

/// Labels as recorded. Generic labels are distinct by construction (ds-brain
/// writes a sorted set); a duplicate is a malformed record, not a second group.
fn labels(
    value: Option<&serde_json::Value>,
    distinct: bool,
) -> Result<Option<Vec<String>>, ArchiveGroupsError> {
    let items = match value {
        None | Some(serde_json::Value::Null) => return Ok(None),
        Some(serde_json::Value::Array(items)) => items,
        Some(_) => return Err(ArchiveGroupsError::LabelsInvalid),
    };
    if items.len() > MAX_ARCHIVE_GROUPS {
        return Err(ArchiveGroupsError::LabelsUnbounded);
    }
    let mut out = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let label = item.as_str().ok_or(ArchiveGroupsError::LabelsInvalid)?;
        if !text_ok(label, MAX_LABEL_CHARS) || (distinct && !seen.insert(label)) {
            return Err(ArchiveGroupsError::LabelsInvalid);
        }
        out.push(label.to_owned());
    }
    Ok(Some(out))
}

fn plan_text(
    block: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    max: usize,
) -> Result<Option<String>, ArchiveGroupsError> {
    match block.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) if value.is_empty() => Ok(None),
        Some(serde_json::Value::String(value)) if text_ok(value, max) => Ok(Some(value.clone())),
        Some(_) => Err(ArchiveGroupsError::PlanInvalid),
    }
}

fn plan_count(
    block: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<u64>, ArchiveGroupsError> {
    count(block.get(key)).map_err(|_| ArchiveGroupsError::PlanInvalid)
}

/// `(applied, plan, detail)` from the `grouping` block.
fn grouping_block(
    value: &serde_json::Value,
) -> Result<(bool, Option<ArchiveGroupingPlan>, Option<String>), ArchiveGroupsError> {
    let block = value.as_object().ok_or(ArchiveGroupsError::PlanInvalid)?;
    let applied = block
        .get("applied")
        .and_then(serde_json::Value::as_bool)
        .ok_or(ArchiveGroupsError::PlanInvalid)?;
    let detail = plan_text(block, "detail", MAX_DETAIL_CHARS)?;
    if !applied {
        return Ok((false, None, detail));
    }
    let definition_ids = match block.get("definition_ids") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(items)) if items.len() <= MAX_DEFINITION_IDS => items
            .iter()
            .map(|item| {
                item.as_str()
                    .filter(|id| text_ok(id, MAX_TOKEN_CHARS))
                    .map(str::to_owned)
                    .ok_or(ArchiveGroupsError::PlanInvalid)
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(ArchiveGroupsError::PlanInvalid),
    };
    let plan = ArchiveGroupingPlan {
        purpose: plan_text(block, "purpose", MAX_TOKEN_CHARS)?,
        definition_ids,
        plan_digest: plan_text(block, "plan_digest", MAX_TOKEN_CHARS)?,
        projection_sha256: plan_text(block, "projection_sha256", MAX_TOKEN_CHARS)?,
        revision: plan_count(block, "revision")?,
        member_count: plan_count(block, "member_count")?,
        unassigned_count: plan_count(block, "unassigned_count")?,
        group_count: plan_count(block, "group_count")?,
    };
    Ok((true, Some(plan), detail))
}

/// Decode one archive's grouping, generic first, legacy only when no generic
/// field is present.
pub fn decode(fields: &ArchiveGroupingFields) -> Result<ArchiveGroups, ArchiveGroupsError> {
    let generic = [&fields.group_count, &fields.groups, &fields.grouping]
        .iter()
        .any(|field| field.as_ref().is_some_and(|value| !value.is_null()));
    if !generic {
        let labels = labels(fields.districts.as_ref(), false)?.unwrap_or_default();
        let recorded = count(fields.district_count.as_ref())?;
        let count = recorded.unwrap_or(labels.len() as u64);
        let any = recorded.is_some() || fields.districts.as_ref().is_some_and(|v| !v.is_null());
        return Ok(ArchiveGroups {
            kind: if any {
                ArchiveGroupingKind::LegacyDistrict
            } else {
                ArchiveGroupingKind::Unrecorded
            },
            count,
            labels,
            key: any.then(|| "district".to_owned()),
            plan: None,
            detail: None,
            noun_key: if any { "pctl_districts" } else { "pctl_groups" },
        });
    }
    let labels = labels(fields.groups.as_ref(), true)?.unwrap_or_default();
    let recorded = count(fields.group_count.as_ref())?;
    if recorded.is_some_and(|recorded| recorded != labels.len() as u64)
        && fields.groups.as_ref().is_some_and(|v| !v.is_null())
    {
        return Err(ArchiveGroupsError::CountMismatch);
    }
    let count = recorded.unwrap_or(labels.len() as u64);
    let (kind, plan, detail) = match fields.grouping.as_ref().filter(|v| !v.is_null()) {
        None => (ArchiveGroupingKind::Recorded, None, None),
        Some(block) => match grouping_block(block)? {
            (true, plan, detail) => (ArchiveGroupingKind::Plan, plan, detail),
            (false, _, detail) => {
                if count > 0 {
                    return Err(ArchiveGroupsError::FlatWithGroups);
                }
                (ArchiveGroupingKind::Flat, None, detail)
            }
        },
    };
    let key = plan
        .as_ref()
        .and_then(|plan| plan.definition_ids.first().cloned());
    Ok(ArchiveGroups {
        kind,
        count,
        labels,
        key,
        plan,
        detail,
        noun_key: "pctl_groups",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Vec<serde_json::Value> {
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/compounded-archive-groups.json"
        ))
        .unwrap();
        doc["compounded_reports"].as_array().unwrap().clone()
    }

    fn read(entry: serde_json::Value) -> Result<ArchiveGroups, ArchiveGroupsError> {
        decode(&serde_json::from_value(entry).unwrap())
    }

    #[test]
    fn a_brain_written_archive_reads_its_plan_and_first_level_groups() {
        let groups = read(fixture()[0].clone()).unwrap();
        assert_eq!(groups.kind, ArchiveGroupingKind::Plan);
        assert_eq!(groups.count, 2);
        assert_eq!(groups.labels, ["Huye", "Nyamagabe"]);
        assert_eq!(groups.key.as_deref(), Some("loc_admin_level_2"));
        let plan = groups.plan.as_ref().unwrap();
        assert_eq!(plan.purpose.as_deref(), Some("report_archive"));
        assert_eq!(
            plan.definition_ids,
            ["loc_admin_level_2", "loc_admin_level_3"]
        );
        assert_eq!(plan.revision, Some(3));
        assert_eq!(plan.member_count, Some(4));
        assert_eq!(plan.unassigned_count, Some(0));
        // The plan's own group count is every level; the archive's is the first.
        assert_eq!(plan.group_count, Some(3));
        assert_eq!(groups.noun_key, "pctl_groups");
        assert!(!groups.filed_no_group());
    }

    #[test]
    fn a_brain_written_flat_archive_says_so_and_is_not_collapsed() {
        let groups = read(fixture()[1].clone()).unwrap();
        assert_eq!(groups.kind, ArchiveGroupingKind::Flat);
        assert_eq!(groups.count, 0);
        assert!(groups.plan.is_none());
        assert!(groups.detail.as_deref().unwrap().contains("flat"));
        assert!(!groups.filed_no_group());
    }

    #[test]
    fn a_legacy_archive_falls_back_to_districts() {
        let groups = read(fixture()[2].clone()).unwrap();
        assert_eq!(groups.kind, ArchiveGroupingKind::LegacyDistrict);
        assert_eq!(groups.count, 3);
        assert_eq!(groups.labels, ["Huye", "Nyamagabe", "Nyaruguru"]);
        assert_eq!(groups.key.as_deref(), Some("district"));
        assert_eq!(groups.noun_key, "pctl_districts");
        // A legacy archive with no district resolved filed nothing.
        let collapsed = read(json!({"district_count": 0})).unwrap();
        assert!(collapsed.filed_no_group());
    }

    #[test]
    fn generic_presence_wins_over_stale_legacy_fields() {
        let groups = read(json!({
            "group_count": 0, "groups": [],
            "grouping": {"applied": true, "definition_ids": ["city"], "member_count": 2, "unassigned_count": 2},
            "district_count": 5, "districts": ["A", "B", "C", "D", "E"],
        }))
        .unwrap();
        assert_eq!(groups.kind, ArchiveGroupingKind::Plan);
        assert_eq!(groups.count, 0);
        // Plan applied, everything unassigned: the archive filed no group.
        assert!(groups.filed_no_group());
    }

    #[test]
    fn nothing_recorded_is_unrecorded_not_zero_districts() {
        let groups = read(json!({"stem": "x"})).unwrap();
        assert_eq!(groups.kind, ArchiveGroupingKind::Unrecorded);
        assert_eq!(groups.key, None);
    }

    #[test]
    fn malformed_grouping_is_refused_typed() {
        let long = "x".repeat(MAX_LABEL_CHARS + 1);
        let many: Vec<String> = (0..=MAX_ARCHIVE_GROUPS).map(|i| format!("g{i}")).collect();
        for (entry, error) in [
            (json!({"group_count": -1}), ArchiveGroupsError::CountInvalid),
            (
                json!({"group_count": "2"}),
                ArchiveGroupsError::CountInvalid,
            ),
            (
                json!({"district_count": 1.5}),
                ArchiveGroupsError::CountInvalid,
            ),
            (json!({"groups": "Huye"}), ArchiveGroupsError::LabelsInvalid),
            (
                json!({"groups": ["Huye", "Huye"]}),
                ArchiveGroupsError::LabelsInvalid,
            ),
            (json!({"groups": [""]}), ArchiveGroupsError::LabelsInvalid),
            (
                json!({"groups": [" Huye"]}),
                ArchiveGroupsError::LabelsInvalid,
            ),
            (json!({"groups": [long]}), ArchiveGroupsError::LabelsInvalid),
            (json!({"groups": [1]}), ArchiveGroupsError::LabelsInvalid),
            (json!({"groups": many}), ArchiveGroupsError::LabelsUnbounded),
            (
                json!({"group_count": 3, "groups": ["A"]}),
                ArchiveGroupsError::CountMismatch,
            ),
            (json!({"grouping": []}), ArchiveGroupsError::PlanInvalid),
            (json!({"grouping": {}}), ArchiveGroupsError::PlanInvalid),
            (
                json!({"grouping": {"applied": "yes"}}),
                ArchiveGroupsError::PlanInvalid,
            ),
            (
                json!({"grouping": {"applied": true, "definition_ids": "city"}}),
                ArchiveGroupsError::PlanInvalid,
            ),
            (
                json!({"grouping": {"applied": true, "revision": -3}}),
                ArchiveGroupsError::PlanInvalid,
            ),
            (
                json!({"group_count": 1, "groups": ["A"], "grouping": {"applied": false}}),
                ArchiveGroupsError::FlatWithGroups,
            ),
        ] {
            assert_eq!(read(entry.clone()), Err(error), "{entry}");
        }
        assert_eq!(
            ArchiveGroupsError::FlatWithGroups.code(),
            "archive_flat_with_groups"
        );
    }
}
