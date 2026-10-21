//! The survey queue: every step of an entry this device captured, held on
//! the outbox's `survey` queue ([`crate::outbox`]) until it is delivered. A
//! create is written straight to Firestore as its author on every platform
//! ([`super::survey_create`]); a correction, deletion or restore goes
//! through Brain's mutate API
//! ([`super::survey::Mutation`]); a photo or other file through the media
//! upload ([`super::survey_media`]), whose bytes stay in
//! [`crate::media_outbox`]. The entry is the record: its create leaves
//! first, then its files, then its corrections.
//!
//! One drain runs both lanes at once, so the small lane runs to empty while
//! one large transfer keeps its lease part by part, and a second drain finds
//! that transfer leased and never dispatches it again. What each delivery
//! observed settles its row: a delivered row leaves, a failure a later
//! attempt may fix waits for its time, and a failure no retry fixes is
//! parked with the remedy the person is offered. Drains run when the person
//! touches the work or the link returns, never on a timer.

use futures_util::future::join;
use serde::Serialize;
use serde_json::{json, Value};

use super::retained::{store_refusal, Edge};
use super::survey::{mutation_outcome, Discarded, Mutation};
use super::survey_create::{Author, Create, Queued, Sent as Posted};
use super::survey_media::{carry, Dispatch, Renew, Transfer};
use super::Refusal;
use crate::durable_store::port::{Reply, StorePort};
use crate::durable_store::Request as Store;
use crate::media_outbox::Operation as Media;
use crate::outbox::{Lane, Operation, Queue, Settlement, LEASE_LOST, MAX_LEASE_MS};

pub const INVALID: &str = "survey_queue_invalid";
/// A near-instant delivery's lease: past Brain's and Firestore's own
/// two-minute deadline.
const SMALL_LEASE_MS: u64 = 3 * 60 * 1000;
/// A large transfer's lease, renewed before every part it reads.
const LARGE_LEASE_MS: u64 = MAX_LEASE_MS;
/// Every claim settles its row, and a row comes due again at once at most
/// once (a stale correction sent against the version Brain named): twice
/// the rows the queue keeps bounds one lane of a drain.
const MAX_ROUNDS: usize = 2 * 10_000 + 1;
const BASE_BACKOFF_MS: u64 = 2_000;
const MAX_BACKOFF_MS: u64 = 300_000;
/// Rejected sessions a step survives before it asks the person to sign in.
const AUTH_CAP: u64 = 3;
const MAX_SAFE: u64 = 9_007_199_254_740_991;

/// Every platform's create route: the signed-in author with their Firebase
/// ID token, and one create-only commit.
#[allow(async_fn_in_trait)]
pub trait Firestore {
    /// Who creates are written as, or why no one can be (signed out, a
    /// build that names no Firebase project).
    async fn author(&self) -> Result<(Author, String), String>;
    async fn commit(&self, url: &str, bearer: &str, body: &Value) -> Posted;
}

/// What one drain did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Drained {
    /// Steps delivered: entries created, corrected, files stored.
    pub delivered: u64,
    /// Steps parked by this drain until the person acts.
    pub failed: u64,
    /// The link dropped or the session ended: the drain stopped there.
    pub stopped: bool,
    /// When the next waiting step falls due, in epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub due_at: Option<u64>,
}

