//! The `ds` process end of reliability reporting.
//!
//! `ds_client_core::reporter` decides everything: how an ending is classified,
//! what an event holds, what never leaves the machine, the bounded outbox,
//! dedupe, the per-minute cap and the daily rollup (ds-command-kernel
//! `docs/contracts/ds-cli-reliability-and-feedback.md` §3). This file supplies
//! only what a host has: the owner-only state file under the DS state root
//! and the fixed send through `ds-cli-auth`. It runs after the answer has been
//! written and never writes to stdout or stderr, so a command's output and
//! exit code are the same with reporting on or off, online or offline.

use std::cell::RefCell;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ds_cli_auth::reliability::reporter::{
    self, EventSink, HostKind, Invocation, Platform, SendFailure, Termination,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Command, Effect};
use serde_json::{Value, json};

use crate::build;

/// Set by a protocol adapter on the `ds` process it runs for one tool call:
/// the surface that invoked it. Such a process records its event and leaves
/// the send to the adapter's own process, off the request path.
pub const SURFACE_ENV: &str = "DS_CLI_SURFACE";
/// The calling agent's name, when the invoking surface knows it.
pub const AGENT_ENV: &str = "DS_CLI_AGENT";
/// `desktop`, `server` or `headless`, when the host that runs `ds` knows it.
pub const HOST_KIND_ENV: &str = "DS_CLI_HOST_KIND";
/// Development builds only: the command id that panics on purpose.
#[cfg(debug_assertions)]
pub const TEST_PANIC_ENV: &str = "DS_CLI_TEST_PANIC";

/// Commands that serve a protocol for a long time and run each tool call as
/// its own `ds` process: they send what those processes record.
pub const ADAPTER_COMMANDS: &[&str] = &["mcp.serve"];
/// How often a long-lived adapter process sends what its tool calls recorded.
pub const ADAPTER_FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const GLOBAL_FLAGS: &[&str] = &[
    "--output",
    "--pretty",
    "--no-color",
    "--yes",
    "--help",
    "--version",
];

#[derive(Default)]
struct Seen {
    command: Option<&'static Command>,
    ending: Option<(String, u32, Termination)>,
}

thread_local! {
    static SEEN: RefCell<Seen> = RefCell::new(Seen::default());
}

/// Whether this process reports at all (`DS_SRE_REPORTING=off` turns it off).
pub fn enabled() -> bool {
    reporter::reporting_enabled(std::env::var(reporter::REPORTING_SWITCH).ok().as_deref())
}

/// The command routing resolved, so flag names are checked against its
/// declaration and its path words are not mistaken for values.
pub fn resolved(command: &'static Command) {
    SEEN.with(|seen| seen.borrow_mut().command = Some(command));
    // A development build can be made to crash on one command, so the crash
    // path (one event, one automatic report) is provable end to end
    // (contract §7.3). A release build has no such hook.
    #[cfg(debug_assertions)]
    if std::env::var(TEST_PANIC_ENV).is_ok_and(|id| id == command.id) {
        panic!("{TEST_PANIC_ENV} asked {} to crash", command.id);
    }
}

/// The failure with `ds feedback draft --last` as its last `next` command when
/// its ending is `failed` (contract §5.4); a refusal is returned unchanged.
/// The SRE event id stays in the journal the draft reads, not in the answer:
/// an id in the envelope would make every failed answer differ between runs,
/// between reporting on and off, and between an MCP tool call and the same
/// command run directly.
pub fn with_feedback(failure: &Failure) -> Option<Failure> {
    let class = reporter::classify(&termination_of(failure), None);
    let draft = reporter::next_feedback(class)?;
    (!failure.next_commands().iter().any(|next| next == draft)).then(|| failure.clone().next(draft))
}

fn termination_of(failure: &Failure) -> Termination {
    Termination::Error {
        class: failure.class().token().to_string(),
        code: failure.code().to_string(),
        message: failure.message().to_string(),
        retryable: failure.class().retryable(),
    }
}

/// How the invocation ended; `failure` is `None` for an answer.
pub fn ended(command: &str, contract: u32, failure: Option<&Failure>) {
    let termination = failure.map_or(Termination::Ok, termination_of);
    SEEN.with(|seen| {
        seen.borrow_mut().ending = Some((command.to_string(), contract, termination));
    });
}

