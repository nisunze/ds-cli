//! Explicit template organization shared by native and web consumers.
//! Names, paper formats and retired layout fields never decide classification.
use super::{page_role::PageRole, PaperSpec};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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

/// Storage identity is explicit: an HTML printing document never becomes a Layout.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Location {
    Layout {
        id: String,
    },
    GovernedHtml {
        project_id: String,
        id: String,
        revision_id: String,
        content_sha256: String,
        html_sha256: String,
    },
}

#[derive(Debug, Serialize)]
pub struct Entry {
    #[serde(flatten)]
    pub row: Row,
    pub location: Location,
}

#[derive(Debug, Serialize)]
pub struct HtmlDocument {
    pub location: Location,
    pub name: &'static str,
    pub html: String,
}

const A4_ID: &str = "voltage-drop-a4-v1";
const A4_NAME: &str = "LV voltage-drop report";

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn digest(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .map(str::to_owned)
}

/// Admit only the project-owned A4 family from the existing governed catalogue.
/// Standard booklet documents have a different consumer and remain outside this slice.
fn governed_entries(project: &str, capture: &Value) -> Result<Vec<Entry>, String> {
    let rows = capture["documents"]
        .as_array()
        .ok_or("print_catalogue_invalid: governed documents missing")?;
    if !identifier(project)
        || capture["scope"] != "project"
        || capture["project_id"] != project
        || capture["schema"] != "ds.printing-standard-catalog/v1"
        || rows.len() > 72
    {
        return Err(
            "print_catalogue_invalid: governed catalogue differs from captured project".into(),
        );
    }
    let mut entries = Vec::new();
    for row in rows {
        match row["kind"].as_str() {
            Some("standard") => continue,
            Some("a4") => {}
            _ => return Err("print_catalogue_invalid: unknown governed document kind".into()),
        }
        // Only the bound canonical document belongs to the existing A4 consumer.
        if row["id"] != A4_ID || row["bound"] != true {
            continue;
        }
        let content_sha256 = digest(&row["content_sha256"])
            .ok_or("print_catalogue_invalid: A4 content pin missing")?;
        let html_sha256 =
            digest(&row["html_sha256"]).ok_or("print_catalogue_invalid: A4 HTML pin missing")?;
        if row["schema"] != "ds.print-a4-default/v1"
            || row["paper"] != "A4"
            || row["orientation"] != "portrait"
        {
            return Err("print_catalogue_invalid: A4 schema or paper differs".into());
        }
        entries.push(Entry {
            row: Row {
                id: A4_ID.into(),
                name: A4_NAME.into(),
                paper: Some(PaperSpec {
                    size: super::Paper::A4,
                    orientation: super::Orientation::Portrait,
                }),
                classification: Some(Classification {
                    purpose: Purpose::VoltageDrop,
                    role: PageRole::Report,
                }),
            },
            location: Location::GovernedHtml {
                project_id: project.into(),
                id: A4_ID.into(),
                revision_id: content_sha256.clone(),
                content_sha256,
                html_sha256,
            },
        });
    }
    if entries.len() > 1 {
        return Err("print_catalogue_invalid: duplicate bound A4 document".into());
    }
    Ok(entries)
}

