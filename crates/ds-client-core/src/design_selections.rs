//! Saved Transformer Status selections, headlessly.
//!
//! This is the one Design decision the campaign deliberately does NOT move into
//! the kernel: membership is evaluated by ds-brain and nothing here re-derives
//! it. What was wrong was residency, not authority — `ds design selection …`
//! could only reach a paired browser, so the server-owned answer needed a GUI to
//! read. This module is that answer's native residency, and nothing more.
//!
//! Three rules the module exists to hold:
//!
//! 1. **The evaluated member digest is echoed, never recomputed.** A promotion
//!    sends back the digest the read returned. Deriving one here would prove
//!    nothing about what the caller saw, and ds-brain would have nothing to
//!    compare it against.
//! 2. **A version is read, never asserted.** Replace and archive take the
//!    version from a read this crate performed. `ds` must not be able to claim
//!    a version it never observed.
//! 3. **A missing member keeps the label it was saved with.** A renamed
//!    transformer mints a new identity, so the member reads as missing —
//!    reported under its saved name, never substituted.
use crate::{ClientError, ErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DESIGN_SELECTIONS_RESPONSE_LIMIT: usize = 1024 * 1024;

/// The object kind every member of a Transformer Status selection anchors to.
/// ds-brain defines only this one today; the field exists there so a future
/// MV-model selection is a value, not a schema change.
const OBJECT_KIND: &str = "lv_transformer";
/// Snapshot selections are the only mode `ds` authors. A query selection can be
/// read and archived like any other; it is simply not something the CLI writes,
/// because its grammar is a separate surface (`describe_query`).
const SNAPSHOT_MODE: &str = "snapshot";

/// The largest saved selection `ds` will author or report. ds-brain enforces its
/// own bound; this one keeps a runaway answer from being decoded at all.
pub const MAX_SELECTION_MEMBERS: usize = 2_000;
/// The largest listing this crate will decode.
pub const MAX_SELECTIONS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignSelectionSave {
    pub selection_id: String,
    pub name: String,
    pub description: Option<String>,
    /// Canonical transformer document names.
    pub member_ids: Vec<String>,
    /// Present to REPLACE an existing selection, absent to create one. The
    /// asymmetry is ds-brain's: a create that carried a version would silently
    /// overwrite, which is the accident the fence exists to prevent.
    pub expected_version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignSelectionArchive {
    pub selection_id: String,
    pub expected_version: i64,
    pub restore: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignSelectionPromotion {
    pub selection_id: String,
    pub assignment_id: String,
    pub expected_version: i64,
    /// The digest the read returned. Echoed, never derived.
    pub expected_member_digest: String,
    pub command_id: String,
    pub title: String,
    pub purpose: Option<String>,
    pub responsible_email: Option<String>,
    pub review_required: bool,
}

/// One closed operation on the saved-selection surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignSelectionRequest {
    List { include_archived: bool },
    Read { selection_id: String },
    Save(DesignSelectionSave),
    Archive(DesignSelectionArchive),
    Promote(DesignSelectionPromotion),
}

impl DesignSelectionRequest {
    pub const fn action(&self) -> &'static str {
        match self {
            Self::List { .. } => "list",
            Self::Read { .. } => "get",
            Self::Save(_) => "save",
            Self::Archive(_) => "archive",
            Self::Promote(_) => "promote_task",
        }
    }

    /// True when ds-brain requires the selection-edit or assign capability.
    /// Reported so a caller can say which grant it needs before it asks.
    pub const fn writes(&self) -> bool {
        !matches!(self, Self::List { .. } | Self::Read { .. })
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        match self {
            Self::List { .. } => Ok(()),
            Self::Read { selection_id } => valid_id("selection", selection_id),
            Self::Save(save) => {
                valid_id("selection", &save.selection_id)?;
                valid_label("name", &save.name, 160)?;
                if let Some(description) = &save.description {
                    valid_label("description", description, 2_000)?;
                }
                if save.member_ids.is_empty() {
                    return Err(invalid("A saved selection names at least one transformer"));
                }
                if save.member_ids.len() > MAX_SELECTION_MEMBERS {
                    return Err(invalid("Too many transformers for one saved selection"));
                }
                for member in &save.member_ids {
                    valid_label("transformer", member, 256)?;
                }
                if save.expected_version.is_some_and(|version| version < 1) {
                    return Err(invalid(
                        "A replaced selection carries the version it was read at",
                    ));
                }
                Ok(())
            }
            Self::Archive(archive) => {
                valid_id("selection", &archive.selection_id)?;
                valid_version(archive.expected_version)
            }
            Self::Promote(promotion) => {
                valid_id("selection", &promotion.selection_id)?;
                valid_id("assignment", &promotion.assignment_id)?;
                valid_version(promotion.expected_version)?;
                if promotion.expected_member_digest.len() != 64
                    || !promotion
                        .expected_member_digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(invalid(
                        "A promotion echoes the member digest the selection read returned",
                    ));
                }
                if promotion.command_id.len() < 8 || promotion.command_id.len() > 128 {
                    return Err(invalid("Invalid project-work command id"));
                }
                if !promotion
                    .command_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
                    || !promotion
                        .command_id
                        .bytes()
                        .next()
                        .is_some_and(|byte| byte.is_ascii_alphanumeric())
                {
                    return Err(invalid("Invalid project-work command id"));
                }
                valid_label("title", &promotion.title, 160)?;
                if let Some(purpose) = &promotion.purpose {
                    valid_label("purpose", purpose, 160)?;
                }
                if let Some(email) = &promotion.responsible_email {
                    valid_label("owner", email, 320)?;
                    if !email.contains('@') {
                        return Err(invalid("Invalid owner address"));
                    }
                }
                Ok(())
            }
        }
    }

    /// Exactly the keys ds-brain's decoder declares. It rejects unknown fields,
    /// so an absent optional is omitted and never sent as null.
    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
        let mut value = json!({ "action": self.action(), "project_id": project });
        match self {
            Self::List { include_archived } => {
                if *include_archived {
                    value["include_archived"] = json!(true);
                }
            }
            Self::Read { selection_id } => value["selection_id"] = json!(selection_id),
            Self::Save(save) => {
                let members: Vec<Value> = save
                    .member_ids
                    .iter()
                    .map(|id| json!({ "object": { "kind": OBJECT_KIND, "id": id } }))
                    .collect();
                let mut selection = json!({
                    "selection_id": save.selection_id,
                    "name": save.name,
                    "mode": SNAPSHOT_MODE,
                    "members": members,
                });
                if let Some(description) = &save.description {
                    selection["description"] = json!(description);
                }
                if let Some(version) = save.expected_version {
                    selection["expected_version"] = json!(version);
                }
                value["selection"] = selection;
            }
            Self::Archive(archive) => {
                let mut payload = json!({
                    "selection_id": archive.selection_id,
                    "expected_version": archive.expected_version,
                });
                if archive.restore {
                    payload["restore"] = json!(true);
                }
                value["archive"] = payload;
            }
            Self::Promote(promotion) => {
                let mut payload = json!({
                    "selection_id": promotion.selection_id,
                    "assignment_id": promotion.assignment_id,
                    "expected_version": promotion.expected_version,
                    "expected_member_digest": promotion.expected_member_digest,
                    "title": promotion.title,
                    "command_id": promotion.command_id,
                    // A headless caller never rendered the work graph, so it
                    // asserts nothing about it. Zero is ds-brain's own "no
                    // expectation"; the selection version and member digest
                    // above are the fences this caller DID observe.
                    "base_revision": 0,
                });
                if let Some(purpose) = &promotion.purpose {
                    payload["purpose"] = json!(purpose);
                }
                if let Some(email) = &promotion.responsible_email {
                    payload["responsible_email"] = json!(email);
                }
                if promotion.review_required {
                    payload["review_required"] = json!(true);
                }
                value["promotion"] = payload;
            }
        }
        serde_json::to_vec(&value).expect("closed validated selection request")
    }
}