/// Record this process's ending and, unless an adapter ran it for one tool
/// call, send whatever is due within the send budget. `crash` carries the
/// panic message when the command panicked. Never prints, never fails.
pub fn report(argv: &[String], started: Instant, crash: Option<String>) {
    if !enabled() {
        return;
    }
    let Some(mut store) = ds_cli_auth::reliability::FileStore::open() else {
        return;
    };
    let (command, ending) = SEEN.with(|seen| {
        let seen = seen.borrow();
        (seen.command, seen.ending.clone())
    });
    let (action, contract, termination) = match (crash, ending) {
        (Some(message), ending) => {
            let (action, contract) = ending
                .map(|(action, contract, _)| (action, contract))
                .or_else(|| command.map(|command| (command.id.to_string(), command.contract)))
                .unwrap_or_else(|| ("ds".to_string(), 1));
            (action, contract, Termination::Crashed { message })
        }
        (None, Some(ending)) => ending,
        // Help, a domain listing or a bare `ds`: an answer, counted as `ds`.
        (None, None) => ("ds".to_string(), 1, Termination::Ok),
    };
    let (flags, argument_values) = split_argv(argv, command);
    let adapter = std::env::var(SURFACE_ENV).is_ok_and(|surface| surface == "mcp");
    let invocation = Invocation {
        platform: if adapter {
            Platform::Mcp
        } else {
            Platform::Cli
        },
        host_kind: host_kind(),
        release: format!(
            "{} {} (kernel {})",
            build::PRODUCT,
            build::VERSION,
            short(build::COMMAND_KERNEL_SHA)
        ),
        lane: ds_cli_auth::reliability::release_lane().to_string(),
        action,
        action_class: None,
        contract: Some(contract),
        latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        status: None,
        request_id: ds_cli_auth::reliability::last_request_id(),
        invocation_id: reporter::new_invocation_id(),
        project: project_flag(argv),
        flags,
        agent: std::env::var(AGENT_ENV)
            .ok()
            .filter(|agent| !agent.is_empty()),
        termination,
        argument_values,
    };
    let now = now_ms();
    // A discovery command promises to probe no network, and a tool call's
    // process leaves the send to its adapter: both only record.
    let discovery = command.is_some_and(|command| matches!(command.effect, Effect::Discovery));
    if adapter || discovery {
        let _ = reporter::record(&mut store, &invocation, now);
        return;
    }
    let _ = reporter::finish_invocation(true, &mut store, &mut Sink, &invocation, now);
    file_automatic(&mut store);
}

/// Send what an adapter's tool calls recorded, every
/// [`ADAPTER_FLUSH_INTERVAL`], for as long as this process lives.
pub fn spawn_adapter_flusher() {
    if !enabled() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("ds-reliability-flush".into())
        .spawn(|| {
            while enabled() {
                std::thread::sleep(ADAPTER_FLUSH_INTERVAL);
                let Some(mut store) = ds_cli_auth::reliability::FileStore::open() else {
                    continue;
                };
                let now = now_ms();
                if reporter::has_due(&mut store, now).unwrap_or(false) {
                    let _ = reporter::flush(&mut store, &mut Sink, now, reporter::SEND_BUDGET);
                }
                file_automatic(&mut store);
            }
        });
}

/// The reporting lines `ds doctor` shows. Off, nothing is read or created.
pub fn doctor_report() -> Value {
    if !enabled() {
        return json!({ "enabled": false, "switch": reporter::REPORTING_SWITCH });
    }
    let lane = ds_cli_auth::reliability::release_lane();
    let Some(mut store) = ds_cli_auth::reliability::FileStore::open() else {
        return json!({ "enabled": true, "lane": lane, "state": "unavailable" });
    };
    match reporter::status(&mut store, true) {
        Ok(status) => {
            let mut report = status.to_json();
            report["lane"] = json!(lane);
            report["state"] = json!("ready");
            report
        }
        Err(_) => json!({ "enabled": true, "lane": lane, "state": "busy" }),
    }
}