impl Drained {
    fn add(&mut self, other: Drained) {
        self.delivered += other.delivered;
        self.failed += other.failed;
        self.stopped |= other.stopped;
        self.due_at = match (self.due_at, other.due_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
}

/// How one delivery settles its row, and whether its lane stops.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub settlement: Settlement,
    pub stop: bool,
}

impl Verdict {
    fn settle(settlement: Settlement) -> Self {
        Self {
            settlement,
            stop: false,
        }
    }
    fn sent() -> Self {
        Self::settle(Settlement::Sent)
    }
    fn failed(error: &str, remedy: &str) -> Self {
        Self::settle(Settlement::Failed {
            error: error.into(),
            remedy: remedy.into(),
        })
    }
    /// Nothing was decided and nothing more can be now: wait, and stop.
    fn stopped(error: &str) -> Self {
        Self {
            settlement: Settlement::Deferred {
                error: Some(error.into()),
                next_attempt_at: None,
            },
            stop: true,
        }
    }
    fn wait(error: &str, at: u64) -> Self {
        Self::settle(Settlement::Deferred {
            error: Some(error.into()),
            next_attempt_at: Some(at),
        })
    }
}

fn backoff(attempts: u64) -> u64 {
    BASE_BACKOFF_MS
        .saturating_mul(1 << attempts.saturating_sub(1).min(7))
        .min(MAX_BACKOFF_MS)
}

/// What one delivery's outcome (the vocabulary of [`super::survey_create`]
/// and [`super::survey::MutationOutcome`]) means to its row, attempted
/// `attempts` times before.
pub fn verdict(outcome: &Value, attempts: u64, now: u64) -> Verdict {
    let retry = |error: &str, after: Option<u64>| {
        let wait = after.unwrap_or_else(|| backoff(attempts + 1)).min(MAX_BACKOFF_MS);
        Verdict::settle(Settlement::Retry {
            error: error.into(),
            next_attempt_at: now.saturating_add(wait),
        })
    };
    match outcome["kind"].as_str() {
        Some("success") => Verdict::sent(),
        Some("transient") => retry("transient", outcome["retry_after_ms"].as_u64()),
        Some("ambiguous_receipt") => retry("ambiguous_receipt", None),
        Some("response") => {
            let status = outcome["status"].as_u64().unwrap_or(0);
            if outcome["network"] == true || status == 0 {
                return Verdict::stopped("offline");
            }
            match status {
                408 | 429 | 500.. => retry("transient", None),
                401 if attempts + 1 < AUTH_CAP => retry("auth_rejected", None),
                401 => Verdict::failed("auth_rejected", "sign_in"),
                403 => Verdict::failed("forbidden", "access"),
                409 => Verdict::failed("conflict", "review"),
                _ => Verdict::failed("invalid", "discard"),
            }
        }
        _ => Verdict::failed("invalid", "discard"),
    }
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

async fn call(store: &impl StorePort, owner: Option<&str>, request: Store) -> Result<Reply, Refusal> {
    store
        .call(request, owner.map(str::to_owned), None)
        .await
        .map_err(store_refusal)
}

/// One drain's doors, its lease and the account its first answer named.
struct Drain<'a, E, S, T, F> {
    edge: &'a E,
    store: &'a S,
    transfer: &'a T,
    firestore: &'a F,
    project: Option<&'a str>,
    lease: &'a str,
    owner: String,
}

/// A transfer's lease, renewed before each part it reads.
struct Keep<'a, D> {
    drain: &'a D,
    id: &'a str,
    lease_ms: u64,
}

impl<E: Edge, S: StorePort, T: Transfer, F: Firestore> Renew for Keep<'_, Drain<'_, E, S, T, F>> {
    async fn renew(&self) -> bool {
        let renew = Operation::Revise {
            queue: Queue::Survey,
            id: self.id.into(),
            lease: self.drain.lease.into(),
            payload: None,
            meta: None,
            lease_ms: Some(self.lease_ms),
            now: self.drain.edge.now_ms(),
        };
        self.drain.outbox(renew).await.is_ok()
    }
}

