//! One queued survey create delivered on this build's route, and what its
//! answer means to the survey queue's replay authority
//! ([`crate::survey_entry_outbox`]). On the web the page writes the create
//! straight to Firestore, create-only, under the signed-in person's own
//! Firebase ID token (owner rule, October 2026), with the commit
//! [`crate::survey_entry_create::direct`] plans; it counts as delivered only
//! when Firestore's answer proves this exact write with its server clock. A
//! native edge sends creates through Brain's mutate API
//! ([`super::survey::Mutation`]). Corrections, deletions and restores always
//! go through that API; photos travel the media outbox.

use serde_json::{json, Value};

use crate::survey_entry_create::{direct, direct_plan_for};
use crate::survey_entry_outbox::Header;

/// Where a create goes. The web writes Firestore directly, create-only;
/// native edges (Desktop, Android, Core) send every survey write through
/// Brain's mutate API (owner ruling: "Native: mutate only").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    FirestoreDirect,
    Mutate,
}

/// This build's create route, fixed at compile time by its platform.
pub const ROUTE: Route = if cfg!(feature = "platform-web") {
    Route::FirestoreDirect
} else {
    Route::Mutate
};

/// Brain's mutate API does not take creates yet, so a native create waits in
/// the queue instead of being sent or failed.
pub const ROUTE_PENDING: &str = "survey_create_route_pending";
/// How long a waiting native create rests before the queue looks again.
const PENDING_RETRY_MS: u64 = 60 * 60 * 1000;

/// The machine's outcome for a create whose route cannot send yet: it waits.
pub fn pending() -> Value {
    json!({"kind": "transient", "retry_after_ms": PENDING_RETRY_MS})
}

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

/// One queued create, ready to commit.
#[derive(Clone)]
pub struct Create {
    plan: direct::DirectPlan,
}

/// The machine's outcome for a create the Kernel will not send.
pub fn invalid() -> Value {
    json!({"kind": "invalid"})
}

impl Create {
    /// Plan the commit of a queued create from its header and wire payload,
    /// written as `author`. `None` when the row or the author is outside the
    /// create grammar: the machine records that as invalid, never a retry.
    pub fn prepare(header: &Header, payload: &Value, author: &Author) -> Option<Self> {
        let row = json!({
            "project_id": header.project_id,
            "form_id": header.form_id,
            "doc_id": header.doc_id,
            "idempotency_key": header.idempotency_key,
            "created_at": header.created_at,
        });
        let authority = direct::Authority {
            firebase_project: author.firebase_project.clone(),
            uid: author.uid.clone(),
            email: author.email.clone(),
            lane: author.lane.clone(),
        };
        if header.owner_uid.as_deref() != Some(author.uid.as_str()) {
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

    /// What the attempt means to the replay authority.
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

/// The typed browser export the survey queue calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    use super::{
        invalid, pending, Author, Create, Route, Sent, RESPONSE_CAP, ROUTE, ROUTE_PENDING,
        TIMEOUT_MS,
    };
    use crate::gateway_exchange::browser::{
        attempt, credentials, direct_post, facts, typed, GatewayHost,
    };
    use crate::gateway_exchange::survey::{mutation_outcome, Mutation};
    use crate::gateway_target::{FIREBASE_PROJECT, LANE};
    use crate::survey_entry_outbox::Header;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// Deliver one queued create on this build's route and answer its outcome
    /// for the survey queue's replay authority: on the web a create-only write
    /// straight to Firestore as the signed-in person; on a native edge Brain's
    /// mutate API.
    #[wasm_bindgen(js_name = surveyCreate, unchecked_return_type = "Record<string, unknown>")]
    pub async fn survey_create(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] header: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] payload: JsValue,
    ) -> Result<JsValue, JsValue> {
        let header: Header = tsify::serde_wasm_bindgen::from_value(header).map_err(unreadable)?;
        let payload: Value = tsify::serde_wasm_bindgen::from_value(payload).map_err(unreadable)?;
        if ROUTE == Route::Mutate {
            let mut body: Map<String, Value> = payload.as_object().cloned().unwrap_or_default();
            body.insert("operation".into(), "create".into());
            return match attempt(&host, Mutation { payload: body }).await? {
                Err(refusal) if refusal.code == ROUTE_PENDING => typed(&pending()),
                outcome => typed(&mutation_outcome(outcome)),
            };
        }
        // A build that names no Firebase project cannot write: refuse, so the
        // queued create waits instead of failing as invalid.
        let Some(firebase_project) = FIREBASE_PROJECT else {
            return Err(unreadable(
                "this build names no Firebase project for survey creates",
            ));
        };
        let session = credentials(&host).await?;
        let author = Author {
            firebase_project: firebase_project.into(),
            uid: facts(&host)?.principal,
            email: session.email.trim().into(),
            lane: LANE.unwrap_or_default().into(),
        };
        let Some(create) = Create::prepare(&header, &payload, &author) else {
            return typed(&invalid());
        };
        let Some(bearer) = session.token.filter(|token| !token.is_empty()) else {
            // Signed out mid-drain: nothing was sent; the machine retries.
            return typed(&create.outcome(Sent::Failed));
        };
        let sent = match direct_post(
            &create.url(),
            &bearer,
            create.body(),
            TIMEOUT_MS,
            RESPONSE_CAP,
        )
        .await
        {
            Some((status, body)) => Sent::Answer { status, body },
            None => Sent::Failed,
        };
        typed(&create.outcome(sent))
    }
}

#[cfg(test)]
#[path = "survey_create_tests.rs"]
mod tests;