fn host_kind() -> HostKind {
    match std::env::var(HOST_KIND_ENV).as_deref() {
        Ok("desktop") => HostKind::Desktop,
        Ok("server") => HostKind::Server,
        _ => HostKind::Headless,
    }
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// The project, only when `--project` was passed.
fn project_flag(argv: &[String]) -> Option<String> {
    let mut tokens = argv.iter().take_while(|token| *token != "--");
    while let Some(token) = tokens.next() {
        if token == "--project" {
            return tokens.next().cloned();
        }
        if let Some(value) = token.strip_prefix("--project=") {
            return Some(value.to_string());
        }
    }
    None
}

/// Flag names the command or the CLI declares, and every other token as a
/// value to scrub. The command's own path words and the closed choices it
/// declares are vocabulary, not caller data, so they are not scrubbed.
fn split_argv(argv: &[String], command: Option<&'static Command>) -> (Vec<String>, Vec<String>) {
    let declared = |name: &str| {
        GLOBAL_FLAGS.contains(&name)
            || command.is_some_and(|command| {
                command
                    .args
                    .iter()
                    .any(|arg| name.strip_prefix("--") == Some(arg.name))
            })
    };
    let vocabulary = |token: &str| {
        command.is_some_and(|command| {
            command.path.contains(&token)
                || command.args.iter().any(|arg| arg.choices.contains(&token))
        })
    };
    let mut flags = Vec::new();
    let mut values = Vec::new();
    let mut operands = false;
    for token in argv {
        if operands {
            values.push(token.clone());
            continue;
        }
        if token == "--" {
            operands = true;
            continue;
        }
        if token.starts_with("--") {
            let (name, value) = match token.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (token.as_str(), None),
            };
            if declared(name) {
                flags.push(name.to_string());
            } else {
                values.push(token.clone());
            }
            if let Some(value) = value.filter(|value| !vocabulary(value)) {
                values.push(value.to_string());
            }
        } else if !vocabulary(token) {
            values.push(token.clone());
        }
    }
    (flags, values)
}

/// Submit the crash reports this machine holds, as the signed-in user. Only
/// when one waits: an ordinary invocation reads the state and goes. Signed out
/// or offline, they wait for a later invocation (contract §5.3).
fn file_automatic(store: &mut ds_cli_auth::reliability::FileStore) {
    if reporter::has_automatic(store).unwrap_or(false) {
        let _ = reporter::file_automatic(store, &mut Backlog);
    }
}

struct Backlog;

impl reporter::ReportSink for Backlog {
    fn submit(&mut self, payload: &Value) -> Result<(), SendFailure> {
        let mut fields = payload.clone();
        fields["operation"] = json!("submit");
        // The kernel built this payload; a field the submit call would not
        // accept is a body the backlog will never take.
        let command: ds_cli_auth::FeedbackCommand =
            serde_json::from_value(fields).map_err(|_| SendFailure::Rejected)?;
        let lane = if ds_cli_auth::reliability::release_lane() == "canary" {
            "canary"
        } else {
            "stable"
        };
        match ds_cli_auth::feedback(lane, &command) {
            Ok(_) => Ok(()),
            Err(failure)
                if failure.class() == ds_cli_contract::outcome::ExitClass::InvalidInput =>
            {
                Err(SendFailure::Rejected)
            }
            Err(_) => Err(SendFailure::Unreachable),
        }
    }
}

struct Sink;

impl EventSink for Sink {
    fn send(&mut self, event: &Value) -> Result<(), SendFailure> {
        ds_cli_auth::reliability::send_event(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn only_declared_flag_names_are_flags_and_everything_else_is_scrubbed() {
        let command = crate::registry::all_commands()
            .into_iter()
            .find(|command| command.id == "feedback.submit")
            .expect("feedback.submit is registered");
        let argv = tokens(&[
            "feedback",
            "submit",
            "--title=SECRET-TITLE",
            "--detail",
            "SECRET DETAIL",
            "--kind",
            "bug",
            "--not-declared",
            "--output",
            "json",
            "--",
            "--yes",
        ]);
        let (flags, values) = split_argv(&argv, Some(command));
        assert_eq!(
            flags,
            tokens(&["--title", "--detail", "--kind", "--output"])
        );
        assert!(values.contains(&"SECRET-TITLE".to_string()));
        assert!(values.contains(&"SECRET DETAIL".to_string()));
        assert!(values.contains(&"--not-declared".to_string()));
        assert!(
            values.contains(&"--yes".to_string()),
            "after `--` it is an operand"
        );
        assert!(
            !values.contains(&"bug".to_string()),
            "a declared choice is vocabulary"
        );
        assert!(
            !values.contains(&"submit".to_string()),
            "a path word is vocabulary"
        );
        assert_eq!(
            project_flag(&tokens(&["x", "--project", "kigali"])).as_deref(),
            Some("kigali")
        );
        assert_eq!(
            project_flag(&tokens(&["x", "--project=kigali"])).as_deref(),
            Some("kigali")
        );
        assert_eq!(project_flag(&tokens(&["x", "--", "--project", "p"])), None);
    }
}