impl<E: Edge, S: StorePort, T: Transfer, F: Firestore> Drain<'_, E, S, T, F> {
    async fn outbox(&self, operation: Operation) -> Result<Reply, Refusal> {
        call(self.store, Some(&self.owner), Store::Outbox { operation }).await
    }

    async fn media(&self, operation: Media) -> Result<Reply, Refusal> {
        let request = Store::MediaOutbox {
            operation: Box::new(operation),
        };
        call(self.store, Some(&self.owner), request).await
    }

    /// Claim, deliver and settle the lane's startable steps until none is.
    async fn lane(&self, lane: Lane) -> Result<Drained, Refusal> {
        let mut drained = Drained::default();
        let lease_ms = match lane {
            Lane::Small => SMALL_LEASE_MS,
            Lane::Large => LARGE_LEASE_MS,
        };
        for _ in 0..MAX_ROUNDS {
            if !self.edge.online() {
                drained.stopped = true;
                break;
            }
            let claim = Operation::Claim {
                queue: Queue::Survey,
                lane,
                project: self.project.map(str::to_owned),
                lease: self.lease.into(),
                lease_ms,
                now: self.edge.now_ms(),
            };
            let reply = self.outbox(claim).await?;
            let row = &reply.answer["claimed"];
            if row.is_null() {
                drained.due_at = reply.answer["due_at"].as_u64();
                break;
            }
            let id = text(&row["id"]);
            let verdict = self.deliver(row, lease_ms).await?;
            match &verdict.settlement {
                Settlement::Sent => drained.delivered += 1,
                Settlement::Failed { .. } => drained.failed += 1,
                _ => {}
            }
            let settle = Operation::Settle {
                queue: Queue::Survey,
                id: id.into(),
                lease: self.lease.into(),
                settlement: verdict.settlement,
                now: self.edge.now_ms(),
            };
            match self.outbox(settle).await {
                // Another drain took over a lapsed lease, or the person
                // discarded the step: theirs to settle now.
                Err(refusal) if refusal.code == LEASE_LOST => {}
                other => {
                    other?;
                }
            }
            if verdict.stop {
                drained.stopped = true;
                break;
            }
        }
        Ok(drained)
    }

    async fn deliver(&self, row: &Value, lease_ms: u64) -> Result<Verdict, Refusal> {
        let payload = &row["payload"];
        let attempts = row["attempts"].as_u64().unwrap_or(0);
        let outcome = match text(&payload["operation"]) {
            "create" => match self.create(row).await {
                Ok(outcome) => outcome,
                Err(reason) => return Ok(Verdict::stopped(&reason)),
            },
            "update" | "delete" | "restore" => {
                let Some(body) = payload["wire"].as_object().cloned() else {
                    return Ok(Verdict::failed("invalid", "discard"));
                };
                let outcome = json!(mutation_outcome(self.edge.fetch(Mutation { payload: body }).await));
                if let Some(rebased) = self.rebase(row, &outcome).await? {
                    return Ok(rebased);
                }
                outcome
            }
            "media" => return self.upload(row, lease_ms).await,
            _ => json!({"kind": "invalid"}),
        };
        Ok(verdict(&outcome, attempts, self.edge.now_ms()))
    }

    /// A queued create written straight to Firestore as its author.
    async fn create(&self, row: &Value) -> Result<Value, String> {
        let payload = &row["payload"];
        let (author, bearer) = self.firestore.author().await?;
        let queued = Queued {
            project: text(&row["project"]),
            form: text(&payload["form"]),
            doc: text(&payload["doc"]),
            idempotency_key: text(&payload["idempotency_key"]),
            created_at: payload["created_at"].as_u64().unwrap_or(0),
            owner_uid: text(&payload["uid"]),
        };
        let Some(create) = Create::prepare(&queued, &payload["wire"], &author) else {
            return Ok(json!({"kind": "invalid"}));
        };
        Ok(create.outcome(self.firestore.commit(&create.url(), &bearer, create.body()).await))
    }

    /// A correction Brain refused as stale goes again, once, against the
    /// version Brain named.
    async fn rebase(&self, row: &Value, outcome: &Value) -> Result<Option<Verdict>, Refusal> {
        let version = outcome["details"]["server_version"]
            .as_u64()
            .filter(|version| *version <= MAX_SAFE);
        let Some(version) = version.filter(|_| outcome["status"] == 409 && row["payload"]["rebased"] != true) else {
            return Ok(None);
        };
        let mut payload = row["payload"].clone();
        payload["wire"]["expected_version"] = json!(version);
        payload["rebased"] = json!(true);
        let revise = Operation::Revise {
            queue: Queue::Survey,
            id: text(&row["id"]).into(),
            lease: self.lease.into(),
            payload: Some(payload),
            meta: None,
            lease_ms: None,
            now: self.edge.now_ms(),
        };
        match self.outbox(revise).await {
            Ok(_) => {}
            Err(refusal) if refusal.code == LEASE_LOST => {}
            Err(refusal) => return Err(refusal),
        }
        let error = format!("expected_version:{version}");
        Ok(Some(Verdict::wait(&error, self.edge.now_ms())))
    }

    /// One file's upload, carried by the media outbox's plan under this
    /// step's lease.
    async fn upload(&self, row: &Value, lease_ms: u64) -> Result<Verdict, Refusal> {
        let Some(path) = row["payload"]["path"].as_str() else {
            return Ok(Verdict::failed("invalid", "discard"));
        };
        let dispatch = Media::Dispatch {
            path: path.into(),
            online: self.edge.online(),
            now: self.edge.now_ms(),
        };
        let reply = self.media(dispatch).await?;
        let answer = &reply.answer;
        // Stored already, or no longer held: nothing is left to carry.
        if answer["pending"] != true {
            return Ok(Verdict::sent());
        }
        let Some(upload) = answer.get("upload").filter(|upload| !upload.is_null()) else {
            return Ok(self.held(&answer["status"], &answer["error"], &answer["wake_at"], false));
        };
        let upload: Dispatch = serde_json::from_value(upload.clone()).map_err(|error| Refusal::new(INVALID, error.to_string()))?;
        let keep = Keep {
            drain: self,
            id: text(&row["id"]),
            lease_ms,
        };
        let outcome = carry(self.edge, self.store, self.transfer, &self.owner, &upload, &keep).await;
        let settle = Media::Settle {
            path: path.into(),
            generation: upload.generation,
            lease: upload.lease,
            outcome,
            now: self.edge.now_ms(),
        };
        let settled = self.media(settle).await?.answer;
        if settled["synced"] == true {
            return Ok(Verdict::sent());
        }
        let held = &settled["row"];
        Ok(self.held(&held["status"], &held["last_error"], &settled["wake_at"], true))
    }

    /// The step of a file the media outbox holds but did not deliver: its
    /// own attempts and backoff decide when it goes again. An attempt that
    /// names no wait (a lost session) goes again at once; the media
    /// outbox bounds its attempts.
    fn held(&self, status: &Value, error: &Value, wake_at: &Value, attempted: bool) -> Verdict {
        let error = error.as_str().unwrap_or("media_waiting");
        match text(status) {
            "failed" => Verdict::failed(error, "retry"),
            "blocked" => Verdict::failed(error, "discard"),
            _ if !self.edge.online() => Verdict::stopped("offline"),
            _ => {
                let now = self.edge.now_ms();
                let again = if attempted { now } else { now + BASE_BACKOFF_MS };
                let at = wake_at.as_u64().filter(|at| *at > now).unwrap_or(again);
                Verdict::wait(error, at)
            }
        }
    }
}

