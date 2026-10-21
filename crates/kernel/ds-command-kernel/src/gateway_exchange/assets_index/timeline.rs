//! The Assets timeline flow. Each served page is retained in
//! [`crate::retained_reads`] (`assets_index_page`) per account, project and
//! query digest, with signed download URLs removed. Every read is decided by
//! [`crate::assets::index_read`]: a fresh held page answers without a request,
//! an offline window answers what it holds, concurrent reads of one page share
//! one request, and an answer that lands after the scope or index changed is
//! never published or retained. An access refusal forgets every page of the
//! project for this account; a committed classification marks them stale.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    predates_index, IndexGet, IndexList, IndexMeta, IndexPage, IndexQuery, IndexRow, RowAnswer,
    INVALID,
};
use crate::assets::index_read::{self, Held, MutationReceipt, Outcome, Scope, Ticket};
use crate::gateway_exchange::retained::{self, Edge};
use crate::gateway_exchange::Refusal;
use crate::retained_reads::Kind;

pub const SUPERSEDED: &str = "assets_index_superseded";
pub const NOT_RETAINED: &str = "assets_index_not_retained";
pub const OFFLINE: &str = "assets_index_offline";

/// What the timeline renders.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TimelinePage {
    Served {
        rows: Vec<IndexRow>,
        more: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        meta: Option<IndexMeta>,
        /// This device's retained copy rather than the network's answer.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        cached: bool,
    },
    /// A ds-brain without the index.
    Unavailable { rows: Vec<IndexRow> },
}

impl IndexPage {
    fn timeline(self, cached: bool) -> TimelinePage {
        TimelinePage::Served {
            rows: self.assets,
            more: self.more,
            cursor: self.next_cursor,
            meta: self.index,
            cached,
        }
    }
}

fn superseded() -> Refusal {
    Refusal::new(
        SUPERSEDED,
        "Project asset session or index changed during acquisition.",
    )
}

/// The coordinator state of one tab: per scope, the epoch fencing completions
/// and the page reads in flight. It holds no page.
#[derive(Debug, Default)]
pub struct Rooms(BTreeMap<String, Room>);
#[derive(Debug, Default)]
struct Room {
    epoch: u64,
    pending: BTreeMap<String, Ticket>,
}
impl Rooms {
    fn room(&mut self, scope_key: &str) -> &mut Room {
        self.0.entry(scope_key.into()).or_default()
    }
}

fn policy<T: serde::de::DeserializeOwned>(request: index_read::Request) -> Result<T, Refusal> {
    let answer = index_read::evaluate(request)
        .map_err(|code| Refusal::new(&code, "The Assets index scope or query is invalid."))?;
    serde_json::from_value(answer).map_err(|error| Refusal::new(INVALID, error.to_string()))
}
#[derive(Deserialize)]
struct Identity {
    scope_key: String,
    key: String,
    refresh: bool,
}
#[derive(Deserialize)]
struct Planned {
    decision: String,
    ticket: Ticket,
    show_retained: bool,
}
#[derive(Deserialize)]
struct Accepted {
    decision: String,
    epoch: u64,
}
#[derive(Deserialize)]
struct Admission {
    admitted: bool,
    epoch: u64,
}

/// A page read its plan started: what the acquisition needs to finish.
#[derive(Debug)]
pub struct Acquisition {
    scope: Scope,
    query: IndexQuery,
    pub ticket: Ticket,
    held: Option<Held>,
    retained: Option<IndexPage>,
    owner: Option<String>,
}

/// How a page read goes on after its plan.
#[derive(Debug)]
pub enum Begun {
    /// Answered from this device.
    Answer(TimelinePage),
    /// The same page is already in flight under this ticket.
    Join(Ticket),
    /// Read the page from the network.
    Acquire(Acquisition),
}

fn accept(
    rooms: &RefCell<Rooms>,
    scope: &Scope,
    ticket: &Ticket,
    outcome: Outcome,
    held: &Option<Held>,
    now: u64,
) -> Result<Accepted, Refusal> {
    let epoch = rooms.borrow_mut().room(&ticket.scope_key).epoch;
    policy(index_read::Request::Accept {
        scope: scope.clone(),
        ticket: ticket.clone(),
        epoch,
        outcome,
        held: held.clone(),
        now_ms: now as i64,
    })
}

