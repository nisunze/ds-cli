//! Form Factory authoring: one account's document, optimistic revision and
//! local draft as Rust signals. The window sends edits and renders `view`;
//! the existing Forms exchange and work-draft store perform every effect.

use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use reactive_graph::computed::ArcMemo;
use reactive_graph::signal::ArcRwSignal;
use reactive_graph::traits::{GetUntracked, Set, Update, With, WithUntracked};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{Demand, OnDemand, Scope};
use crate::durable_store::{Call, Request};
use crate::gateway_exchange::forms::{self, FormsEndpoint};
use crate::gateway_exchange::retained::Edge;
use crate::gateway_exchange::Refusal;
use crate::work_drafts::{Kind, Operation};

pub const INVALID: &str = "form_editor_invalid";
pub const UNAVAILABLE: &str = "form_editor_unavailable";
pub const ANSWER_INVALID: &str = "form_editor_answer_invalid";

#[derive(Clone, Default)]
struct Session {
    owner: String,
    fence: Option<String>,
    slug: String,
    document: Value,
    snapshot: Option<Value>,
    version: u64,
    loaded_version: u64,
    drawing_settings: Value,
    field_types: Vec<Value>,
    loading: bool,
    saving: bool,
    draft_saved: bool,
    error: Option<Refusal>,
    revision: u64,
}

/// Commands contain no store key, request route or draft policy.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    Metadata {
        key: String,
        value: Value,
    },
    Add {
        kind: String,
    },
    Remove {
        id: String,
    },
    Update {
        id: String,
        patch: Map<String, Value>,
    },
    Move {
        from: usize,
        to: usize,
    },
}

#[derive(Clone)]
pub struct FormEditor {
    scope: ArcRwSignal<Scope>,
    session: ArcRwSignal<Session>,
    epoch: Rc<Cell<u64>>,
    pub view: OnDemand<Value>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(INVALID, message)
}
fn unavailable(message: impl Into<String>) -> Refusal {
    Refusal::new(UNAVAILABLE, message)
}
fn factory(mut request: Value) -> Result<Value, Refusal> {
    if request.get("operation").is_none() {
        request["operation"] = json!("factory");
    }
    crate::survey::survey_evaluate(
        &serde_json::to_vec(&request).map_err(|error| invalid(error.to_string()))?,
    )
    .map_err(invalid)
}
fn document(value: &Value) -> Result<Value, Refusal> {
    let mut value = value.clone();
    value["mode"] = json!("document");
    factory(value)
}
fn request(action: &str, slug: Option<&str>) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("action".into(), json!(action));
    if let Some(slug) = slug {
        body.insert("slug".into(), json!(slug));
    }
    body
}
fn form(value: &Value, slug: &str) -> Result<(Value, u64, Value), Refusal> {
    let form = &value["form"];
    let version = form["version"]
        .as_u64()
        .filter(|_| form["slug"] == slug)
        .ok_or_else(|| {
            Refusal::new(
                ANSWER_INVALID,
                "Form Factory did not return the requested form and revision.",
            )
        })?;
    let mut source = form.clone();
    if source.get("fields").is_none() {
        source["fields"] = json!([]);
    }
    Ok((
        document(&source)?,
        version,
        form.get("drawing_settings")
            .cloned()
            .unwrap_or_else(|| json!({})),
    ))
}
fn types(value: &Value) -> Result<Vec<Value>, Refusal> {
    value["field_types"]
        .as_array()
        .filter(|values| values.len() <= 512)
        .cloned()
        .ok_or_else(|| {
            Refusal::new(
                ANSWER_INVALID,
                "Form Factory did not return its field-type registry.",
            )
        })
}
fn shown(session: &Session) -> Value {
    let dirty = session
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot != &session.document);
    json!({"open":!session.slug.is_empty(),"slug":session.slug,"document":session.document,
        "version":session.version,"drawing_settings":session.drawing_settings,"field_types":session.field_types,
        "loading":session.loading,"saving":session.saving,"dirty":dirty,"draft_saved":session.draft_saved,"error":session.error})
}

impl FormEditor {
    pub(super) fn new(scope: &ArcRwSignal<Scope>) -> Self {
        let session = ArcRwSignal::new(Session::default());
        let value = ArcMemo::new({
            let (scope, session) = (scope.clone(), session.clone());
            move |_: Option<&Value>| {
                let owner = scope.with(|scope| scope.principal.clone());
                session.with(|session| {
                    if owner.as_deref() == Some(&session.owner) {
                        shown(session)
                    } else {
                        json!({"open":false})
                    }
                })
            }
        });
        Self {
            scope: scope.clone(),
            session,
            epoch: Rc::new(Cell::new(0)),
            view: OnDemand {
                value,
                demand: Demand::default(),
            },
        }
    }