/// Deliver every startable step of the account's survey queue (of one
/// project when named), both lanes at once, under `lease`, a
/// token unique to this drain. Files held before the queue carried them are
/// scheduled first. Every store call after the first is fenced by the
/// account the first answered for, so a sign-out mid-drain stops it.
pub async fn drain(
    edge: &impl Edge,
    store: &impl StorePort,
    transfer: &impl Transfer,
    firestore: &impl Firestore,
    project: Option<&str>,
    lease: &str,
) -> Result<Drained, Refusal> {
    let schedule = Store::MediaOutbox {
        operation: Box::new(Media::Schedule { now: edge.now_ms() }),
    };
    let reply = call(store, None, schedule).await?;
    let drain = Drain {
        edge,
        store,
        transfer,
        firestore,
        project,
        lease,
        owner: reply.owner().unwrap_or_default().to_owned(),
    };
    let (small, large) = join(drain.lane(Lane::Small), drain.lane(Lane::Large)).await;
    let mut drained = small?;
    drained.add(large?);
    Ok(drained)
}

/// One queued step as the window reads it: what it does to which entry,
/// where it stands, and the feature shown until it is delivered.
fn step(row: &Value) -> Value {
    let payload = &row["payload"];
    json!({
        "id": row["id"], "project": row["project"], "operation": payload["operation"],
        "form": payload["form"], "doc": payload["doc"], "path": payload["path"],
        "status": row["status"], "attempts": row["attempts"], "error": row["error"],
        "remedy": row["remedy"], "next_attempt_at": row["next_attempt_at"],
        "created_at": row["created_at"], "feature": payload["overlay"],
    })
}

async fn list(store: &impl StorePort, project: Option<&str>) -> Result<(Vec<Value>, Option<String>), Refusal> {
    let operation = Operation::List {
        queue: Queue::Survey,
        project: project.map(str::to_owned),
    };
    let reply = call(store, None, Store::Outbox { operation }).await?;
    let rows = reply.answer["rows"].as_array().cloned().unwrap_or_default();
    Ok((rows, reply.owner().map(str::to_owned)))
}

/// Every step the account holds (of one project when named), oldest first.
pub async fn steps(store: &impl StorePort, project: Option<&str>) -> Result<Vec<Value>, Refusal> {
    Ok(list(store, project).await?.0.iter().map(step).collect())
}

