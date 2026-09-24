//! `ds pm plan` — the project's plan in one screen.
//!
//! The cheapest useful question a caller can ask about Project Work, and the
//! one that answers "what should I look at": the Dashboard's own rollups, the
//! progress of each discipline, and the items whose state has earned
//! attention — blocked first, then late, then paused, then in review, then
//! carrying an open residual.
//!
//! It also publishes the project's field-model vocabulary, which is what makes
//! the write commands usable without reading anything else: `--delivery`,
//! `--review` and `--closeout` take values from here, and the engine — not
//! this CLI — decides what they are.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("10"),
    choices: &[],
    summary: "Attention and recent rows to return (1-100). Totals are always reported.",
};

pub static COMMAND: Command = Command {
    id: "pm.plan",
    path: &["pm", "plan"],
    contract: 1,
    summary: "The plan's rollups, phases, attention list and vocabulary.",
    purpose: "\
Start here. Returns the same rollups the Dashboard renders — plan nodes, \
overall progress, what is in progress, blocked, late, under review, and how \
many open residuals block acceptance or closeout — plus progress by \
discipline, the items that have earned attention, and the field-model \
vocabulary the write commands take their state values from.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    // Headless, because this route is: it asks the native owner through
    // `ds_cli_auth::project_management` and needs no paired application.
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LIMIT_ARG, crate::LANE_ARG],
    output: "\
`project`, `revision`, `today`, `dashboard` with the rollups, `phases` by \
discipline, `attention` rows with their magnitude, `recent` task changes, \
full `phaseTotal`/`attentionTotal`/`recentTotal` counts for those bounded lists, \
`permissions` for the signed-in user, `vocabulary` — the delivery, review \
and closeout states this project's engine accepts, plus the correspondence \
lists (channels, record categories and states, directions, response \
statuses, blocker kinds, party kinds and roles) — and `correspondence`: the \
counters `recordsOutstanding`, `recordsOverdue`, `tasksAwaitingCorrespondence` \
and the attention rows grouped by the party the answer is owed to (`null` on \
a server that predates the contract).",
    examples: &[Example {
        command: "ds pm plan --output json",
        note: "Read .data.vocabulary before calling `ds pm task update --delivery`.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<17>(&[crate::INVALID_NUMBER]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "schedule",
        "gantt",
        "wbs",
        "backlog",
        "programme",
        "program",
        "progress",
        "milestone",
        "late",
        "blocked",
        "who owes",
        "ball in court",
        "correspondence",
        "overdue",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = match inputs.value("limit") {
        Some(value) => crate::integer(value, "limit", 1, 100)?,
        None => 10,
    };
    let read = crate::graph(inputs.value("lane").unwrap_or("stable"))?;
    // The graph is the SERVER's answer; what it MEANS is the kernel's. The
    // browser folded it for itself, which is why the dashboard and the
    // attention list could disagree about the same task — one answer now, for
    // the CLI, the Server and the page alike.
    fold_plan(&read.project_id, read.raw, limit)
}

pub fn render(data: &Value) -> String {
    let dashboard = &data["dashboard"];
    let percent = |key: &str| (dashboard[key].as_f64().unwrap_or(0.0) * 100.0).round() as u64;
    let count = |key: &str| dashboard[key].as_u64().unwrap_or(0);

    let mut out = format!(
        "{} · revision {} · {}% complete over {}\n",
        data["project"].as_str().unwrap_or("?"),
        data["revision"].as_u64().unwrap_or(0),
        percent("overallProgress"),
        crate::plural(count("planNodes"), "plan node"),
    );
    out.push_str(&format!(
        "  in progress {} · blocked {} · paused {} · late {} · in review {}\n",
        count("inProgress"),
        count("blocked"),
        count("paused"),
        count("pastPlannedFinish"),
        count("underReview"),
    ));
    out.push_str(&format!(
        "  {} open across {} · {} block acceptance · {} block closeout\n",
        crate::plural(count("openResiduals"), "residual"),
        crate::plural(count("residualTaskCount"), "task"),
        count("acceptanceBlocked"),
        count("closeoutBlocked"),
    ));

    if let Some(phases) = data["phases"].as_array().filter(|rows| !rows.is_empty()) {
        out.push_str("\nBy discipline\n");
        for phase in phases {
            out.push_str(&format!(
                "  {:<24} {:>4}%  {} of {}\n",
                crate::truncate(
                    phase["discipline"]
                        .as_str()
                        .filter(|name| !name.is_empty())
                        .unwrap_or("(unassigned)"),
                    24,
                ),
                (phase["progress"].as_f64().unwrap_or(0.0) * 100.0).round() as u64,
                phase["complete"].as_u64().unwrap_or(0),
                phase["total"].as_u64().unwrap_or(0),
            ));
        }
    }

    if let Some(attention) = data["attention"].as_array().filter(|rows| !rows.is_empty()) {
        out.push_str(&format!(
            "\nNeeds attention ({} of {})\n",
            attention.len(),
            data["attentionTotal"]
                .as_u64()
                .unwrap_or(attention.len() as u64),
        ));
        for row in attention {
            out.push_str(&format!(
                "  {:<9} {:<10} {}\n",
                row["kind"].as_str().unwrap_or("?"),
                row["wbs"].as_str().unwrap_or("—"),
                crate::truncate(row["title"].as_str().unwrap_or("?"), 52),
            ));
        }
    }

    let correspondence = &data["correspondence"];
    if correspondence.is_object() {
        let counters = &correspondence["counters"];
        out.push_str(&format!(
            "\nCorrespondence · {} outstanding · {} overdue · {} waiting on an answer\n",
            counters["recordsOutstanding"].as_u64().unwrap_or(0),
            counters["recordsOverdue"].as_u64().unwrap_or(0),
            counters["tasksAwaitingCorrespondence"]
                .as_u64()
                .unwrap_or(0),
        ));
        for party in correspondence["parties"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {} ({} overdue, {} due within {} days)\n",
                party["partyName"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .or(party["partyId"].as_str())
                    .unwrap_or("(no party)"),
                party["overdue"].as_u64().unwrap_or(0),
                party["dueWithinWindow"].as_u64().unwrap_or(0),
                correspondence["dueWindowDays"].as_u64().unwrap_or(7),
            ));
            for row in party["records"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "    {:<12} {:<11} {:<40} owed by {}{}\n",
                    row["recordId"].as_str().unwrap_or("?"),
                    row["responseStatus"].as_str().unwrap_or("?"),
                    crate::truncate(row["subject"].as_str().unwrap_or(""), 40),
                    row["owedBy"].as_str().unwrap_or("?"),
                    row["responseDueDate"]
                        .as_str()
                        .map(|due| format!(" · due {due}"))
                        .unwrap_or_default(),
                ));
            }
            for task in party["blockedTasks"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "    task {:<12} blocked · {}\n",
                    task["taskId"].as_str().unwrap_or("?"),
                    crate::truncate(task["title"].as_str().unwrap_or(""), 44),
                ));
            }
        }
    }
    out
}

/// Fold the server's canonical graph into the plan the operator reads.
///
/// The host supplies the day because the kernel holds no clock: every date in
/// the answer is compared against this one value, so a plan is reproducible
/// from the pair (graph, today).
fn fold_plan(project: &str, graph: Value, limit: i64) -> Result<Value, Failure> {
    let request = serde_json::json!({
        "schema": ds_command_kernel::project_management::SCHEMA,
        "action": "plan",
        "today": today_utc(),
        "ds_project": project,
        "graph": graph,
        "limit": limit,
    });
    let unreadable = |error: String| {
        Failure::internal(crate::PLAN_UNREADABLE.code, error).remedy(crate::PLAN_UNREADABLE.remedy)
    };
    let bytes = serde_json::to_vec(&request).map_err(|error| unreadable(error.to_string()))?;
    let answer = ds_command_kernel::project_management::evaluate(&bytes).map_err(unreadable)?;
    serde_json::from_str(&answer).map_err(|error| unreadable(error.to_string()))
}

/// The host's day as `YYYY-MM-DD`, UTC.
fn today_utc() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default();
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's days-from-civil, inverted. Pure arithmetic: no chrono, no
/// locale, and no dependency added for four lines.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
