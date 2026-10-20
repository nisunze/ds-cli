//! Governed platform architecture: the revision the architecture page draws,
//! and every planning command (read, list, history, preview, apply) as one
//! JSON command. Brain holds the revisions, validates each command and
//! fences it by the expected revision; a refused proposal carries Brain's
//! violations and current snapshot in the refusal's details.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/architecture/planning";
const SNAPSHOT_CAP: u64 = 8 * 1024 * 1024;
const HISTORY_MAX: u32 = 200;
pub const INVALID: &str = "architecture_answer_invalid";
pub const INPUT_INVALID: &str = "architecture_input_invalid";

/// One planning command, exactly as Brain's planning endpoint reads it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanningCommand {
    /// The head revision, or one earlier revision.
    Get {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        revision: Option<u64>,
    },
    /// The head's nodes and edges, filtered by chapter and delivery state.
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        chapter: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        state: Option<String>,
        #[serde(default)]
        include_archived: bool,
    },
    /// The revision history, newest first.
    History {
        limit: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        cursor: Option<String>,
    },
    /// What a command would change, without changing it.
    Preview {
        expected_revision: u64,
        idempotency_key: String,
        #[cfg_attr(
            all(feature = "wasm", target_arch = "wasm32"),
            tsify(type = "Record<string, unknown>")
        )]
        command: Value,
    },
    /// Apply a command to the expected revision.
    Apply {
        expected_revision: u64,
        idempotency_key: String,
        #[cfg_attr(
            all(feature = "wasm", target_arch = "wasm32"),
            tsify(type = "Record<string, unknown>")
        )]
        command: Value,
    },
}

impl PlanningCommand {
    fn valid(&self) -> bool {
        match self {
            Self::History { limit, .. } => (1..=HISTORY_MAX).contains(limit),
            Self::Preview {
                idempotency_key,
                command,
                ..
            }
            | Self::Apply {
                idempotency_key,
                command,
                ..
            } => !idempotency_key.trim().is_empty() && command.is_object(),
            _ => true,
        }
    }
}

/// One planning command and Brain's answer, as served.
#[derive(Clone, Debug, PartialEq)]
pub struct Planning {
    pub command: PlanningCommand,
}

impl Exchange for Planning {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        if !self.command.valid() {
            return Err(Refusal::new(
                INPUT_INVALID,
                "The planning command is outside its bounds.",
            ));
        }
        let body = serde_json::to_value(&self.command)
            .map_err(|_| Refusal::new(INPUT_INVALID, "The planning command is unreadable."))?;
        Ok(Request::post(PATH, body).response_cap(SNAPSHOT_CAP).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        if !answer.is_object() {
            return Err(Refusal::new(
                INVALID,
                "The architecture service answered an invalid result.",
            ));
        }
        Ok(Admitted::Done(answer.clone()))
    }
}

fn default_kind() -> String {
    "service".into()
}
fn default_lane() -> String {
    "cloud".into()
}
fn default_edge_kind() -> String {
    "read".into()
}
fn default_confidence() -> String {
    "inferred".into()
}
fn default_state() -> String {
    "implemented".into()
}

/// The page's spelling of Brain's records: camelCase, with a node's
/// container as its `parent` and evidence's repository and note as `repo`
/// and `anchor`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Evidence {
    #[serde(default, rename(serialize = "repo", deserialize = "repository"))]
    pub repo: String,
    #[serde(default)]
    pub path: String,
    #[serde(
        default,
        rename(serialize = "anchor", deserialize = "note"),
        skip_serializing_if = "Option::is_none"
    )]
    pub anchor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Node {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub chapter: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default = "default_lane")]
    pub lane: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_authority: Option<String>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(
        default,
        rename(serialize = "parent", deserialize = "container_id"),
        skip_serializing_if = "Option::is_none"
    )]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapter_ref: Option<String>,
    #[serde(default)]
    pub column: i64,
    #[serde(default)]
    pub row: i64,
    #[serde(default = "default_state")]
    pub delivery_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Edge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default = "default_edge_kind")]
    pub kind: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default = "default_confidence")]
    pub confidence: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Chapter {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title_key: String,
    #[serde(default)]
    pub description_key: String,
    /// Filled by the page from the snapshot's nodes and edges.
    #[serde(default, skip_deserializing)]
    pub nodes: Vec<Node>,
    #[serde(default, skip_deserializing)]
    pub edges: Vec<Edge>,
}

/// One architecture revision as the page draws it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Snapshot {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
}

/// The head revision, or one earlier revision, as the page draws it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotRead {
    pub revision: Option<u64>,
}

impl Exchange for SnapshotRead {
    type Output = Snapshot;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = match self.revision {
            Some(revision) if revision > 0 => json!({"action": "get", "revision": revision}),
            _ => json!({"action": "get"}),
        };
        Ok(Request::post(PATH, body).response_cap(SNAPSHOT_CAP).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        data(&body)
            .get("snapshot")
            .filter(|snapshot| snapshot.is_object())
            .and_then(|snapshot| Snapshot::deserialize(snapshot).ok())
            .map(Admitted::Done)
            .ok_or_else(|| {
                Refusal::new(
                    INVALID,
                    "The architecture service answered without its snapshot.",
                )
            })
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{Planning, PlanningCommand, SnapshotRead};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// One architecture revision in the page's spelling (`ArchitectureSnapshot`).
    #[wasm_bindgen(js_name = architectureSnapshot, unchecked_return_type = "unknown")]
    pub async fn architecture_snapshot(
        host: GatewayHost,
        revision: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let call = SnapshotRead {
            revision: revision.map(u64::from),
        };
        typed(&run(&host, call).await?)
    }

    /// One planning command; answers Brain's result as served.
    #[wasm_bindgen(js_name = architecturePlanning, unchecked_return_type = "Record<string, unknown>")]
    pub async fn architecture_planning(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "PlanningCommand")] command: JsValue,
    ) -> Result<JsValue, JsValue> {
        let command: PlanningCommand = tsify::serde_wasm_bindgen::from_value(command)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        typed(&run(&host, Planning { command }).await?)
    }
}

#[cfg(test)]
#[path = "architecture_tests.rs"]
mod tests;
