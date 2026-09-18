//! `ds design activities read` — the cross-project answer, offline.
//!
//! Nothing here reaches the network. Every capture this reads was taken by a
//! sweep someone asked for, and the newest capture of each project plus the
//! one retained before it are the entire evidence. What the fold says about a
//! person is what those two photographs said; a gap between them renders as a
//! gap, not as a guess.

use ds_cli_contract::args::integer;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use super::{
    BUCKET_ARG, LANE_ARG, PROJECT_ARG, STATE_DIR_ARG, TZ_OFFSET_ARG, account_root, activities_call,
    inventory, not_claimed, now_ms, read_capture, spell_ms, state_root, store_call,
};

const USER_ARG: Arg = Arg::value(
    "user",
    "<actor>",
    "Show only this actor's rows; totals stay project-wide.",
);

const SINCE_ARG: Arg = Arg::value(
    "since",
    "<epoch-ms>",
    "Diff against the newest capture taken at or before this instant.",
);

const LIMIT_ARG: Arg = Arg::value(
    "limit",
    "<count>",
    "At most this many entries per list (1..5000); truncation is reported in `more`.",
)
.default("50");

const REFUSALS: &[Refusal] = &[
    crate::transformer::HEADLESS_SIGNED_OUT,
    super::STORE_UNAVAILABLE,
    super::STORE_UNSAFE,
    super::STORE_EMPTY,
    super::SNAPSHOT_INVALID,
    super::ACTIVITIES_REFUSED,
    ds_cli_contract::args::INVALID_NUMBER,
];

