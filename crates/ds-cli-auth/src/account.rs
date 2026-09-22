//! `ds account connect` — the one sign-in a person is ever asked to do.
//!
//! A non-technical engineer met the old advice on 2026-09-22: an MCP refusal
//! that steered them to a terminal sign-in with an address and a hidden
//! prompt. That is not a step a person without a terminal habit can take. The
//! step they CAN take is approving a request in the DS GridDesign Desktop
//! they already have open and signed in. This command is that step, and only
//! that step: it begins the protected device link, shows what to approve,
//! waits for the approval, and completes the link. The three commands it
//! composes (`auth link begin`, `auth link status`, `auth link complete`)
//! stay as CLI contracts; this is the path every surface advertises.
//!
//! It is idempotent and resumable. A lane that already holds a device
//! credential answers `already_connected`. A lane with a pending link resumes
//! it rather than starting another. A run that stops waiting leaves the
//! pending link in place and names itself as the next step, so a machine host
//! that cannot block for minutes calls it once to start and once more to
//! finish — the `next` hint says exactly that.

use std::time::Duration;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Domain, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs, args};
use serde_json::{Value, json};

use crate::device;
use crate::profile::Lane;

/// The one sentence every signed-out refusal gives, on every surface. There
/// is no second sentence: a terminal, an MCP host and a skill document all
/// send a signed-out person to the same step. A refusal that knows its lane
/// says the same sentence with the lane in it ([`signed_out_remedy`]), so
/// following it exactly connects the lane that refused — on 2026-09-19 an
/// agent followed a lane-less remedy onto the default lane and reproduced
/// the refusal.
///
/// Terse on purpose: every command that can be signed out prints it in its
/// own help, twice where `auth_revoked` sits beside it, and the help budget
/// is priced per refusal — a dozen commands sit within twenty bytes of
/// theirs. The command names the step; what to approve, and where in the
/// Desktop, is what the command itself says ([`APPROVAL_INSTRUCTIONS`]).
pub const SIGNED_OUT_REMEDY: &str = "run `ds account connect`";
const SIGNED_OUT_REMEDY_HEAD: &str = "run `";
const SIGNED_OUT_REMEDY_TAIL: &str = "`";
/// The command a signed-out refusal names as its next step. Callers that know
/// the lane append `--lane <lane>`; see [`signed_out_next`].
pub const SIGNED_OUT_NEXT: &str = "ds account connect";
/// What a person does once the request exists. Carried in every pending
/// answer and printed for a human, word for word.
pub const APPROVAL_INSTRUCTIONS: &str = "In your signed-in DS GridDesign Desktop open Account > Link a trusted device, paste the request id and device fingerprint shown here, choose Approve, then Confirm.";

/// The shared signed-out refusal. Every domain that needs a signed-in lane
/// declares this one rather than its own copy of the sentence.
pub const SIGNED_OUT_REFUSAL: Refusal = Refusal {
    code: "headless_signed_out",
    when: "no credential is connected for the selected lane",
    remedy: SIGNED_OUT_REMEDY,
};

/// The next step for a signed-out lane, with the lane it was asked about.
pub fn signed_out_next(lane: &str) -> String {
    format!("{SIGNED_OUT_NEXT} --lane {lane}")
}

/// [`SIGNED_OUT_REMEDY`] with the lane it was asked about in the command.
pub fn signed_out_remedy(lane: &str) -> String {
    format!(
        "{SIGNED_OUT_REMEDY_HEAD}{}{SIGNED_OUT_REMEDY_TAIL}",
        signed_out_next(lane)
    )
}

/// How long a terminal run waits for the approval when nothing is asked.
pub const DEFAULT_WAIT_SECONDS: u64 = 300;
/// The longest any run may wait, so a stuck approval cannot hold a host.
pub const MAX_WAIT_SECONDS: u64 = 900;
/// The slowest the endpoint's own interval is honoured; it is never polled
/// faster than the interval it names.
const MIN_POLL: Duration = Duration::from_secs(2);
const MAX_POLL: Duration = Duration::from_secs(30);