/// One saved selection's head, as ds-brain reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSelectionSummary {
    pub selection_id: String,
    pub name: String,
    pub mode: String,
    pub version: i64,
    pub state: String,
    /// `None` for a query selection, whose membership is evaluated on read.
    pub members: Option<usize>,
    pub assignments: i64,
}

/// One evaluated member: `present`, `changed` or `missing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSelectionMemberRow {
    pub id: String,
    pub label: String,
    pub state: String,
}

/// What a selection read answers. `member_digest` is what a promotion echoes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSelectionRead {
    pub selection: DesignSelectionSummary,
    pub member_digest: String,
    pub present: usize,
    pub changed: usize,
    pub missing: usize,
    pub members: Vec<DesignSelectionMemberRow>,
}

/// The immutable receipt a promotion returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSelectionAssignmentReceipt {
    pub assignment_id: String,
    pub task_id: String,
    pub member_digest: String,
    pub member_ids: Vec<String>,
    pub missing_member_ids: Vec<String>,
    pub committed_revision: i64,
}

/// Every shape this surface answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DesignSelectionAnswer {
    List(Vec<DesignSelectionSummary>),
    Read(Box<DesignSelectionRead>),
    Saved(DesignSelectionSummary),
    Assigned(Box<DesignSelectionAssignmentReceipt>),
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

