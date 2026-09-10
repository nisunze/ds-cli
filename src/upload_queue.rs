//! Upload admission — who runs next, who is a duplicate, and what a
//! cancellation settles.
//!
//! This module is **pure**: no IO, no timers, no promises, no store. It answers
//! the four questions a host must not answer for itself:
//!
//!   * **Is this a new unit of work?** Re-submitting a key that is already
//!     admitted must join the in-flight work, never start a second transfer of
//!     the same bytes. A drain loop that re-enqueues is then idempotent.
//!   * **Which admitted job runs next?** Admission order, always. Ordering
//!     matters more than parallelism when something fails halfway: a queue that
//!     finishes uploads one after another is one you can reason about.
//!   * **How many run at once?** [`MAX_CONCURRENT_UPLOADS`]. On a consumer link
//!     extra parallelism splits one pipe rather than adding throughput, while
//!     multiplying the transfers a connectivity blip can interrupt.
//!   * **What does a cancellation do?** A running job is signalled and reports
//!     back; a job that never started will never be settled by a transfer, so
//!     the queue must settle it. Getting this wrong leaves a caller awaiting a
//!     promise forever, which is what "cancel" looked like before.
//!
//! The host owns only mechanism: dispatching a transfer, firing a cancellation
//! signal, resolving a caller's promise, and drawing the result. It re-decides
//! none of the above, so a browser tab, an installed shell and a headless CLI
//! admit, order and bound uploads identically.
//!
//! [`ds-command-kernel::transfer`](crate::transfer) owns what happens *inside*
//! one transfer. This owns what happens *between* them. Neither borrows the
//! other's decisions.
use serde::{Deserialize, Serialize};

pub const UPLOAD_QUEUE_SCHEMA: &str = "ds.upload-queue/v1";

/// How many transfers run at once. A policy, not a tuning knob a caller passes.
pub const MAX_CONCURRENT_UPLOADS: usize = 2;

/// Bound on admitted work, so a runaway producer surfaces as a refusal instead
/// of an unbounded queue.
pub const MAX_ADMITTED_JOBS: usize = 512;

/// One admitted unit of work. `key` is its identity — the caller's stable name
/// for these exact bytes, never a session URI.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub key: String,
    /// Whether a transfer has been dispatched for it and has not yet settled.
    pub running: bool,
}

/// Durable queue state. The host persists it verbatim and hands it back; it is
/// the only thing the next decision needs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueueState {
    pub schema: String,
    /// Admitted work in admission order. Order IS the scheduling decision.
    pub jobs: Vec<Job>,
}

impl QueueState {
    fn empty() -> Self {
        QueueState {
            schema: UPLOAD_QUEUE_SCHEMA.to_string(),
            jobs: Vec::new(),
        }
    }

    fn running(&self) -> usize {
        self.jobs.iter().filter(|job| job.running).count()
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.jobs.iter().position(|job| job.key == key)
    }

    /// Fill every free slot with the earliest-admitted job that is not running.
    fn pump(&mut self, actions: &mut Vec<Action>) {
        while self.running() < MAX_CONCURRENT_UPLOADS {
            let Some(next) = self.jobs.iter_mut().find(|job| !job.running) else {
                return;
            };
            next.running = true;
            actions.push(Action::Start {
                key: next.key.clone(),
            });
        }
    }
}

/// What the host reports. Facts about its own mechanism — never a decision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// A caller asked for these bytes to be uploaded.
    Enqueue { key: String },
    /// A caller asked to cancel one key.
    Cancel { key: String },
    /// Every admitted upload must stop — the signed-in identity changed, and
    /// work authorized as one account must never continue against the next.
    CancelAll,
    /// A dispatched transfer reached a terminal result, whatever that result
    /// was. The queue does not care which: it cares that the slot is free.
    Settled { key: String },
}

/// The mechanism the host must now perform, in order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Dispatch the transfer for this key.
    Start { key: String },
    /// Fire this key's cancellation signal. The transfer is in flight and will
    /// report back with `Settled`, so the host must NOT resolve it here.
    Abort { key: String },
    /// Nothing will ever settle this key — no transfer was dispatched for it —
    /// so the host resolves its caller as aborted, now.
    SettleAborted { key: String },
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Reply {
    pub state: QueueState,
    pub actions: Vec<Action>,
    /// For `Enqueue`: whether this admitted NEW work. `false` means the key was
    /// already admitted and the caller must be handed the in-flight completion
    /// rather than a second transfer.
    pub admitted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    #[serde(default)]
    pub state: Option<QueueState>,
    pub event: Event,
}

