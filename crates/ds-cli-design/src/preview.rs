//! `ds design bulk|download|version|conflict|presence` — the previews the
//! register kept to itself.
//!
//! Every batch verb `ds` offers today takes an explicit list and performs it,
//! so an agent never learns what a run would SKIP, nor which of its rows are
//! already fresh. The browser knew: it drew those denominators beside the
//! buttons. These six reads answer the same questions from the same kernel
//! over the same `list_transformers_status` rows, with no browser.
//!
//! What a headless caller cannot know, it says. A conflict is a fact about a
//! browser's working copy, and a lease is a fact about a browser's room: when
//! the status rows carry no client facts, these commands report
//! `room_state: "unknown"` and an empty answer rather than a confident zero.

use ds_cli_auth::TransformerStatusList;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::transformer::{LANE_ARG, TRANSFORMER_ARG};

const PLAN_INVALID: Refusal = Refusal {
    code: "design_plan_invalid",
    when: "The kernel refuses the assembled request",
    remedy: "Name fewer transformers, or a known --action or --format",
};

const REFUSALS: &[Refusal] = &[
    crate::transformer::NATIVE_PROFILE,
    crate::transformer::NATIVE_PROFILE_DIGEST,
    crate::transformer::NATIVE_PROFILE_UNSAFE,
    crate::transformer::HEADLESS_SIGNED_OUT,
    crate::transformer::HEADLESS_NO_PROJECT,
    crate::transformer::PROJECT_CONTEXT_STALE,
    PLAN_INVALID,
];

/// Browser-only facts, named rather than guessed. Every command here stamps
/// it so a reader never mistakes "this client cannot see it" for "there is
/// none".
const ROOM_STATE_UNKNOWN: &str = "unknown";

pub const ACTION_ARG: Arg = Arg::value("action", "<verb>", "Which bulk verb to preview.")
    .required()
    .choices(&[
        "add_to_combined",
        "combined_and_export",
        "generate_reports",
        "retry_process",
        "save",
        "delete",
        "version",
    ]);

pub const CAPABILITY_ARG: Arg = Arg::repeated(
    "capability",
    "<name>",
    "A capability this operator holds; repeat. Omit to preview with none.",
);

pub const FORMAT_ARG: Arg = Arg::repeated(
    "format",
    "<xlsx|shp|kmz|gpkg>",
    "Keep only artifacts of this format; repeat to combine.",
)
.choices(&["xlsx", "shp", "kmz", "gpkg"]);

pub const MIRROR_ARG: Arg = Arg::switch(
    "combined-mirror",
    "This project uses the combined mirror; without it those two verbs target nothing.",
);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn rows_of(list: &TransformerStatusList) -> Vec<Value> {
    list.rows().iter().map(|row| row.row().clone()).collect()
}

fn plan_invalid(detail: String) -> Failure {
    Failure::invalid(PLAN_INVALID.code, detail).remedy(PLAN_INVALID.remedy)
}

fn kernel<F>(request: Value, call: F) -> Result<Value, Failure>
where
    F: Fn(&[u8]) -> Result<String, String>,
{
    let input = serde_json::to_vec(&request).map_err(|error| plan_invalid(error.to_string()))?;
    let reply = call(&input).map_err(plan_invalid)?;
    serde_json::from_str(&reply).map_err(|error| plan_invalid(error.to_string()))
}

// ── bulk plan ───────────────────────────────────────────────────────────

