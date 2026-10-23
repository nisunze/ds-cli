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
    /// Booklet front matter; its explicit page role determines consumed input.
    FrontMatter,
}

/// The page roles Front matter holds, in booklet order.
pub const FRONT_MATTER_ROLES: [PageRole; 9] = [
    PageRole::Cover,
    PageRole::Naming,
    PageRole::SheetIndex,
    PageRole::KeyMap,
    PageRole::KeyPlan,
    PageRole::Legend,
    PageRole::GeneralNotes,
    PageRole::LegendNotes,
    PageRole::Revisions,
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Classification {
    pub purpose: Purpose,
    pub role: PageRole,
}

impl Classification {
    /// A pair the document itself may carry. Only the purposes with a closed
    /// role set refuse here; every stored pair of the older purposes still loads.
    pub fn admit(&self) -> Result<(), String> {
        if self.purpose == Purpose::FrontMatter && !FRONT_MATTER_ROLES.contains(&self.role) {
            return Err(
                "print_classification_invalid: Front matter holds only cover, naming, sheet index, key map, key plan, legend, general notes, legend and notes, or revisions pages"
                    .into(),
            );
        }
        Ok(())
    }

    /// What authoring may set on `layout`. MV Sheets hold only the A3 landscape
    /// plan-and-profile sheet: `mv_model / plan_profile` on a layout carrying an
    /// MV plan and profile setup, whose own validation fixes the A3 landscape
    /// page. Stored pairs are not re-judged on load; the catalogue flags them.
    pub fn admit_for(&self, layout: &super::Layout) -> Result<(), String> {
        self.admit()?;
        if self.purpose == Purpose::MvModel
            && (self.role != PageRole::PlanProfile || layout.mv.is_none())
        {
            return Err(format!("print_classification_invalid: {MV_ADMISSION}"));
        }
        Ok(())
    }
}

const MV_ADMISSION: &str = "MV Sheets hold only the A3 landscape plan-and-profile sheet (mv_model / plan_profile on a layout carrying an MV plan and profile setup); classify district and overview maps as project / overview";

/// Why a stored classification is listed as Unclassified instead of under its purpose.
#[derive(Debug, Serialize)]
pub struct Mismatch {
    pub code: &'static str,
    pub message: &'static str,
    pub stored: Classification,
}

/// An MV Sheets row that is not the A3 landscape plan-and-profile sheet.
fn stored_mismatch(row: &Row) -> Option<Mismatch> {
    let stored = row.classification?;
    // A row without saved paper cannot be judged by it; its role still decides.
    let a3_landscape = row.paper.is_none_or(|paper| {
        paper.size == super::Paper::A3 && paper.orientation == super::Orientation::Landscape
    });
    (stored.purpose == Purpose::MvModel && (stored.role != PageRole::PlanProfile || !a3_landscape))
        .then_some(Mismatch {
            code: "print_classification_mv_requires_plan_profile",
            message: MV_ADMISSION,
            stored,
        })
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
    /// A governed standard page held by the project (`report.standard.get
    /// --kind standard`), pinned to the revision the catalogue listed.
    GovernedStandard {
        project_id: String,
        id: String,
        template_id: String,
        revision_id: String,
        /// Older catalogues may list the page without enabling pinned preview.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_sha256: Option<String>,
    },
}

#[derive(Debug, Serialize)]
pub struct Entry {
    #[serde(flatten)]
    pub row: Row,
    pub location: Location,
    /// What a preview of this template renders against, by its admitted purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<SubjectKind>,
    /// A stored classification the catalogue does not admit under its purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mismatch: Option<Mismatch>,
}

impl Entry {
    fn new(row: Row, location: Location) -> Self {
        let mismatch = stored_mismatch(&row);
        let subject = match (&mismatch, row.classification) {
            (None, Some(classification)) => Some(SubjectKind::of(classification)),
            _ => None,
        };
        Self {
            row,
            location,
            subject,
            mismatch,
        }
    }
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

/// One governed standard page under Front matter. Pages of other roles keep
/// their existing consumer (the governed LV set) and stay out of the catalogue.
fn standard_entry(project: &str, row: &Value) -> Result<Option<Entry>, String> {
    let role: PageRole = serde_json::from_value(row["role"].clone())
        .map_err(|_| "print_catalogue_invalid: standard page role unknown")?;
    if !FRONT_MATTER_ROLES.contains(&role) {
        return Ok(None);
    }
    let id = row["id"]
        .as_str()
        .filter(|id| identifier(id))
        .ok_or("print_catalogue_invalid: standard page id invalid")?;
    let template_id = row["template_id"]
        .as_str()
        .filter(|id| identifier(id))
        .ok_or("print_catalogue_invalid: standard page template missing")?;
    let revision_id =
        digest(&row["revision_id"]).ok_or("print_catalogue_invalid: standard page pin missing")?;
    let content_sha256 = if row
        .get("content_sha256")
        .is_some_and(|value| !value.is_null())
    {
        Some(
            digest(&row["content_sha256"])
                .ok_or("print_catalogue_invalid: standard page content pin invalid")?,
        )
    } else {
        None
    };
    let label = ROLES
        .iter()
        .find(|(r, _)| *r == role)
        .map_or("Page", |(_, label)| *label);
    Ok(Some(Entry::new(
        Row {
            id: id.into(),
            name: format!("{label} · {template_id}"),
            paper: serde_json::from_value(
                json!({"size": row["paper"], "orientation": row["orientation"]}),
            )
            .ok(),
            classification: Some(Classification {
                purpose: Purpose::FrontMatter,
                role,
            }),
        },
        Location::GovernedStandard {
            project_id: project.into(),
            id: id.into(),
            template_id: template_id.into(),
            revision_id,
            content_sha256,
        },
    )))
}

/// Admit the project-owned A4 family and the governed front-matter pages from
/// the existing governed catalogue.
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
    let mut standard = Vec::new();
    for row in rows {
        match row["kind"].as_str() {
            Some("standard") => {
                standard.extend(standard_entry(project, row)?);
                continue;
            }
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
        entries.push(Entry::new(
            Row {
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
            Location::GovernedHtml {
                project_id: project.into(),
                id: A4_ID.into(),
                revision_id: content_sha256.clone(),
                content_sha256,
                html_sha256,
            },
        ));
    }
    if entries.len() > 1 {
        return Err("print_catalogue_invalid: duplicate bound A4 document".into());
    }
    entries.extend(standard);
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

const PURPOSES: [(Purpose, &str); 5] = [
    (Purpose::FrontMatter, "Front matter"),
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
        let location = Location::Layout { id: row.id.clone() };
        entries.push(Entry::new(row, location));
    }
    if let Some(capture) = governed {
        entries.extend(governed_entries(
            project_id.ok_or("print_catalogue_invalid: explicit project required")?,
            &capture,
        )?);
    }
    for entry in entries {
        // A stored mismatch is listed as Unclassified, flagged, never under its purpose.
        let key = match (&entry.mismatch, entry.row.classification) {
            (None, Some(c)) => (Some(c.purpose), Some(c.role)),
            _ => (None, None),
        };
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

/// Why a host could not read the governed documents, by its own refusal code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernedUnread {
    pub code: String,
    pub message: String,
}

/// The one grouped catalogue read (`report.layout.list --catalogue`) that the
/// CLI and the Desktop door both answer: the printing list's `setups` and, when
/// it could be read, the project's governed documents (`report.standard.list`).
/// A governed read that failed keeps the custom layouts listed and names the
/// refusal under `governed`; it never hides the setups the list returned.
pub fn from_listing(
    listed: &Value,
    project: &str,
    governed: Result<Value, GovernedUnread>,
) -> Result<Value, String> {
    let rows = listed["setups"]
        .as_array()
        .ok_or("print_catalogue_invalid: the printing list carried no setups")?
        .iter()
        .map(|setup| {
            serde_json::from_value(json!({
                "id": setup["id"],
                "name": setup["name"],
                "paper": setup["paper"],
                "classification": setup["classification"],
            }))
            .map_err(|error| format!("print_catalogue_invalid: {error}"))
        })
        .collect::<Result<Vec<Row>, _>>()?;
    let (capture, status) = match governed {
        Ok(capture) => (Some(capture), json!({"read": true})),
        Err(unread) => (
            None,
            json!({"read": false, "code": unread.code, "message": unread.message}),
        ),
    };
    let mut out = serde_json::to_value(self::project(rows, Some(project), capture)?)
        .map_err(|error| error.to_string())?;
    out["project_id"] = json!(project);
    out["governed"] = status;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn listed() -> Value {
        json!({"setups": [
            {"id":"mv-a3-plan-profile-template","name":"A3 LANDSCAPE · MV PLAN AND PROFILE · TEMPLATE","revision":"r1","layout":null,
             "paper":{"orientation":"landscape","size":"A3"},"classification":{"purpose":"mv_model","role":"plan_profile"}},
            {"id":"nyaruguru-district-mv-a3-landscape-core-v0","name":"A3 LANDSCAPE · NYARUGURU MV DISTRICT MAP · v0","revision":"r2","layout":null,
             "paper":{"orientation":"landscape","size":"A3"},"classification":{"purpose":"mv_model","role":"network"}},
            {"id":"project-overview-a3","name":"Project overview · A3","revision":"r3","layout":null,
             "paper":{"orientation":"landscape","size":"A3"},"classification":{"purpose":"project","role":"overview"}},
            {"id":"print-legacy","name":"A0-Landscape HuYE","revision":"r4","layout":null,"paper":null}
        ]})
    }

    #[test]
    fn the_listing_groups_front_matter_mv_and_overview_sheets_from_one_read() {
        let pin = "a".repeat(64);
        let governed = json!({"scope":"project","project_id":"template_project","schema":"ds.printing-standard-catalog/v1","documents":[
            {"kind":"standard","id":"mv-cover-a3","template_id":"mv-booklet-a3-v1","role":"cover","paper":"A3","orientation":"landscape","revision_id":pin,"bound":false}
        ]});
        let out = from_listing(&listed(), "template_project", Ok(governed)).unwrap();
        assert_eq!(out["project_id"], "template_project");
        assert_eq!(out["governed"], json!({"read": true}));
        let labels: Vec<&str> = out["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["label"].as_str().unwrap())
            .collect();
        assert_eq!(
            labels,
            [
                "Front matter",
                "MV Sheets",
                "Overview Sheets",
                "Unclassified"
            ]
        );
        let front = &out["groups"][0]["roles"][0]["templates"][0];
        assert_eq!(front["location"]["kind"], "governed_standard");
        assert_eq!(front["subject"], "layout");
        let mv = &out["groups"][1]["roles"][0];
        assert_eq!(mv["role"], "plan_profile");
        assert_eq!(mv["templates"].as_array().unwrap().len(), 1);
        assert_eq!(
            out["groups"][2]["roles"][0]["templates"][0]["subject"],
            "extent"
        );
        let unclassified = out["groups"][3]["roles"][0]["templates"]
            .as_array()
            .unwrap();
        assert_eq!(unclassified.len(), 2);
        let district = unclassified
            .iter()
            .find(|t| t["id"] == "nyaruguru-district-mv-a3-landscape-core-v0")
            .unwrap();
        assert_eq!(
            district["mismatch"]["code"],
            "print_classification_mv_requires_plan_profile"
        );
    }

    #[test]
    fn an_unread_governed_catalogue_keeps_the_layouts_and_names_the_refusal() {
        let unread = GovernedUnread {
            code: "print_standard_route_unavailable".into(),
            message: "this deployment does not serve the governed printing read".into(),
        };
        let out = from_listing(&listed(), "template_project", Err(unread.clone())).unwrap();
        assert_eq!(out["governed"]["read"], false);
        assert_eq!(out["governed"]["code"], "print_standard_route_unavailable");
        assert_eq!(out["groups"][0]["label"], "MV Sheets");
        assert!(from_listing(&json!({}), "template_project", Err(unread))
            .unwrap_err()
            .starts_with("print_catalogue_invalid"));
    }

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
                {"value":"front_matter","label":"Front matter"},
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
    fn governed_front_matter_pages_list_under_front_matter_with_their_pins() {
        let pin = "a".repeat(64);
        let capture = json!({"scope":"project","project_id":"project_one","schema":"ds.printing-standard-catalog/v1","documents":[
            {"kind":"standard","id":"mv-cover-a3","template_id":"mv-booklet-a3-v1","role":"cover","paper":"A3","orientation":"landscape","revision_id":pin,"content_sha256":pin,"bound":false},
            {"kind":"standard","id":"mv-naming-a3","template_id":"mv-booklet-a3-v1","role":"naming","paper":"A3","orientation":"landscape","revision_id":pin,"bound":false},
            {"kind":"standard","id":"lv-network-a0","template_id":"lv-transformer-a0-v1","role":"network","paper":"A0","orientation":"landscape","revision_id":pin,"bound":false}
        ]});
        let out = serde_json::to_value(
            project(Vec::new(), Some("project_one"), Some(capture.clone())).unwrap(),
        )
        .unwrap();
        assert_eq!(out["groups"].as_array().unwrap().len(), 1);
        let group = &out["groups"][0];
        assert_eq!(
            (group["purpose"].as_str(), group["label"].as_str()),
            (Some("front_matter"), Some("Front matter"))
        );
        assert_eq!(group["roles"][0]["role"], "cover");
        assert_eq!(group["roles"][1]["role"], "naming");
        let cover = &group["roles"][0]["templates"][0];
        assert_eq!(cover["name"], "Cover · mv-booklet-a3-v1");
        assert_eq!(
            cover["paper"],
            json!({"size":"A3","orientation":"landscape"})
        );
        assert_eq!(
            cover["location"],
            json!({"kind":"governed_standard","project_id":"project_one","id":"mv-cover-a3","template_id":"mv-booklet-a3-v1","revision_id":pin,"content_sha256":pin})
        );
        // An unpinned or unknown-role page refuses the capture instead of guessing.
        for broken in [
            json!({"revision_id": null}),
            json!({"role": "frontispiece"}),
            json!({"template_id": ""}),
        ] {
            let mut changed = capture.clone();
            for (key, value) in broken.as_object().unwrap() {
                changed["documents"][0][key] = value.clone();
            }
            assert!(project(Vec::new(), Some("project_one"), Some(changed)).is_err());
        }
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

/// The kind of subject a template previews against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    Layout,
    Transformer,
    MvModels,
    Extent,
}

impl SubjectKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::Transformer => "transformer",
            Self::MvModels => "mv_models",
            Self::Extent => "extent",
        }
    }
    /// Explicit role narrows front matter to captured page or geographic input.
    pub fn of(classification: Classification) -> Self {
        match classification.purpose {
            Purpose::FrontMatter
                if matches!(classification.role, PageRole::KeyMap | PageRole::KeyPlan) =>
            {
                Self::Extent
            }
            Purpose::FrontMatter => Self::Layout,
            Purpose::MvModel => Self::MvModels,
            Purpose::Project => Self::Extent,
            Purpose::LvTransformer | Purpose::VoltageDrop => Self::Transformer,
        }
    }
}
