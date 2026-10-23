use std::collections::VecDeque;

use super::*;

/// A scripted driver: what each call answers, and what was said.
#[derive(Default)]
struct Script {
    connected: VecDeque<Option<Value>>,
    pending: Option<Value>,
    statuses: VecDeque<&'static str>,
    begun: usize,
    completed: usize,
    discarded: usize,
    waited: Duration,
    told: Vec<String>,
}

fn public(request: &str) -> Value {
    json!({
        "request_id": request,
        "user_code": "BLUE-OTTER",
        "verification_uri": "https://example.test/device",
        "expires_at": "2026-09-22T12:00:00Z",
        "poll_interval_seconds": 5,
        "device_fingerprint": format!("sha256:{}", "a".repeat(64)),
        "scopes": ["ds.api"],
        "binding": {},
    })
}

fn status_shape() -> Value {
    json!({
        "lane": "canary", "signed_in": true, "uid": "uid-1",
        "email": "owner@example.test", "credential_provider": "ds_device",
        "device_id": "device-1", "auth_context": {},
    })
}

impl LinkDriver for Script {
    fn connected(&mut self, _: Lane) -> Result<Option<Value>, Failure> {
        Ok(self.connected.pop_front().unwrap_or(None))
    }
    fn pending(&mut self, _: Lane) -> Result<Option<Value>, Failure> {
        Ok(self.pending.clone())
    }
    fn begin(&mut self, _: Lane, device_name: &str) -> Result<Value, Failure> {
        self.begun += 1;
        Ok(public(&format!("request-{}-{device_name}", self.begun)))
    }
    fn status(&mut self, _: Lane) -> Result<Value, Failure> {
        let token = self.statuses.pop_front().unwrap_or("pending");
        // The endpoint answers for the request this lane holds.
        let mut observed = self
            .pending
            .as_ref()
            .and_then(|pending| pending["request_id"].as_str())
            .map_or_else(|| public("request-1-box"), public);
        observed["status"] = json!(token);
        Ok(observed)
    }
    fn complete(&mut self, _: Lane) -> Result<Value, Failure> {
        self.completed += 1;
        Ok(json!({ "linked": true }))
    }
    fn discard(&mut self, _: Lane) -> Result<(), Failure> {
        self.discarded += 1;
        Ok(())
    }
    fn wait(&mut self, duration: Duration) {
        self.waited += duration;
    }
    fn tell(&mut self, line: &str) {
        self.told.push(line.to_owned());
    }
}

#[test]
fn a_connected_lane_answers_already_connected_and_touches_nothing() {
    let mut script = Script {
        connected: VecDeque::from([Some(status_shape())]),
        ..Default::default()
    };
    let answer = connect_with(&mut script, Lane::Canary, "box", 300).unwrap();
    assert_eq!(answer["state"], "already_connected");
    assert_eq!(answer["connected"], true);
    assert_eq!(answer["signed_in"], true);
    assert_eq!(answer["email"], "owner@example.test");
    assert_eq!(script.begun, 0);
    assert_eq!(script.waited, Duration::ZERO);
    assert!(script.told.is_empty());
}

#[test]
fn a_machine_host_gets_the_request_at_once_and_its_own_next_step() {
    let mut script = Script::default();
    let answer = connect_with(&mut script, Lane::Stable, "box", 0).unwrap();
    assert_eq!(answer["state"], "pending_approval");
    assert_eq!(answer["connected"], false);
    assert_eq!(answer["request_id"], "request-1-box");
    assert_eq!(answer["status"], "unobserved");
    assert_eq!(answer["next"], "ds account connect --lane stable");
    assert_eq!(answer["approve"], APPROVAL_INSTRUCTIONS);
    assert_eq!(script.begun, 1);
    assert_eq!(script.waited, Duration::ZERO);
    // A person reading stderr sees the same two values the Desktop asks for.
    let said = script.told.join("\n");
    assert!(said.contains("request-1-box"), "{said}");
    assert!(
        said.contains(&format!("sha256:{}", "a".repeat(64))),
        "{said}"
    );
    assert!(said.contains("Link a trusted device"), "{said}");
}

