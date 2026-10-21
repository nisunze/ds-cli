//! Survey capture as signals: the form a person fills
//! ([`crate::survey_capture`]) for the signed-in account, its draft kept on
//! this device ([`crate::work_drafts`]) after every change, and the
//! submission that queues the entry on the survey queue
//! ([`crate::gateway_exchange::survey_queue`]). The window renders `view`
//! and sends intents. The host keeps the files a person picks and turns each
//! planned upload into held bytes (photo processing is its effect).

use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use reactive_graph::computed::ArcMemo;
use reactive_graph::signal::ArcRwSignal;
use reactive_graph::traits::{GetUntracked, Set, Update, With, WithUntracked};
use serde_json::{json, Map, Value};

use super::{Demand, Effects, OnDemand, Scope};
use crate::durable_store::Request;
use crate::gateway_exchange::Refusal;
use crate::media_outbox::Operation as Media;
use crate::outbox::{Operation as Outbox, Queue, MAX_BATCH};
use crate::retained_reads::{Kind as Retained, Operation as Reads};
use crate::survey_capture::{plan, plan_cascade, plan_mutation, view, Context, Refused, Session, Submit, Upload};
use crate::work_drafts::{Kind as Drafts, Operation as Draft};

/// A capture the person cannot make now: signed out, another account's
/// form, a refused form.
pub const UNAVAILABLE: &str = "survey_capture_unavailable";
/// Fields the form refuses; each error shows beside its field.
pub const INVALID: &str = "survey_capture_invalid";
/// The grants a capture and its corrections are checked against.
const GRANTS: [&str; 4] = ["entries.create", "entries.delete", "entries.edit_all", "entries.edit_own"];

pub struct Capture {
    scope: ArcRwSignal<Scope>,
    session: ArcRwSignal<Session>,
    /// What the window shows of the form being filled; closed for any
    /// account but the one that opened it.
    pub view: OnDemand<Value>,
    /// A draft write is in flight; `dirty` asks it to write again.
    saving: Rc<Cell<bool>>,
    dirty: Rc<Cell<bool>>,
}

fn unavailable(message: impl Into<String>) -> Refusal {
    Refusal::new(UNAVAILABLE, message)
}

/// A grant as the held capability snapshot names it.
fn granted(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
        Some(Value::Null) | None => false,
    }
}

impl Capture {
    pub(super) fn new(scope: &ArcRwSignal<Scope>) -> Self {
        let session = ArcRwSignal::new(Session::default());
        let shown = ArcMemo::new({
            let (scope, session) = (scope.clone(), session.clone());
            move |_: Option<&Value>| {
                let principal = scope.with(|scope| scope.principal.clone());
                session.with(|session| match principal {
                    Some(principal) if session.open && session.owner == principal => view(session),
                    _ => json!({"open": false}),
                })
            }
        });
        Self {
            scope: scope.clone(),
            session,
            view: OnDemand {
                value: shown,
                demand: Demand::default(),
            },
            saving: Rc::new(Cell::new(false)),
            dirty: Rc::new(Cell::new(false)),
        }
    }

    /// The form being filled, as it stands now.
    pub fn session(&self) -> Session {
        self.session.get_untracked()
    }

    /// Command: open a form for the signed-in account on its defaults, then
    /// restore the draft this device holds of it, if it was authored against
    /// this exact form revision.
    pub fn open<E: Effects>(&self, effects: &Rc<E>, context: Context) -> impl Future<Output = Result<(), Refusal>> + use<E> {
        let owner = self.scope.with_untracked(|scope| scope.principal.clone());
        let opened = owner
            .ok_or_else(|| unavailable("Sign in to fill a form"))
            .and_then(|owner| Session::open(&owner, context).map_err(unavailable));
        if let Ok(session) = &opened {
            self.session.set(session.clone());
        }
        let (effects, current) = (Rc::clone(effects), self.session.clone());
        async move {
            let session = opened?;
            let read = Draft::Read {
                kind: Drafts::SurveyEntry,
                project: Some(session.project.clone()),
                key: session.slug.clone(),
            };
            let held = effects.store(Request::WorkDrafts { operation: read }).await?;
            let Some(draft) = held.get("held").and_then(|held| held.get("value")).filter(|draft| draft.is_object()) else {
                return Ok(());
            };
            current.update(|now| {
                // Only the form this read was for, untouched since.
                if now.owner == session.owner && now.project == session.project && now.slug == session.slug && now.values == session.values {
                    now.restore(draft);
                }
            });
            Ok(())
        }
    }

    /// Command: one field's value; its error clears and the draft is kept.
    pub fn set_value<E: Effects>(&self, effects: &Rc<E>, key: &str, value: Value) {
        self.session.update(|session| session.set_value(key, value));
        self.keep(effects);
    }

    /// Command: the device's location (`gps`) or a point picked on the map.
    pub fn locate<E: Effects>(&self, effects: &Rc<E>, location: Value, source: &str) {
        self.session.update(|session| session.locate(location, source));
        self.keep(effects);
    }

