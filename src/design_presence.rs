//! Which of this browser's rooms should hold a server lease, which should
//! release, and how many lock calls one pass may spend.
//!
//! Lease bookkeeping over local rooms — a desktop holds the same rooms, so
//! none of this needs a browser. Four steps, in order:
//!
//!  1. a room that is present and CLEAN was saved (or downloaded clean): the
//!     save transaction already ended the hold and dropped the draft, so this
//!     pass only forgets it;
//!  2. a held room that VANISHED was discarded — but only a COMPLETE pass may
//!     say so. A partial refresh saw a subset, and releasing on that evidence
//!     would drop the lease of a room that is merely off-screen;
//!  3. a dirty, server-known room (re)takes its hold when it is new, when its
//!     digest moved, or when the last hold is older than `HOLD_REFRESH_MS`;
//!  4. and the whole pass is capped.
//!
//! The cap is ONE budget shared by both loops — the part a naive port loses.
//! Holds are spent first, so a pass that takes twenty holds defers EVERY
//! release and a vanished room keeps its server lease until a later pass.
//! That is deliberate: an unsaved room's presence is what another operator
//! needs to see, and a stale lease on a discarded room costs only its bounded
//! lifetime. Deferrals are COUNTED and named, never silently dropped.
//!
//! The kernel plans; the host performs the lock calls in the returned order
//! and feeds the results back, exactly as `transfer` does. No IO, no clock —
//! `now_ms` is an input. ds-brain's lease authority does not move.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::design::canonical_row_name;

pub const SCHEMA: &str = "ds.design-presence/v1";

const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

/// A liveness refresh is five minutes apart; this module never polls.
pub const HOLD_REFRESH_MS: i64 = 5 * 60_000;
/// A draft republish is rate-limited per transformer.
pub const DRAFT_MIN_INTERVAL_MS: i64 = 30_000;
/// One pass sends at most this many lock calls; the rest wait for the next.
pub const MAX_LOCK_CALLS_PER_PASS: usize = 20;

// ── the answer ──────────────────────────────────────────────────────────

/// Why a room (re)takes its hold.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// This browser holds no lease for it yet.
    New,
    /// The room's content digest moved under the lease.
    DigestChanged,
    /// The lease is older than `HOLD_REFRESH_MS`.
    Stale,
    /// Another account holds it; this is the periodic re-ask.
    ConflictRefresh,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct HoldCall {
    pub name: String,
    pub reason: HoldReason,
    /// Carried forward from the lease this browser already holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsaved_since: Option<String>,
    /// The anchor for a FIRST hold, as milliseconds. The host formats it —
    /// a calendar string is presentation, the choice of instant is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsaved_since_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub feature_counts: Map<String, Value>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ReleaseCall {
    pub name: String,
    /// This browser published a draft for it; releasing drops that too.
    pub delete_draft: bool,
}

/// Which call waited for the next pass, and why.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct DeferredCall {
    pub name: String,
    pub call: &'static str,
    pub reason: &'static str,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Budget {
    pub limit: usize,
    pub spent: usize,
    pub left: usize,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Plan {
    pub schema: &'static str,
    pub op: &'static str,
    /// Rooms whose lease and draft this browser should simply forget.
    pub clear: Vec<String>,
    pub hold: Vec<HoldCall>,
    pub release: Vec<ReleaseCall>,
    pub deferred: Vec<DeferredCall>,
    /// The count the reconcile result reports, kept as its own field so the
    /// host never has to know that a length is the number.
    pub deferred_count: usize,
    /// Dirty, server-known rooms whose digest the host has not read. The plan
    /// is still complete without them — a hold without a digest falls through
    /// to the staleness clause — but reading them makes it sharper.
    pub needs_digest: Vec<String>,
    pub budget: Budget,
    pub complete: bool,
}

// ── the request ─────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Plan(Box<PlanRequest>),
}

/// One of this browser's cached rooms for the project.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub name: String,
    pub dirty: bool,
    /// The server document exists — a lease can only be held on a known
    /// transformer.
    #[serde(default)]
    pub server_known: bool,
    /// Room write time; the first hold uses it as "unsaved since".
    #[serde(default)]
    pub updated_at_ms: i64,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub feature_counts: Map<String, Value>,
}