/// Queue planned steps (a capture's create, corrections, a deletion and its
/// cascade), all or none, then schedule the files a capture admitted, so
/// they leave after its create.
pub async fn enqueue(edge: &impl Edge, store: &impl StorePort, rows: Vec<Value>) -> Result<u64, Refusal> {
    let rows = serde_json::from_value(Value::Array(rows)).map_err(|error| Refusal::new(INVALID, error.to_string()))?;
    let operation = Operation::Enqueue {
        queue: Queue::Survey,
        rows,
        now: edge.now_ms(),
    };
    let reply = call(store, None, Store::Outbox { operation }).await?;
    let owner = reply.owner().map(str::to_owned);
    let schedule = Store::MediaOutbox {
        operation: Box::new(Media::Schedule { now: edge.now_ms() }),
    };
    call(store, owner.as_deref(), schedule).await?;
    Ok(reply.answer["queued"].as_u64().unwrap_or(0))
}

/// The person's remedy: one parked step, or every one, due again now, with
/// the file uploads the media outbox parked.
pub async fn retry(edge: &impl Edge, store: &impl StorePort, id: Option<String>) -> Result<u64, Refusal> {
    let operation = Operation::Retry {
        queue: Queue::Survey,
        id,
        now: edge.now_ms(),
    };
    let reply = call(store, None, Store::Outbox { operation }).await?;
    let media = Store::MediaOutbox {
        operation: Box::new(Media::RetryFailed { now: edge.now_ms() }),
    };
    call(store, reply.owner(), media).await?;
    Ok(reply.answer["retried"].as_u64().unwrap_or(0))
}

/// The person's remedy: drop the named steps of one project, or every
/// parked one. Dropping a create drops its entry's every step; a dropped
/// file loses its held bytes. Brain's dead-letter audit records them first,
/// so nothing leaves unrecorded; a step being delivered now stays.
pub async fn discard(edge: &impl Edge, store: &impl StorePort, project: &str, ids: Option<Vec<String>>) -> Result<u64, Refusal> {
    let (rows, owner) = list(store, Some(project)).await?;
    let now = edge.now_ms();
    let named = |row: &Value| match &ids {
        Some(ids) => ids.iter().any(|id| row["id"] == id.as_str()),
        None => row["status"] == "failed",
    };
    let dropped_entries: Vec<&Value> = rows
        .iter()
        .filter(|row| named(row) && row["payload"]["operation"] == "create")
        .map(|row| &row["record"])
        .collect();
    let live = |row: &Value| row["status"] == "syncing" && row["lease_until"].as_u64().is_some_and(|until| until > now);
    let chosen: Vec<&Value> = rows
        .iter()
        .filter(|row| (named(row) || dropped_entries.contains(&&row["record"])) && !live(row))
        .collect();
    if chosen.is_empty() {
        return Ok(0);
    }
    // Brain's audit reads `doc_id`, `form_id` and `operation`.
    let entries = chosen
        .iter()
        .map(|row| {
            let payload = &row["payload"];
            json!({"id": row["id"], "doc_id": payload["doc"], "form_id": payload["form"],
                "operation": payload["operation"], "path": payload["path"], "status": row["status"],
                "attempts": row["attempts"], "error": row["error"], "created_at": row["created_at"],
                "payload": payload["wire"]})
        })
        .collect();
    edge.fetch(Discarded {
        project: project.into(),
        entries,
    })
    .await?;
    let mut removed = 0;
    for row in chosen {
        let operation = Operation::Discard {
            queue: Queue::Survey,
            id: text(&row["id"]).into(),
            now: edge.now_ms(),
        };
        let reply = call(store, owner.as_deref(), Store::Outbox { operation }).await?;
        removed += reply.answer["removed"].as_u64().unwrap_or(0);
        if let Some(path) = row["payload"]["path"].as_str() {
            let media = Store::MediaOutbox {
                operation: Box::new(Media::Remove { path: path.into() }),
            };
            call(store, owner.as_deref(), media).await?;
        }
    }
    Ok(removed)
}