/// ds-brain's own id grammar: `^[a-z0-9][a-z0-9_-]{1,127}$`. Checked here so an
/// unusable id is refused before a credential is spent on it.
fn valid_id(name: &'static str, value: &str) -> Result<(), ClientError> {
    let bytes = value.as_bytes();
    let shaped = (2..=128).contains(&bytes.len())
        && bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-' || *byte == b'_'
        });
    if shaped {
        return Ok(());
    }
    Err(ClientError::new(
        ErrorKind::InvalidInput,
        match name {
            "assignment" => "Invalid assignment id",
            _ => "Invalid selection id",
        },
    ))
}

fn valid_label(name: &'static str, value: &str, max: usize) -> Result<(), ClientError> {
    if value.trim().is_empty()
        || value.trim() != value
        || value.len() > max
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            match name {
                "name" => "Invalid selection name",
                "description" => "Invalid selection description",
                "transformer" => "Invalid transformer name",
                "title" => "Invalid task title",
                "purpose" => "Invalid purpose",
                _ => "Invalid owner address",
            },
        ));
    }
    Ok(())
}

fn valid_version(version: i64) -> Result<(), ClientError> {
    if version < 1 {
        return Err(invalid(
            "A selection mutation carries the version it was read at",
        ));
    }
    Ok(())
}