/// Plan one page read: answer it from this device, join the read in flight
/// or start an acquisition. `on_saved` receives the retained first page at
/// once when the network is about to be asked for a newer one.
pub async fn begin(
    edge: &impl Edge,
    rooms: &RefCell<Rooms>,
    scope: Scope,
    query: IndexQuery,
    on_saved: impl Fn(&TimelinePage),
) -> Result<Begun, Refusal> {
    let wire =
        serde_json::to_value(&query).map_err(|error| Refusal::new(INVALID, error.to_string()))?;
    let identity: Identity = policy(index_read::Request::Identity {
        scope: scope.clone(),
        query: wire.clone(),
    })?;
    let captured = rooms.borrow_mut().room(&identity.scope_key).epoch;
    let observed = retained::read::<IndexPage>(
        edge,
        Kind::AssetsIndexPage,
        Some(&scope.project),
        &identity.key,
    )
    .await?;
    let held = observed.held.as_ref().map(|held| Held {
        key: identity.key.clone(),
        saved_at_ms: held.saved_at_ms as i64,
        invalidated: held.invalidated,
    });
    let retained = observed.held.map(|held| held.value);
    let now = edge.now_ms();
    let initial = Ticket {
        scope_key: identity.scope_key.clone(),
        key: identity.key.clone(),
        epoch: captured,
        refresh: identity.refresh,
    };
    if accept(rooms, &scope, &initial, Outcome::Success, &held, now)?.decision != "retain" {
        return Err(superseded());
    }
    let (epoch, pending) = {
        let mut rooms = rooms.borrow_mut();
        let room = rooms.room(&identity.scope_key);
        (room.epoch, room.pending.get(&identity.key).cloned())
    };
    let plan: Planned = policy(index_read::Request::Plan {
        scope: scope.clone(),
        query: wire,
        epoch,
        held: held.clone(),
        pending,
        online: edge.online(),
        now_ms: now as i64,
    })?;
    match plan.decision.as_str() {
        "retained" => {
            return retained
                .map(|page| Begun::Answer(page.timeline(true)))
                .ok_or_else(|| Refusal::new(NOT_RETAINED, "Project assets have no retained page."))
        }
        "unavailable" => {
            return Err(Refusal::new(
                OFFLINE,
                "Project assets are unavailable offline until they have been opened online.",
            ))
        }
        _ => {}
    }
    if plan.show_retained {
        if let Some(page) = &retained {
            on_saved(&page.clone().timeline(true));
        }
    }
    if plan.decision == "pending" {
        return Ok(Begun::Join(plan.ticket));
    }
    // An acquisition takes its ticket's epoch: an explicit refresh that
    // supersedes an ordinary read in flight fences that read's completion.
    {
        let mut rooms = rooms.borrow_mut();
        let room = rooms.room(&plan.ticket.scope_key);
        room.epoch = plan.ticket.epoch;
        room.pending
            .insert(plan.ticket.key.clone(), plan.ticket.clone());
    }
    Ok(Begun::Acquire(Acquisition {
        scope,
        query,
        ticket: plan.ticket,
        held,
        retained,
        owner: observed.owner,
    }))
}

/// Every key named `download_url`, at any depth, removed: a signed URL never
/// enters durable storage.
fn unsigned(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("download_url");
            map.values_mut().for_each(unsigned);
        }
        Value::Array(items) => items.iter_mut().for_each(unsigned),
        _ => {}
    }
}

/// Read one page from the network, retain it and answer it, unless the scope
/// or index changed meanwhile. The read leaves the in-flight set either way.
pub async fn acquire(
    edge: &impl Edge,
    rooms: &RefCell<Rooms>,
    acquisition: Acquisition,
) -> Result<TimelinePage, Refusal> {
    let ticket = acquisition.ticket.clone();
    let answer = edge
        .fetch(IndexList {
            project: acquisition.scope.project.clone(),
            query: acquisition.query.clone(),
        })
        .await;
    let settled = settle(edge, rooms, acquisition, answer).await;
    let mut rooms = rooms.borrow_mut();
    let room = rooms.room(&ticket.scope_key);
    if room.pending.get(&ticket.key) == Some(&ticket) {
        room.pending.remove(&ticket.key);
    }
    settled
}

