//! `ds design activities sweep` — take one photograph of each project.
//!
//! A sweep is expensive and it is expensive for everybody: ds-brain serialises
//! full status scans to ONE in-flight scan per instance, so two sweeps racing
//! each other simply queue, and a sweep racing a colleague's Design page makes
//! that page wait. So the kernel plans the order, the CLI walks it one project
//! at a time with a pause between them, and `--limit` is small by default.
//!
//! Nothing here schedules anything. A sweep happens because someone asked.

use ds_cli_auth::TransformerSet;
use ds_cli_contract::args::integer;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use super::{
    BUCKET_ARG, DIRECTORY_SCHEMA, LANE_ARG, PROJECT_ARG, STATE_DIR_ARG, TZ_OFFSET_ARG,
    account_root, activities_call, atomic_write, capture_key, inventory, not_claimed, now_ms,
    state_root, store_call,
};

const LIMIT_ARG: Arg = Arg::value(
    "limit",
    "<count>",
    "At most this many projects are captured in one sweep (1..5000).",
)
.default("25");

const MAX_AGE_ARG: Arg = Arg::value(
    "max-age",
    "<minutes>",
    "A capture younger than this is left alone (0..525600).",
)
.default("360");

const PAUSE_ARG: Arg = Arg::value(
    "pause-ms",
    "<milliseconds>",
    "Wait between two captures; ds-brain runs one full status scan at a time (0..600000).",
)
.default("2000");

const FAST_ARG: Arg = Arg::switch(
    "fast",
    "Read each project as the Fast lane does: no Draft/Sketch summary or notes.",
);

const REFUSALS: &[Refusal] = &[
    crate::transformer::NATIVE_PROFILE,
    crate::transformer::NATIVE_PROFILE_DIGEST,
    crate::transformer::NATIVE_PROFILE_UNSAFE,
    crate::transformer::HEADLESS_SIGNED_OUT,
    crate::transformer::CONTEXT_CORRUPT,
    crate::transformer::AUTH_REVOKED,
    crate::transformer::AUTH_TRANSIENT,
    crate::transformer::AUTH_UNREADABLE,
    super::STORE_UNAVAILABLE,
    super::STORE_UNSAFE,
    super::SNAPSHOT_INVALID,
    super::ACTIVITIES_REFUSED,
    ds_cli_contract::args::INVALID_NUMBER,
];