pub static BULK_PLAN: Command = Command {
    id: "design.bulk.plan",
    path: &["design", "bulk", "plan"],
    contract: 1,
    summary: "Preview which rows a bulk verb targets and which it skips.",
    purpose: "\
Every batch verb takes a list and performs it; this says what it would do \
first. Reads the selected project's status rows headlessly, then asks the \
shared kernel which of the named transformers the verb may target, how many \
of those are already fresh, and why each remaining row is skipped. The same \
answer the Status page draws beside its buttons. Naming no transformer \
previews an empty tick set, which is what the page shows before a tick.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ACTION_ARG,
        TRANSFORMER_ARG,
        CAPABILITY_ARG,
        MIRROR_ARG,
        LANE_ARG,
    ],
    output: "\
Lane and project identity, the verb, the targets with their freshness, the \
skipped rows with a reason each, the fresh/stale counts, and `unavailable` \
when a missing capability or a disabled combined mirror refuses the whole \
verb.",
    examples: &[Example {
        command: "ds design bulk plan --action generate_reports --transformer TX-1 --output json",
        note: "`.data.plan.stale` is how many would actually run.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run_bulk_plan(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = crate::transformer::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let selection: Vec<String> = inputs.repeated("transformer").to_vec();
    let plan = kernel(
        json!({
            "action": inputs.require("action")?,
            "rows": rows_of(headless.result()),
            "selection": selection,
            "capabilities": inputs.repeated("capability"),
            "combined_mirror_ui": inputs.switch("combined-mirror"),
            "now_ms": now_ms(),
        }),
        |input| {
            ds_command_kernel::plan_project_control_bulk_action(input)
                .map_err(|error| error.to_string())
        },
    )?;
    let mut out = crate::transformer::project_receipt(&headless);
    out["plan"] = plan;
    Ok(out)
}

pub fn render_bulk_plan(data: &Value) -> String {
    let plan = &data["plan"];
    let mut out = format!(
        "{} · {} target(s) · {} fresh · {} stale · {} skipped{}\n",
        plan["action"].as_str().unwrap_or("?"),
        plan["targets"].as_array().map(Vec::len).unwrap_or(0),
        plan["fresh"].as_u64().unwrap_or(0),
        plan["stale"].as_u64().unwrap_or(0),
        plan["skipped"].as_array().map(Vec::len).unwrap_or(0),
        plan["unavailable"]
            .as_str()
            .map(|reason| format!(" · unavailable: {reason}"))
            .unwrap_or_default(),
    );
    for target in plan["targets"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<32} {}\n",
            target["name"].as_str().unwrap_or("?"),
            if target["fresh"] == json!(true) {
                "fresh"
            } else {
                "stale"
            },
        ));
    }
    out
}

// ── download plan ───────────────────────────────────────────────────────

pub static DOWNLOAD_PLAN: Command = Command {
    id: "design.download.plan",
    path: &["design", "download", "plan"],
    contract: 1,
    summary: "Preview a download: rows in scope, URLs, and which copy wins.",
    purpose: "\
Which artifacts a download would produce. Reads the selected project's status \
rows headlessly and asks the shared kernel for the scope (naming \
transformers narrows it; naming none takes the whole project), the ordered \
URL list, the fresh/stale/missing summary, and the placement of every \
artifact name a row carries twice — a local copy outranks a cloud pointer \
whatever order they arrived in. The fetch itself stays with the host.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, FORMAT_ARG, LANE_ARG],
    output: "\
Lane and project identity, the rows in scope, every delivered URL (and the \
format-filtered list), the fresh/stale/missing/cached summary, the source-\
upload counts, and one placement row per artifact name with the copy it \
resolved to and why.",
    examples: &[Example {
        command: "ds design download plan --format xlsx --output json",
        note: "`.data.plan.filtered_urls` is what a spreadsheet-only run fetches.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run_download_plan(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = crate::transformer::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let plan = kernel(
        json!({
            "rows": rows_of(headless.result()),
            "selection": inputs.repeated("transformer"),
            "formats": inputs.repeated("format"),
        }),
        |input| {
            ds_command_kernel::plan_project_control_artifact_download(input)
                .map_err(|error| error.to_string())
        },
    )?;
    let mut out = crate::transformer::project_receipt(&headless);
    out["plan"] = plan;
    Ok(out)
}

pub fn render_download_plan(data: &Value) -> String {
    let plan = &data["plan"];
    let summary = &plan["summary"];
    let mut out = format!(
        "{} row(s) in scope ({}) · {} fresh · {} stale · {} missing · {} url(s)\n",
        plan["scope"].as_array().map(Vec::len).unwrap_or(0),
        plan["scope_precedence"].as_str().unwrap_or("?"),
        summary["fresh"].as_u64().unwrap_or(0),
        summary["stale"].as_u64().unwrap_or(0),
        summary["missing"].as_u64().unwrap_or(0),
        plan["filtered_urls"].as_array().map(Vec::len).unwrap_or(0),
    );
    for entry in plan["placement"].as_array().into_iter().flatten() {
        if entry["why"] == json!("only") {
            continue;
        }
        out.push_str(&format!(
            "  {:<32} {:<12} {}\n",
            entry["name"].as_str().unwrap_or("?"),
            entry["why"].as_str().unwrap_or("-"),
            entry["chosen"].as_str().unwrap_or(""),
        ));
    }
    out
}

// ── version status ──────────────────────────────────────────────────────

pub static VERSION_STATUS: Command = Command {
    id: "design.version.status",
    path: &["design", "version", "status"],
    contract: 1,
    summary: "Read version state and whether beginning one is warranted.",
    purpose: "\
`ds map design version begin` could always be called; nothing said whether it \
was warranted. This reads the selected project's status rows headlessly and \
answers, per transformer: the version in force, the highest ever assigned, \
the next ordinal, whether something was restored, whether the SAVED state has \
moved since the lead was cut, and whether that name may carry versions at \
all. The ordinal itself stays ds-brain's to assign.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG],
    output: "\
Lane and project identity and one row per transformer: `version`, `latest`, \
`next`, `count`, `restored`, `unversioned`, `changed_since_version`, \
`versionable` and the lead's reason.",
    examples: &[Example {
        command: "ds design version status --transformer TX-1 --output json",
        note: "`.data.versions[0].changed_since_version` says whether a cut is warranted.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run_version_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = crate::transformer::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let reply = kernel(
        json!({
            "op": "facts",
            "schema": ds_command_kernel::design_version::SCHEMA,
            "rows": rows_of(headless.result()),
        }),
        ds_command_kernel::design_version::evaluate,
    )?;
    let mut out = crate::transformer::project_receipt(&headless);
    out["versions"] = reply["versions"].clone();
    out["count"] = json!(reply["versions"].as_array().map(Vec::len).unwrap_or(0));
    Ok(out)
}

pub fn render_version_status(data: &Value) -> String {
    let mut out = format!("{} transformer(s)\n", data["count"].as_u64().unwrap_or(0));
    for row in data["versions"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<32} {:<5} next {:<5} {:<10} {}\n",
            row["name"].as_str().unwrap_or("?"),
            row["label"].as_str().unwrap_or("-"),
            row["next"].as_u64().unwrap_or(0),
            if row["restored"] == json!(true) {
                "restored"
            } else if row["unversioned"] == json!(true) {
                "unversioned"
            } else {
                ""
            },
            if row["versionable"] == json!(false) {
                "not versionable"
            } else if row["changed_since_version"] == json!(true) {
                "changed since lead"
            } else {
                "unchanged"
            },
        ));
    }
    out
}

