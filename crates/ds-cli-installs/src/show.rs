//! `ds install show` — one installation, its observed users and its history.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::installs;
use serde_json::Value;

const LIMIT: Arg = Arg::value(
    "limit",
    "<n>",
    "Users and history entries per page; 1..100.",
)
.default("50");
const USER_CURSOR: Arg = Arg::value(
    "user-cursor",
    "<cursor>",
    "Continue the observed-user list.",
);
const EVENT_CURSOR: Arg = Arg::value("event-cursor", "<cursor>", "Continue the history list.");

pub static COMMAND: Command = Command {
    id: "install.show",
    path: &["install", "show"],
    contract: 1,
    summary: "Read one install: state, observed users, governed history.",
    purpose: "\
Read one installation in full. `state` carries the two governed tokens, the \
lease the installation is actually holding, and whether it has been recorded \
as removed. `users` lists the accounts that completed an authenticated \
handshake on this machine — observations, not a count of sign-ins, and earlier \
history is not reconstructed. `events` is the installation's immutable \
history: every block, unblock, retirement and restoration, with its actor, its \
reason and the revision it moved to. That revision is what a governed change \
must be applied against.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::INSTALL_ARG,
        LIMIT,
        USER_CURSOR,
        EVENT_CURSOR,
        crate::LANE_ARG,
    ],
    output: "\
`install` in the same shape `ds install list` prints, plus `users` (uid, \
email, first and last observation, handshake count) and `events` (revision, \
kind, actor, reason, device/licence/retired transitions); \
`next_user_cursor` and `next_event_cursor` continue each list.",
    examples: &[Example {
        command: "ds install show --install <install-id> --output json",
        note: "Read the revision before changing policy or recording a removal.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<4, { crate::READ_REFUSALS_LEN }>([
        crate::NOT_PERMITTED,
        crate::INVALID_SELECTION,
        crate::UNREADABLE,
        crate::PROJECTION_UNAVAILABLE,
    ]),
    reference: Some("docs/reference/installations.md"),
    search: &["license", "inventory", "audit", "sign-in"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(
        inputs.value("limit").unwrap_or("50"),
        "limit",
        1,
        crate::MAX_PAGE as i64,
    )?;
    let detail = crate::invoke(
        inputs,
        &installs::Command::Get {
            install_id: inputs.require("install")?.to_owned(),
            user_cursor: inputs.value("user-cursor").map(str::to_owned),
            event_cursor: inputs.value("event-cursor").map(str::to_owned),
            limit: limit as u32,
        },
    )?;
    crate::project_detail(detail)
}

pub fn render(data: &Value) -> String {
    let mut out = crate::render_row(&data["install"]);
    out.push_str(&format!(
        "  revision {} · apply governed changes against it\n",
        data["install"]["policy"]["revision"].as_u64().unwrap_or(0)
    ));
    let users = data["users"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    out.push_str(&format!("\nobserved users ({})\n", users.len()));
    for user in users {
        out.push_str(&format!(
            "  {} · last seen {} ago · {} handshakes\n",
            user["email"]
                .as_str()
                .unwrap_or_else(|| user["uid"].as_str().unwrap_or("?")),
            user["bucket"].as_str().unwrap_or("never"),
            user["handshake_count"].as_u64().unwrap_or(0),
        ));
    }
    let events = data["events"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    out.push_str(&format!("\nhistory ({})\n", events.len()));
    for event in events {
        out.push_str(&format!(
            "  r{} {} by {} · device {}→{} · licence {}→{} · retired {}→{} · {}\n",
            event["revision"].as_u64().unwrap_or(0),
            event["kind"].as_str().unwrap_or("policy"),
            event["actor"].as_str().unwrap_or("?"),
            event["device"]["from"].as_str().unwrap_or("?"),
            event["device"]["to"].as_str().unwrap_or("?"),
            event["licence"]["from"].as_str().unwrap_or("?"),
            event["licence"]["to"].as_str().unwrap_or("?"),
            event["retired"]["from"].as_bool().unwrap_or(false),
            event["retired"]["to"].as_bool().unwrap_or(false),
            event["reason"].as_str().unwrap_or(""),
        ));
    }
    out
}