pub static COMMAND: Command = Command {
    id: "design.activities.sweep",
    path: &["design", "activities", "sweep"],
    contract: 1,
    summary: "Capture each visible project's design ledger, one project at a time.",
    purpose: "\
Reads every project this account can reach, folds each one's status rows into \
a ledger through the shared kernel, and retains the result on this machine so \
`ds design activities read` can answer offline. The kernel decides which \
projects are worth re-capturing and in which order; the sweep walks that \
order sequentially, pausing between projects, because ds-brain runs ONE full \
status scan at a time per instance. A project that refuses the read is \
recorded as a refusal, never as an absence, and never aborts the sweep. \
Nothing is scheduled: a capture exists because someone asked for it.",
    chapter: Chapter::Design,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        BUCKET_ARG,
        LIMIT_ARG,
        MAX_AGE_ARG,
        PAUSE_ARG,
        FAST_ARG,
        LANE_ARG,
        STATE_DIR_ARG,
        TZ_OFFSET_ARG,
    ],
    output: "\
`store` and `captured_by`, the `plan` the kernel decided (`refresh`, `skip`, \
`pause_ms`, `sequential`), then one `projects` row per planned project: \
`captured` with its capture time, row and transformer counts, digest and the \
captures retention kept and dropped — or `refused` with the code and reason \
the read gave. `not_claimed` states what a capture can never show.",
    examples: &[
        Example {
            command: "ds design activities sweep --limit 3 --yes --output json",
            note: "`.data.projects[].outcome` is `captured` or `refused`, per project.",
            runnable: false,
        },
        Example {
            command: "ds design activities sweep --project <id> --yes --output json",
            note: "One project, one capture; `.data.plan.skip` names what was left alone.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let bucket = inputs.require("bucket")?;
    let limit = integer(inputs.require("limit")?, "limit", 1, 5000)?;
    let max_age_minutes = integer(inputs.require("max-age")?, "max-age", 0, 525_600)?;
    let pause_ms = integer(inputs.require("pause-ms")?, "pause-ms", 0, 600_000)?;
    let tz_offset = integer(
        inputs.require("tz-offset-minutes")?,
        "tz-offset-minutes",
        -840,
        840,
    )?;
    let fast_lane = inputs.switch("fast");
    let root = state_root(inputs)?;
    let named: Vec<String> = inputs.repeated("project").to_vec();

    let directory = ds_cli_auth::project_directory(lane)?;
    let uid = directory.identity().uid().to_owned();
    let audience = directory.identity().credential_audience_sha256().to_owned();
    let account = account_root(&root, &uid, &audience, lane)?;

    // The directory is retained first, so a later offline read can still name
    // a project this account may by then have lost access to.
    let directory_file = account.join("directory.json");
    let retained_directory = json!({
        "schema": DIRECTORY_SCHEMA,
        "lane": lane,
        "captured_at_ms": now_ms(),
        "captured_by": uid,
        "projects": directory.projects(),
    });
    atomic_write(
        &directory_file,
        &serde_json::to_vec_pretty(&retained_directory).unwrap_or_default(),
    )?;

    let held = inventory(&account)?;
    let mut cached: Vec<Value> = Vec::new();
    for (project, captures) in &held {
        if let Some(latest) = captures.last() {
            cached.push(json!({ "ds_project": project, "captured_at_ms": latest }));
        }
    }

    // A project named on the command line that the directory does not carry
    // is a refusal with a name, not a silent nothing.
    let mut rows: Vec<Value> = Vec::new();
    let mut visible: Vec<Value> = Vec::new();
    for project in directory.projects() {
        let id = project["ds_project"].as_str().unwrap_or_default();
        if id.is_empty() || (!named.is_empty() && !named.iter().any(|name| name == id)) {
            continue;
        }
        visible.push(json!({
            "ds_project": id,
            "status": project["status"],
            "display_name": project["display_name"],
        }));
    }
    for name in &named {
        if !visible
            .iter()
            .any(|project| project["ds_project"].as_str() == Some(name.as_str()))
        {
            rows.push(json!({
                "ds_project": name,
                "outcome": "refused",
                "code": "project_not_visible",
                "reason": "the project directory this account can reach does not carry that exact id",
            }));
        }
    }

    let mut plan_request = Map::new();
    plan_request.insert("visible".into(), json!(visible));
    plan_request.insert("cached".into(), json!(cached));
    plan_request.insert("now_ms".into(), json!(now_ms()));
    plan_request.insert("bucket".into(), json!(bucket));
    plan_request.insert("max_age_ms".into(), json!(max_age_minutes * 60_000));
    plan_request.insert("limit".into(), json!(limit));
    plan_request.insert("pause_ms".into(), json!(pause_ms));
    let plan = store_call("plan", plan_request)?;

    let refresh: Vec<Value> = plan["refresh"].as_array().cloned().unwrap_or_default();
    let planned = refresh.len();
    let mut captured = 0usize;
    for (index, entry) in refresh.iter().enumerate() {
        let Some(project) = entry["ds_project"].as_str() else {
            continue;
        };
        // The etiquette the kernel attached: one scan at a time, with a pause
        // between them. Never before the first, so a one-project sweep is not
        // slower for being polite.
        if index > 0 && pause_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(pause_ms as u64));
        }
        let listed = visible
            .iter()
            .find(|candidate| candidate["ds_project"].as_str() == Some(project))
            .cloned()
            .unwrap_or_else(|| json!({ "ds_project": project }));
        match capture(
            lane,
            project,
            &root,
            &uid,
            &audience,
            fast_lane,
            tz_offset,
            &listed,
            entry["reason"].as_str().unwrap_or("missing"),
        ) {
            Ok(row) => {
                captured += 1;
                rows.push(row);
            }
            Err(failure) => rows.push(json!({
                "ds_project": project,
                "outcome": "refused",
                "code": failure.code(),
                "reason": failure.message(),
            })),
        }
    }

    Ok(json!({
        "lane": lane,
        "store": account.to_string_lossy(),
        "captured_by": uid,
        "bucket": bucket,
        "plan": plan,
        "planned": planned,
        "captured": captured,
        "refused": rows.len() - captured,
        "projects": rows,
        "not_claimed": not_claimed(),
    }))
}

