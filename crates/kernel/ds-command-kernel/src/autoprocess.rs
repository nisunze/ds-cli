//! AutoProcess admission: whether the process action runs on its own or on
//! request, whether a committed edit warrants re-running the LV network,
//! and when queued work dispatches. LV differential scope lives in ds-network.
//!
//! Three decisions live here. The host keeps accumulator bookkeeping,
//! the `setTimeout`, and the engine latch. It keeps no
//! rule: a second copy of "does this field feed the rebuild" is exactly the
//! hand-transcription this migration exists to remove (the previous TS list
//! offered `tr_size`, an output, and missed every sizing pin).
//!
//! The two attribute vocabularies are NOT restated here. `LockableCell` and the
//! design status fields are ds-network's, runtime-installed into the host, and
//! the host passes them in. This module owns the RULE that unions them with the
//! extras below; ds-network owns the words.
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

pub const SCHEMA: &str = "ds.design-autoprocess/v4";
const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// Settle window. A rapid burst coalesces behind it even at the every-edit
/// cadence: zero delay ran the whole pipeline once per pointer-up mid-burst.
pub const SETTLE_MS: u64 = 650;

/// Geometry/structural commit reasons that always schedule a re-process.
const GEOMETRY_COMMIT_REASONS: [&str; 4] = [
    "design-edit",
    "design-edit-all",
    "geometry-edit",
    "geometry_changed",
];

const ATTRIBUTE_COMMIT_REASONS: [&str; 3] = [
    "attribute-row-delete",
    "attribute-cell-edit",
    "attribute-column-delete",
];