    /// Invalidate flights even when A signs out and signs in as A again.
    pub(super) fn scope_changed(&self) {
        self.close();
    }
    pub fn close(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.session.set(Session::default());
    }
    fn current(&self, epoch: u64, owner: &str) -> bool {
        self.epoch.get() == epoch
            && self
                .scope
                .with_untracked(|scope| scope.principal.as_deref() == Some(owner))
    }
    fn active(&self) -> Result<(u64, Session), Refusal> {
        let session = self.session.get_untracked();
        if session.snapshot.is_some() && self.current(self.epoch.get(), &session.owner) {
            Ok((self.epoch.get(), session))
        } else {
            Err(unavailable(
                "Open a form for the signed-in account before editing it.",
            ))
        }
    }
    fn error(&self, epoch: u64, owner: &str, refusal: &Refusal) {
        if self.current(epoch, owner) {
            self.session.update(|session| {
                session.error = Some(refusal.clone());
                session.loading = false;
            });
        }
    }

    /// Field defaults, ordering and all document mutations belong to this owner.
    pub fn edit(&self, intent: Intent, nonce: &str) -> Result<Value, Refusal> {
        let (epoch, mut session) = self.active()?;
        let result = (|| {
            let fields = session.document["fields"]
                .as_array_mut()
                .ok_or_else(|| invalid("Form fields must be an array."))?;
            let mut added = Value::Null;
            match intent {
                Intent::Metadata { key, value } => {
                    let admitted = match key.as_str() {
                        "display_name" | "description" => value.is_string(),
                        "capture_detailed_location" => value.is_boolean(),
                        _ => false,
                    };
                    if !admitted {
                        return Err(invalid("Unknown metadata or an invalid metadata value."));
                    }
                    session.document[key] = value;
                }
                Intent::Add { kind } => {
                    if fields.len() >= crate::survey::MAX_SURVEY_FIELDS {
                        return Err(invalid("Survey form exceeds 512 fields."));
                    }
                    let metadata = session
                        .field_types
                        .iter()
                        .find(|value| value["key"] == kind)
                        .ok_or_else(|| {
                            invalid("Field type is unavailable in the held backend registry.")
                        })?;
                    let label = metadata
                        .get("display_name")
                        .or_else(|| metadata.get("displayName"))
                        .unwrap_or(&metadata["key"]);
                    let field = factory(
                        json!({"operation":"new_field","kind":kind,"id":format!("f_{nonce}"),"metadata":{"type":kind,"label":label}}),
                    )?;
                    if fields.iter().any(|value| value["id"] == field["id"]) {
                        return Err(invalid("The new field identity is already present."));
                    }
                    added = field["id"].clone();
                    fields.push(field);
                }
                Intent::Remove { id } => {
                    let index = fields
                        .iter()
                        .position(|field| field["id"] == id)
                        .ok_or_else(|| invalid("The field is no longer in this form."))?;
                    fields.remove(index);
                }
                Intent::Update { id, patch } => {
                    if patch.contains_key("id") {
                        return Err(invalid("A field identity cannot be changed."));
                    }
                    let field = fields
                        .iter_mut()
                        .find(|field| field["id"] == id)
                        .and_then(Value::as_object_mut)
                        .ok_or_else(|| invalid("The field is no longer in this form."))?;
                    field.extend(patch);
                }
                Intent::Move { from, to } => {
                    if from >= fields.len() || to >= fields.len() {
                        return Err(invalid("The field order no longer matches this form."));
                    }
                    if from != to {
                        let field = fields.remove(from);
                        fields.insert(to, field);
                    }
                }
            }
            session.document = document(&session.document)?;
            session.revision = session.revision.wrapping_add(1);
            session.draft_saved = false;
            session.error = None;
            Ok(added)
        })();
        match result {
            Ok(added) => {
                self.session.set(session);
                Ok(added)
            }
            Err(refusal) => {
                self.error(epoch, &session.owner, &refusal);
                Err(refusal)
            }
        }
    }