/// Check the exact `report.standard.get` response against the held catalogue pins.
/// The returned bytes remain the governed HTML source, with no sample/result injection.
pub fn read_governed(location: Location, response: Value) -> Result<HtmlDocument, String> {
    let Location::GovernedHtml {
        project_id,
        id,
        revision_id,
        content_sha256,
        html_sha256,
    } = &location
    else {
        return Err("print_document_invalid: an HTML document location is required".into());
    };
    let doc = &response["document"];
    if !identifier(project_id)
        || id != A4_ID
        || response["scope"] != "project"
        || response["project_id"] != *project_id
        || response["kind"] != "a4"
        || response["id"] != *id
        || response["html_available"] != true
        || doc["schema"] != "ds.print-a4-document/v1"
        || doc["project_id"] != *project_id
        || response["revision_id"] != *revision_id
        || doc["revision_id"] != *revision_id
        || response["content_sha256"] != *content_sha256
        || doc["content_sha256"] != *content_sha256
        || response["html_sha256"] != *html_sha256
        || revision_id != content_sha256
    {
        return Err(
            "print_document_invalid: governed response differs from held project or revision"
                .into(),
        );
    }
    let html = doc["html"]
        .as_str()
        .ok_or("print_document_invalid: HTML missing")?;
    if html.is_empty()
        || html.len() > 4 * 1024 * 1024
        || response["html"] != html
        || format!("{:x}", Sha256::digest(html.as_bytes())) != *html_sha256
        || super::resolved_styles::content_digest(
            &json!({"html":html,"status_layout":doc["status_layout"],"logo_slots":doc["logo_slots"]}),
        )? != *content_sha256
    {
        return Err("print_document_invalid: governed HTML or content digest differs".into());
    }
    Ok(HtmlDocument {
        location,
        name: A4_NAME,
        html: html.into(),
    })
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
    pub templates: Vec<Entry>,
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
    (Purpose::LvTransformer, "LV Sheets"),
    (Purpose::MvModel, "MV Sheets"),
    (Purpose::Project, "Overview Sheets"),
    (Purpose::VoltageDrop, "Voltage Drop"),
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

pub fn project(
    rows: Vec<Row>,
    project_id: Option<&str>,
    governed: Option<Value>,
) -> Result<Catalogue, String> {
    if rows.len() > 200 {
        return Err("print_catalogue_bound: at most 200 named templates".into());
    }
    let mut ids = BTreeSet::new();
    let mut grouped = BTreeMap::<(Option<Purpose>, Option<PageRole>), Vec<Entry>>::new();
    let mut entries = Vec::new();
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
        entries.push(Entry {
            location: Location::Layout { id: row.id.clone() },
            row,
        });
    }
    if let Some(capture) = governed {
        entries.extend(governed_entries(
            project_id.ok_or("print_catalogue_invalid: explicit project required")?,
            &capture,
        )?);
    }
    for entry in entries {
        let key = entry
            .row
            .classification
            .map(|c| (Some(c.purpose), Some(c.role)))
            .unwrap_or((None, None));
        grouped.entry(key).or_default().push(entry);
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
                    a.row
                        .name
                        .to_lowercase()
                        .cmp(&b.row.name.to_lowercase())
                        .then(a.row.name.cmp(&b.row.name))
                        .then(a.row.id.cmp(&b.row.id))
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
        let out = serde_json::to_value(project(rows, None, None).unwrap()).unwrap();
        assert_eq!(
            out["purposes"],
            json!([
                {"value":"lv_transformer","label":"LV Sheets"},
                {"value":"mv_model","label":"MV Sheets"},
                {"value":"project","label":"Overview Sheets"},
                {"value":"voltage_drop","label":"Voltage Drop"}
            ])
        );
        assert_eq!(out["groups"][0]["purpose"], "mv_model");
        assert_eq!(out["groups"][0]["label"], "MV Sheets");
        assert_eq!(out["groups"][1]["label"], "Overview Sheets");
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

    fn governed_capture() -> (Value, Value) {
        let html = "<!doctype html><html><body>Governed document source</body></html>";
        let payload = json!({"html":html,"status_layout":{"paper":{"size":"A4","orientation":"portrait"}},"logo_slots":[]});
        let content = super::super::resolved_styles::content_digest(&payload).unwrap();
        let html_pin = format!("{:x}", Sha256::digest(html.as_bytes()));
        let catalogue = json!({"scope":"project","project_id":"project_one","schema":"ds.printing-standard-catalog/v1","documents":[{
            "kind":"a4","id":A4_ID,"bound":true,"schema":"ds.print-a4-default/v1","paper":"A4","orientation":"portrait","content_sha256":content,"html_sha256":html_pin
        }]});
        let mut document = payload;
        document["schema"] = json!("ds.print-a4-document/v1");
        document["project_id"] = json!("project_one");
        document["revision_id"] = json!(content);
        document["content_sha256"] = json!(content);
        let response = json!({"scope":"project","project_id":"project_one","kind":"a4","id":A4_ID,
            "revision_id":content,"content_sha256":content,"html_sha256":html_pin,"html":html,"html_available":true,"document":document});
        (catalogue, response)
    }

    #[test]
    fn governed_a4_keeps_document_identity_separate_from_a_layout_with_same_id() {
        let (capture, response) = governed_capture();
        let rows =
            serde_json::from_value(json!([{"id":A4_ID,"name":"Ordinary custom layout"}])).unwrap();
        let catalogue = project(rows, Some("project_one"), Some(capture.clone())).unwrap();
        let a4 = &catalogue.groups[0].roles[0].templates[0];
        assert_eq!(catalogue.groups[0].purpose, Some(Purpose::VoltageDrop));
        assert_eq!(catalogue.groups[0].roles[0].role, Some(PageRole::Report));
        assert!(matches!(a4.location, Location::GovernedHtml { .. }));
        assert!(matches!(
            catalogue.groups[1].roles[0].templates[0].location,
            Location::Layout { .. }
        ));
        assert_eq!(
            read_governed(a4.location.clone(), response.clone())
                .unwrap()
                .html,
            response["html"]
        );
        assert!(project(Vec::new(), Some("another_project"), Some(capture)).is_err());
        let mut changed = response.clone();
        changed["document"]["html"] = json!("changed HTML");
        changed["html"] = json!("changed HTML");
        assert!(read_governed(a4.location.clone(), changed).is_err());
        let mut changed = response.clone();
        changed["document"]["status_layout"]["paper"]["size"] = json!("A3");
        assert!(read_governed(a4.location.clone(), changed).is_err());
        let mut changed = response;
        changed["project_id"] = json!("another_project");
        assert!(read_governed(a4.location.clone(), changed).is_err());
        assert!(read_governed(Location::Layout { id: A4_ID.into() }, Value::Null).is_err());
    }
}