    /// Command: the geometry a drawing or an edge chain gives the entry.
    pub fn set_geometry<E: Effects>(&self, effects: &Rc<E>, geometry: Value) {
        self.session.update(|session| session.geometry = geometry);
        self.keep(effects);
    }

    /// Command: the entry's network connectivity, and the node picked as its
    /// precedent (`{docId, formSlug, coordinates}`, or null).
    pub fn connect<E: Effects>(&self, effects: &Rc<E>, connectivity: Value, precedent: Value) {
        self.session.update(|session| {
            session.connectivity = connectivity;
            session.precedent = precedent;
        });
        self.keep(effects);
    }

    /// Command: drop the held draft and start again from the form's defaults.
    pub fn discard_draft<E: Effects>(&self, effects: &Rc<E>) {
        let session = self.session.get_untracked();
        if !session.open {
            return;
        }
        let context = Context {
            project: session.project.clone(),
            form: session.form.clone(),
            geometry: session.geometry.clone(),
            location: session.location.clone(),
            location_source: session.location_source.clone(),
            connectivity: session.connectivity.clone(),
        };
        if let Ok(fresh) = Session::open(&session.owner, context) {
            self.session.set(fresh);
        }
        let delete = Draft::Delete {
            kind: Drafts::SurveyEntry,
            project: Some(session.project),
            key: session.slug,
        };
        let removing = effects.store(Request::WorkDrafts { operation: delete });
        any_spawner::Executor::spawn_local(async move {
            let _ = removing.await;
        });
    }

    /// Command: close the form; its draft stays for the next opening.
    pub fn close(&self) {
        self.session.set(Session::default());
    }

    /// Keep the draft after a change: one write at a time, and one more
    /// when the form changed while it ran.
    fn keep<E: Effects>(&self, effects: &Rc<E>) {
        self.dirty.set(true);
        if self.saving.replace(true) {
            return;
        }
        let (effects, session) = (Rc::clone(effects), self.session.clone());
        let (saving, dirty) = (Rc::clone(&self.saving), Rc::clone(&self.dirty));
        any_spawner::Executor::spawn_local(async move {
            while dirty.replace(false) {
                let session = session.get_untracked();
                if !session.open || session.submitted {
                    continue;
                }
                let Ok(draft) = session.draft(effects.now_ms()) else {
                    continue;
                };
                let put = Draft::Put {
                    kind: Drafts::SurveyEntry,
                    project: Some(session.project.clone()),
                    key: session.slug.clone(),
                    value: draft,
                    meta: None,
                    now: effects.now_ms(),
                };
                // A draft that cannot be kept leaves the form as it is.
                let _ = effects.store(Request::WorkDrafts { operation: put }).await;
            }
            saving.set(false);
        });
    }

    /// Command: submit the form. The host's `keep` turns each planned
    /// upload's picked file into held bytes; the create is then queued
    /// after them and the files scheduled behind it. Answers the new
    /// entry's id and the feature the map shows until it is delivered.
    pub fn submit<E, K, Fut>(&self, effects: &Rc<E>, keep: K) -> impl Future<Output = Result<Value, Refusal>> + use<E, K, Fut>
    where
        E: Effects,
        K: Fn(Upload) -> Fut + 'static,
        Fut: Future<Output = Result<(), Refusal>>,
    {
        let (effects, current, scope) = (Rc::clone(effects), self.session.clone(), self.scope.clone());
        async move {
            let session = current.get_untracked();
            let principal = scope.with_untracked(|scope| scope.principal.clone());
            if !session.open || principal.as_deref() != Some(session.owner.as_str()) {
                return Err(unavailable("Open a project before creating an entry"));
            }
            if session.submitting {
                return Err(unavailable("The entry is being saved"));
            }
            let (grants, email) = held_grants(&*effects, &session.project).await;
            let submit = Submit {
                now_ms: effects.now_ms(),
                doc_id: format!("e_{}", effects.nonce()),
                mutation_id: format!("survey_{}", effects.nonce()),
                uid: session.owner.clone(),
                email,
                grants,
            };
            let planned = match plan(&session, &submit) {
                Ok(planned) => planned,
                Err(Refused::Invalid(errors)) => {
                    current.update(|now| now.errors = errors);
                    return Err(Refusal::new(INVALID, "Check the highlighted fields"));
                }
                Err(Refused::Unavailable(message)) => {
                    current.update(|now| now.error = Some(message.clone()));
                    return Err(unavailable(message));
                }
            };
            current.update(|now| {
                now.submitting = true;
                now.error = None;
                now.errors = Map::new();
            });
            let mut kept: Vec<String> = Vec::new();
            let mut outcome = Ok(());
            for upload in planned.uploads {
                let path = upload.path.clone();
                outcome = keep(upload).await;
                if outcome.is_err() {
                    break;
                }
                kept.push(path);
            }
            if outcome.is_ok() {
                outcome = queue(&*effects, vec![planned.row]).await;
            }
            if let Err(refusal) = outcome {
                // Nothing of a capture that did not queue is left to upload.
                for path in kept {
                    let _ = effects.store(media(Media::Remove { path })).await;
                }
                current.update(|now| {
                    now.submitting = false;
                    now.error = Some(refusal.message.clone());
                });
                return Err(refusal);
            }
            let _ = effects.store(media(Media::Schedule { now: effects.now_ms() })).await;
            let delete = Draft::Delete {
                kind: Drafts::SurveyEntry,
                project: Some(session.project.clone()),
                key: session.slug.clone(),
            };
            let _ = effects.store(Request::WorkDrafts { operation: delete }).await;
            current.update(|now| {
                now.submitting = false;
                now.submitted = true;
            });
            Ok(json!({"project": session.project, "form": session.slug, "doc_id": submit.doc_id, "feature": planned.feature}))
        }
    }