// ── conflict list / check ───────────────────────────────────────────────

pub static CONFLICT_LIST: Command = Command {
    id: "design.conflict.list",
    path: &["design", "conflict", "list"],
    contract: 1,
    summary: "List transformers whose cloud head moved under a local copy.",
    purpose: "\
A `ds` caller could not discover that a transformer was conflicted at all. \
This reads the selected project's status rows headlessly and applies the \
shared kernel's detection rule. A conflict is a fact about a WORKING COPY, \
so a client with no rooms reports `room_state: unknown` and lists only what \
the rows themselves carry, rather than reporting a confident zero.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG],
    output: "\
Lane and project identity, `room_state`, and one row per detected conflict \
with its base and current version.",
    examples: &[Example {
        command: "ds design conflict list --output json",
        note: "`.data.room_state` says whether this client can see working copies.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run_conflict_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = crate::transformer::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let reply = kernel(
        json!({
            "op": "detect",
            "schema": ds_command_kernel::design_conflict::SCHEMA,
            "rows": rows_of(headless.result()),
        }),
        ds_command_kernel::design_conflict::evaluate,
    )?;
    let mut out = crate::transformer::project_receipt(&headless);
    out["room_state"] = json!(ROOM_STATE_UNKNOWN);
    out["conflicts"] = reply["conflicts"].clone();
    out["count"] = json!(reply["conflicts"].as_array().map(Vec::len).unwrap_or(0));
    Ok(out)
}

pub fn render_conflict_list(data: &Value) -> String {
    let mut out = format!(
        "{} conflict(s) · room state {}\n",
        data["count"].as_u64().unwrap_or(0),
        data["room_state"].as_str().unwrap_or("?"),
    );
    for row in data["conflicts"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<32} base {} · cloud {}\n",
            row["name"].as_str().unwrap_or("?"),
            row["base_version"].as_u64().unwrap_or(0),
            row["current_version"].as_u64().unwrap_or(0),
        ));
    }
    out
}

