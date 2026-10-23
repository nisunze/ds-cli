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

const LANE: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.");
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
    summary: "Sign in: connect this machine to your DS account through the Desktop.",
    purpose: "The one sign-in. Begins a protected device link for the lane, shows the request id and device fingerprint a person approves in the signed-in DS GridDesign Desktop (Account > Link a trusted device), waits for that approval, then completes the link. Nothing is typed but this command. Run it again to resume: a connected lane answers already_connected, a pending request keeps waiting, an approved one completes. With --timeout 0 (the machine-host default) it never waits: a new request returns at once, and a resumed one is checked once, so running it again after approving completes the link.",
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

    let (mut public, resumed) = match driver.pending(lane)? {
        Some(pending) if pending["expired"] != json!(true) => {
            driver.tell(&format!(
                "Resuming the pending link for lane {}.",
                lane.token()
            ));
            (pending, true)
        }
        Some(_) => {
            // An expired pending link is replaced by `begin` itself; there is
            // nothing to discard first.
            (driver.begin(lane, device_name)?, false)
        }
        None => (driver.begin(lane, device_name)?, false),
    };
    tell_approval(driver, lane, &public);

    let budget = Duration::from_secs(timeout_seconds);
    let mut waited = Duration::ZERO;
    let mut restarted = false;
    let mut last: Option<LinkState> = None;
    // A resumed request may already be approved: observe it once at once,
    // even with --timeout 0, so running the command again after approving
    // completes the link as the contract says (feedback 38f435d1). A request
    // just begun cannot be approved yet, so it waits before its first poll.
    let mut observe_now = resumed;
    while observe_now || waited < budget {
        if !std::mem::take(&mut observe_now) {
            let interval = poll_interval(&public).min(budget - waited);
            driver.wait(interval);
            waited += interval;
        }
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
#[path = "account_tests.rs"]
mod tests;