    /// Command: queue one correction, deletion or restore of an entry,
    /// planned against the row this device shows of it (`prior`). Answers the
    /// feature the map shows until it is delivered.
    pub fn mutate<E: Effects>(&self, effects: &Rc<E>, payload: Value, prior: Value) -> impl Future<Output = Result<Value, Refusal>> + use<E> {
        let (effects, scope) = (Rc::clone(effects), self.scope.clone());
        async move {
            let project = text(payload.get("project_id").or_else(|| payload.get("eds_project_id")));
            let submit = edit(&*effects, &scope, &project).await?;
            let (row, feature) = plan_mutation(&payload, &prior, &submit).map_err(unavailable)?;
            queue(&*effects, vec![row]).await?;
            Ok(feature)
        }
    }

    /// Command: delete a node and the edges it cascades to, planned against
    /// the rows this device shows: `{project, form, doc, prior, candidates,
    /// priors: {"form/doc": row}, edges_only?, match?}`. Answers what was
    /// deleted and each tombstone the map shows until it is delivered.
    pub fn delete_cascade<E: Effects>(&self, effects: &Rc<E>, request: Value) -> impl Future<Output = Result<Value, Refusal>> + use<E> {
        let (effects, scope) = (Rc::clone(effects), self.scope.clone());
        async move {
            let project = text(request.get("project"));
            let submit = edit(&*effects, &scope, &project).await?;
            let priors = |form: &str, doc: &str| request["priors"][format!("{form}/{doc}")].clone();
            let plan = plan_cascade(&request, priors, &submit).map_err(unavailable)?;
            let rows: Vec<Value> = plan["rows"].as_array().cloned().unwrap_or_default();
            if rows.len() > MAX_BATCH {
                return Err(unavailable(format!("A deletion cascades to at most {} entries", MAX_BATCH - 1)));
            }
            let tombstones: Vec<Value> = rows
                .iter()
                .map(|row| json!({"project": row["project"], "form": row["payload"]["form"], "doc": row["payload"]["doc"],
                    "feature": row["payload"]["overlay"]}))
                .collect();
            if !rows.is_empty() {
                queue(&*effects, rows).await?;
            }
            Ok(json!({"deleted": plan["deleted"], "cascaded_edges": plan["cascaded_edges"], "edges": plan["edges"],
                "tombstones": tombstones}))
        }
    }
}

fn text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

/// What an edit of the signed-in account's entries is checked and queued as.
async fn edit<E: Effects>(effects: &E, scope: &ArcRwSignal<Scope>, project: &str) -> Result<Submit, Refusal> {
    let uid = scope
        .with_untracked(|scope| scope.principal.clone())
        .ok_or_else(|| unavailable("Open a project before updating an entry"))?;
    let (grants, email) = held_grants(effects, project).await;
    Ok(Submit {
        now_ms: effects.now_ms(),
        doc_id: String::new(),
        mutation_id: format!("survey_{}", effects.nonce()),
        uid,
        email,
        grants,
    })
}

/// Queue planned rows on the survey queue, all or none.
async fn queue<E: Effects>(effects: &E, rows: Vec<Value>) -> Result<(), Refusal> {
    let rows = serde_json::from_value(Value::Array(rows)).map_err(|error| unavailable(error.to_string()))?;
    let enqueue = Outbox::Enqueue {
        queue: Queue::Survey,
        rows,
        now: effects.now_ms(),
    };
    effects.store(Request::Outbox { operation: enqueue }).await.map(|_| ())
}

fn media(operation: Media) -> Request {
    Request::MediaOutbox {
        operation: Box::new(operation),
    }
}

/// The entry grants and email of the account's held capabilities for the
/// project; none held grants nothing.
async fn held_grants<E: Effects>(effects: &E, project: &str) -> (Value, String) {
    let read = Reads::Read {
        kind: Retained::AccountCapabilities,
        project: None,
        key: format!("project:{project}"),
        now: effects.now_ms(),
    };
    let held = effects
        .store(Request::RetainedReads { operation: read })
        .await
        .ok()
        .and_then(|answer| answer.get("held").and_then(|held| held.get("value")).cloned())
        .unwrap_or(Value::Null);
    let grants: Map<String, Value> = GRANTS
        .iter()
        .map(|grant| (grant.to_string(), granted(held["capabilities"].get(grant)).into()))
        .collect();
    let email = held["user"]["email"].as_str().unwrap_or_default().trim().to_owned();
    (Value::Object(grants), email)
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