/// One project: read its rows, fold them twice, retain the envelope, then let
/// the kernel decide which older captures survive.
#[allow(clippy::too_many_arguments)]
fn capture(
    lane: &str,
    project: &str,
    root: &std::path::Path,
    uid: &str,
    audience: &str,
    fast_lane: bool,
    tz_offset: i64,
    listed: &Value,
    planned_reason: &str,
) -> Result<Value, Failure> {
    let whole_project = TransformerSet::new(std::iter::empty::<String>())
        .map_err(|error| Failure::internal("invalid_transformer_scope", error.to_string()))?;
    let headless = ds_cli_auth::transformer_status_for_project(lane, project, &whole_project)?;
    // ONE capture instant for the file name, the envelope and the ledger: the
    // kernel refuses an envelope whose ledger claims a different moment, and
    // it is right to — a capture named by one instant that holds another's
    // rows is not evidence of anything.
    let captured_at_ms = now_ms();
    let rows: Vec<Value> = headless
        .result()
        .rows()
        .iter()
        .map(|row| row.row().clone())
        .collect();
    let row_count = rows.len();

    let mut ledger_request = Map::new();
    ledger_request.insert("ds_project".into(), json!(project));
    ledger_request.insert("captured_at_ms".into(), json!(captured_at_ms));
    ledger_request.insert("rows".into(), json!(rows));
    ledger_request.insert("now_ms".into(), json!(captured_at_ms));
    let ledger = activities_call("ledger", ledger_request)?["ledger"].clone();

    // The project's own Dashboard, folded from the same rows at the same
    // instant. A headless client holds no live process diagnostics, and the
    // envelope says so rather than letting a reader assume there were none.
    let dashboard_request = json!({
        "schema": ds_command_kernel::design_dashboard::SCHEMA,
        "rows": rows,
        "diagnostics": [],
        "now_ms": captured_at_ms,
        "fast_lane": fast_lane,
        "tz_offset_minutes": tz_offset,
    });
    let dashboard = serde_json::to_vec(&dashboard_request)
        .ok()
        .and_then(|bytes| ds_command_kernel::design_dashboard::evaluate(&bytes).ok())
        .and_then(|reply| serde_json::from_str::<Value>(&reply).ok())
        .map(|reply| reply["dashboard"].clone())
        .filter(|dashboard| !dashboard.is_null());

    let mut digest_request = Map::new();
    digest_request.insert("ledger".into(), ledger.clone());
    let digest = store_call("digest", digest_request)?["digest"].clone();

    let mut snapshot = Map::new();
    snapshot.insert(
        "schema".into(),
        json!(ds_command_kernel::design_activities_store::SNAPSHOT_SCHEMA),
    );
    snapshot.insert("ds_project".into(), json!(project));
    // The directory's own spelling of the project travels with the capture,
    // so an offline read can still name a project this account may later lose
    // access to — and can still say which bucket it was in.
    if let Some(name) = listed["display_name"].as_str() {
        snapshot.insert("display_name".into(), json!(name));
    }
    if let Some(status) = listed["status"].as_str() {
        snapshot.insert("status".into(), json!(status));
    }
    snapshot.insert("lane".into(), json!(lane));
    snapshot.insert("captured_at_ms".into(), json!(captured_at_ms));
    snapshot.insert("captured_by".into(), json!(uid));
    snapshot.insert(
        "source".into(),
        json!({ "rows": row_count, "fast_lane": fast_lane, "diagnostics_source": "none" }),
    );
    snapshot.insert("ledger".into(), ledger.clone());
    if let Some(dashboard) = dashboard {
        snapshot.insert("dashboard".into(), dashboard);
    }
    snapshot.insert("digest".into(), digest.clone());
    let snapshot = Value::Object(snapshot);

    let mut validate = Map::new();
    validate.insert("snapshot".into(), snapshot.clone());
    let admitted = store_call("validate", validate)?;

    let key = capture_key(root, uid, audience, lane, project, captured_at_ms)?;
    let path = key.directory.join(&key.file_name);
    atomic_write(
        &path,
        &serde_json::to_vec_pretty(&snapshot).unwrap_or_default(),
    )?;

    // Retention runs over what is on disk AFTER the write, so the capture
    // just taken is part of the decision and can never be the one dropped.
    let mut existing: Vec<i64> = std::fs::read_dir(&key.directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|file| {
            file.file_name()
                .to_string_lossy()
                .strip_suffix(".json")?
                .parse::<i64>()
                .ok()
        })
        .collect();
    existing.sort_unstable();
    let mut retain = Map::new();
    retain.insert("existing".into(), json!(existing));
    retain.insert("now_ms".into(), json!(captured_at_ms));
    let retention = store_call("retain", retain)?;
    let mut dropped = 0usize;
    for stale in retention["drop"].as_array().into_iter().flatten() {
        let Some(stale) = stale.as_i64() else {
            continue;
        };
        if std::fs::remove_file(key.directory.join(format!("{stale:013}.json"))).is_ok() {
            dropped += 1;
        }
    }

    Ok(json!({
        "ds_project": project,
        "outcome": "captured",
        "reason_planned": planned_reason,
        "captured_at_ms": captured_at_ms,
        "rows": row_count,
        "transformers": admitted["ledger_transformers"],
        "digest": digest,
        "kept": retention["keep"].as_array().map(Vec::len).unwrap_or(0),
        "dropped": dropped,
        "path": path.to_string_lossy(),
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} captured, {} refused of {} planned · store {}\n",
        data["lane"].as_str().unwrap_or("?"),
        data["captured"].as_u64().unwrap_or(0),
        data["refused"].as_u64().unwrap_or(0),
        data["planned"].as_u64().unwrap_or(0),
        data["store"].as_str().unwrap_or("?"),
    );
    for row in data["projects"].as_array().into_iter().flatten() {
        if row["outcome"] == "captured" {
            out.push_str(&format!(
                "  captured {:<24} {} transformers of {} rows · {}\n",
                row["ds_project"].as_str().unwrap_or("?"),
                row["transformers"].as_u64().unwrap_or(0),
                row["rows"].as_u64().unwrap_or(0),
                super::spell_ms(row["captured_at_ms"].as_i64().unwrap_or(0)),
            ));
        } else {
            out.push_str(&format!(
                "  refused  {:<24} {} — {}\n",
                row["ds_project"].as_str().unwrap_or("?"),
                row["code"].as_str().unwrap_or("?"),
                row["reason"].as_str().unwrap_or(""),
            ));
        }
    }
    for skipped in data["plan"]["skip"].as_array().into_iter().flatten() {
        if skipped["reason"] == "limit" {
            out.push_str(&format!(
                "  not reached {:<21} raise --limit to include it\n",
                skipped["ds_project"].as_str().unwrap_or("?"),
            ));
        }
    }
    out.push_str(
        "  a capture shows the latest stamp per phase, no device, and nothing between two sweeps\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_declares_the_etiquette_its_cost_requires() {
        assert_eq!(COMMAND.effect, Effect::ArtifactWrite);
        assert!(COMMAND.effect.needs_confirmation());
        assert_eq!(COMMAND.requires, Requires::Server);
        assert!(COMMAND.purpose.contains("ONE full"));
        assert!(COMMAND.purpose.contains("Nothing is scheduled"));
    }

    #[test]
    fn the_default_limit_is_small_because_the_scan_slot_is_shared() {
        let limit = COMMAND.arg("limit").expect("declared").default;
        assert_eq!(limit, Some("25"));
        assert_eq!(
            COMMAND.arg("pause-ms").expect("declared").default,
            Some("2000")
        );
    }

    #[test]
    fn a_human_sweep_line_says_what_a_capture_cannot_show() {
        let rendered = render(&json!({
            "lane": "stable", "captured": 1, "refused": 1, "planned": 2, "store": "/s",
            "projects": [
                {"outcome": "captured", "ds_project": "p_one", "transformers": 4, "rows": 5,
                 "captured_at_ms": 1_758_000_000_000i64},
                {"outcome": "refused", "ds_project": "p_two", "code": "auth_rejected",
                 "reason": "membership"},
            ],
            "plan": {"skip": [{"ds_project": "p_three", "reason": "limit"}]},
        }));
        assert!(rendered.contains("captured p_one"));
        assert!(rendered.contains("refused  p_two"));
        assert!(rendered.contains("not reached p_three"));
        assert!(rendered.contains("no device"));
    }
}
