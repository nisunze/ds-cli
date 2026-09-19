//! `ds install policy` — block or allow one installation's device and licence.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::installs;
use serde_json::Value;

const REVISION: Arg = Arg::value(
    "expected-revision",
    "<n>",
    "The revision `ds install show` printed; the change is refused if it moved.",
)
.required();
const DEVICE: Arg = Arg::value(
    "device",
    "<allowed|blocked>",
    "Device admission for this installation.",
)
.choices(&["allowed", "blocked"])
.required();
const LICENCE: Arg = Arg::value(
    "licence",
    "<allowed|blocked>",
    "Licence state for this installation.",
)
.choices(&["allowed", "blocked"])
.required();

pub static COMMAND: Command = Command {
    id: "install.policy",
    path: &["install", "policy"],
    contract: 1,
    summary: "Block or allow one install's device and its licence.",
    purpose: "\
Set the two governed tokens on one installation. At work admission they are \
equivalent: either one blocked refuses the installation. They differ in reach \
— a blocked LICENCE is carried into the signed offline lease, so the \
installation refuses itself with no network, while a blocked DEVICE takes \
effect on the next handshake. Neither expires and neither is ever set \
automatically; this command is the only thing that changes them. The change is \
fenced on the revision you read, requires a reason, and is appended to the \
installation's immutable history.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::INSTALL_ARG,
        DEVICE,
        LICENCE,
        REVISION,
        crate::REASON_ARG,
        crate::LANE_ARG,
    ],
    output: "The installation as it now stands, in the shape `ds install show` prints, including its new revision.",
    examples: &[Example {
        command: "ds install policy --install <id> --device allowed --licence blocked --expected-revision 0 --reason \"licence not paid\" --yes",
        note: "Blocking the licence also blocks the retained offline lease.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<5, { crate::DETAIL_REFUSALS_LEN }>([
        crate::NOT_PERMITTED,
        crate::WRITE_REFUSED,
        crate::NOT_FOUND,
        crate::PROJECTION_UNAVAILABLE,
        ds_cli_contract::args::INVALID_NUMBER,
    ]),
    reference: Some("docs/reference/installations.md"),
    // The words an operator types at this command are not the words its
    // summary uses: "ban this machine", "lock them out", "disable it". The
    // summary's own words (block, allow, device, licence) already match and
    // may not be repeated here.
    search: &[
        "license",
        "unblock",
        "revoke",
        "suspend",
        "entitlement",
        "ban",
        "lock out",
        "disable",
        "deactivate",
        "blacklist",
        "machine",
        "enforcement",
        "cut off",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The operator's vocabulary is allowed/blocked; the owner's stored token is
/// active/blocked. One translation, in one place, so neither side has to learn
/// the other's word.
fn token(value: &str) -> &'static str {
    if value == "blocked" {
        "blocked"
    } else {
        "active"
    }
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let revision = crate::integer(
        inputs.require("expected-revision")?,
        "expected-revision",
        0,
        i64::MAX,
    )?;
    let changed = crate::invoke(
        inputs,
        &installs::Command::SetPolicy {
            install_id: inputs.require("install")?.to_owned(),
            expected_revision: revision,
            device_status: token(inputs.require("device")?).to_owned(),
            license_status: token(inputs.require("licence")?).to_owned(),
            reason: inputs.require("reason")?.to_owned(),
        },
    )?;
    crate::project_detail(serde_json::json!({ "install": changed, "users": [], "events": [] }))
}

pub fn render(data: &Value) -> String {
    // The write returns the row, not its history. Printing an empty users and
    // history section here would read as "this installation has none", which is
    // a different claim entirely.
    let install = &data["install"];
    format!(
        "{}  revision {} · run `ds install show --install {}` for its history
",
        crate::render_row(install),
        install["policy"]["revision"].as_u64().unwrap_or(0),
        install["install_id"].as_str().unwrap_or("?"),
    )
}