/// The typed browser exports: the page's gateway, store and storage link.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::{discard, drain, enqueue, retry, steps, Author, Drained, Firestore, Posted, Refusal};
    use crate::durable_store::port::{PageStore, StoreHost};
    use crate::gateway_exchange::browser::{credentials, direct_post, facts, rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use crate::gateway_exchange::survey_create::{RESPONSE_CAP, TIMEOUT_MS};
    use crate::gateway_exchange::survey_media::browser::Storage;
    use crate::gateway_target::{FIREBASE_PROJECT, LANE};
    use serde_json::Value;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    struct Page(GatewayHost);

    impl Firestore for Page {
        async fn author(&self) -> Result<(Author, String), String> {
            let firebase_project = FIREBASE_PROJECT.ok_or("this build names no Firebase project for survey creates")?;
            let session = credentials(&self.0).await.map_err(|_| "sign in to deliver queued entries")?;
            let uid = facts(&self.0).map_err(|_| "sign in to deliver queued entries")?.principal;
            let bearer = session
                .token
                .filter(|token| !token.is_empty())
                .ok_or("sign in to deliver queued entries")?;
            let author = Author {
                firebase_project: firebase_project.into(),
                uid,
                email: session.email.trim().into(),
                lane: LANE.unwrap_or_default().into(),
            };
            Ok((author, bearer))
        }
        async fn commit(&self, url: &str, bearer: &str, body: &Value) -> Posted {
            match direct_post(url, bearer, body, TIMEOUT_MS, RESPONSE_CAP).await {
                Some((status, body)) => Posted::Answer { status, body },
                None => Posted::Failed,
            }
        }
    }

    fn edge(host: &GatewayHost, store: &StoreHost) -> BrowserEdge {
        // Imported hosts clone as their JsValue; each port holds its own handle.
        BrowserEdge::new(JsValue::clone(host).unchecked_into(), JsValue::clone(store).unchecked_into())
    }

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// Deliver every startable step of the account's survey queue (of one
    /// project when named), both lanes at once. Call it when the person
    /// touches the work or the link returns; a second call while one runs
    /// is safe and delivers only what the first has not leased.
    #[wasm_bindgen(js_name = surveyQueueDrain, unchecked_return_type = "Drained")]
    pub async fn survey_queue_drain(host: GatewayHost, store: StoreHost, project: Option<String>) -> Result<JsValue, JsValue> {
        let drained = drain_page(host.into(), store.into(), project)
            .await
            .map_err(|refusal| rejection(&refusal))?;
        typed(&drained)
    }

    /// One drain on the page's hosts, under a lease of its own.
    pub(crate) async fn drain_page(host: JsValue, store: JsValue, project: Option<String>) -> Result<Drained, Refusal> {
        let (host, store): (GatewayHost, StoreHost) = (host.unchecked_into(), store.unchecked_into());
        let page = edge(&host, &store);
        let port = PageStore::new(store);
        let lease = format!("survey-{}", uuid::Uuid::new_v4());
        let transfer = Storage(JsValue::clone(&host).unchecked_into());
        drain(&page, &port, &transfer, &Page(host), project.as_deref(), &lease).await
    }

    /// Every queued step (of one project when named), oldest first: what it
    /// does to which entry, where it stands, its remedy and the feature the
    /// map shows until it is delivered.
    #[wasm_bindgen(js_name = surveyQueueSteps, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn survey_queue_steps(store: StoreHost, project: Option<String>) -> Result<JsValue, JsValue> {
        let port = PageStore::new(store);
        typed(&steps(&port, project.as_deref()).await.map_err(|refusal| rejection(&refusal))?)
    }

    /// Queue planned steps, all or none; answers how many.
    #[wasm_bindgen(js_name = surveyQueueEnqueue)]
    pub async fn survey_queue_enqueue(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>[]")] rows: JsValue,
    ) -> Result<f64, JsValue> {
        let rows: Vec<Value> = tsify::serde_wasm_bindgen::from_value(rows).map_err(unreadable)?;
        let page = edge(&host, &store);
        let queued = enqueue(&page, &PageStore::new(store), rows).await.map_err(|refusal| rejection(&refusal))?;
        Ok(queued as f64)
    }

    /// The person's remedy: one parked step, or every one, due again now.
    #[wasm_bindgen(js_name = surveyQueueRetry)]
    pub async fn survey_queue_retry(host: GatewayHost, store: StoreHost, id: Option<String>) -> Result<f64, JsValue> {
        let page = edge(&host, &store);
        let retried = retry(&page, &PageStore::new(store), id).await.map_err(|refusal| rejection(&refusal))?;
        Ok(retried as f64)
    }

    /// The person's remedy: drop the named steps of one project, or every
    /// parked one, audited first.
    #[wasm_bindgen(js_name = surveyQueueDiscard)]
    pub async fn survey_queue_discard(host: GatewayHost, store: StoreHost, project: String, ids: Option<Vec<String>>) -> Result<f64, JsValue> {
        let page = edge(&host, &store);
        let dropped = discard(&page, &PageStore::new(store), &project, ids)
            .await
            .map_err(|refusal| rejection(&refusal))?;
        Ok(dropped as f64)
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) use browser::drain_page;

#[cfg(test)]
#[path = "survey_queue_tests.rs"]
mod tests;