    /// Paint held schema and draft before any request. A held open never needs
    /// incidental freshness; an explicit refresh requests fresh data while the
    /// held document stays visible.
    pub fn load<E: Edge + 'static>(
        &self,
        edge: Rc<E>,
        slug: String,
        refresh: bool,
    ) -> impl Future<Output = Result<(), Refusal>> + use<E> {
        let this = self.clone();
        let owner = self.scope.with_untracked(|scope| scope.principal.clone());
        let continuing = refresh
            && self.session.with_untracked(|session| {
                session.owner.as_str() == owner.as_deref().unwrap_or("")
                    && session.slug == slug
                    && session.snapshot.is_some()
            });
        let busy = continuing && self.session.with_untracked(|session| session.saving);
        if !busy {
            self.epoch.set(self.epoch.get().wrapping_add(1));
        }
        let epoch = self.epoch.get();
        if !continuing && !busy {
            self.session.set(
                owner
                    .as_ref()
                    .map(|owner| Session {
                        owner: owner.clone(),
                        slug: slug.clone(),
                        loading: true,
                        ..Session::default()
                    })
                    .unwrap_or_default(),
            );
        }
        async move {
            let owner = owner.ok_or_else(|| unavailable("Sign in to edit a form."))?;
            if busy {
                let refusal = unavailable("Wait for this publish before refreshing the form.");
                this.error(epoch, &owner, &refusal);
                return Err(refusal);
            }
            let result = this.open(&*edge, epoch, &owner, &slug, refresh).await;
            if let Err(refusal) = &result {
                this.error(epoch, &owner, refusal);
            }
            result
        }
    }

    async fn open(
        &self,
        edge: &impl Edge,
        epoch: u64,
        owner: &str,
        slug: &str,
        refresh: bool,
    ) -> Result<(), Refusal> {
        if slug.is_empty()
            || slug.len() > 256
            || slug.trim() != slug
            || slug.chars().any(char::is_control)
        {
            return Err(invalid("A form editor names one printable form slug."));
        }
        let draft = edge
            .store(Call {
                request: Request::WorkDrafts {
                    operation: Operation::Read {
                        kind: Kind::FormEditor,
                        project: None,
                        key: slug.into(),
                    },
                },
                expect_owner: None,
            })
            .await?;
        if !self.current(epoch, owner) {
            return Ok(());
        }
        let fence = draft["owner"]
            .as_str()
            .ok_or_else(|| unavailable("The draft store did not bind its account."))?
            .to_owned();
        let form_request = request("get", Some(slug));
        let types_request = request("get_field_types", None);
        let held_form = forms::held(edge, FormsEndpoint::FormFactory, &form_request).await?;
        let held_types = forms::held(edge, FormsEndpoint::FormFactory, &types_request).await?;
        if !self.current(epoch, owner) {
            return Ok(());
        }
        if let Some(held) = &held_types {
            let values = types(&held.value)?;
            self.session.update(|session| session.field_types = values);
        }
        if let Some(held) = &held_form {
            self.adopt(epoch, owner, &held.value, &draft, &fence)?;
        }
        // A refresh or first open owns its network demand. Offline held data
        // remains usable; a cold offline open returns the shared typed refusal.
        if held_form.is_none() || refresh {
            let value = forms::request(edge, FormsEndpoint::FormFactory, form_request).await?;
            self.adopt(epoch, owner, &value, &draft, &fence)?;
        }
        if (held_types.is_none() || refresh) && self.current(epoch, owner) {
            let value = forms::request(edge, FormsEndpoint::FormFactory, types_request).await?;
            if self.current(epoch, owner) {
                let values = types(&value)?;
                self.session.update(|session| session.field_types = values);
            }
        }
        if self.current(epoch, owner) {
            self.session.update(|session| session.loading = false);
        }
        Ok(())
    }

    fn adopt(
        &self,
        epoch: u64,
        owner: &str,
        value: &Value,
        draft: &Value,
        fence: &str,
    ) -> Result<(), Refusal> {
        if !self.current(epoch, owner) {
            return Ok(());
        }
        let session = self.session.get_untracked();
        let (document, version, drawing) = form(value, &session.slug)?;
        // Edits made while refresh is in flight stay work against their
        // original optimistic version. Only the reset baseline advances.
        let edited = session.revision > 0;
        let mut next = session;
        next.snapshot = Some(document.clone());
        next.loaded_version = version;
        next.drawing_settings = drawing;
        next.fence = Some(fence.into());
        next.loading = false;
        if !edited {
            next.document = document;
            next.version = version;
            next.draft_saved = false;
            if let Some(held) = draft.get("held").and_then(|held| held.get("value")) {
                let version = held["version"]
                    .as_u64()
                    .ok_or_else(|| invalid("The held form draft has no optimistic revision."))?;
                next.document = document_fn(&held["document"])?;
                next.version = version;
                next.draft_saved = true;
            }
        }
        let admission = factory(
            json!({"mode":"draft_admission","form_version":next.loaded_version,"draft_version":next.version}),
        )?;
        next.error = if admission["stale"] == true {
            Some(Refusal::new("form_editor_draft_stale","This local draft uses an older form version. Review it before publishing; version conflicts will be refused."))
        } else {
            None
        };
        self.session.set(next);
        Ok(())
    }

    async fn draft(
        &self,
        edge: &impl Edge,
        session: &Session,
        delete: bool,
    ) -> Result<(), Refusal> {
        let operation = if delete {
            Operation::Delete {
                kind: Kind::FormEditor,
                project: None,
                key: session.slug.clone(),
            }
        } else {
            Operation::Put {
                kind: Kind::FormEditor,
                project: None,
                key: session.slug.clone(),
                value: json!({"version":session.version,"document":session.document}),
                meta: None,
                now: edge.now_ms(),
            }
        };
        edge.store(Call {
            request: Request::WorkDrafts { operation },
            expect_owner: session.fence.clone(),
        })
        .await?;
        Ok(())
    }

    pub fn save_draft<E: Edge + 'static>(
        &self,
        edge: Rc<E>,
    ) -> impl Future<Output = Result<(), Refusal>> + use<E> {
        let this = self.clone();
        let active = self.active();
        async move {
            let (epoch, session) = active?;
            if session.saving {
                return Err(unavailable(
                    "Wait for this publish before saving another draft.",
                ));
            }
            let result = this.draft(&*edge, &session, false).await;
            match &result {
                Ok(()) if this.current(epoch, &session.owner) => this
                    .session
                    .update(|now| now.draft_saved = now.revision == session.revision),
                Err(refusal) => this.error(epoch, &session.owner, refusal),
                _ => {}
            }
            result
        }
    }

    pub fn reset<E: Edge + 'static>(
        &self,
        edge: Rc<E>,
    ) -> impl Future<Output = Result<(), Refusal>> + use<E> {
        let this = self.clone();
        let active = self.active();
        async move {
            let (epoch, session) = active?;
            if session.saving {
                return Err(unavailable(
                    "Wait for this publish before discarding its draft.",
                ));
            }
            let result = this.draft(&*edge, &session, true).await;
            match &result {
                Ok(()) if this.current(epoch, &session.owner) => this.session.update(|now| {
                    // A new edit after Reset was requested must not disappear.
                    if now.revision == session.revision {
                        now.document = now.snapshot.clone().unwrap();
                        now.version = now.loaded_version;
                        now.draft_saved = false;
                        now.error = None;
                        now.revision = now.revision.wrapping_add(1);
                    }
                }),
                Err(refusal) => this.error(epoch, &session.owner, refusal),
                _ => {}
            }
            result
        }
    }

    pub fn save<E: Edge + 'static>(
        &self,
        edge: Rc<E>,
    ) -> impl Future<Output = Result<(), Refusal>> + use<E> {
        let this = self.clone();
        let active = self.active();
        if let Ok((_, session)) = &active {
            if !session.saving {
                self.session.update(|now| {
                    now.saving = true;
                    now.error = None;
                });
            }
        }
        async move {
            let (epoch, session) = active?;
            if session.saving {
                return Err(unavailable("This form already has a publish in flight."));
            }
            let result = this.publish(&*edge, epoch, &session).await;
            if let Err(refusal) = &result {
                this.error(epoch, &session.owner, refusal);
            }
            if this.current(epoch, &session.owner) {
                this.session.update(|now| now.saving = false);
            }
            result
        }
    }

    async fn publish(
        &self,
        edge: &impl Edge,
        epoch: u64,
        session: &Session,
    ) -> Result<(), Refusal> {
        self.draft(edge, session, false).await?;
        if !self.current(epoch, &session.owner) {
            return Ok(());
        }
        self.session
            .update(|now| now.draft_saved = now.revision == session.revision);
        let mut body = session
            .document
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("The editor has no document."))?;
        body.extend(request("update", Some(&session.slug)));
        body.insert("version".into(), json!(session.version));
        let answer = forms::request(edge, FormsEndpoint::FormFactory, body).await?;
        let version = answer.get("form").unwrap_or(&answer)["version"]
            .as_u64()
            .ok_or_else(|| {
                Refusal::new(
                    ANSWER_INVALID,
                    "Form Factory did not confirm the published revision.",
                )
            })?;
        if !self.current(epoch, &session.owner) {
            return Ok(());
        }
        self.session.update(|now| {
            now.snapshot = Some(session.document.clone());
            now.loaded_version = version;
            now.version = version;
        });
        // Keep subsequent edits, even edits made during the final store write.
        loop {
            let now = self.session.get_untracked();
            if !self.current(epoch, &session.owner) {
                return Ok(());
            }
            let outcome = factory(
                json!({"mode":"save_result","saved":session.document,"current":now.document,"version":version}),
            )?;
            let clear = outcome["draft_action"] == "clear";
            self.draft(edge, &now, clear).await?;
            if !self.current(epoch, &session.owner) {
                return Ok(());
            }
            if self
                .session
                .with_untracked(|current| current.revision == now.revision)
            {
                self.session.update(|current| current.draft_saved = !clear);
                break;
            }
        }
        Ok(())
    }
}

// Avoid shadowing `document` in the adoption path.
fn document_fn(value: &Value) -> Result<Value, Refusal> {
    document(value)
}

#[cfg(test)]
#[path = "form_editor_tests.rs"]
mod tests;
