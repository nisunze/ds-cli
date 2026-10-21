//! `ds install list` — the whole inventory, grouped by host and freshest first.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::installs;
use serde_json::{Value, json};

const SEARCH: Arg = Arg::value(
    "search",
    "<text>",
    "Match installation id, platform, version, lane, host kind, account email or uid, on every page.",
);
const LIMIT: Arg = Arg::value(
    "limit",
    "<n>",
    "Installations per page, or shown when --search reads every page; 1..100.",
)
.default("50");
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
    refusals: &crate::native_refusals::<4, { crate::LIST_REFUSALS_LEN }>([
        crate::NOT_PERMITTED,
        crate::INVALID_SELECTION,
        crate::PROJECTION_UNAVAILABLE,
        ds_cli_contract::args::INVALID_NUMBER,
    ]),
    reference: Some("docs/reference/installations.md"),
    search: &[
        "license",
        "licence",
        "fleet",
        "inventory",
        "uninstalled",
        "seat",
    ],
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
    let search = inputs
        .value("search")
        .map(str::trim)
        .filter(|needle| !needle.is_empty());
    let include_retired = inputs.switch("include-retired");
    let Some(needle) = search else {
        let page = crate::invoke(
            inputs,
            &installs::Command::List {
                cursor,
                limit: limit as u32,
            },
        )?;
        return crate::project_page(page, None, include_retired, limit);
    };
    // A search is a question about the whole inventory, and ds-brain's list
    // route takes no filter: it pages, and nothing more. So the filter runs
    // here over every page, not over whichever page happened to be first.
    let page = whole_inventory(cursor, |cursor| {
        crate::invoke(
            inputs,
            &installs::Command::List {
                cursor,
                limit: crate::MAX_PAGE,
            },
        )
    })?;
    searched(page, needle, include_retired, limit)
}

/// Every page of the inventory from `start`, accumulated into the one page
/// the kernel projects — up to the kernel's own bounds on that page.
///
/// The kernel merges by identity and orders by freshness, so pages are
/// simply appended. Two bounds end a walk early: the kernel's row count, and
/// its request size in bytes, which a few thousand real records reach first.
/// A walk that stops at either, or at a cursor that does not move, keeps
/// `next_cursor` on the page it returns; one that reaches the end drops it.
/// That is the one fact [`searched`] needs to say whether a filter saw
/// everything.
pub fn whole_inventory(
    start: Option<String>,
    mut fetch: impl FnMut(Option<String>) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    use ds_command_kernel::installation_inventory::{MAX_REQUEST_BYTES, MAX_ROWS};
    // What the projection request carries besides the rows: its schema,
    // clock, filter and cursor. Generous, so a page the size of the last one
    // still fits under the kernel's bound after it is added.
    const ENVELOPE_BYTES: usize = 4 * 1024;

    let mut installs: Vec<Value> = Vec::new();
    let mut bytes = 0usize;
    let mut cursor = start;
    loop {
        let mut page = fetch(cursor.clone())?;
        let Some(rows) = page["installs"].as_array_mut() else {
            // Not a page: hand it on unchanged, so the kernel refuses it with
            // the same words it always has.
            return Ok(page);
        };
        let page_bytes = serde_json::to_vec(rows).map(|json| json.len()).unwrap_or(0);
        bytes += page_bytes;
        installs.append(rows);
        let next = page["next_cursor"]
            .as_str()
            .map(str::trim)
            .filter(|next| !next.is_empty())
            .map(str::to_owned);
        let Some(next) = next else {
            return Ok(json!({ "installs": installs }));
        };
        let stuck = cursor.as_deref() == Some(next.as_str());
        let rows_bound = installs.len() + crate::MAX_PAGE as usize > MAX_ROWS;
        let bytes_bound = bytes + page_bytes + ENVELOPE_BYTES > MAX_REQUEST_BYTES;
        if stuck || rows_bound || bytes_bound {
            return Ok(json!({ "installs": installs, "next_cursor": next }));
        }
        cursor = Some(next);
    }
}

/// Project a searched inventory, and never let `truncated: false` stand for
/// a walk that stopped short: a page with a cursor still on it is an
/// inventory the filter did not finish reading, whatever it matched.
pub fn searched(
    page: Value,
    needle: &str,
    include_retired: bool,
    limit: i64,
) -> Result<Value, Failure> {
    let unfinished = page["next_cursor"]
        .as_str()
        .is_some_and(|next| !next.trim().is_empty());
    let mut projected = crate::project_page(page, Some(needle), include_retired, limit)?;
    if unfinished {
        projected["totals"]["truncated"] = json!(true);
    }
    Ok(projected)
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
        if totals["matched"].as_u64() > totals["limit"].as_u64() {
            out.push_str(&format!(
                "  bounded at {} of {} matching rows; pass --limit or --cursor for the rest\n",
                totals["limit"].as_u64().unwrap_or(0),
                totals["matched"].as_u64().unwrap_or(0),
            ));
        } else {
            out.push_str(&format!(
                "  not every installation was read: {} so far; pass --cursor to continue\n",
                totals["received"].as_u64().unwrap_or(0),
            ));
        }
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