pub static DOMAIN: Domain = Domain {
    id: "account",
    summary: "Your DS account: connect this machine once, approved in the Desktop.",
    commands: &[&CONNECT_COMMAND],
};

const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);
const TIMEOUT: Arg = Arg::value(
    "timeout",
    "<0-900>",
    "Seconds to wait for the approval; 300 at a terminal, 0 from a machine host.",
);
const DEVICE_NAME: Arg = Arg::value(
    "device-name",
    "<name>",
    "How the Desktop names this machine; default is its hostname.",
);

const DENIED: Refusal = Refusal {
    code: "device_link_denied",
    when: "the request was denied in the Desktop; the pending link is discarded",
    remedy: "run ds account connect again and approve the new request",
};
const STALE: Refusal = Refusal {
    code: "device_link_stale",
    when: "the endpoint no longer recognises the pending link; it is discarded",
    remedy: "run ds account connect again to begin a new request",
};

const REFUSALS: &[Refusal] = &[
    crate::PROFILE_REFUSAL,
    crate::STATE_REFUSAL,
    device::STATE_UNAVAILABLE,
    device::STATE_CONFLICT,
    device::AUTH_TRANSIENT,
    device::AUTH_RESPONSE_INVALID,
    device::AUTH_ENDPOINT_UNAVAILABLE,
    device::RNG_UNAVAILABLE,
    device::REQUEST_MISMATCH,
    DENIED,
    STALE,
];