/// A lease this tab believes it holds.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Held {
    pub name: String,
    #[serde(default)]
    pub digest: Option<String>,
    /// When the lease was last taken or refreshed.
    pub at_ms: i64,
    #[serde(default)]
    pub unsaved_since: Option<String>,
    /// Another account holds the lease; the row shows their presence.
    #[serde(default)]
    pub conflict_with: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    /// Accepted so a caller may pin the contract; the `op` tag dispatches.
    #[serde(default)]
    #[allow(dead_code)]
    schema: Option<String>,
    #[serde(default)]
    rooms: Vec<Room>,
    #[serde(default)]
    held: Vec<Held>,
    /// Transformers this tab published a working draft for.
    #[serde(default)]
    drafts: Vec<String>,
    now_ms: i64,
    /// True when `rooms` is EVERY room of the project. Only then may a
    /// vanished room release its lease.
    #[serde(default)]
    complete: bool,
    #[serde(default = "default_budget")]
    budget: usize,
}

fn default_budget() -> usize {
    MAX_LOCK_CALLS_PER_PASS
}

// ── the rule ────────────────────────────────────────────────────────────

pub fn plan(request: &PlanRequestView<'_>) -> Plan {
    let now = request.now_ms;
    let limit = request.budget.min(MAX_LOCK_CALLS_PER_PASS);
    let mut clear = Vec::new();
    // Keyed sets, not scans. The browser looked each of these up in a `Set`
    // and a `Map`; a `contains` over a `Vec` would turn one pass over a
    // project's rooms into rooms x held work, which is the linear-scan shape
    // the Pain Ledger already paid for once.
    let present: BTreeSet<String> = request
        .rooms
        .iter()
        .map(|room| canonical_row_name(&room.name))
        .collect();

    // 1) Present and clean: the save transaction already ended the hold.
    for room in request.rooms {
        if !room.dirty {
            clear.push(room.name.clone());
        }
    }
    let cleared: BTreeSet<String> = clear.iter().map(|name| canonical_row_name(name)).collect();
    let drafts: BTreeSet<String> = request
        .drafts
        .iter()
        .map(|draft| canonical_row_name(draft))
        .collect();
    // First-wins, exactly as the `find` it replaces: two entries that
    // canonicalise to one name keep the earlier state.
    let mut held_by_name: BTreeMap<String, &Held> = BTreeMap::new();
    for held in request.held {
        held_by_name
            .entry(canonical_row_name(&held.name))
            .or_insert(held);
    }

    // 2) Held rooms that vanished release their hold and draft — complete
    //    passes only.
    let mut to_release: Vec<ReleaseCall> = Vec::new();
    if request.complete {
        for held in request.held {
            let canonical = canonical_row_name(&held.name);
            if cleared.contains(&canonical) || present.contains(&canonical) {
                continue;
            }
            to_release.push(ReleaseCall {
                delete_draft: drafts.contains(&canonical),
                name: held.name.clone(),
            });
        }
    }

    // 3) Dirty, server-known rooms (re)take their hold when new, changed or
    //    stale.
    let mut to_hold: Vec<HoldCall> = Vec::new();
    let mut needs_digest = Vec::new();
    for room in request
        .rooms
        .iter()
        .filter(|room| room.dirty && room.server_known)
    {
        let canonical = canonical_row_name(&room.name);
        let digest = room.digest.clone().filter(|value| !value.is_empty());
        if digest.is_none() {
            needs_digest.push(room.name.clone());
        }
        let state = held_by_name
            .get(&canonical)
            .copied()
            .filter(|_| !cleared.contains(&canonical));
        let reason = match state {
            None => Some(HoldReason::New),
            Some(state) if state.conflict_with.is_some() => {
                (now - state.at_ms > HOLD_REFRESH_MS).then_some(HoldReason::ConflictRefresh)
            }
            Some(state) => {
                if digest.is_some() && state.digest != digest {
                    Some(HoldReason::DigestChanged)
                } else {
                    (now - state.at_ms > HOLD_REFRESH_MS).then_some(HoldReason::Stale)
                }
            }
        };
        let Some(reason) = reason else { continue };
        let carried = state.and_then(|state| state.unsaved_since.clone());
        let anchor = if room.updated_at_ms != 0 {
            room.updated_at_ms
        } else {
            now
        };
        to_hold.push(HoldCall {
            name: room.name.clone(),
            reason,
            unsaved_since_ms: carried.is_none().then_some(anchor),
            unsaved_since: carried,
            digest,
            feature_counts: room.feature_counts.clone(),
        });
    }

    // 4) ONE budget, spent by the holds first and then by the releases.
    let mut budget = limit;
    let mut hold = Vec::new();
    let mut release = Vec::new();
    let mut deferred = Vec::new();
    for call in to_hold {
        if budget == 0 {
            deferred.push(DeferredCall {
                name: call.name,
                call: "hold",
                reason: "budget_exhausted",
            });
            continue;
        }
        budget -= 1;
        hold.push(call);
    }
    for call in to_release {
        if budget == 0 {
            deferred.push(DeferredCall {
                name: call.name,
                call: "release",
                reason: "budget_exhausted",
            });
            continue;
        }
        budget -= 1;
        release.push(call);
    }

    Plan {
        schema: SCHEMA,
        op: "plan",
        clear,
        hold,
        release,
        deferred_count: deferred.len(),
        deferred,
        needs_digest,
        budget: Budget {
            limit,
            spent: limit - budget,
            left: budget,
        },
        complete: request.complete,
    }
}