pub fn apply(state: Option<QueueState>, event: Event) -> Result<Reply, String> {
    let mut state = match state {
        Some(existing) => {
            if existing.schema != UPLOAD_QUEUE_SCHEMA {
                return Err("Unsupported upload queue state schema.".into());
            }
            if existing.jobs.len() > MAX_ADMITTED_JOBS {
                return Err("Upload queue state exceeds its admission bound.".into());
            }
            existing
        }
        None => QueueState::empty(),
    };
    let mut actions = Vec::new();
    let mut admitted = false;

    match event {
        Event::Enqueue { key } => {
            if key.is_empty() {
                return Err("An upload needs an identity.".into());
            }
            // A key already admitted is the SAME work. Starting a second
            // transfer for it would duplicate bytes against one session.
            if state.index_of(&key).is_none() {
                if state.jobs.len() >= MAX_ADMITTED_JOBS {
                    return Err("Upload queue is full.".into());
                }
                state.jobs.push(Job {
                    key,
                    running: false,
                });
                admitted = true;
            }
            state.pump(&mut actions);
        }
        Event::Cancel { key } => {
            if let Some(index) = state.index_of(&key) {
                if state.jobs[index].running {
                    // In flight: the transfer honours the signal and reports
                    // back. Removing it here would free a slot that is still
                    // occupied and lose the caller's completion.
                    actions.push(Action::Abort { key });
                } else {
                    state.jobs.remove(index);
                    actions.push(Action::SettleAborted { key });
                }
            }
            state.pump(&mut actions);
        }
        Event::CancelAll => {
            // Admission order, so the report reads the way the queue ran.
            let mut remaining = Vec::new();
            for job in std::mem::take(&mut state.jobs) {
                if job.running {
                    actions.push(Action::Abort {
                        key: job.key.clone(),
                    });
                    remaining.push(job);
                } else {
                    actions.push(Action::SettleAborted { key: job.key });
                }
            }
            state.jobs = remaining;
        }
        Event::Settled { key } => {
            if let Some(index) = state.index_of(&key) {
                state.jobs.remove(index);
            }
            state.pump(&mut actions);
        }
    }

    Ok(Reply {
        state,
        actions,
        admitted,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > 256 * 1024 {
        return Err("Upload queue command exceeds its byte bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid upload queue command: {error}"))?;
    if request.schema != UPLOAD_QUEUE_SCHEMA {
        return Err("Unsupported upload queue command schema.".into());
    }
    let reply = apply(request.state, request.event)?;
    serde_json::to_string(&reply).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enqueue(state: Option<QueueState>, key: &str) -> Reply {
        apply(
            state,
            Event::Enqueue {
                key: key.to_string(),
            },
        )
        .expect("enqueue")
    }

    fn started(reply: &Reply) -> Vec<String> {
        reply
            .actions
            .iter()
            .filter_map(|action| match action {
                Action::Start { key } => Some(key.clone()),
                _ => None,
            })
            .collect()
    }

    /// Admit three, in order, against a cap of two.
    fn three() -> Reply {
        let a = enqueue(None, "a");
        let b = enqueue(Some(a.state), "b");
        enqueue(Some(b.state), "c")
    }

    #[test]
    fn bounds_concurrency_and_admits_in_order() {
        let a = enqueue(None, "a");
        assert_eq!(started(&a), vec!["a"]);
        let b = enqueue(Some(a.state), "b");
        assert_eq!(started(&b), vec!["b"]);
        let c = enqueue(Some(b.state), "c");
        assert_eq!(started(&c), Vec::<String>::new(), "the cap holds the third");
        assert_eq!(c.state.running(), MAX_CONCURRENT_UPLOADS);
    }

    #[test]
    fn a_freed_slot_admits_the_next_in_admission_order() {
        let three = three();
        let settled = apply(
            Some(three.state),
            Event::Settled {
                key: "a".to_string(),
            },
        )
        .expect("settle");
        assert_eq!(started(&settled), vec!["c"]);
        assert_eq!(
            settled
                .state
                .jobs
                .iter()
                .map(|j| j.key.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"],
        );
    }

    #[test]
    fn a_resubmitted_key_joins_the_in_flight_work() {
        let first = enqueue(None, "same");
        assert!(first.admitted);
        let second = enqueue(Some(first.state), "same");
        assert!(!second.admitted, "the caller gets the in-flight completion");
        assert_eq!(started(&second), Vec::<String>::new());
        assert_eq!(
            second.state.jobs.len(),
            1,
            "never two transfers for one key"
        );
    }

    #[test]
    fn a_resubmitted_key_that_is_still_queued_is_also_one_job() {
        let three = three();
        let again = enqueue(Some(three.state), "c");
        assert!(!again.admitted);
        assert_eq!(again.state.jobs.len(), 3);
    }

    #[test]
    fn cancelling_a_running_job_signals_it_and_waits_for_its_report() {
        let three = three();
        let cancelled = apply(
            Some(three.state),
            Event::Cancel {
                key: "a".to_string(),
            },
        )
        .expect("cancel");
        assert_eq!(
            cancelled.actions,
            vec![Action::Abort {
                key: "a".to_string()
            }],
            "no slot is freed until the transfer reports back",
        );
        assert!(cancelled.state.index_of("a").is_some());
    }

    #[test]
    fn cancelling_a_job_that_never_started_settles_it_here() {
        let three = three();
        let cancelled = apply(
            Some(three.state),
            Event::Cancel {
                key: "c".to_string(),
            },
        )
        .expect("cancel");
        // Nothing was dispatched for `c`, so no transfer will ever settle it.
        assert_eq!(
            cancelled.actions,
            vec![Action::SettleAborted {
                key: "c".to_string()
            }],
        );
        assert!(cancelled.state.index_of("c").is_none());
    }

    #[test]
    fn cancelling_an_unknown_key_decides_nothing() {
        let three = three();
        let before = three.state.clone();
        let cancelled = apply(
            Some(three.state),
            Event::Cancel {
                key: "never-queued".to_string(),
            },
        )
        .expect("cancel");
        assert_eq!(cancelled.actions, Vec::new());
        assert_eq!(cancelled.state, before);
    }

    #[test]
    fn cancel_all_signals_the_running_and_settles_the_queued() {
        let three = three();
        let all = apply(Some(three.state), Event::CancelAll).expect("cancel all");
        assert_eq!(
            all.actions,
            vec![
                Action::Abort {
                    key: "a".to_string()
                },
                Action::Abort {
                    key: "b".to_string()
                },
                Action::SettleAborted {
                    key: "c".to_string()
                },
            ],
        );
        // The two in flight stay admitted until they report; nothing is started
        // in their place, so a cancelled account cannot pull new work forward.
        assert_eq!(all.state.jobs.len(), 2);
        assert!(all.state.jobs.iter().all(|job| job.running));
    }

    #[test]
    fn a_settled_key_that_is_gone_is_not_an_error() {
        let three = three();
        let once = apply(
            Some(three.state),
            Event::Settled {
                key: "a".to_string(),
            },
        )
        .expect("settle");
        let twice = apply(
            Some(once.state),
            Event::Settled {
                key: "a".to_string(),
            },
        )
        .expect("duplicate settle");
        assert_eq!(started(&twice), Vec::<String>::new());
        assert_eq!(twice.state.jobs.len(), 2);
    }

    #[test]
    fn an_upload_needs_an_identity() {
        assert!(apply(None, Event::Enqueue { key: String::new() }).is_err());
    }

    #[test]
    fn a_foreign_state_schema_is_refused() {
        let mut state = QueueState::empty();
        state.schema = "ds.upload-queue/v0".to_string();
        assert!(apply(
            Some(state),
            Event::Enqueue {
                key: "a".to_string()
            }
        )
        .is_err());
    }

    #[test]
    fn the_json_door_round_trips() {
        let request = serde_json::json!({
            "schema": UPLOAD_QUEUE_SCHEMA,
            "event": { "kind": "enqueue", "key": "a" },
        });
        let raw = evaluate(request.to_string().as_bytes()).expect("evaluate");
        let reply: serde_json::Value = serde_json::from_str(&raw).expect("json");
        assert_eq!(reply["admitted"], true);
        assert_eq!(reply["actions"][0]["kind"], "start");
        assert_eq!(reply["actions"][0]["key"], "a");
        assert_eq!(reply["state"]["jobs"][0]["running"], true);
    }

    #[test]
    fn an_oversized_command_is_refused() {
        let huge = vec![b'x'; 256 * 1024 + 1];
        assert!(evaluate(&huge).is_err());
        assert!(evaluate(&[]).is_err());
    }
}
