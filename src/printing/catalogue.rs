//! Explicit template organization shared by native and web consumers.
//! Names, paper formats and retired layout fields never decide classification.
use super::{page_role::PageRole, PaperSpec};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    LvTransformer,
    MvModel,
    Project,
    VoltageDrop,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Classification {
    pub purpose: Purpose,
    pub role: PageRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<PaperSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
}

#[derive(Debug, Serialize)]
pub struct Choice<T> {
    pub value: T,
    pub label: &'static str,
}
#[derive(Debug, Serialize)]
pub struct RoleGroup {
    pub role: Option<PageRole>,
    pub label: &'static str,
    pub templates: Vec<Row>,
}
#[derive(Debug, Serialize)]
pub struct Group {
    pub purpose: Option<Purpose>,
    pub label: &'static str,
    pub roles: Vec<RoleGroup>,
}
#[derive(Debug, Serialize)]
pub struct Catalogue {
    pub groups: Vec<Group>,
    pub purposes: Vec<Choice<Purpose>>,
    pub roles: Vec<Choice<PageRole>>,
}

const PURPOSES: [(Purpose, &str); 4] = [
    (Purpose::LvTransformer, "LV sheets"),
    (Purpose::MvModel, "MV sheets"),
    (Purpose::Project, "Project pages"),
    (Purpose::VoltageDrop, "Voltage drop"),
];
const ROLES: [(PageRole, &str); 15] = [
    (PageRole::Cover, "Cover"),
    (PageRole::Naming, "Naming"),
    (PageRole::SheetIndex, "Sheet index"),
    (PageRole::KeyMap, "Key map"),
    (PageRole::KeyPlan, "Key plan"),
    (PageRole::Legend, "Legend"),
    (PageRole::GeneralNotes, "General notes"),
    (PageRole::LegendNotes, "Legend and notes"),
    (PageRole::Revisions, "Revisions"),
    (PageRole::Network, "Network"),
    (PageRole::Schedule, "Schedule"),
    (PageRole::PlanProfile, "Plan and profile"),
    (PageRole::Context, "Context"),
    (PageRole::Overview, "Overview"),
    (PageRole::Report, "Report"),
];

pub fn project(rows: Vec<Row>) -> Result<Catalogue, String> {
    if rows.len() > 200 {
        return Err("print_catalogue_bound: at most 200 named templates".into());
    }
    let mut ids = BTreeSet::new();
    let mut grouped = BTreeMap::<(Option<Purpose>, Option<PageRole>), Vec<Row>>::new();
    for row in rows {
        if row.id.is_empty()
            || row.id.len() > 80
            || row.name.trim().is_empty()
            || row.name.len() > 200
            || !ids.insert(row.id.clone())
        {
            return Err(
                "print_catalogue_invalid: unique bounded id and template name required".into(),
            );
        }
        let key = row
            .classification
            .map(|c| (Some(c.purpose), Some(c.role)))
            .unwrap_or((None, None));
        grouped.entry(key).or_default().push(row);
    }
    let mut groups = Vec::new();
    for (purpose, label) in PURPOSES
        .iter()
        .map(|(p, l)| (Some(*p), *l))
        .chain([(None, "Unclassified")])
    {
        let mut roles = Vec::new();
        for (role, role_label) in ROLES
            .iter()
            .map(|(r, l)| (Some(*r), *l))
            .chain([(None, "Unclassified")])
        {
            if let Some(mut templates) = grouped.remove(&(purpose, role)) {
                templates.sort_by(|a, b| {
                    a.name
                        .to_lowercase()
                        .cmp(&b.name.to_lowercase())
                        .then(a.name.cmp(&b.name))
                        .then(a.id.cmp(&b.id))
                });
                roles.push(RoleGroup {
                    role,
                    label: role_label,
                    templates,
                });
            }
        }
        if !roles.is_empty() {
            groups.push(Group {
                purpose,
                label,
                roles,
            });
        }
    }
    Ok(Catalogue {
        groups,
        purposes: PURPOSES
            .into_iter()
            .map(|(value, label)| Choice { value, label })
            .collect(),
        roles: ROLES
            .into_iter()
            .map(|(value, label)| Choice { value, label })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_groups_do_not_classify_legacy_names_or_paper() {
        let rows = serde_json::from_value(json!([
            {"id":"legacy","name":"LV A4 voltage drop overview","paper":{"size":"A4","orientation":"portrait"}},
            {"id":"z","name":"Zulu","classification":{"purpose":"project","role":"overview"}},
            {"id":"a","name":"Alpha","classification":{"purpose":"project","role":"overview"}},
            {"id":"mv","name":"A0 LV","classification":{"purpose":"mv_model","role":"plan_profile"}}
        ])).unwrap();
        let out = serde_json::to_value(project(rows).unwrap()).unwrap();
        assert_eq!(out["groups"][0]["purpose"], "mv_model");
        assert_eq!(out["groups"][1]["roles"][0]["role"], "overview");
        assert_eq!(out["groups"][1]["roles"][0]["templates"][0]["id"], "a");
        assert_eq!(out["groups"][2]["label"], "Unclassified");
        assert_eq!(out["groups"][2]["roles"][0]["templates"][0]["id"], "legacy");
        assert_eq!(
            out["groups"][2]["roles"][0]["templates"][0]["paper"]["size"],
            "A4"
        );
    }

    #[test]
    fn classification_edit_preserves_body_and_clear_restores_legacy_shape() {
        let original = serde_json::to_value(super::super::default_layout()).unwrap();
        assert!(original.get("classification").is_none());
        let run = |request| -> Result<serde_json::Value, String> {
            let bytes = serde_json::to_vec(&request).unwrap();
            let output = super::super::evaluate(&bytes)?;
            serde_json::from_str(&output).map_err(|e| e.to_string())
        };
        let classified = run(json!({"op":"set_classification","layout":original,"classification":{"purpose":"lv_transformer","role":"network"}})).unwrap();
        let mut expected = original.clone();
        expected["classification"] = json!({"purpose":"lv_transformer","role":"network"});
        assert_eq!(classified["layout"], expected);
        assert_eq!(run(json!({"op":"set_classification","layout":classified["layout"],"classification":null})).unwrap()["layout"], original);
        assert!(run(json!({"op":"set_classification","layout":original,"classification":{"purpose":"a4","role":"report"}})).is_err());
        assert!(run(json!({"op":"set_classification","layout":original,"classification":{"purpose":"project","role":"guess","paper":"A0"}})).is_err());
    }
}
