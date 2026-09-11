//! The force gate: which expensive actions need a grant before they bypass the
//! freshness guards, and how often the operator re-confirms.
//!
//! ONE policy for the register and for Project Control, which asked the same
//! question in two places with two different gate conditions. The prompt stays
//! in the page — this module never draws anything — but the policy, the
//! confirmation secret and the grant accounting are here.
//!
//! Why the secret moved: a literal in shipped client source is not a control.
//! It is still readable in a compiled artefact, so this is friction, not
//! authorization — `pipeline.force` on ds-brain is the authorization gate and
//! re-checks server-side on every call. What the move buys is that a page (and
//! `ds`) can no longer compare against it locally and skip the gate: they get a
//! minted grant token or nothing, and the grant is bound to the gesture and the
//! action it was minted for.
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.design-force-gate/v1";
const MAX_INPUT_BYTES: usize = 64 * 1024;

/// The shared confirmation code. Rotate here, in one place.
const CONFIRMATION_CODE: &str = "xxx";
/// Domain separation so a grant digest cannot be replayed as any other token.
const GRANT_SALT: &str = "ds.design-force-gate/grant/v1";
/// A grant outlives the gesture that minted it by minutes, never a session.
pub const GRANT_TTL_MS: u64 = 10 * 60 * 1000;

/// Every gated action, in ONE vocabulary. `report_export` and
/// `reports_only_export` are the two the installed compute exempts: local
/// regeneration spends no shared resource, so there is nothing to ration.
const ACTIONS: [&str; 8] = [
    "report_export",
    "reports_only_export",
    "retry_process",
    "process_batch",
    "add_to_combined",
    "combined_and_export",
    "project_run",
    "save_overwrite",
];

const LOCAL_COMPUTE_EXEMPT: [&str; 2] = ["report_export", "reports_only_export"];

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    /// Does this action need a grant before it runs forced?
    Policy {
        action: String,
        force: bool,
        /// The operator holds `pipeline.force`. Without it there is nothing to
        /// confirm: ds-brain refuses the call itself.
        capability_force: bool,
        #[serde(default)]
        native_report_generation: bool,
        #[serde(default)]
        target_count: u32,
    },
    /// Check the operator's typed answer and mint the gesture's grant.
    Verify {
        action: String,
        gesture: String,
        answer: String,
        now_ms: u64,
    },
    /// Is this grant still good for this action, in this gesture?
    ///
    /// `expires_at_ms` is echoed from the mint. It is inside the digest, so a
    /// caller that lengthens it invalidates the token it is presenting.
    Check {
        action: String,
        gesture: String,
        token: String,
        expires_at_ms: u64,
        now_ms: u64,
    },
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("force gate request exceeds 64 KiB".into());
    }
    let request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let result = match request {
        Request::Policy {
            action,
            force,
            capability_force,
            native_report_generation,
            target_count,
        } => {
            check_action(&action)?;
            policy(
                &action,
                force,
                capability_force,
                native_report_generation,
                target_count,
            )
        }
        Request::Verify {
            action,
            gesture,
            answer,
            now_ms,
        } => {
            check_action(&action)?;
            check_gesture(&gesture)?;
            if answer.trim() != CONFIRMATION_CODE {
                json!({"granted": false, "reason_key": "force_gate_answer_rejected"})
            } else {
                let expires_at_ms = now_ms.saturating_add(GRANT_TTL_MS);
                json!({
                    "granted": true,
                    "reason_key": "force_gate_granted",
                    "token": grant_token(&gesture, &action, expires_at_ms),
                    "expires_at_ms": expires_at_ms,
                })
            }
        }
        Request::Check {
            action,
            gesture,
            token,
            expires_at_ms,
            now_ms,
        } => {
            check_action(&action)?;
            check_gesture(&gesture)?;
            let matches = constant_time_eq(&token, &grant_token(&gesture, &action, expires_at_ms));
            let reason = if token.trim().is_empty() {
                "force_gate_grant_absent"
            } else if !matches {
                "force_gate_grant_invalid"
            } else if now_ms > expires_at_ms {
                "force_gate_grant_expired"
            } else {
                "force_gate_grant_valid"
            };
            json!({"granted": reason == "force_gate_grant_valid", "reason_key": reason})
        }
    };
    Ok(json!({"schema": SCHEMA, "result": result}).to_string())
}

fn check_action(action: &str) -> Result<(), String> {
    if ACTIONS.contains(&action) {
        return Ok(());
    }
    Err(format!(
        "unknown force-gated action; expected one of {}",
        ACTIONS.join(", ")
    ))
}

fn check_gesture(gesture: &str) -> Result<(), String> {
    if gesture.is_empty()
        || gesture.len() > 128
        || gesture.trim() != gesture
        || gesture.chars().any(char::is_control)
    {
        return Err("gesture id must be a bounded, explicit identifier".into());
    }
    Ok(())
}

