//! Closed item-level checklist transport over the existing PM command owner.
//! Membership, ownership, list state, promotion and geometry remain server
//! decisions. No graph is modified or derived by this transport adapter.
use super::{project_note_id_valid, Command};
use crate::{ClientError, ErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const MAX_EDIT_BYTES: usize = 192 << 10;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditAction {
    Add,
    Edit,
    Check,
    Uncheck,
    Move,
    Remove,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    Checklist,
    Issues,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub action: EditAction,
    pub item_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checklist_kind: Option<ListKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_geometry: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Promotion {
    pub item_id: String,
    pub child_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_emails: Vec<String>,
}
// Match projectwork.validLinkPart for embedded item/attachment identities.
// These are array values, not document paths; admitted interior spaces stay exact.
fn item_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id != "."
        && id != ".."
        && !id
            .chars()
            .any(|c| matches!(c, '/' | '\\' | '\r' | '\n' | '\t'))
}
fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}
impl Edit {
    pub fn validate(&self) -> Result<(), ClientError> {
        if !item_id_valid(&self.item_id)
            || self
                .text
                .as_ref()
                .is_some_and(|text| text.chars().count() > 1000)
            || self
                .body
                .as_ref()
                .is_some_and(|body| body.len() > 128 << 10)
            || self
                .answer
                .as_ref()
                .is_some_and(|answer| answer.len() > 128 << 10)
            || self
                .links
                .as_ref()
                .is_some_and(|links| links.len() > 64 || links.iter().any(|link| !link.is_object()))
            || self.index.is_some_and(|index| index >= 200)
        {
            return Err(invalid(
                "checklist edit exceeds its item/text/links/index transport bound",
            ));
        }
        if let Some(ids) = &self.attachment_ids {
            if ids.len() > 20
                || ids.iter().any(|id| !item_id_valid(id))
                || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
            {
                return Err(invalid(
                    "checklist item references at most 20 distinct stable task-held asset ids",
                ));
            }
        }
        if self.geometry.is_some() && self.clear_geometry {
            return Err(invalid(
                "geometry and clear_geometry are mutually exclusive",
            ));
        }
        if self
            .geometry
            .as_ref()
            .is_some_and(|geometry| !geometry.is_object())
        {
            return Err(invalid("checklist geometry must be a GeoJSON object"));
        }
        let content = self.text.is_some()
            || self.body.is_some()
            || self.geometry.is_some()
            || self.clear_geometry
            || self.links.is_some()
            || self.attachment_ids.is_some();
        if !matches!(self.action, EditAction::Add | EditAction::Edit) && content {
            return Err(invalid("checklist content belongs to add or edit"));
        }
        if self.answer.is_some() && self.action != EditAction::Check {
            return Err(invalid("checklist answer belongs only to check"));
        }
        if self.index.is_some() && !matches!(self.action, EditAction::Add | EditAction::Move) {
            return Err(invalid("checklist index belongs only to add or move"));
        }
        if self.action == EditAction::Move && self.index.is_none() {
            return Err(invalid(
                "moving a checklist item requires an explicit index",
            ));
        }
        if self.action == EditAction::Add
            && self.text.as_ref().is_none_or(|text| text.trim().is_empty())
        {
            return Err(invalid("adding a checklist item requires text"));
        }
        if serde_json::to_vec(self)
            .map_err(|_| invalid("checklist edit is not JSON"))?
            .len()
            > MAX_EDIT_BYTES
        {
            return Err(invalid("checklist edit exceeds 192 KiB"));
        }
        Ok(())
    }
    pub fn command(&self, task: &str, id: &str, revision: i64) -> Result<Command, ClientError> {
        self.validate()?;
        let mut payload =
            serde_json::to_value(self).map_err(|_| invalid("checklist edit is not JSON"))?;
        payload["task_id"] = json!(task);
        command(task, id, revision, "edit_checklist", payload)
    }
}
impl Promotion {
    pub fn command(&self, task: &str, id: &str, revision: i64) -> Result<Command, ClientError> {
        if !item_id_valid(&self.item_id)
            || !project_note_id_valid(&self.child_id)
            || self.request_emails.len() > 20
        {
            return Err(invalid(
                "promotion needs stable item/child ids and at most 20 recipients",
            ));
        }
        let mut payload =
            serde_json::to_value(self).map_err(|_| invalid("checklist promotion is not JSON"))?;
        payload["task_id"] = json!(task);
        command(task, id, revision, "promote_checklist_item", payload)
    }
}
fn command(
    task: &str,
    id: &str,
    revision: i64,
    kind: &str,
    payload: Value,
) -> Result<Command, ClientError> {
    if !project_note_id_valid(task) || revision == i64::MAX {
        return Err(invalid(
            "checklist command needs a stable task and incrementable revision",
        ));
    }
    let command = Command::Commit {
        command_id: id.to_owned(),
        base_revision: revision,
        command: json!({"kind":kind,kind:payload}),
    };
    command.validate()?;
    Ok(command)
}

