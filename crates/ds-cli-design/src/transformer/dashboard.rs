//! `ds design dashboard` — what the whole Dashboard says about the project.
//!
//! The same native status read `ds design status` performs, folded once by
//! `ds-command-kernel::design_dashboard` into the model the application's
//! Design wall renders: pipeline, momentum, crew, geography, phase summaries,
//! lane split, governance mix, the attention pile, the health score and the
//! fun facts. One question, one answer, whether it is asked from a browser or
//! from a terminal.
//!
//! Labels come back as i18n keys, because naming them is the reading
//! surface's job — `--output json` gives a caller the keys, and the renderer
//! below spells the few it shows.

use ds_cli_auth::TransformerStatusList;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::LANE_ARG;

pub const FAST_ARG: Arg = Arg::switch(
    "fast",
    "Read the project as the Fast lane does: no Draft/Sketch summary or notes.",
);

pub static COMMAND: Command = Command {
    id: "design.dashboard",
    path: &["design", "dashboard"],
    contract: 1,
    summary: "Read the project's whole Design dashboard, headlessly.",
    purpose: "\
The project's own progress story, folded by the shared kernel from the same \
status rows `ds design status` returns. Every attention note is the shared \
health verdict for that row, so this and the register cannot disagree. \
Restores the native user and reads only that user's audience-fenced selected \
project; no project, Desktop descriptor, URL, body or action override, and no \
fallback to a browser. The reference document describes each member.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG, FAST_ARG],
    output: "\
Lane and selected-project identity/status, then `dashboard`: the counts, \
`pipeline`, `momentum`, `crew`, `errors_by_user`, `districts`, \
`phase_summaries`, `lanes`, `governance`, `attention`, `health`, `recent` and \
`facts`. Labels are i18n keys, timestamps epoch millis, and a headless client \
holds none of the live diagnostics the application folds in.",
    examples: &[
        Example {
            command: "ds design dashboard --output json",
            note: "`.data.dashboard.health.score` is the project's health.",
            runnable: false,
        },
        Example {
            command: "ds design dashboard --fast --output json",
            note: "`.data.dashboard.attention` as the Fast lane reads it.",
            runnable: false,
        },
    ],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The whole project, always: a dashboard over a subset is a different
    // question, and every percentage here is measured against the fleet.
    let whole_project = super::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &whole_project)?;
    let mut output = super::project_receipt(&headless);
    let dashboard = dashboard_json(headless.result(), inputs.switch("fast"));
    output
        .as_object_mut()
        .expect("receipt is an object")
        .insert("dashboard".into(), dashboard);
    Ok(output)
}

/// One kernel call over the rows as the service sent them. Special rows travel
/// with the list: which names are not transformers is the kernel's own table.
fn dashboard_json(list: &TransformerStatusList, fast_lane: bool) -> Value {
    let rows: Vec<Value> = list.rows().iter().map(|row| row.row().clone()).collect();
    let request = json!({
        "schema": ds_command_kernel::design_dashboard::SCHEMA,
        "rows": rows,
        // A headless client holds no browser session, so it holds none of the
        // live process diagnostics the application folds in.
        "diagnostics": [],
        "now_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0),
        "fast_lane": fast_lane,
    });
    let Ok(input) = serde_json::to_vec(&request) else {
        return Value::Null;
    };
    let Ok(reply) = ds_command_kernel::design_dashboard::evaluate(&input) else {
        return Value::Null;
    };
    let reply: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
    reply["dashboard"].clone()
}

fn count(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}

pub fn render(data: &Value) -> String {
    let dashboard = &data["dashboard"];
    let health = &dashboard["health"];
    let mut out = format!(
        "project {} ({}) · {} · {} transformers · health {} ({} clean, {} error, {} warning)\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        count(dashboard, "total"),
        count(health, "score"),
        count(health, "clean_count"),
        count(health, "error_count"),
        count(health, "warning_count"),
    );
    for stage in dashboard["pipeline"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<10} {:>5} ({}%)\n",
            stage["key"].as_str().unwrap_or("?"),
            count(stage, "count"),
            count(stage, "pct_of_total"),
        ));
    }
    let momentum = &dashboard["momentum"];
    out.push_str(&format!(
        "  momentum   last 7d {} vs {} ({}), {} active days, busiest {}\n",
        count(momentum, "last7"),
        count(momentum, "prev7"),
        momentum["trend"].as_str().unwrap_or("?"),
        count(momentum, "active_days"),
        momentum["busiest"]["key"].as_str().unwrap_or("—"),
    ));
    out.push_str(&format!(
        "  crew       {} designing, {} districts, {} sectors\n",
        count(&dashboard["crew"], "contributors"),
        count(dashboard, "district_count"),
        count(dashboard, "sector_count"),
    ));
    let attention = dashboard["attention"].as_array().map(Vec::as_slice);
    let notes = attention.unwrap_or_default();
    if notes.is_empty() {
        out.push_str("  attention  nothing — every row is clean\n");
    }
    for note in notes.iter().take(10) {
        out.push_str(&format!(
            "  {:<6} {:<28} {} {}\n",
            note["tone"].as_str().unwrap_or("?"),
            note["name"].as_str().unwrap_or("?"),
            note["label_key"].as_str().unwrap_or("?"),
            note["message"].as_str().unwrap_or(""),
        ));
    }
    if notes.len() > 10 {
        out.push_str(&format!("  … {} more\n", notes.len() - 10));
    }
    out
}
