//! One queued survey create, and what its answer means to the survey queue
//! ([`super::survey_queue`]). On every platform (Web, Desktop, Android and
//! Core) a create is written straight to Firestore, create-only, under the
//! signed-in author's own Firebase ID token, with the commit
//! [`crate::survey_entry_create::direct`] plans; it counts as delivered only
//! when Firestore's answer proves this exact write with its server clock.
//! Brain never creates. Corrections, deletions and restores go through
//! Brain's mutate API ([`super::survey::Mutation`]); files travel the media
//! upload.

use serde_json::{json, Value};

use crate::survey_entry_create::{direct, direct_plan_for};

/// Firestore answers a single-document commit well within this.
pub const TIMEOUT_MS: u64 = 120_000;
/// A commit receipt is a few hundred bytes; anything past this is not one.
pub const RESPONSE_CAP: u64 = 64 * 1024;

/// What one attempt observed.
#[derive(Clone, Debug, PartialEq)]
pub enum Sent {
    /// Firestore answered with this status and body.
    Answer { status: u16, body: Vec<u8> },
    /// No answer arrived: offline, a broken link or the deadline.
    Failed,
}

/// The signed-in person a create is written as, and the build's Firebase
/// project and lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub firebase_project: String,
    pub uid: String,
    pub email: String,
    pub lane: String,
}

/// What the queue holds of one create besides its wire payload: the entry,
/// the intent's key and time, and the account that captured it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Queued<'a> {
    pub project: &'a str,
    pub form: &'a str,
    pub doc: &'a str,
    pub idempotency_key: &'a str,
    pub created_at: u64,
    pub owner_uid: &'a str,
}

/// One queued create, ready to commit.
#[derive(Clone)]
pub struct Create {
    plan: direct::DirectPlan,
}

impl Create {
    /// Plan the commit of a queued create from what the queue holds and its
    /// wire payload, written as `author`. `None` when the row or the author is
    /// outside the create grammar: the queue records that as invalid, never a
    /// retry.
    pub fn prepare(queued: &Queued, payload: &Value, author: &Author) -> Option<Self> {
        let row = json!({
            "project_id": queued.project,
            "form_id": queued.form,
            "doc_id": queued.doc,
            "idempotency_key": queued.idempotency_key,
            "created_at": queued.created_at,
        });
        let authority = direct::Authority {
            firebase_project: author.firebase_project.clone(),
            uid: author.uid.clone(),
            email: author.email.clone(),
            lane: author.lane.clone(),
        };
        if queued.owner_uid != author.uid {
            return None;
        }
        direct_plan_for(payload, &row, &authority)
            .ok()
            .map(|plan| Self { plan })
    }

    /// Where the commit goes: Firestore's REST commit of this project.
    pub fn url(&self) -> String {
        format!("{}{}", self.plan.origin, self.plan.path)
    }

    /// The commit body: one create-only write (`currentDocument.exists =
    /// false`) with the server clock transform.
    pub fn body(&self) -> &Value {
        &self.plan.body
    }

    /// What the attempt means to the survey queue.
    pub fn outcome(&self, sent: Sent) -> Value {
        let Sent::Answer { status, body } = sent else {
            return json!({"kind": "response", "status": 0, "network": true});
        };
        let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if status == 200 {
            return match direct::receipt(&self.plan, status, &body) {
                Ok(_) => json!({"kind": "success"}),
                Err(_) => json!({"kind": "ambiguous_receipt"}),
            };
        }
        json!({"kind": "response", "status": status, "details": body.get("error_details").cloned().unwrap_or(Value::Null)})
    }
}

#[cfg(test)]
#[path = "survey_create_tests.rs"]
mod tests;