async fn settle(
    edge: &impl Edge,
    rooms: &RefCell<Rooms>,
    acquisition: Acquisition,
    answer: Result<IndexPage, Refusal>,
) -> Result<TimelinePage, Refusal> {
    let Acquisition {
        scope,
        ticket,
        held,
        retained,
        owner,
        ..
    } = acquisition;
    let page = match answer {
        Ok(page) => page,
        Err(refusal) if predates_index(&refusal) => {
            return Ok(TimelinePage::Unavailable { rows: Vec::new() })
        }
        Err(refusal) => {
            let outcome = if matches!(refusal.status, Some(401 | 403 | 404)) {
                Outcome::AccessRefused
            } else {
                Outcome::TemporaryFailure
            };
            let accepted = accept(rooms, &scope, &ticket, outcome, &held, edge.now_ms())?;
            match accepted.decision.as_str() {
                "drop" => {
                    {
                        let mut rooms = rooms.borrow_mut();
                        let room = rooms.room(&ticket.scope_key);
                        room.epoch = accepted.epoch;
                        room.pending.clear();
                    }
                    // The refusal is the answer; a failed removal leaves the
                    // pages for the next refusal to forget.
                    let _ = retained::forget(
                        edge,
                        owner.as_deref(),
                        Kind::AssetsIndexPage,
                        Some(&scope.project),
                        None,
                    )
                    .await;
                }
                "retained" => {
                    if let Some(page) = retained {
                        return Ok(page.timeline(true));
                    }
                }
                _ => {}
            }
            return Err(refusal);
        }
    };
    if accept(
        rooms,
        &scope,
        &ticket,
        Outcome::Success,
        &held,
        edge.now_ms(),
    )?
    .decision
        != "retain"
    {
        return Err(superseded());
    }
    let mut stored =
        serde_json::to_value(&page).map_err(|error| Refusal::new(INVALID, error.to_string()))?;
    unsigned(&mut stored);
    match retained::write(
        edge,
        owner.as_deref(),
        Kind::AssetsIndexPage,
        Some(&scope.project),
        &ticket.key,
        &stored,
        None,
    )
    .await
    {
        Err(refusal) if refusal.code == crate::durable_store::OWNER_CHANGED => {
            return Err(superseded())
        }
        written => written?,
    }
    if accept(
        rooms,
        &scope,
        &ticket,
        Outcome::Success,
        &held,
        edge.now_ms(),
    )?
    .decision
        != "retain"
    {
        return Err(superseded());
    }
    Ok(page.timeline(false))
}

/// One row and its versions; `unavailable` from a ds-brain without the index.
pub async fn row(
    edge: &impl Edge,
    project: String,
    asset_id: String,
) -> Result<RowAnswer, Refusal> {
    match edge.fetch(IndexGet { project, asset_id }).await {
        Err(refusal) if predates_index(&refusal) => Ok(RowAnswer::Unavailable),
        answer => answer,
    }
}

/// A classification this caller committed: when its receipt is exactly the
/// change it asked for, the project's held pages turn stale, without a read.
/// Anything else changes nothing and answers `false`.
pub async fn classified(
    edge: &impl Edge,
    rooms: &RefCell<Rooms>,
    scope: Scope,
    captured: Scope,
    asset_id: String,
    expected_version: u64,
    receipt: MutationReceipt,
) -> bool {
    let Ok(identity) = policy::<Identity>(index_read::Request::Identity {
        scope: scope.clone(),
        query: json!({}),
    }) else {
        return false;
    };
    let epoch = rooms.borrow_mut().room(&identity.scope_key).epoch;
    let admission = policy::<Admission>(index_read::Request::Mutation {
        scope: scope.clone(),
        captured_scope: captured,
        expected_asset_id: asset_id,
        expected_version,
        receipt,
        epoch,
    });
    let Ok(Admission {
        admitted: true,
        epoch,
    }) = admission
    else {
        return false;
    };
    {
        let mut rooms = rooms.borrow_mut();
        let room = rooms.room(&identity.scope_key);
        room.epoch = epoch;
        room.pending.clear();
    }
    retained::invalidate(
        edge,
        None,
        Kind::AssetsIndexPage,
        Some(&scope.project),
        None,
    )
    .await
    .is_ok()
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