pub(crate) fn decode(
    response: crate::TransportResponse,
    request: &DesignSelectionRequest,
) -> Result<DesignSelectionAnswer, ClientError> {
    if response.body.len() > DESIGN_SELECTIONS_RESPONSE_LIMIT {
        return Err(unreadable("Saved selection response exceeds limit"));
    }
    let value: Value = serde_json::from_slice(&response.body)
        .map_err(|_| unreadable("Invalid saved selection response"))?;
    if response.status != 200 || value["success"] != true {
        return Err(ClientError::new(
            match response.status {
                401 => ErrorKind::SignedOut,
                403 => ErrorKind::AuthenticationRejected,
                404 => ErrorKind::ResourceNotFound,
                409 => ErrorKind::InvalidInput,
                _ => ErrorKind::UnreadableResponse,
            },
            match response.status {
                401 => "Saved selections need a signed-in native session",
                403 => "This account may not use saved selections in this project",
                404 => "No such saved selection",
                409 => "The saved selection moved; read it again",
                _ => "Saved selection request refused by ds-brain",
            },
        ));
    }
    let data = &value["data"];
    match request {
        DesignSelectionRequest::List { .. } => {
            let rows = data["selections"]
                .as_array()
                .ok_or_else(|| unreadable("Invalid saved selection listing"))?;
            if rows.len() > MAX_SELECTIONS {
                return Err(unreadable("Saved selection listing exceeds limit"));
            }
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                out.push(summary(row)?);
            }
            Ok(DesignSelectionAnswer::List(out))
        }
        DesignSelectionRequest::Read { .. } => {
            let selection = summary(&data["selection"])?;
            let digest = data["evaluated_member_digest"]
                .as_str()
                .filter(|value| value.len() == 64)
                .ok_or_else(|| unreadable("Selection read carries no evaluated member digest"))?
                .to_owned();
            let rows = data["members"]
                .as_array()
                .ok_or_else(|| unreadable("Invalid saved selection membership"))?;
            if rows.len() > MAX_SELECTION_MEMBERS {
                return Err(unreadable("Saved selection membership exceeds limit"));
            }
            let mut members = Vec::with_capacity(rows.len());
            for row in rows {
                let id = text(&row["object"]["id"])
                    .ok_or_else(|| unreadable("Saved selection member has no identity"))?;
                // The saved label is what a missing member is named by. Falling
                // back to the id is naming, not substitution: it is the same
                // document identity, spelled the only way it still exists.
                let label = text(&row["saved_label"]).unwrap_or_else(|| id.clone());
                let state = text(&row["state"])
                    .ok_or_else(|| unreadable("Saved selection member has no state"))?;
                members.push(DesignSelectionMemberRow { id, label, state });
            }
            Ok(DesignSelectionAnswer::Read(Box::new(DesignSelectionRead {
                selection,
                member_digest: digest,
                present: count(&data["present_count"]),
                changed: count(&data["changed_count"]),
                missing: count(&data["missing_count"]),
                members,
            })))
        }
        DesignSelectionRequest::Save(_) | DesignSelectionRequest::Archive(_) => {
            Ok(DesignSelectionAnswer::Saved(summary(&data["selection"])?))
        }
        DesignSelectionRequest::Promote(_) => {
            let assignment = &data["assignment"];
            let receipt = DesignSelectionAssignmentReceipt {
                assignment_id: text(&assignment["assignment_id"])
                    .ok_or_else(|| unreadable("Assignment receipt has no identity"))?,
                task_id: text(&assignment["task_id"])
                    .ok_or_else(|| unreadable("Assignment receipt names no task"))?,
                member_digest: text(&assignment["member_digest"])
                    .ok_or_else(|| unreadable("Assignment receipt carries no member digest"))?,
                member_ids: names(&assignment["member_ids"])?,
                missing_member_ids: names(&assignment["missing_member_ids"])?,
                committed_revision: assignment["committed_revision"].as_i64().unwrap_or(0),
            };
            Ok(DesignSelectionAnswer::Assigned(Box::new(receipt)))
        }
    }
}

fn summary(row: &Value) -> Result<DesignSelectionSummary, ClientError> {
    let selection_id =
        text(&row["selection_id"]).ok_or_else(|| unreadable("Saved selection has no identity"))?;
    let mode = text(&row["mode"]).unwrap_or_else(|| SNAPSHOT_MODE.to_owned());
    // A query selection reports no member count: its membership is not a
    // stored list, and reporting zero would read as "empty" rather than
    // "evaluated on read".
    let members = if mode == SNAPSHOT_MODE {
        Some(row["members"].as_array().map_or(0, Vec::len))
    } else {
        None
    };
    Ok(DesignSelectionSummary {
        selection_id,
        name: text(&row["name"]).unwrap_or_default(),
        mode,
        version: row["version"].as_i64().unwrap_or(0),
        state: text(&row["state"]).unwrap_or_else(|| "active".to_owned()),
        members,
        assignments: row["assignment_count"].as_i64().unwrap_or(0),
    })
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .map(str::to_owned)
}

fn count(value: &Value) -> usize {
    usize::try_from(value.as_i64().unwrap_or(0).max(0)).unwrap_or(0)
}

fn names(value: &Value) -> Result<Vec<String>, ClientError> {
    let Some(rows) = value.as_array() else {
        return Ok(Vec::new());
    };
    if rows.len() > MAX_SELECTION_MEMBERS {
        return Err(unreadable("Assignment receipt exceeds limit"));
    }
    Ok(rows.iter().filter_map(text).collect())
}