pub static CONNECT_COMMAND: Command = Command {
    id: "account.connect",
    path: &["account", "connect"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Connect this machine to your DS account through the Desktop.",
    purpose: "The one sign-in. Begins a protected device link for the lane, shows the request id and device fingerprint a person approves in the signed-in DS GridDesign Desktop (Account > Link a trusted device), waits for that approval, then completes the link. Nothing is typed but this command. Run it again to resume: a connected lane answers already_connected, a pending request keeps waiting, an approved one completes. With --timeout 0 (the machine-host default) it returns the pending request at once.",
    effect: Effect::LocalAuthState,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[LANE, TIMEOUT, DEVICE_NAME],
    output: "Lane and state (already_connected, connected, pending_approval); the account email and UID once connected; while pending, the request id, device fingerprint, approval instructions and the next command. Never credential material.",
    examples: &[
        Example {
            command: "ds account connect",
            note: "Prints what to approve, waits up to five minutes, then reports the connected account.",
            runnable: false,
        },
        Example {
            command: "ds account connect --lane canary --timeout 0",
            note: "Begin or resume without waiting; run it again after approving.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[
        "sign in",
        "login",
        "link device",
        "authenticate",
        "signed out",
        "credential",
    ],
    requires: Requires::Server,
    availability: crate::native_availability,
};

/// One observed state of the link, as the driver reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkState {
    Pending,
    Approved,
    Denied,
    Expired,
    Stale,
}

impl LinkState {
    fn from_token(token: &str) -> Self {
        match token {
            "pending" => Self::Pending,
            "approved" => Self::Approved,
            "denied" => Self::Denied,
            "expired" => Self::Expired,
            _ => Self::Stale,
        }
    }
}

/// The link operations `connect` composes, so the wait/resume logic can be
/// driven by a script in tests and by the protected native state in
/// production. Each answer is the public JSON the matching `auth link`
/// command emits.
pub(crate) trait LinkDriver {
    /// The `auth status` shape when the lane already holds a device
    /// credential; `None` when it does not.
    fn connected(&mut self, lane: Lane) -> Result<Option<Value>, Failure>;
    /// The pending public link in local state, with `expired`; `None` when
    /// the lane holds no pending link.
    fn pending(&mut self, lane: Lane) -> Result<Option<Value>, Failure>;
    fn begin(&mut self, lane: Lane, device_name: &str) -> Result<Value, Failure>;
    fn status(&mut self, lane: Lane) -> Result<Value, Failure>;
    fn complete(&mut self, lane: Lane) -> Result<Value, Failure>;
    fn discard(&mut self, lane: Lane) -> Result<(), Failure>;
    /// Wait this long before the next poll. Tests advance a clock instead.
    fn wait(&mut self, duration: Duration);
    /// Tell a person what to approve. Stderr in production; recorded in
    /// tests.
    fn tell(&mut self, line: &str);
}

/// The protected native state and the fixed endpoints.
struct NativeDriver;

impl LinkDriver for NativeDriver {
    fn connected(&mut self, lane: Lane) -> Result<Option<Value>, Failure> {
        crate::device_status(lane)
    }
    fn pending(&mut self, lane: Lane) -> Result<Option<Value>, Failure> {
        device::pending_link_local(lane)
    }
    fn begin(&mut self, lane: Lane, device_name: &str) -> Result<Value, Failure> {
        device::begin_link(lane, device_name)
    }
    fn status(&mut self, lane: Lane) -> Result<Value, Failure> {
        device::link_status(lane)
    }
    fn complete(&mut self, lane: Lane) -> Result<Value, Failure> {
        device::complete_link(lane, None)
    }
    fn discard(&mut self, lane: Lane) -> Result<(), Failure> {
        device::discard_pending_link(lane)
    }
    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
    fn tell(&mut self, line: &str) {
        eprintln!("{line}");
    }
}

pub fn run_connect(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let noninteractive =
        std::env::var_os("DS_CLI_NONINTERACTIVE").is_some_and(|value| value == "1");
    let timeout = match inputs.value("timeout") {
        Some(raw) => args::integer(raw, "timeout", 0, MAX_WAIT_SECONDS as i64)? as u64,
        None if noninteractive => 0,
        None => DEFAULT_WAIT_SECONDS,
    };
    let device_name = match inputs.value("device-name") {
        Some(name) => bounded_device_name(name)?,
        None => default_device_name(),
    };
    connect_with(&mut NativeDriver, lane, &device_name, timeout)
}

/// The whole of `connect`, over any driver.
pub(crate) fn connect_with(
    driver: &mut dyn LinkDriver,
    lane: Lane,
    device_name: &str,
    timeout_seconds: u64,
) -> Result<Value, Failure> {
    if let Some(mut status) = driver.connected(lane)? {
        status["state"] = json!("already_connected");
        status["connected"] = json!(true);
        return Ok(status);
    }

    let mut public = match driver.pending(lane)? {
        Some(pending) if pending["expired"] != json!(true) => {
            driver.tell(&format!(
                "Resuming the pending link for lane {}.",
                lane.token()
            ));
            pending
        }
        Some(_) => {
            // An expired pending link is replaced by `begin` itself; there is
            // nothing to discard first.
            driver.begin(lane, device_name)?
        }
        None => driver.begin(lane, device_name)?,
    };
    tell_approval(driver, lane, &public);

    let budget = Duration::from_secs(timeout_seconds);
    let mut waited = Duration::ZERO;
    let mut restarted = false;
    let mut last: Option<LinkState> = None;
    while waited < budget {
        let interval = poll_interval(&public).min(budget - waited);
        driver.wait(interval);
        waited += interval;
        let observed = driver.status(lane)?;
        let state = LinkState::from_token(observed["status"].as_str().unwrap_or(""));
        match state {
            LinkState::Pending => {
                public = observed;
                last = Some(state);
            }
            LinkState::Approved => {
                driver.complete(lane)?;
                let Some(mut status) = driver.connected(lane)? else {
                    return Err(Failure::failed(
                        "device_state_unavailable",
                        "the completed link left no readable device credential",
                    )
                    .remedy("repair the owner-only DS config directory, then run ds account connect again"));
                };
                status["state"] = json!("connected");
                status["connected"] = json!(true);
                status["waited_seconds"] = json!(waited.as_secs());
                driver.tell(&format!(
                    "Connected lane {} as {}.",
                    lane.token(),
                    status["email"].as_str().unwrap_or("")
                ));
                return Ok(status);
            }
            LinkState::Denied => {
                driver.discard(lane)?;
                return Err(Failure::unauthorized(
                    "device_link_denied",
                    format!(
                        "the request was denied in the Desktop; the pending link for lane {} is discarded",
                        lane.token()
                    ),
                )
                .remedy(DENIED.remedy)
                .next(signed_out_next(lane.token())));
            }
            LinkState::Expired if !restarted => {
                restarted = true;
                public = driver.begin(lane, device_name)?;
                driver.tell("The previous request expired; a new one replaces it.");
                tell_approval(driver, lane, &public);
                last = Some(LinkState::Pending);
            }
            LinkState::Expired | LinkState::Stale => {
                driver.discard(lane)?;
                return Err(Failure::conflict(
                    "device_link_stale",
                    format!(
                        "the endpoint no longer recognises the pending link for lane {}; it is discarded",
                        lane.token()
                    ),
                )
                .remedy(STALE.remedy)
                .next(signed_out_next(lane.token())));
            }
        }
    }

    Ok(pending_json(lane, &public, waited, last.is_some()))
}

fn tell_approval(driver: &mut dyn LinkDriver, lane: Lane, public: &Value) {
    driver.tell(&format!(
        "Approve this device for lane {}:\n  request id          {}\n  device fingerprint  {}\n{}",
        lane.token(),
        public["request_id"].as_str().unwrap_or(""),
        public["device_fingerprint"].as_str().unwrap_or(""),
        APPROVAL_INSTRUCTIONS
    ));
}

fn poll_interval(public: &Value) -> Duration {
    Duration::from_secs(public["poll_interval_seconds"].as_u64().unwrap_or(5))
        .clamp(MIN_POLL, MAX_POLL)
}

fn pending_json(lane: Lane, public: &Value, waited: Duration, observed: bool) -> Value {
    json!({
        "lane": lane.token(),
        "state": "pending_approval",
        "connected": false,
        "signed_in": false,
        "request_id": public["request_id"],
        "device_fingerprint": public["device_fingerprint"],
        "user_code": public["user_code"],
        "verification_uri": public["verification_uri"],
        "expires_at": public["expires_at"],
        "poll_interval_seconds": public["poll_interval_seconds"],
        "status": if observed { "pending" } else { "unobserved" },
        "waited_seconds": waited.as_secs(),
        "approve": APPROVAL_INSTRUCTIONS,
        "next": signed_out_next(lane.token()),
    })
}

fn bounded_device_name(name: &str) -> Result<String, Failure> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 100 || trimmed.chars().any(char::is_control)
    {
        return Err(Failure::invalid(
            "auth_input_invalid",
            "`--device-name` must be 1 to 100 printable characters",
        )
        .remedy("pass a short name a person recognises, such as the machine's hostname"));
    }
    Ok(trimmed.to_owned())
}

