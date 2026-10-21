//! Closed authored transport fields; Brain remains the normalizer and evidence owner.
use super::{invalid, note_id_valid};
use crate::{
    project_management::checklist::{Edit, EditAction},
    ClientError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub title: String,
    #[serde(default)]
    pub folder: String,
    pub content: Content,
    #[serde(default)]
    pub checklist: Vec<Item>,
    #[serde(default)]
    pub checklist_kind: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<Geometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder: Option<Reminder>,
    #[serde(default)]
    pub canvas: Canvas,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Content {
    #[serde(default)]
    pub format: String,
    pub markdown: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    #[serde(rename = "type")]
    pub kind: String,
    pub coordinates: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub kind: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub entity_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub form_slug: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub checked: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<Geometry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reminder {
    pub at: String,
    pub timezone: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Canvas {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub z: i64,
    pub color: String,
}
impl Geometry {
    fn validate(&self) -> Result<(), ClientError> {
        if !matches!(self.kind.as_str(), "Point" | "LineString" | "Polygon")
            || !self.coordinates.is_array()
        {
            return Err(invalid());
        }
        // Shape, CRS/range and aggregate positions remain with the existing server owner.
        Ok(())
    }
}
impl Link {
    fn validate(&self) -> Result<(), ClientError> {
        if !note_id_valid(&self.project_id)
            || self.project_id != self.project_id.trim()
            || self.label.chars().count() > 200
        {
            return Err(invalid());
        }
        let stable_entity =
            note_id_valid(&self.entity_id) && self.entity_id == self.entity_id.trim();
        let valid = match self.kind.as_str() {
            "project" => self.entity_id.is_empty() && self.form_slug.is_empty(),
            "pm_task" | "project_record" | "deliverable_version" => {
                stable_entity && self.form_slug.is_empty()
            }
            "survey_entry" => {
                stable_entity
                    && note_id_valid(&self.form_slug)
                    && self.form_slug == self.form_slug.trim()
            }
            // Private notes cannot adopt DS-object pins or write another scope.
            _ => false,
        };
        if !valid {
            return Err(invalid());
        }
        Ok(())
    }
}
impl Draft {
    pub fn validate(&self, creating: bool) -> Result<(), ClientError> {
        if self.title.chars().count() > 300
            || self.folder.chars().count() > 200
            || self.folder.contains(['\r', '\n', '\0'])
            || !matches!(self.content.format.as_str(), "" | "markdown")
            || self.content.markdown.len() > 128 << 10
            || self.tags.len() > 24
            || self.tags.iter().any(|t| t.chars().count() > 64)
            || self.links.len() > 128
            || self.checklist.len() > 200
            || !matches!(self.checklist_kind.as_str(), "" | "checklist" | "issues")
        {
            return Err(invalid());
        }
        for link in &self.links {
            link.validate()?;
        }
        if let Some(g) = &self.geometry {
            g.validate()?;
        }
        if let Some(r) = &self.reminder {
            if time::OffsetDateTime::parse(&r.at, &time::format_description::well_known::Rfc3339)
                .is_err()
                || r.timezone.trim().is_empty()
                || r.timezone.len() > 100
                || !matches!(r.status.as_str(), "" | "scheduled" | "dismissed")
            {
                return Err(invalid());
            }
        }
        let c = &self.canvas;
        if [c.x, c.y, c.width, c.height]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
            || (c.width != 0.0 && !(220.0..=1200.0).contains(&c.width))
            || (c.height != 0.0 && !(140.0..=1200.0).contains(&c.height))
            || !(0..=100000).contains(&c.z)
            || !matches!(
                c.color.as_str(),
                "" | "sand" | "rose" | "mint" | "sky" | "lilac" | "slate"
            )
        {
            return Err(invalid());
        }
        let mut ids = BTreeSet::new();
        for item in &self.checklist {
            if !ids.insert(item.id.trim()) || (creating && !item.attachment_ids.is_empty()) {
                return Err(invalid());
            }
            for link in &item.links {
                link.validate()?;
            }
            if let Some(g) = &item.geometry {
                g.validate()?;
            }
            // Reuse the shared item transport validator rather than forking its bounds.
            Edit {
                action: EditAction::Edit,
                item_id: item.id.clone(),
                checklist_kind: None,
                text: Some(item.text.clone()),
                body: Some(item.body.clone()),
                answer: None,
                geometry: item
                    .geometry
                    .as_ref()
                    .map(|g| serde_json::to_value(g).expect("closed geometry")),
                clear_geometry: false,
                links: Some(
                    item.links
                        .iter()
                        .map(|l| serde_json::to_value(l).expect("closed link"))
                        .collect(),
                ),
                attachment_ids: Some(item.attachment_ids.clone()),
                index: None,
            }
            .validate()
            .map_err(|_| invalid())?;
            Edit {
                action: EditAction::Check,
                item_id: item.id.clone(),
                checklist_kind: None,
                text: None,
                body: None,
                answer: Some(item.answer.clone()),
                geometry: None,
                clear_geometry: false,
                links: None,
                attachment_ids: None,
                index: None,
            }
            .validate()
            .map_err(|_| invalid())?;
        }
        if serde_json::to_vec(&self.checklist)
            .map_err(|_| invalid())?
            .len()
            > 192 << 10
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn verify_authored_receipt(&self, note: &Value) -> Result<(), ClientError> {
        if note["title"].as_str() != Some(self.title.trim())
            || note["folder"].as_str().unwrap_or("") != self.folder.trim()
            || note["content"] != json!({"format":"markdown","markdown":self.content.markdown})
            || note["pinned"] != self.pinned
            || note["archived"] != self.archived
            || note["trashed"] != false
        {
            return Err(super::unreadable());
        }
        let tags: BTreeSet<_> = self
            .tags
            .iter()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let returned = note["tags"].as_array().ok_or_else(super::unreadable)?;
        let actual: Option<BTreeSet<_>> = returned
            .iter()
            .map(|t| t.as_str().map(str::to_owned))
            .collect();
        if actual.as_ref() != Some(&tags) || returned.len() != tags.len() {
            return Err(super::unreadable());
        }
        let items = note["checklist"].as_array().ok_or_else(super::unreadable)?;
        if items.len() != self.checklist.len()
            || items.iter().zip(&self.checklist).any(|(got, want)| {
                got["id"].as_str() != Some(want.id.trim())
                    || got["text"].as_str() != Some(want.text.trim())
                    || got["checked"] != want.checked
            })
        {
            return Err(super::unreadable());
        }
        // The owner normalizes defaults and audit fields. Compare authored
        // values only; operational reminder delivery state is not authored.
        let c = &self.canvas;
        let canvas = &note["canvas"];
        if canvas["x"].as_f64() != Some(c.x)
            || canvas["y"].as_f64() != Some(c.y)
            || canvas["width"].as_f64() != Some(if c.width == 0.0 { 300.0 } else { c.width })
            || canvas["height"].as_f64() != Some(if c.height == 0.0 { 240.0 } else { c.height })
            || canvas["z"].as_i64() != Some(c.z)
            || canvas["color"].as_str() != Some(if c.color.is_empty() { "sand" } else { &c.color })
            || !geometry_matches(self.geometry.as_ref(), &note["geometry"])
            || !links_match(&self.links, &note["links"])
        {
            return Err(super::unreadable());
        }
        match &self.reminder {
            None if !note["reminder"].is_null() => return Err(super::unreadable()),
            Some(want) => {
                let parse = |s: &str| {
                    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                        .ok()
                };
                let expected = parse(&want.at).map(|t| t.unix_timestamp_nanos() / 1000);
                let actual = note["reminder"]["at"]
                    .as_str()
                    .and_then(parse)
                    .map(|t| t.unix_timestamp_nanos() / 1000);
                if expected.is_none()
                    || actual != expected
                    || note["reminder"]["timezone"].as_str() != Some(want.timezone.trim())
                {
                    return Err(super::unreadable());
                }
            }
            _ => {}
        }
        for (got, want) in items.iter().zip(&self.checklist) {
            // Simple legacy rows deliberately retain rich fields. A supplied
            // rich row replaces them, while server audit stamps remain private.
            let rich = !want.body.is_empty()
                || !want.answer.is_empty()
                || want.geometry.is_some()
                || !want.links.is_empty()
                || !want.attachment_ids.is_empty();
            if rich
                && (got["body"].as_str().unwrap_or("") != want.body
                    || got["answer"].as_str().unwrap_or("") != want.answer
                    || !geometry_matches(want.geometry.as_ref(), &got["geometry"])
                    || !links_match(&want.links, &got["links"])
                    || array(&got["attachment_ids"]).is_none_or(|a| {
                        a != want
                            .attachment_ids
                            .iter()
                            .map(|s| json!(s))
                            .collect::<Vec<_>>()
                    }))
            {
                return Err(super::unreadable());
            }
        }
        Ok(())
    }
}
fn array(value: &Value) -> Option<&[Value]> {
    if value.is_null() {
        Some(&[])
    } else {
        value.as_array().map(Vec::as_slice)
    }
}
fn json_numbers_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_numbers_equal(a, b))
        }
        _ => a == b,
    }
}
fn geometry_matches(want: Option<&Geometry>, got: &Value) -> bool {
    match want {
        None => got.is_null(),
        Some(want) => {
            got["type"].as_str() == Some(&want.kind)
                && json_numbers_equal(&want.coordinates, &got["coordinates"])
        }
    }
}
fn links_match(want: &[Link], got: &Value) -> bool {
    let mut seen = BTreeSet::new();
    let expected: Vec<_> = want
        .iter()
        .filter(|l| seen.insert((&l.kind, &l.project_id, &l.form_slug, &l.entity_id)))
        .collect();
    let Some(actual) = array(got) else {
        return false;
    };
    expected.len() == actual.len()
        && expected.iter().zip(actual).all(|(want, got)| {
            got["kind"].as_str() == Some(&want.kind)
                && got["project_id"].as_str() == Some(&want.project_id)
                && got["entity_id"].as_str().unwrap_or("") == want.entity_id
                && got["form_slug"].as_str().unwrap_or("") == want.form_slug
                && got["label"].as_str().unwrap_or("") == want.label.trim()
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn draft() -> Draft {
        serde_json::from_value(json!({"title":"Private","content":{"markdown":"Body"}})).unwrap()
    }
    #[test]
    fn closed_authored_fields_never_accept_identity_or_model_metadata() {
        let mut raw = serde_json::to_value(draft()).unwrap();
        for field in [
            "owner_uid",
            "version",
            "created_by",
            "authoring_model",
            "trashed",
        ] {
            raw[field] = json!("spoof");
            assert!(serde_json::from_value::<Draft>(raw.clone()).is_err());
            raw.as_object_mut().unwrap().remove(field);
        }
        let mut d = draft();
        d.content.markdown = "x".repeat((128 << 10) + 1);
        assert!(d.validate(true).is_err());
        let mut d = draft();
        d.checklist = serde_json::from_value(
            json!([{"id":"i","text":"Do","checked":false,"attachment_ids":["a"]}]),
        )
        .unwrap();
        assert!(d.validate(true).is_err());
        assert!(d.validate(false).is_ok());
    }
    #[test]
    fn rich_items_share_bounds_and_refuse_cross_scope_pins() {
        let mut d = draft();
        d.checklist = serde_json::from_value(
            json!([{"id":"i","text":"Do","checked":true,"answer":"Done","body":"Evidence"}]),
        )
        .unwrap();
        d.validate(true).unwrap();
        d.checklist[0].answer = "x".repeat((128 << 10) + 1);
        assert!(d.validate(false).is_err());
        let mut raw = serde_json::to_value(draft()).unwrap();
        raw["links"] = json!([{"kind":"ds_object","project_id":"p","entity_id":"tr"}]);
        let d: Draft = serde_json::from_value(raw).unwrap();
        assert!(d.validate(true).is_err());
    }
}
