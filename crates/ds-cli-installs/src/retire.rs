//! `ds install retire` — record that an installation is gone, or that it is back.
//!
//! No uninstall is observable anywhere in this product today: the `.deb` ships
//! no maintainer script and the Windows uninstaller runs no custom step, so
//! nothing on a machine reports its own removal. That is exactly why this verb
//! exists. It is the explicit statement an operator makes about a machine that
//! was wiped, returned or decommissioned — never something inferred from a row
//! that went quiet. A silent installation is reported as silent, with how long,
//! and it is left for a person to decide about.
//!
//! A retirement is a transition, not a deletion. The row is retained, dated and
//! attributed, and excluded from the default view. This family exists to
//! investigate use; dropping a row would destroy the evidence.

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
const RESTORE: Arg = Arg::switch(
    "restore",
    "Reverse a recorded removal: the installation is back in service.",
);

pub static COMMAND: Command = Command {
    id: "install.retire",
    path: &["install", "retire"],
    contract: 1,
    summary: "Record that an install was removed, or restore it.",
    purpose: "\
State that a product is gone from a machine, so the inventory stops carrying \
it. Nothing observes an uninstallation today — neither platform's uninstaller \
reports one — so this is the operator's own statement, and it is never \
inferred from an installation that merely stopped reporting. The row is \
retained, stamped with who recorded the removal, when and why, and dropped \
from the default view; --include-retired on `ds install list` brings it back. \
--restore reverses it. An installation that keeps checking in after being \
retired is shown as exactly that contradiction rather than silently corrected.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::INSTALL_ARG,
        RESTORE,
        REVISION,
        crate::REASON_ARG,
        crate::LANE_ARG,
    ],
    output: "The installation as it now stands, including `state.retired`, who recorded it, when, and its new revision.",
    examples: &[
        Example {
            command: "ds install retire --install <id> --expected-revision 0 --reason \"laptop wiped and returned\" --yes",
            note: "The row is kept and dated; only the default view loses it.",
            runnable: false,
        },
        Example {
            command: "ds install retire --install <id> --restore --expected-revision 1 --reason \"recorded in error\" --yes",
            note: "Reversing a removal is itself an entry in the history.",
            runnable: false,
        },
    ],
    refusals: &crate::native_refusals::<5, { crate::WRITE_REFUSALS_LEN }>([
        crate::NOT_PERMITTED,
        crate::INVALID_SELECTION,
        crate::REVISION_CONFLICT,
        crate::UNREADABLE,
        crate::PROJECTION_UNAVAILABLE,
    ]),
    reference: Some("docs/reference/installations.md"),
    search: &[
        "uninstall",
        "uninstalled",
        "decommission",
        "wiped",
        "inventory",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let revision = crate::integer(
        inputs.require("expected-revision")?,
        "expected-revision",
        0,
        i64::MAX,
    )?;
    let changed = crate::invoke(
        inputs,
        &installs::Command::SetRetirement {
            install_id: inputs.require("install")?.to_owned(),
            expected_revision: revision,
            retired: !inputs.switch("restore"),
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