/// The machine's own name, as the Desktop will show it. Read from the
/// environment first, then the kernel's file on Unix; a machine that names
/// itself nowhere is still connectable.
fn default_device_name() -> String {
    let candidates = ["COMPUTERNAME", "HOSTNAME"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .chain(std::fs::read_to_string("/etc/hostname").ok());
    for candidate in candidates {
        if let Ok(name) = bounded_device_name(&candidate) {
            return name;
        }
    }
    "this machine".to_owned()
}

pub fn render_connect(data: &Value) -> String {
    let lane = data["lane"].as_str().unwrap_or("");
    match data["state"].as_str().unwrap_or("") {
        "already_connected" => format!(
            "already connected ({lane})  {}\n",
            data["email"].as_str().unwrap_or("")
        ),
        "connected" => format!(
            "connected ({lane})  {}\n",
            data["email"].as_str().unwrap_or("")
        ),
        _ => format!(
            "waiting for approval ({lane})\n  request id          {}\n  device fingerprint  {}\n{}\nThen run: {}\n",
            data["request_id"].as_str().unwrap_or(""),
            data["device_fingerprint"].as_str().unwrap_or(""),
            APPROVAL_INSTRUCTIONS,
            data["next"].as_str().unwrap_or(SIGNED_OUT_NEXT)
        ),
    }
}

#[cfg(test)]
mod tests {
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
            let mut observed = public("request-1-box");
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
}