fn policy(
    action: &str,
    force: bool,
    capability_force: bool,
    native_report_generation: bool,
    target_count: u32,
) -> serde_json::Value {
    // ONE gate condition, from the two that disagreed: Project Control asked
    // `canForce && force`, the register asked `force` alone (its toggle is
    // already capability-gated, so the two only ever differed in wording).
    if !force {
        return json!({"requires_grant": false, "reason_key": "force_not_requested"});
    }
    if !capability_force {
        return json!({"requires_grant": false, "reason_key": "force_capability_absent"});
    }
    if native_report_generation && LOCAL_COMPUTE_EXEMPT.contains(&action) {
        return json!({"requires_grant": false, "reason_key": "force_local_compute"});
    }
    json!({
        "requires_grant": true,
        "reason_key": "force_shared_compute",
        "target_count": target_count,
    })
}

fn grant_token(gesture: &str, action: &str, expires_at_ms: u64) -> String {
    let mut hasher = Sha256::new();
    for part in [GRANT_SALT, gesture, action, &expires_at_ms.to_string()] {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn run(request: Value) -> Value {
        let raw = evaluate(request.to_string().as_bytes()).unwrap();
        serde_json::from_str::<Value>(&raw).unwrap()["result"].clone()
    }

    #[test]
    fn one_policy_answers_both_surfaces() {
        let base =
            json!({"op":"policy","action":"retry_process","force":true,"capability_force":true});
        assert_eq!(run(base.clone())["requires_grant"], true);
        let mut unforced = base.clone();
        unforced["force"] = json!(false);
        assert_eq!(run(unforced)["reason_key"], "force_not_requested");
        let mut uncapable = base.clone();
        uncapable["capability_force"] = json!(false);
        assert_eq!(run(uncapable)["reason_key"], "force_capability_absent");
    }

    #[test]
    fn installed_compute_exempts_only_the_report_exports() {
        let request = |action: &str| json!({"op":"policy","action":action,"force":true,"capability_force":true,"native_report_generation":true});
        assert_eq!(run(request("report_export"))["requires_grant"], false);
        assert_eq!(run(request("reports_only_export"))["requires_grant"], false);
        assert_eq!(run(request("add_to_combined"))["requires_grant"], true);
        assert_eq!(run(request("project_run"))["requires_grant"], true);
        // A reviewed-overwrite bulk save spends shared write budget, not
        // local compute, so the installed engine never exempts it.
        assert_eq!(run(request("save_overwrite"))["requires_grant"], true);
    }

    #[test]
    fn a_grant_is_bound_to_its_gesture_and_action() {
        let minted = run(
            json!({"op":"verify","action":"retry_process","gesture":"g-1","answer":"xxx","now_ms":1_000}),
        );
        assert_eq!(minted["granted"], true);
        let token = minted["token"].as_str().unwrap().to_string();
        let expires_at_ms = minted["expires_at_ms"].as_u64().unwrap();
        assert_eq!(expires_at_ms, 1_000 + GRANT_TTL_MS);
        let check = |action: &str, gesture: &str, expires: u64, now_ms: u64| {
            run(json!({"op":"check","action":action,"gesture":gesture,"token":token,"expires_at_ms":expires,"now_ms":now_ms}))
                ["granted"]
                .as_bool()
                .unwrap()
        };
        assert!(check("retry_process", "g-1", expires_at_ms, 1_000));
        assert!(check("retry_process", "g-1", expires_at_ms, expires_at_ms));
        assert!(!check("add_to_combined", "g-1", expires_at_ms, 1_000));
        assert!(!check("retry_process", "g-2", expires_at_ms, 1_000));
        assert!(!check(
            "retry_process",
            "g-1",
            expires_at_ms,
            expires_at_ms + 1
        ));
        // Lengthening the expiry invalidates the token it is presented with.
        assert!(!check(
            "retry_process",
            "g-1",
            expires_at_ms + 60_000,
            1_000
        ));
    }

    #[test]
    fn a_wrong_answer_mints_nothing_and_an_absent_grant_says_so() {
        let refused = run(
            json!({"op":"verify","action":"project_run","gesture":"g","answer":"nope","now_ms":1}),
        );
        assert_eq!(refused["granted"], false);
        assert_eq!(refused["reason_key"], "force_gate_answer_rejected");
        assert!(refused.get("token").is_none());
        let absent = run(
            json!({"op":"check","action":"project_run","gesture":"g","token":"","expires_at_ms":9,"now_ms":1}),
        );
        assert_eq!(absent["reason_key"], "force_gate_grant_absent");
    }

    #[test]
    fn unknown_actions_and_gestures_are_refused_by_name() {
        assert!(evaluate(
            json!({"op":"policy","action":"delete_project","force":true,"capability_force":true})
                .to_string()
                .as_bytes()
        )
        .is_err());
        assert!(evaluate(
            json!({"op":"verify","action":"project_run","gesture":"","answer":"xxx","now_ms":1})
                .to_string()
                .as_bytes()
        )
        .is_err());
    }
}