/// Borrowed request, so callers inside the workspace plan without building a
/// JSON document first.
pub struct PlanRequestView<'a> {
    pub rooms: &'a [Room],
    pub held: &'a [Held],
    pub drafts: &'a [String],
    pub now_ms: i64,
    pub complete: bool,
    pub budget: usize,
}

// ── the entry point ─────────────────────────────────────────────────────

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Design presence request exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design presence request: {error}"))?;
    let Request::Plan(request) = request;
    let view = PlanRequestView {
        rooms: &request.rooms,
        held: &request.held,
        drafts: &request.drafts,
        now_ms: request.now_ms,
        complete: request.complete,
        budget: request.budget,
    };
    serde_json::to_string(&plan(&view)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(name: &str, dirty: bool) -> Room {
        Room {
            name: name.into(),
            dirty,
            server_known: true,
            updated_at_ms: 1_000,
            digest: Some("d1".into()),
            feature_counts: Map::new(),
        }
    }

    fn held(name: &str, at_ms: i64) -> Held {
        Held {
            name: name.into(),
            digest: Some("d1".into()),
            at_ms,
            unsaved_since: Some("2026-09-11T07:00:00.000Z".into()),
            conflict_with: None,
        }
    }

    fn run(rooms: &[Room], held: &[Held], drafts: &[String], now: i64, complete: bool) -> Plan {
        plan(&PlanRequestView {
            rooms,
            held,
            drafts,
            now_ms: now,
            complete,
            budget: MAX_LOCK_CALLS_PER_PASS,
        })
    }

    #[test]
    fn a_clean_room_is_forgotten_and_never_re_held() {
        let plan = run(
            &[room("tx_1", false)],
            &[held("tx_1", 0)],
            &[],
            10_000,
            true,
        );
        assert_eq!(plan.clear, vec!["tx_1"]);
        assert!(plan.hold.is_empty() && plan.release.is_empty());
    }

    #[test]
    fn a_vanished_room_releases_only_on_a_complete_pass() {
        let drafts = vec!["tx_9".to_string()];
        let partial = run(&[], &[held("tx_9", 0)], &drafts, 10_000, false);
        assert!(partial.release.is_empty(), "a partial pass saw a subset");
        let complete = run(&[], &[held("tx_9", 0)], &drafts, 10_000, true);
        assert_eq!(
            complete.release,
            vec![ReleaseCall {
                name: "tx_9".into(),
                delete_draft: true
            }]
        );
    }

    /// The lease lookup is keyed, not scanned — a `Vec::contains` per room
    /// would make one pass over a project's rooms quadratic. Keying has one
    /// semantic to preserve: two entries that canonicalise to the same name
    /// keep the EARLIER state, exactly as the `find` it replaces did.
    #[test]
    fn two_leases_under_one_canonical_name_keep_the_earlier_state() {
        let mut stale = held("TX_1", 0);
        stale.digest = Some("earlier".into());
        let mut fresh = held("tx_1", HOLD_REFRESH_MS + 9_000);
        fresh.digest = Some("later".into());
        let mut subject = room("tx_1", true);
        subject.digest = Some("earlier".into());

        let plan = run(
            &[subject],
            &[stale, fresh],
            &[],
            HOLD_REFRESH_MS + 10_000,
            true,
        );
        // The earlier lease is old enough to be stale; the later one would not
        // be, so a last-wins lookup would plan no hold at all.
        assert_eq!(plan.hold.len(), 1, "the earlier lease decides");
        assert_eq!(plan.hold[0].reason, HoldReason::Stale);
        assert!(plan.release.is_empty(), "the room is present");
    }

    #[test]
    fn a_new_room_holds_and_anchors_unsaved_since_to_its_write_time() {
        let plan = run(&[room("tx_1", true)], &[], &[], 10_000, true);
        assert_eq!(plan.hold.len(), 1);
        assert_eq!(plan.hold[0].reason, HoldReason::New);
        assert_eq!(plan.hold[0].unsaved_since_ms, Some(1_000));
        assert!(plan.hold[0].unsaved_since.is_none());
    }

    #[test]
    fn a_moved_digest_re_holds_and_carries_the_original_anchor() {
        let mut moved = room("tx_1", true);
        moved.digest = Some("d2".into());
        let plan = run(&[moved], &[held("tx_1", 9_999)], &[], 10_000, true);
        assert_eq!(plan.hold[0].reason, HoldReason::DigestChanged);
        assert_eq!(
            plan.hold[0].unsaved_since.as_deref(),
            Some("2026-09-11T07:00:00.000Z")
        );
        assert!(plan.hold[0].unsaved_since_ms.is_none());
    }

    #[test]
    fn an_unchanged_room_waits_for_the_refresh_window() {
        let fresh = run(
            &[room("tx_1", true)],
            &[held("tx_1", 9_999)],
            &[],
            10_000,
            true,
        );
        assert!(fresh.hold.is_empty());
        let stale = run(
            &[room("tx_1", true)],
            &[held("tx_1", 0)],
            &[],
            HOLD_REFRESH_MS + 1,
            true,
        );
        assert_eq!(stale.hold[0].reason, HoldReason::Stale);
    }

    #[test]
    fn a_conflicted_lease_is_re_asked_only_after_the_refresh_window() {
        let mut conflicted = held("tx_1", 0);
        conflicted.conflict_with = Some("someone@example.com".into());
        conflicted.digest = Some("stale".into());
        let early = run(&[room("tx_1", true)], &[conflicted.clone()], &[], 10, true);
        assert!(
            early.hold.is_empty(),
            "a moved digest must not jump the window"
        );
        let late = run(
            &[room("tx_1", true)],
            &[conflicted],
            &[],
            HOLD_REFRESH_MS + 1,
            true,
        );
        assert_eq!(late.hold[0].reason, HoldReason::ConflictRefresh);
    }

    #[test]
    fn a_room_the_server_does_not_know_never_takes_a_lease() {
        let mut unknown = room("tx_1", true);
        unknown.server_known = false;
        assert!(run(&[unknown], &[], &[], 10_000, true).hold.is_empty());
    }

    #[test]
    fn one_budget_is_shared_and_holds_spend_it_first() {
        let rooms: Vec<Room> = (0..MAX_LOCK_CALLS_PER_PASS)
            .map(|index| room(&format!("tx_{index}"), true))
            .collect();
        let mut held_state: Vec<Held> = Vec::new();
        held_state.push(held("gone_1", 0));
        held_state.push(held("gone_2", 0));
        let plan = run(&rooms, &held_state, &[], 10_000, true);
        assert_eq!(plan.hold.len(), MAX_LOCK_CALLS_PER_PASS);
        assert!(
            plan.release.is_empty(),
            "twenty holds defer every release — the shared-budget interaction"
        );
        assert_eq!(plan.deferred_count, 2);
        assert!(plan.deferred.iter().all(|call| call.call == "release"));
        assert_eq!(plan.budget.left, 0);
        assert_eq!(plan.budget.spent, MAX_LOCK_CALLS_PER_PASS);
    }

    #[test]
    fn a_twenty_first_hold_is_deferred_and_counted() {
        let rooms: Vec<Room> = (0..MAX_LOCK_CALLS_PER_PASS + 1)
            .map(|index| room(&format!("tx_{index}"), true))
            .collect();
        let plan = run(&rooms, &[], &[], 10_000, true);
        assert_eq!(plan.hold.len(), MAX_LOCK_CALLS_PER_PASS);
        assert_eq!(plan.deferred_count, 1);
        assert_eq!(plan.deferred[0].call, "hold");
        assert_eq!(plan.deferred[0].reason, "budget_exhausted");
    }

    #[test]
    fn a_room_with_no_digest_is_named_and_still_planned() {
        let mut blind = room("tx_1", true);
        blind.digest = None;
        let plan = run(&[blind], &[held("tx_1", 9_999)], &[], 10_000, true);
        assert_eq!(plan.needs_digest, vec!["tx_1"]);
        assert!(
            plan.hold.is_empty(),
            "an unknown digest falls through to the staleness clause"
        );
    }

    #[test]
    fn evaluate_answers_the_plan() {
        let request = serde_json::json!({
            "op": "plan",
            "rooms": [{"name": "tx_1", "dirty": true, "server_known": true, "updated_at_ms": 5}],
            "held": [],
            "now_ms": 10,
            "complete": true,
        });
        let reply = evaluate(&serde_json::to_vec(&request).unwrap()).expect("plan answers");
        assert!(reply.contains(SCHEMA), "{reply}");
        assert!(reply.contains("\"reason\":\"new\""), "{reply}");
    }
}