/// Fields that feed the rebuild but sit OUTSIDE the two ds-network vocabularies
/// unioned below: topology/connection identity plus the transformer sizing pins
/// (`plan_kva` overrides everything, `ex_tr_size` overrides the calculated
/// size — LV build contract § sizing).
const EXTRA_RELEVANT_FIELDS: [&str; 14] = [
    "abc_size",
    "connection_status",
    "connection_type",
    "cum_load",
    "ex_tr_size",
    "house_number",
    "is_connected",
    "line_number",
    "load",
    "phase",
    "plan_kva",
    "pole_number",
    "root_span_cable_size",
    "status",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    /// ds-network's `LockableCell` wire names, as installed into the host.
    #[serde(default)]
    pub lockable_cells: Vec<String>,
    /// The design status fields (existing/drafting/construction + details +
    /// `drafting_locked_properties`).
    #[serde(default)]
    pub status_fields: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cadence {
    pub min_pending_edits: u32,
    pub max_pending_seconds: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CadenceRequest {
    pub enabled: bool,
    pub running: bool,
    pub queued: bool,
    pub force: bool,
    pub pending_edit_count: u32,
    /// 0 = no window open yet.
    pub window_started_at_ms: u64,
    pub cadence: Cadence,
    pub now_ms: u64,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    /// Which process action the host presents for this editing context:
    /// `auto` when the process is available here and AutoProcess is on,
    /// otherwise `manual`. There is one way to process a transformer, so the
    /// only question is whether it runs by itself.
    Mode {
        /// The process action is available in this editing context (not a
        /// temporary edit).
        process_active: bool,
        auto_process_enabled: bool,
    },
    /// Does this committed edit warrant re-running the LV network?
    Trigger {
        reason: String,
        #[serde(default)]
        changed_fields: Vec<String>,
        vocabulary: Vocabulary,
    },
    /// When does queued work actually dispatch?
    Cadence(CadenceRequest),
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("autoprocess request exceeds 1 MiB".into());
    }
    let request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let result = match request {
        Request::Mode {
            process_active,
            auto_process_enabled,
        } => process_mode(process_active, auto_process_enabled),
        Request::Trigger {
            reason,
            changed_fields,
            vocabulary,
        } => trigger(&reason, &changed_fields, &vocabulary),
        Request::Cadence(request) => cadence(&request)?,
    };
    Ok(json!({"schema": SCHEMA, "result": result}).to_string())
}

fn process_mode(process_active: bool, auto_process_enabled: bool) -> serde_json::Value {
    if process_active && auto_process_enabled {
        return json!({"mode": "auto", "reason_key": "autoprocess_mode_enabled"});
    }
    json!({"mode": "manual", "reason_key": "autoprocess_mode_manual"})
}

/// Lowercase, then collapse every non-alphanumeric run to one `_` and trim.
pub fn clean_field_name(field: &str) -> String {
    let lowered = field.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut pending = false;
    for ch in lowered.chars() {
        if ch.is_ascii_digit() || ch.is_ascii_lowercase() {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.push(ch);
        } else {
            pending = true;
        }
    }
    out
}

fn trigger(reason: &str, changed_fields: &[String], vocabulary: &Vocabulary) -> serde_json::Value {
    if GEOMETRY_COMMIT_REASONS.contains(&reason) {
        return json!({"schedule": true, "reason_key": "autoprocess_trigger_geometry"});
    }
    if !ATTRIBUTE_COMMIT_REASONS.contains(&reason) {
        return json!({"schedule": false, "reason_key": "autoprocess_trigger_unrelated_reason"});
    }
    if reason == "attribute-row-delete" {
        return json!({"schedule": true, "reason_key": "autoprocess_trigger_row_delete"});
    }
    let relevant: BTreeSet<String> = vocabulary
        .lockable_cells
        .iter()
        .chain(vocabulary.status_fields.iter())
        .map(|s| s.to_string())
        .chain(EXTRA_RELEVANT_FIELDS.iter().map(|s| s.to_string()))
        .collect();
    let fields: Vec<String> = changed_fields
        .iter()
        .map(|f| clean_field_name(f))
        .filter(|f| !f.is_empty())
        .collect();
    if fields.is_empty() {
        return json!({"schedule": true, "reason_key": "autoprocess_trigger_fields_unnamed"});
    }
    let matched: Vec<&String> = fields.iter().filter(|f| relevant.contains(*f)).collect();
    if matched.is_empty() {
        return json!({"schedule": false, "reason_key": "autoprocess_trigger_fields_irrelevant"});
    }
    json!({"schedule": true, "reason_key": "autoprocess_trigger_fields_relevant", "fields": matched})
}

fn cadence(request: &CadenceRequest) -> Result<serde_json::Value, String> {
    if request.cadence.max_pending_seconds > 24 * 60 * 60 {
        return Err("cadence max pending window exceeds one day".into());
    }
    // A waiting queue is never work in flight. docs/autoprocess-contract.md
    // §HARD RULE: the host must render this field, never a running spinner.
    let waiting = |decision: &str, wait_ms: Option<u64>, key: &str| {
        let mut value = json!({
            "decision": decision,
            "reason_key": key,
            "waiting_not_executing": decision != "dispatch",
            // The coalescing window this verdict was decided with. It is here
            // because the host arms ONE more timer the cadence never sees: the
            // retry that keeps queued work alive while the engine latch is
            // held. That retry is this window, and a second copy of the number
            // in TypeScript is a cadence rule living outside the kernel.
            "settle_ms": SETTLE_MS,
        });
        if let Some(ms) = wait_ms {
            value["wait_ms"] = json!(ms);
        }
        value
    };
    if !request.enabled {
        return Ok(waiting("idle", None, "autoprocess_disabled"));
    }
    if !request.queued && request.pending_edit_count == 0 && !request.force {
        return Ok(waiting("idle", None, "autoprocess_no_pending_work"));
    }
    if request.running {
        // Queued behind the engine latch: no timer is armed, and the host
        // re-evaluates when the run releases. `queued: true` with no armed
        // timer and no running run is the forever-spinner, so this branch is
        // the ONLY one that may answer without a wait_ms.
        return Ok(waiting("wait", None, "autoprocess_engine_busy"));
    }
    let (delay_ms, key) = if request.force {
        (SETTLE_MS, "autoprocess_forced_settle")
    } else if request.cadence.min_pending_edits <= 1 {
        (SETTLE_MS, "autoprocess_every_edit_settle")
    } else {
        let max_pending_ms = u64::from(request.cadence.max_pending_seconds) * 1000;
        if request.window_started_at_ms == 0 {
            (max_pending_ms, "autoprocess_window_unopened")
        } else {
            let elapsed = request.now_ms.saturating_sub(request.window_started_at_ms);
            let remaining = max_pending_ms.saturating_sub(elapsed);
            if request.pending_edit_count >= request.cadence.min_pending_edits {
                (
                    SETTLE_MS.min(remaining),
                    "autoprocess_edit_threshold_reached",
                )
            } else {
                (remaining, "autoprocess_window_remaining")
            }
        }
    };
    if delay_ms == 0 {
        return Ok(waiting("dispatch", Some(0), "autoprocess_window_elapsed"));
    }
    Ok(waiting("wait", Some(delay_ms), key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn run(request: Value) -> Value {
        let raw = evaluate(request.to_string().as_bytes()).unwrap();
        serde_json::from_str::<Value>(&raw).unwrap()["result"].clone()
    }

    fn vocabulary() -> Value {
        json!({
            "lockable_cells": ["struct_type", "tr_size", "cable_size"],
            "status_fields": ["existing_status", "drafting_locked_properties"],
        })
    }

    #[test]
    fn geometry_reasons_always_fire_and_unrelated_never_does() {
        for reason in GEOMETRY_COMMIT_REASONS {
            let out = run(json!({"op":"trigger","reason":reason,"vocabulary":vocabulary()}));
            assert_eq!(out["schedule"], true, "{reason}");
        }
        let out =
            run(json!({"op":"trigger","reason":"selection-changed","vocabulary":vocabulary()}));
        assert_eq!(out["schedule"], false);
        assert_eq!(out["reason_key"], "autoprocess_trigger_unrelated_reason");
    }

    #[test]
    fn process_mode_has_one_answer_for_every_host() {
        let mode = |process_active, auto_process_enabled| {
            run(json!({
                "op": "mode",
                "process_active": process_active,
                "auto_process_enabled": auto_process_enabled,
            }))
        };
        assert_eq!(mode(false, false)["mode"], "manual");
        assert_eq!(mode(false, false)["reason_key"], "autoprocess_mode_manual");
        assert_eq!(mode(true, false)["mode"], "manual");
        assert_eq!(mode(false, true)["mode"], "manual");
        assert_eq!(mode(true, true)["mode"], "auto");
        assert_eq!(mode(true, true)["reason_key"], "autoprocess_mode_enabled");
        // The lane switch is gone from the request.
        let stale = serde_json::to_vec(&json!({
            "op": "mode", "is_fast_lane": true, "fast_process_active": true, "auto_process_enabled": true,
        }))
        .unwrap();
        assert!(evaluate(&stale).is_err());
    }

    #[test]
    fn attribute_commits_are_field_filtered_over_the_union() {
        let case = |fields: Value| {
            run(
                json!({"op":"trigger","reason":"attribute-cell-edit","changed_fields":fields,"vocabulary":vocabulary()}),
            )
        };
        assert_eq!(case(json!(["Struct Type"]))["schedule"], true);
        assert_eq!(
            case(json!(["drafting_locked_properties"]))["schedule"],
            true
        );
        assert_eq!(case(json!(["plan_kva"]))["schedule"], true);
        assert_eq!(case(json!(["comment"]))["schedule"], false);
        // An unnamed field set is conservative: schedule.
        assert_eq!(case(json!([]))["schedule"], true);
        assert_eq!(case(json!(["   "]))["schedule"], true);
        // A row delete never inspects fields.
        assert_eq!(
            run(
                json!({"op":"trigger","reason":"attribute-row-delete","changed_fields":["comment"],"vocabulary":vocabulary()})
            )["schedule"],
            true
        );
    }

    #[test]
    fn cadence_reproduces_the_settle_and_window_arithmetic() {
        let base = json!({
            "op": "cadence", "enabled": true, "running": false, "queued": true, "force": false,
            "pending_edit_count": 1, "window_started_at_ms": 1_000, "now_ms": 1_100,
            "cadence": {"min_pending_edits": 1, "max_pending_seconds": 30},
        });
        let with = |edits: [(&str, Value); 2]| {
            let mut request = base.clone();
            for (key, value) in edits {
                request[key] = value;
            }
            run(request)
        };
        assert_eq!(run(base.clone())["wait_ms"], 650);
        assert_eq!(
            with([("force", json!(true)), ("pending_edit_count", json!(9))])["wait_ms"],
            650
        );
        // min_pending_edits 3, 1 edit in: wait out the remaining max window.
        let mut request = base.clone();
        request["cadence"] = json!({"min_pending_edits": 3, "max_pending_seconds": 30});
        assert_eq!(run(request.clone())["wait_ms"], 29_900);
        request["pending_edit_count"] = json!(3);
        assert_eq!(run(request.clone())["wait_ms"], 650);
        // Window fully elapsed: dispatch.
        request["now_ms"] = json!(40_000);
        assert_eq!(run(request.clone())["decision"], "dispatch");
        assert_eq!(run(request.clone())["waiting_not_executing"], false);
        // No window open yet: the whole max window.
        request["window_started_at_ms"] = json!(0);
        assert_eq!(run(request)["wait_ms"], 30_000);
    }

    #[test]
    fn a_run_holding_the_engine_waits_without_a_timer() {
        let out = run(json!({
            "op": "cadence", "enabled": true, "running": true, "queued": true, "force": false,
            "pending_edit_count": 2, "window_started_at_ms": 1_000, "now_ms": 1_100,
            "cadence": {"min_pending_edits": 1, "max_pending_seconds": 30},
        }));
        assert_eq!(out["decision"], "wait");
        assert_eq!(out["reason_key"], "autoprocess_engine_busy");
        assert!(out.get("wait_ms").is_none());
        assert_eq!(out["waiting_not_executing"], true);
        // No dispatch timer, but the host still re-asks while the latch is
        // held — `queued: true` with nothing armed is the forever-spinner. The
        // window it re-asks after is this one, owned here and nowhere else.
        assert_eq!(out["settle_ms"], SETTLE_MS);
    }

    #[test]
    fn disabled_and_empty_queues_are_idle_not_waiting_work() {
        let out = run(json!({
            "op": "cadence", "enabled": false, "running": false, "queued": true, "force": false,
            "pending_edit_count": 3, "window_started_at_ms": 0, "now_ms": 1,
            "cadence": {"min_pending_edits": 1, "max_pending_seconds": 30},
        }));
        assert_eq!(out["decision"], "idle");
        let out = run(json!({
            "op": "cadence", "enabled": true, "running": false, "queued": false, "force": false,
            "pending_edit_count": 0, "window_started_at_ms": 0, "now_ms": 1,
            "cadence": {"min_pending_edits": 1, "max_pending_seconds": 30},
        }));
        assert_eq!(out["reason_key"], "autoprocess_no_pending_work");
    }

    #[test]
    fn unknown_fields_and_oversized_input_are_refused() {
        assert!(evaluate(
            json!({"op":"trigger","reason":"design-edit","vocabulary":{},"extra":1})
                .to_string()
                .as_bytes()
        )
        .is_err());
        assert!(evaluate(&vec![b'x'; MAX_INPUT_BYTES + 1]).is_err());
    }

    #[test]
    fn clean_field_name_matches_the_host_spelling() {
        assert_eq!(clean_field_name("  Struct Type  "), "struct_type");
        assert_eq!(clean_field_name("PLAN--KVA"), "plan_kva");
        assert_eq!(clean_field_name("__x__"), "x");
        assert_eq!(clean_field_name("***"), "");
    }
}