#[test]
fn an_approval_completes_the_link_and_answers_the_status_shape() {
    let mut script = Script {
        connected: VecDeque::from([None, Some(status_shape())]),
        statuses: VecDeque::from(["pending", "approved"]),
        ..Default::default()
    };
    let answer = connect_with(&mut script, Lane::Canary, "box", 300).unwrap();
    assert_eq!(answer["state"], "connected");
    assert_eq!(answer["connected"], true);
    assert_eq!(answer["uid"], "uid-1");
    assert_eq!(answer["credential_provider"], "ds_device");
    assert_eq!(answer["waited_seconds"], 10);
    assert_eq!(script.completed, 1);
    assert_eq!(script.begun, 1);
}

#[test]
fn a_pending_link_is_resumed_not_restarted() {
    let mut script = Script {
        pending: Some({
            let mut pending = public("request-earlier");
            pending["expired"] = json!(false);
            pending
        }),
        ..Default::default()
    };
    let answer = connect_with(&mut script, Lane::Stable, "box", 0).unwrap();
    assert_eq!(answer["request_id"], "request-earlier");
    assert_eq!(script.begun, 0);
    assert!(script.told.iter().any(|line| line.contains("Resuming")));
}

/// 38f435d1: after approving in the Desktop, running the command again from a
/// machine host (--timeout 0) completes the link instead of answering the
/// same pending request, unobserved, forever.
#[test]
fn a_machine_host_resuming_an_approved_link_completes_it_without_waiting() {
    let mut script = Script {
        connected: VecDeque::from([None, Some(status_shape())]),
        pending: Some({
            let mut pending = public("request-earlier");
            pending["expired"] = json!(false);
            pending
        }),
        statuses: VecDeque::from(["approved"]),
        ..Default::default()
    };
    let answer = connect_with(&mut script, Lane::Canary, "box", 0).unwrap();
    assert_eq!(answer["state"], "connected");
    assert_eq!(answer["connected"], true);
    assert_eq!(answer["waited_seconds"], 0);
    assert_eq!(script.completed, 1);
    assert_eq!(script.begun, 0);
    assert_eq!(script.waited, Duration::ZERO);
}

#[test]
fn a_machine_host_resuming_a_pending_link_observes_it_once_and_returns() {
    let mut script = Script {
        pending: Some({
            let mut pending = public("request-earlier");
            pending["expired"] = json!(false);
            pending
        }),
        statuses: VecDeque::from(["pending", "approved"]),
        ..Default::default()
    };
    let answer = connect_with(&mut script, Lane::Stable, "box", 0).unwrap();
    assert_eq!(answer["state"], "pending_approval");
    assert_eq!(answer["status"], "pending");
    assert_eq!(answer["request_id"], "request-earlier");
    // Exactly one observation, no wait, no completion.
    assert_eq!(script.statuses, VecDeque::from(["approved"]));
    assert_eq!(script.waited, Duration::ZERO);
    assert_eq!(script.completed, 0);
}

#[test]
fn a_resumed_denial_with_no_wait_is_still_reported() {
    let mut script = Script {
        pending: Some({
            let mut pending = public("request-earlier");
            pending["expired"] = json!(false);
            pending
        }),
        statuses: VecDeque::from(["denied"]),
        ..Default::default()
    };
    let failure = connect_with(&mut script, Lane::Stable, "box", 0).unwrap_err();
    assert_eq!(failure.code(), "device_link_denied");
    assert_eq!(script.discarded, 1);
}

#[test]
fn an_expired_pending_link_is_replaced_by_a_new_request() {
    let mut script = Script {
        pending: Some({
            let mut pending = public("request-old");
            pending["expired"] = json!(true);
            pending
        }),
        ..Default::default()
    };
    let answer = connect_with(&mut script, Lane::Stable, "box", 0).unwrap();
    assert_eq!(answer["request_id"], "request-1-box");
    assert_eq!(script.begun, 1);
}

#[test]
fn waiting_runs_out_with_the_link_still_pending_and_the_next_step_named() {
    let mut script = Script {
        statuses: VecDeque::from(["pending", "pending", "pending"]),
        ..Default::default()
    };
    let answer = connect_with(&mut script, Lane::Canary, "box", 12).unwrap();
    assert_eq!(answer["state"], "pending_approval");
    assert_eq!(answer["status"], "pending");
    assert_eq!(answer["waited_seconds"], 12);
    assert_eq!(answer["next"], "ds account connect --lane canary");
    // Three polls: 5 + 5 + the 2 left in the budget — never past it.
    assert_eq!(script.waited, Duration::from_secs(12));
    assert_eq!(script.completed, 0);
}

