//! `ds install list` — the whole inventory, grouped by host and freshest first.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::installs;
use serde_json::Value;

const SEARCH: Arg = Arg::value(
    "search",
    "<text>",
    "Match installation id, platform, version, lane, host kind, account email or uid.",
);
const LIMIT: Arg =
    Arg::value("limit", "<n>", "Installations to read per page; 1..100.").default("50");
const CURSOR: Arg = Arg::value(
    "cursor",
    "<cursor>",
    "Continue from a previous page's next_cursor.",
);
const INCLUDE_RETIRED: Arg = Arg::switch(
    "include-retired",
    "Also show installations recorded as removed. They are retained, never deleted.",
);

pub static COMMAND: Command = Command {
    id: "install.list",
    path: &["install", "list"],
    contract: 1,
    summary: "List every registered install, desktop and server.",
    purpose: "\
Read the product's installation inventory: which copies of DS GridDesign are \
registered, on what platform, version and lane, whether each is a desktop or a \
windowless server host, who last signed in on it, how long since it last \
proved it was alive, and whether an administrator has blocked its device or \
its licence. Rows are grouped by host kind and ordered freshest first. \
`last seen` is stamped by the installation's own licence refresh and by \
nothing else, so it means the installation contacted the server — not that \
somebody opened a window. Installations recorded as removed are excluded \
unless --include-retired is passed; they are retained, never deleted. Silence \
is reported as silence and is never read as an uninstallation.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[SEARCH, INCLUDE_RETIRED, LIMIT, CURSOR, crate::LANE_ARG],
    output: "\
`groups` (desktop, server), each row carrying `identity`, `principal`, \
`last_seen` (age, bucket, staleness, source), `state` (governed, device, \
licence, lease, retired) and one `remedy` code; `totals` reports shown, \
matched, received, retired_hidden, blocked, silent and whether the page was \
truncated; `next_cursor` continues the read.",
    examples: &[
        Example {
            command: "ds install list --output json",
            note: "The default view: everything still in service, freshest first.",
            runnable: false,
        },
        Example {
            command: "ds install list --include-retired --search linux",
            note: "Include removals, and narrow to one platform.",
            runnable: false,
        },
    ],
    refusals: &crate::native_refusals::<4, { crate::READ_REFUSALS_LEN }>([
        crate::NOT_PERMITTED,
        crate::INVALID_SELECTION,
        crate::UNREADABLE,
        crate::PROJECTION_UNAVAILABLE,
    ]),
    reference: Some("docs/reference/installations.md"),
    search: &["license", "fleet", "inventory", "uninstalled", "seat"],
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
    let cursor = inputs.value("cursor").map(str::to_owned);
    let page = crate::invoke(
        inputs,
        &installs::Command::List {
            cursor,
            limit: limit as u32,
        },
    )?;
    crate::project_page(
        page,
        inputs.value("search"),
        inputs.switch("include-retired"),
        limit,
    )
}

pub fn render(data: &Value) -> String {
    let totals = &data["totals"];
    let mut out = format!(
        "{} installation(s) shown of {} read · {} blocked · {} silent · {} retired hidden\n",
        totals["shown"].as_u64().unwrap_or(0),
        totals["received"].as_u64().unwrap_or(0),
        totals["blocked"].as_u64().unwrap_or(0),
        totals["silent"].as_u64().unwrap_or(0),
        totals["retired_hidden"].as_u64().unwrap_or(0),
    );
    if totals["truncated"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  bounded at {} of {} matching rows; pass --limit or --cursor for the rest\n",
            totals["limit"].as_u64().unwrap_or(0),
            totals["matched"].as_u64().unwrap_or(0),
        ));
    }
    for group in data["groups"].as_array().into_iter().flatten() {
        let rows = group["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "\n{} ({})\n",
            group["key"].as_str().unwrap_or("?"),
            rows.len()
        ));
        for row in rows {
            out.push_str(&crate::render_row(row));
        }
    }
    if let Some(cursor) = data["next_cursor"].as_str() {
        out.push_str(&format!("\nmore: --cursor {cursor}\n"));
    }
    out
}