/// Preserve the server's projection, including item state/geometry/links and
/// overlays. A client must not recompute promoted-child completion locally.
pub fn projection(reply: &Value) -> Result<Value, ClientError> {
    let checklist = reply
        .get("checklist")
        .filter(|value| value.is_object())
        .ok_or_else(|| {
            ClientError::new(
                ErrorKind::UnreadableResponse,
                "task checklist projection is missing",
            )
        })?;
    let items = checklist["items"].as_array().ok_or_else(|| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "task checklist items are missing",
        )
    })?;
    if !matches!(checklist["kind"].as_str(), Some("checklist" | "issues"))
        || items.iter().any(|item| {
            !item["id"].as_str().is_some_and(item_id_valid)
                || !matches!(item["state"].as_str(), Some("open" | "done" | "answered"))
        })
        || items.len() > 200
        || checklist["total"].as_u64() != Some(items.len() as u64)
        || checklist["completed"]
            .as_u64()
            .is_none_or(|completed| completed > items.len() as u64)
    {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "task checklist projection is unbounded or inconsistent",
        ));
    }
    Ok(checklist.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(value: Value) -> Edit {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn one_edit_keeps_item_scope_absence_and_explicit_clears() {
        let input = edit(
            json!({"action":"edit","item_id":"item-1","body":"","attachment_ids":[],"clear_geometry":true}),
        );
        let Command::Commit {
            command_id,
            base_revision,
            command,
        } = input.command("task-1", "retry-0001", 7).unwrap()
        else {
            panic!()
        };
        assert_eq!(command_id, "retry-0001");
        assert_eq!(base_revision, 7);
        assert_eq!(
            command,
            json!({"kind":"edit_checklist","edit_checklist":{"task_id":"task-1","action":"edit","item_id":"item-1","body":"","attachment_ids":[],"clear_geometry":true}})
        );
    }
    #[test]
    fn closed_inputs_reject_overwrite_and_unsupported_fields() {
        assert!(serde_json::from_value::<Edit>(
            json!({"action":"edit","item_id":"i","checked":true})
        )
        .is_err());
        for value in [
            json!({"action":"move","item_id":"i"}),
            json!({"action":"check","item_id":"i","body":"x"}),
            json!({"action":"edit","item_id":"i","answer":"x"}),
            json!({"action":"add","item_id":"i"}),
            json!({"action":"edit","item_id":"i","geometry":{},"clear_geometry":true}),
            json!({"action":"edit","item_id":"i","attachment_ids":["a","a"]}),
        ] {
            assert!(edit(value).validate().is_err())
        }
    }
    #[test]
    fn assets_geometry_and_answer_are_only_selected_item_values() {
        let value = json!({"action":"edit","item_id":"i","geometry":{"type":"Point","coordinates":[30,-2]},"links":[{"kind":"ds_object","target_id":"x"}],"attachment_ids":["asset-1"]});
        let input = edit(value.clone());
        let Command::Commit { command, .. } = input.command("t", "retry-0002", 8).unwrap() else {
            panic!()
        };
        assert_eq!(command["edit_checklist"]["geometry"], value["geometry"]);
        assert_eq!(
            command["edit_checklist"]["attachment_ids"],
            json!(["asset-1"])
        );
        assert!(
            edit(json!({"action":"check","item_id":"i","answer":"resolved"}))
                .validate()
                .is_ok()
        );
    }
    #[test]
    fn embedded_id_transport_preserves_legacy_spaces_and_refuses_path_separators() {
        let input = edit(
            json!({"action":"edit","item_id":"legacy item","attachment_ids":["legacy asset"]}),
        );
        assert!(input.validate().is_ok());
        assert!(edit(json!({"action":"edit","item_id":"bad\\id"}))
            .validate()
            .is_err());
        let view = json!({"checklist":{"kind":"checklist","items":[{"id":"legacy item","state":"open"}],"total":1,"completed":0}});
        assert_eq!(projection(&view).unwrap(), view["checklist"]);
    }
    #[test]
    fn promotion_is_one_existing_command_with_stable_identity() {
        let promotion = Promotion {
            item_id: "i".into(),
            child_id: "child-1".into(),
            request_emails: vec!["member@example.com".into()],
        };
        let command = promotion.command("t", "retry-0003", 9).unwrap();
        assert_eq!(command, promotion.command("t", "retry-0003", 9).unwrap());
        let Command::Commit { command, .. } = command else {
            panic!()
        };
        assert_eq!(
            command,
            json!({"kind":"promote_checklist_item","promote_checklist_item":{"task_id":"t","item_id":"i","child_id":"child-1","request_emails":["member@example.com"]}})
        );
    }
    #[test]
    fn read_preserves_server_derived_promoted_state_and_overlay() {
        let raw = json!({"checklist":{"kind":"issues","items":[{"id":"i","state":"answered","promoted_to":"child","geometry":{"type":"Point","coordinates":[30,-2]}}],"total":1,"completed":1,"overlay":{"type":"MultiPoint","coordinates":[[30,-2]]}}});
        assert_eq!(projection(&raw).unwrap(), raw["checklist"]);
        assert!(projection(&json!({"checklist":{"items":[],"total":3,"completed":0}})).is_err());
        assert!(projection(
            &json!({"checklist":{"kind":"other","items":[],"total":0,"completed":0}})
        )
        .is_err());
        assert!(projection(&json!({"checklist":{"kind":"checklist","items":[{"id":"i","state":"invented"}],"total":1,"completed":1}})).is_err());
    }
}