#[test]
fn a_denial_discards_the_pending_link_and_refuses() {
    let mut script = Script {
        statuses: VecDeque::from(["denied"]),
        ..Default::default()
    };
    let failure = connect_with(&mut script, Lane::Stable, "box", 60).unwrap_err();
    assert_eq!(failure.code(), "device_link_denied");
    assert_eq!(
        failure.next_commands(),
        ["ds account connect --lane stable"]
    );
    assert_eq!(script.discarded, 1);
    assert_eq!(script.completed, 0);
}

#[test]
fn an_expiry_while_waiting_starts_one_new_request_then_gives_up() {
    let mut script = Script {
        statuses: VecDeque::from(["expired", "expired"]),
        ..Default::default()
    };
    let failure = connect_with(&mut script, Lane::Stable, "box", 60).unwrap_err();
    assert_eq!(failure.code(), "device_link_stale");
    assert_eq!(script.begun, 2);
    assert_eq!(script.discarded, 1);
}

#[test]
fn the_refusals_and_answers_name_no_terminal_sign_in() {
    // The whole point of this command is that nothing it says sends a
    // person to a terminal sign-in. Every sentence it can emit is checked
    // for the words such advice would use.
    let mut prose = vec![
        SIGNED_OUT_REMEDY.to_owned(),
        APPROVAL_INSTRUCTIONS.to_owned(),
        CONNECT_COMMAND.purpose.to_owned(),
        CONNECT_COMMAND.output.to_owned(),
        DOMAIN.summary.to_owned(),
    ];
    prose.extend(
        REFUSALS
            .iter()
            .map(|refusal| format!("{} {}", refusal.when, refusal.remedy)),
    );
    prose.extend(
        CONNECT_COMMAND
            .args
            .iter()
            .map(|arg| arg.summary.to_owned()),
    );
    prose.extend(
        CONNECT_COMMAND
            .examples
            .iter()
            .map(|example| example.note.to_owned()),
    );
    let mut script = Script::default();
    prose.push(
        connect_with(&mut script, Lane::Stable, "box", 0)
            .unwrap()
            .to_string(),
    );
    prose.push(render_connect(&pending_json(
        Lane::Stable,
        &public("r"),
        Duration::ZERO,
        false,
    )));
    for text in prose {
        let lower = text.to_lowercase();
        for banned in ["auth login", "--email", "password"] {
            assert!(!lower.contains(banned), "`{banned}` in: {text}");
        }
    }
}

#[test]
fn the_lane_aware_remedy_is_the_one_sentence_with_the_lane_in_it() {
    assert_eq!(
        SIGNED_OUT_REMEDY,
        format!("{SIGNED_OUT_REMEDY_HEAD}{SIGNED_OUT_NEXT}{SIGNED_OUT_REMEDY_TAIL}")
    );
    let canary = signed_out_remedy("canary");
    assert!(
        canary.contains("`ds account connect --lane canary`"),
        "{canary}"
    );
    assert_eq!(canary.replace(" --lane canary", ""), SIGNED_OUT_REMEDY);
}

#[test]
fn a_device_name_is_bounded_and_the_default_is_never_empty() {
    assert_eq!(bounded_device_name("  ds-server ").unwrap(), "ds-server");
    assert_eq!(
        bounded_device_name("").unwrap_err().code(),
        "auth_input_invalid"
    );
    assert_eq!(
        bounded_device_name(&"x".repeat(101)).unwrap_err().code(),
        "auth_input_invalid"
    );
    assert!(!default_device_name().is_empty());
}

#[test]
fn the_command_is_found_by_a_strangers_words_for_signing_in() {
    // The declared terms are the words a person who has never read this
    // code would type. None repeats the id or summary — `meta.rs` holds
    // that rule — so each buys a way in the command did not already have.
    for term in ["sign in", "login", "link device", "signed out"] {
        assert!(CONNECT_COMMAND.search.contains(&term), "{term}");
    }
}