pub static CONFLICT_CHECK: Command = Command {
    id: "design.conflict.check",
    path: &["design", "conflict", "check"],
    contract: 1,
    summary: "Say whether overwriting one transformer is admissible now.",
    purpose: "\
Overwrite admissibility, asked of the shared kernel with the facts this \
client actually holds. The answer names the first refusal in the ordered \
list, so a caller learns WHICH fact is missing rather than that something \
is. A comparison and a recorded conflict are facts about a working copy: \
without rooms this reports `room_state: unknown` and the refusal that \
follows from having none, never a confident yes.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG],
    output: "\
Lane and project identity, `room_state`, and per transformer: whether the \
overwrite may be sent, whether the box may be ticked, and the refusal code \
plus message key behind each.",
    examples: &[Example {
        command: "ds design conflict check --transformer TX-1 --output json",
        note: "`.data.checks[0].refusal.code` names the missing fact.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run_conflict_check(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = crate::transformer::transformer_set(inputs, true)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let detected = kernel(
        json!({
            "op": "detect",
            "schema": ds_command_kernel::design_conflict::SCHEMA,
            "rows": rows_of(headless.result()),
        }),
        ds_command_kernel::design_conflict::evaluate,
    )?;
    let mut checks = Vec::new();
    for row in headless.result().rows() {
        let conflict = detected["conflicts"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["name"] == json!(row.name()))
            .map(|entry| {
                json!({
                    "base_version": entry["base_version"],
                    "current_version": entry["current_version"],
                })
            });
        let mut check = kernel(
            json!({
                "op": "check",
                "schema": ds_command_kernel::design_conflict::SCHEMA,
                "name": row.name(),
                // A headless client holds no capability grant, no tick and no
                // comparison; the kernel names the first thing it is missing.
                "can_force": true,
                "explicitly_selected": true,
                "conflict": conflict,
            }),
            ds_command_kernel::design_conflict::evaluate,
        )?;
        check["room_state"] = json!(ROOM_STATE_UNKNOWN);
        checks.push(check);
    }
    let mut out = crate::transformer::project_receipt(&headless);
    out["room_state"] = json!(ROOM_STATE_UNKNOWN);
    out["count"] = json!(checks.len());
    out["checks"] = Value::Array(checks);
    Ok(out)
}

pub fn render_conflict_check(data: &Value) -> String {
    let mut out = format!(
        "room state {}\n",
        data["room_state"].as_str().unwrap_or("?")
    );
    for check in data["checks"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<32} send {:<12} tick {:<12} {}\n",
            check["name"].as_str().unwrap_or("?"),
            if check["eligible"] == json!(true) {
                "eligible"
            } else {
                "refused"
            },
            if check["tick_admissible"] == json!(true) {
                "admissible"
            } else {
                "refused"
            },
            check["refusal"]["code"].as_str().unwrap_or(""),
        ));
    }
    out
}

// ── presence status ─────────────────────────────────────────────────────

pub static PRESENCE_STATUS: Command = Command {
    id: "design.presence.status",
    path: &["design", "presence", "status"],
    contract: 1,
    summary: "Read the lease plan and this client's own room visibility.",
    purpose: "\
Which rooms should hold a server lease, which should release, and how many \
lock calls one pass may spend. `ds` had no presence verb at all. The plan is \
the shared kernel's over the rooms this client can see — and a headless \
client sees none, so it reports `room_state: unknown` with the bounds and an \
empty pass rather than inventing room state. Run it where the rooms are and \
the same kernel answers over real ones.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG],
    output: "\
Lane and project identity, `room_state`, the per-pass bounds (hold refresh \
window, lock-call cap), the plan (holds, releases, deferrals, budget), and \
one row per transformer with the room state this client can report.",
    examples: &[Example {
        command: "ds design presence status --output json",
        note: "`.data.bounds.max_lock_calls_per_pass` is the shared per-pass budget.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run_presence_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = crate::transformer::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    // No rooms are visible from here, so the pass is deliberately empty and
    // NOT complete: a complete pass is the one permission to release a lease,
    // and this client has no evidence to release one on.
    let plan = kernel(
        json!({
            "op": "plan",
            "schema": ds_command_kernel::design_presence::SCHEMA,
            "rooms": [],
            "held": [],
            "drafts": [],
            "now_ms": now_ms(),
            "complete": false,
        }),
        ds_command_kernel::design_presence::evaluate,
    )?;
    let transformers: Vec<Value> = headless
        .result()
        .rows()
        .iter()
        .map(|row| json!({ "name": row.name(), "room_state": ROOM_STATE_UNKNOWN }))
        .collect();
    let mut out = crate::transformer::project_receipt(&headless);
    out["room_state"] = json!(ROOM_STATE_UNKNOWN);
    out["bounds"] = json!({
        "hold_refresh_ms": ds_command_kernel::design_presence::HOLD_REFRESH_MS,
        "draft_min_interval_ms": ds_command_kernel::design_presence::DRAFT_MIN_INTERVAL_MS,
        "max_lock_calls_per_pass": ds_command_kernel::design_presence::MAX_LOCK_CALLS_PER_PASS,
    });
    out["count"] = json!(transformers.len());
    out["transformers"] = Value::Array(transformers);
    out["plan"] = plan;
    Ok(out)
}

pub fn render_presence_status(data: &Value) -> String {
    let plan = &data["plan"];
    format!(
        "room state {} · {} transformer(s) · {} hold · {} release · {} deferred · budget {}/{}\n",
        data["room_state"].as_str().unwrap_or("?"),
        data["count"].as_u64().unwrap_or(0),
        plan["hold"].as_array().map(Vec::len).unwrap_or(0),
        plan["release"].as_array().map(Vec::len).unwrap_or(0),
        plan["deferred_count"].as_u64().unwrap_or(0),
        plan["budget"]["left"].as_u64().unwrap_or(0),
        plan["budget"]["limit"].as_u64().unwrap_or(0),
    )
}