pub static COMMAND: Command = Command {
    id: "design.activities.read",
    path: &["design", "activities", "read"],
    contract: 1,
    summary: "Fold retained project captures into one cross-project answer.",
    purpose: "\
Who has been designing, in which projects, and what changed since the capture \
before. Reads only what `ds design activities sweep` retained on this machine, \
so it answers offline. Successive captures are the ENTIRE history: nothing \
between them is visible, no actor is inferred, and no row carries a device — \
credentials used by someone else read as their owner. `not_claimed` repeats \
those limits in every reply.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        BUCKET_ARG,
        USER_ARG,
        SINCE_ARG,
        LIMIT_ARG,
        LANE_ARG,
        STATE_DIR_ARG,
        TZ_OFFSET_ARG,
    ],
    output: "\
`captured`, `totals`, `users`, `projects`, `changes`, `timeline`, \
`anomalies`, `facts` and `more` — the shared kernel's cross-project model, \
labels as i18n keys and timestamps as epoch millis. `sources` names the two \
captures used per project; `not_claimed` states what none of it can show.",
    examples: &[
        Example {
            command: "ds design activities read --output json",
            note: "`.data.users[0]` is the busiest actor across every retained project.",
            runnable: false,
        },
        Example {
            command: "ds design activities read --user someone@example.com --output json",
            note: "`.data.filtered_by_user` echoes the projection; `.data.totals` stays whole.",
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
    let tz_offset = integer(
        inputs.require("tz-offset-minutes")?,
        "tz-offset-minutes",
        -840,
        840,
    )?;
    let since = match inputs.value("since").filter(|value| !value.is_empty()) {
        Some(raw) => Some(integer(raw, "since", 1, 9_999_999_999_999)?),
        None => None,
    };
    let named: Vec<String> = inputs.repeated("project").to_vec();
    let user = inputs.value("user").unwrap_or_default().trim().to_owned();
    let root = state_root(inputs)?;

    // The identity is restored from protected state alone: this command never
    // opens a socket, so an offline operator still gets their own store.
    let identity =
        ds_cli_auth::probe_headless_identity_for_named_project(lane)?.ok_or_else(|| {
            Failure::unauthorized(
                crate::transformer::HEADLESS_SIGNED_OUT.code,
                "no native user is signed in for this lane and profile",
            )
            .remedy(crate::transformer::HEADLESS_SIGNED_OUT.remedy)
        })?;
    let account = account_root(
        &root,
        identity.uid(),
        identity.credential_audience_sha256(),
        lane,
    )?;

    let held = inventory(&account)?;
    let mut projects: Vec<Value> = Vec::new();
    let mut sources: Vec<Value> = Vec::new();
    for (project, captures) in &held {
        if !named.is_empty() && !named.iter().any(|name| name == project) {
            continue;
        }
        let mut select = Map::new();
        select.insert("existing".into(), json!(captures));
        let selected = store_call("select", select)?;
        let Some(latest) = selected["latest"].as_i64() else {
            continue;
        };
        // `--since` moves the comparison point: the capture the fold diffs
        // against is the newest one taken at or before that instant, so the
        // answer covers everything from then to now rather than only the last
        // step. Without it the kernel's own pair is used.
        let previous = match since {
            Some(since) => captures
                .iter()
                .copied()
                .rfind(|capture| *capture <= since && *capture < latest),
            None => selected["previous"].as_i64(),
        };

        let snapshot = read_capture(&account.join(project).join(format!("{latest:013}.json")))?;
        if !admits(bucket, snapshot["status"].as_str()) {
            continue;
        }
        let mut entry = Map::new();
        entry.insert("ds_project".into(), json!(project));
        if let Some(name) = snapshot["display_name"].as_str() {
            entry.insert("display_name".into(), json!(name));
        }
        if let Some(status) = snapshot["status"].as_str() {
            entry.insert("status".into(), json!(status));
        }
        if let Some(by) = snapshot["captured_by"].as_str() {
            entry.insert("captured_by".into(), json!(by));
        }
        entry.insert("ledger".into(), snapshot["ledger"].clone());
        // The Dashboard's own health verdict travels with the capture, so the
        // cross-project answer and the project page cannot disagree about it.
        if let Some(score) = snapshot["dashboard"]["health"]["score"].as_i64() {
            entry.insert("dashboard_summary".into(), json!({ "health_score": score }));
        }
        if let Some(previous) = previous {
            let older = read_capture(&account.join(project).join(format!("{previous:013}.json")))?;
            entry.insert("previous".into(), older["ledger"].clone());
        }
        projects.push(Value::Object(entry));
        sources.push(json!({
            "ds_project": project,
            "latest_ms": latest,
            "previous_ms": previous,
            "retained": captures.len(),
        }));
    }

    if projects.is_empty() {
        return Err(Failure::invalid(
            super::STORE_EMPTY.code,
            "no retained capture matches this lane, account, bucket and project selection",
        )
        .remedy(super::STORE_EMPTY.remedy)
        .next("ds design activities sweep --limit 3 --yes"));
    }

    let mut fold = Map::new();
    fold.insert("now_ms".into(), json!(now_ms()));
    fold.insert("tz_offset_minutes".into(), json!(tz_offset));
    fold.insert(
        "limits".into(),
        json!({ "users": limit, "timeline": limit, "projects": limit, "changes": limit }),
    );
    fold.insert("projects".into(), json!(projects));
    let reply = activities_call("fold", fold)?;

    let mut activities = reply["activities"].clone();
    if !user.is_empty() {
        project_onto_user(&mut activities, &user);
    }
    let mut out = activities.as_object().cloned().unwrap_or_default();
    out.insert("lane".into(), json!(lane));
    out.insert("bucket".into(), json!(bucket));
    out.insert("store".into(), json!(account.to_string_lossy()));
    out.insert("sources".into(), json!(sources));
    if !user.is_empty() {
        out.insert("filtered_by_user".into(), json!(user));
    }
    if let Some(since) = since {
        out.insert("since_ms".into(), json!(since));
    }
    out.insert("not_claimed".into(), not_claimed());
    Ok(Value::Object(out))
}

/// Whether a capture's retained lifecycle status belongs to the bucket asked
/// for. A capture whose directory entry carried no status counts as active,
/// exactly as the project list itself reads it.
fn admits(bucket: &str, status: Option<&str>) -> bool {
    let status = status.unwrap_or("active").trim();
    match bucket {
        "all" => true,
        "archived" => status == "archived",
        "testing" => status == "testing",
        _ => status.is_empty() || status == "active",
    }
}

/// Narrow the lists to one actor. The totals are deliberately left whole:
/// "this person did four of the project's ninety designs" is the sentence
/// worth reading, and it needs both numbers.
fn project_onto_user(activities: &mut Value, user: &str) {
    let matches = |value: &Value| value.as_str().is_some_and(|actor| actor == user);
    if let Some(users) = activities["users"].as_array() {
        let kept: Vec<Value> = users
            .iter()
            .filter(|entry| matches(&entry["name"]))
            .cloned()
            .collect();
        activities["users"] = json!(kept);
    }
    for list in ["changes", "timeline"] {
        if let Some(entries) = activities[list].as_array() {
            let kept: Vec<Value> = entries
                .iter()
                .filter(|entry| matches(&entry["user"]) || matches(&entry["previous_user"]))
                .cloned()
                .collect();
            activities[list] = json!(kept);
        }
    }
    if let Some(anomalies) = activities["anomalies"].as_array() {
        let kept: Vec<Value> = anomalies
            .iter()
            .filter(|entry| matches(&entry["user"]))
            .cloned()
            .collect();
        activities["anomalies"] = json!(kept);
    }
}

pub fn render(data: &Value) -> String {
    let captured = &data["captured"];
    let totals = &data["totals"];
    let mut out = format!(
        "{} projects captured {} → {} · {} actors · {} transformers · {} designed · {} errors\n",
        captured["project_count"].as_u64().unwrap_or(0),
        spell_ms(captured["earliest_ms"].as_i64().unwrap_or(0)),
        spell_ms(captured["latest_ms"].as_i64().unwrap_or(0)),
        totals["actors"].as_u64().unwrap_or(0),
        totals["transformers"].as_u64().unwrap_or(0),
        totals["designed"].as_u64().unwrap_or(0),
        totals["errors"].as_u64().unwrap_or(0),
    );
    if let Some(user) = data["filtered_by_user"].as_str() {
        out.push_str(&format!(
            "  showing only {user}; totals stay project-wide\n"
        ));
    }
    for user in data["users"].as_array().into_iter().flatten().take(10) {
        out.push_str(&format!(
            "  {:<32} {:>4} designed · {:>3} errors · {} projects · last {}\n",
            user["name"].as_str().unwrap_or("?"),
            user["designs"].as_u64().unwrap_or(0),
            user["errors"].as_u64().unwrap_or(0),
            user["project_count"].as_u64().unwrap_or(0),
            spell_ms(user["last_seen_ms"].as_i64().unwrap_or(0)),
        ));
    }
    for anomaly in data["anomalies"].as_array().into_iter().flatten().take(10) {
        out.push_str(&format!(
            "  ! {:<14} {} {}\n",
            anomaly["label_key"].as_str().unwrap_or("?"),
            anomaly["ds_project"].as_str().unwrap_or("?"),
            anomaly["user"].as_str().unwrap_or(""),
        ));
    }
    for change in data["changes"].as_array().into_iter().flatten().take(10) {
        out.push_str(&format!(
            "  {} {} {} {} {}\n",
            spell_ms(change["when_ms"].as_i64().unwrap_or(0)),
            change["ds_project"].as_str().unwrap_or("?"),
            change["name"].as_str().unwrap_or("?"),
            change["phase"].as_str().unwrap_or("?"),
            change["user"].as_str().unwrap_or("?"),
        ));
    }
    let more = &data["more"];
    if more["users"] == true || more["changes"] == true || more["timeline"] == true {
        out.push_str("  … more entries than the limit; raise --limit\n");
    }
    for line in data["not_claimed"].as_array().into_iter().flatten() {
        out.push_str(&format!("  · {}\n", line.as_str().unwrap_or("")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_read_is_offline_and_says_so_where_a_caller_reads_it() {
        assert_eq!(COMMAND.effect, Effect::ReadOnly);
        assert_eq!(COMMAND.authority, Authority::HeadlessUser);
        assert!(!COMMAND.effect.needs_confirmation());
        assert!(COMMAND.purpose.contains("it answers offline"));
        assert!(COMMAND.purpose.contains("no row carries a device"));
    }

    #[test]
    fn a_bucket_admits_a_capture_that_never_carried_a_status() {
        assert!(admits("active", None));
        assert!(admits("active", Some("")));
        assert!(!admits("active", Some("archived")));
        assert!(admits("archived", Some("archived")));
        assert!(admits("all", Some("testing")));
    }

    #[test]
    fn a_user_projection_narrows_the_lists_and_leaves_the_totals_whole() {
        let mut activities = json!({
            "totals": { "designed": 90, "actors": 3 },
            "users": [{ "name": "a@x" }, { "name": "b@x" }],
            "changes": [
                { "user": "a@x" },
                { "user": "b@x" },
                { "user": "c@x", "previous_user": "a@x" },
            ],
            "timeline": [{ "user": "b@x" }],
            "anomalies": [{ "user": "a@x" }, { "user": "b@x" }],
        });
        project_onto_user(&mut activities, "a@x");
        assert_eq!(activities["users"].as_array().expect("users").len(), 1);
        assert_eq!(activities["changes"].as_array().expect("changes").len(), 2);
        assert!(
            activities["timeline"]
                .as_array()
                .expect("timeline")
                .is_empty()
        );
        assert_eq!(
            activities["anomalies"].as_array().expect("anomalies").len(),
            1
        );
        assert_eq!(activities["totals"]["designed"], 90);
    }

    #[test]
    fn the_human_answer_carries_the_not_claimed_list_with_it() {
        let rendered = render(&json!({
            "captured": { "project_count": 2, "earliest_ms": 1_758_000_000_000i64,
                          "latest_ms": 1_758_000_000_000i64 },
            "totals": { "actors": 1, "transformers": 4, "designed": 2, "errors": 0 },
            "users": [], "anomalies": [], "changes": [], "more": {},
            "not_claimed": super::not_claimed(),
        }));
        assert!(rendered.contains("No device or installation attribution"));
        assert!(rendered.contains("Nothing between two captures"));
    }
}
