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
    "Show only this actor's rows, by account or short name; totals stay project-wide.",
);

const SINCE_ARG: Arg = Arg::value(
    "since",
    "<epoch-ms>",
    "Diff against the newest capture taken at or before this instant; none is refused, not zero.",
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
    super::SINCE_NO_BASELINE,
    super::UNKNOWN_ACTOR,
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
`anomalies`, `facts` and `more` — the kernel's cross-project model, labels \
as i18n keys and times as epoch millis. `sources` names the two captures \
read per project, `unreadable` any it refused, `not_claimed` what none of \
it shows.",
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
    search: &["audit", "licence", "license", "unlicensed", "usage", "who"],
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
    // A `--since` window with nothing retained at or before it has no
    // baseline, and a fold over one photograph reports `changes: 0`. That
    // number is indistinguishable from "nobody did anything", which is the
    // one sentence this command must never say by accident.
    let mut without_baseline: Vec<Value> = Vec::new();
    // A capture the kernel will not admit is a named row, never an abort: one
    // damaged file out of thirty must not cost the operator the other
    // twenty-nine, and it must not vanish either.
    let mut unreadable: Vec<Value> = Vec::new();
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
            Some(since) => baseline_before(captures, since, latest),
            None => selected["previous"].as_i64(),
        };
        let no_baseline = since.is_some() && previous.is_none();

        let snapshot = match read_capture(&account.join(project).join(format!("{latest:013}.json")))
        {
            Ok(snapshot) => snapshot,
            Err(failure) => {
                unreadable.push(json!({
                    "ds_project": project,
                    "captured_at_ms": latest,
                    "code": failure.code(),
                    "reason": failure.message(),
                }));
                continue;
            }
        };
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
        // The pair is what makes `changes` mean anything, so a refused older
        // capture is named too and the answer holds one photograph and no
        // diff, rather than a diff against something the kernel refused.
        let mut compared = previous;
        if let Some(previous) = previous {
            match read_capture(&account.join(project).join(format!("{previous:013}.json"))) {
                Ok(older) => {
                    entry.insert("previous".into(), older["ledger"].clone());
                }
                Err(failure) => {
                    compared = None;
                    unreadable.push(json!({
                        "ds_project": project,
                        "captured_at_ms": previous,
                        "code": failure.code(),
                        "reason": failure.message(),
                    }));
                }
            }
        }
        projects.push(Value::Object(entry));
        if no_baseline {
            without_baseline.push(json!({
                "ds_project": project,
                "earliest_ms": captures.first().copied(),
                "latest_ms": latest,
            }));
        }
        sources.push(json!({
            "ds_project": project,
            "latest_ms": latest,
            "previous_ms": compared,
            "retained": captures.len(),
        }));
    }

    if projects.is_empty() {
        // Empty and damaged are different answers, and a remedy that says
        // "sweep" when the real trouble is an unreadable file sends the
        // operator the wrong way.
        if !unreadable.is_empty() {
            let named: Vec<String> = unreadable
                .iter()
                .map(|entry| {
                    format!(
                        "{}@{}",
                        entry["ds_project"].as_str().unwrap_or("?"),
                        entry["captured_at_ms"].as_i64().unwrap_or(0)
                    )
                })
                .collect();
            return Err(Failure::invalid(
                super::SNAPSHOT_INVALID.code,
                format!(
                    "every retained capture this request names is inadmissible: {}",
                    named.join(", ")
                ),
            )
            .remedy(super::SNAPSHOT_INVALID.remedy)
            .next("ds design activities sweep --limit 3 --yes"));
        }
        return Err(Failure::invalid(
            super::STORE_EMPTY.code,
            "no retained capture matches this lane, account, bucket and project selection",
        )
        .remedy(super::STORE_EMPTY.remedy)
        .next("ds design activities sweep --limit 3 --yes"));
    }

    // Some projects without a baseline is a named gap beside a real answer.
    // Every project without one is not an answer at all: there is nothing to
    // diff, and `changes: 0` would be a statement about the store rather than
    // about the people this command reports on.
    if let Some(since) = since
        && !without_baseline.is_empty()
        && without_baseline.len() == projects.len()
    {
        return Err(no_baseline_anywhere(since, &without_baseline));
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
        project_onto_user(&mut activities, &user)?;
    }
    let mut out = activities.as_object().cloned().unwrap_or_default();
    out.insert("lane".into(), json!(lane));
    out.insert("bucket".into(), json!(bucket));
    out.insert("store".into(), json!(account.to_string_lossy()));
    out.insert("sources".into(), json!(sources));
    out.insert("unreadable".into(), json!(unreadable));
    if !user.is_empty() {
        out.insert("filtered_by_user".into(), json!(user));
    }
    if let Some(since) = since {
        out.insert("since_ms".into(), json!(since));
        out.insert("since_no_baseline".into(), json!(without_baseline));
    }
    out.insert("not_claimed".into(), not_claimed());
    Ok(Value::Object(out))
}

/// The capture a `--since` window diffs against: the newest one taken at or
/// before that instant, and never the latest itself.
///
/// `None` is the honest answer when every retained capture was taken after
/// the instant asked for — the window has no floor, and the caller must be
/// told so rather than handed a fold over one photograph.
fn baseline_before(captures: &[i64], since: i64, latest: i64) -> Option<i64> {
    captures
        .iter()
        .copied()
        .rfind(|capture| *capture <= since && *capture < latest)
}

/// Not one selected project has a capture at or before `--since`.
fn no_baseline_anywhere(since: i64, without_baseline: &[Value]) -> Failure {
    let named: Vec<String> = without_baseline
        .iter()
        .map(|entry| {
            format!(
                "{}@{}",
                entry["ds_project"].as_str().unwrap_or("?"),
                spell_ms(entry["earliest_ms"].as_i64().unwrap_or(0))
            )
        })
        .collect();
    Failure::invalid(
        super::SINCE_NO_BASELINE.code,
        format!(
            "--since {since} falls before every capture retained for {}, so there is no \
             baseline to diff against and no window this store can answer for",
            named.join(", ")
        ),
    )
    .remedy(super::SINCE_NO_BASELINE.remedy)
    .detail(json!({ "since_ms": since, "projects": without_baseline }))
    .next("ds design activities read --output json")
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
///
/// The name is matched the way the reply prints it: an actor is its account
/// and its short name, compared without case, because a filter that rejects
/// the very spelling this command just printed is a filter nobody can use.
/// An actor the fold has never seen is a refusal and not an empty list — a
/// zero for a person who does not exist reads exactly like a zero for a
/// person who did nothing.
fn project_onto_user(activities: &mut Value, user: &str) -> Result<(), Failure> {
    let wanted = user.trim().to_lowercase();
    let is_wanted = |value: &Value| {
        value
            .as_str()
            .is_some_and(|actor| actor.trim().to_lowercase() == wanted)
    };
    let names_wanted = |entry: &Value| is_wanted(&entry["name"]) || is_wanted(&entry["short"]);
    let stamped_by_wanted = |entry: &Value| {
        is_wanted(&entry["user"])
            || is_wanted(&entry["short"])
            || is_wanted(&entry["previous_user"])
    };

    let known: Vec<Value> = activities["users"].as_array().cloned().unwrap_or_default();
    if !known.iter().any(names_wanted) {
        return Err(unknown_actor(user, &known));
    }

    let kept: Vec<Value> = known
        .iter()
        .filter(|entry| names_wanted(entry))
        .cloned()
        .collect();
    activities["users"] = json!(kept);
    for list in ["changes", "timeline"] {
        if let Some(entries) = activities[list].as_array() {
            let kept: Vec<Value> = entries
                .iter()
                .filter(|entry| stamped_by_wanted(entry))
                .cloned()
                .collect();
            activities[list] = json!(kept);
        }
    }
    if let Some(anomalies) = activities["anomalies"].as_array() {
        let kept: Vec<Value> = anomalies
            .iter()
            .filter(|entry| is_wanted(&entry["user"]) || is_wanted(&entry["short"]))
            .cloned()
            .collect();
        activities["anomalies"] = json!(kept);
    }
    Ok(())
}

/// `--user` named somebody the retained captures do not hold.
fn unknown_actor(user: &str, known: &[Value]) -> Failure {
    let mut spelled: Vec<String> = known
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    spelled.sort();
    spelled.dedup();
    let names = if spelled.is_empty() {
        "no actor at all".to_owned()
    } else {
        spelled.join(", ")
    };
    Failure::invalid(
        super::UNKNOWN_ACTOR.code,
        format!("no retained capture holds `{user}`; these captures hold {names}"),
    )
    .remedy(super::UNKNOWN_ACTOR.remedy)
    .detail(json!({ "requested": user, "actors": spelled }))
    .next("ds design activities read --output json")
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
            "  {:<32} {:>4} designed · {:>3} errors · {} projects · last stamp {}\n",
            user["name"].as_str().unwrap_or("?"),
            user["designs"].as_u64().unwrap_or(0),
            user["errors"].as_u64().unwrap_or(0),
            user["project_count"].as_u64().unwrap_or(0),
            spell_ms(user["last_seen_ms"].as_i64().unwrap_or(0)),
        ));
    }
    for refused in data["unreadable"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  ! unreadable {} @{} — {}\n",
            refused["ds_project"].as_str().unwrap_or("?"),
            spell_ms(refused["captured_at_ms"].as_i64().unwrap_or(0)),
            refused["reason"].as_str().unwrap_or(""),
        ));
    }
    for gap in data["since_no_baseline"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  ! no baseline {} — every retained capture is later than --since (earliest {})\n",
            gap["ds_project"].as_str().unwrap_or("?"),
            spell_ms(gap["earliest_ms"].as_i64().unwrap_or(0)),
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

    /// Both ways this read can be silently wrong are published, so a caller
    /// can plan for them from `--help` rather than from a zero.
    #[test]
    fn the_two_confident_zeroes_are_declared_refusals() {
        let codes: Vec<&str> = COMMAND
            .refusals
            .iter()
            .map(|refusal| refusal.code)
            .collect();
        assert!(
            codes.contains(&super::super::SINCE_NO_BASELINE.code),
            "{codes:?}"
        );
        assert!(
            codes.contains(&super::super::UNKNOWN_ACTOR.code),
            "{codes:?}"
        );
    }

    #[test]
    fn a_since_before_every_capture_has_no_baseline_to_diff_against() {
        // Two captures, both later than the instant asked for: there is no
        // floor for the window, and the fold would run on one photograph.
        assert_eq!(baseline_before(&[200, 300], 100, 300), None);
        assert_eq!(baseline_before(&[200, 300], 250, 300), Some(200));
        assert_eq!(baseline_before(&[200, 300], 300, 300), Some(200));
        // The latest capture is never its own baseline.
        assert_eq!(baseline_before(&[300], 400, 300), None);
    }

    #[test]
    fn no_baseline_anywhere_names_the_projects_rather_than_reporting_zero() {
        let failure = no_baseline_anywhere(
            1_700_000_000_000,
            &[json!({ "ds_project": "p_one", "earliest_ms": 1_758_000_000_000i64 })],
        );
        assert_eq!(failure.code(), super::super::SINCE_NO_BASELINE.code);
        assert!(failure.message().contains("p_one"), "{}", failure.message());
        assert_eq!(
            failure.remedy_text(),
            Some(super::super::SINCE_NO_BASELINE.remedy)
        );
    }

    #[test]
    fn a_user_filter_accepts_the_short_name_this_command_prints() {
        let mut activities = json!({
            "totals": { "designed": 90 },
            "users": [{ "name": "Nisunze@Example.com", "short": "nisunze" }, { "name": "b@x", "short": "b" }],
            "changes": [{ "user": "Nisunze@Example.com", "short": "nisunze" }, { "user": "b@x" }],
            "timeline": [],
            "anomalies": [],
        });
        project_onto_user(&mut activities, "nisunze").expect("the short name is an actor");
        assert_eq!(activities["users"].as_array().expect("users").len(), 1);
        assert_eq!(activities["changes"].as_array().expect("changes").len(), 1);

        let mut upper = json!({
            "users": [{ "name": "nisunze@example.com", "short": "nisunze" }],
            "changes": [], "timeline": [], "anomalies": [],
        });
        project_onto_user(&mut upper, "NISUNZE@EXAMPLE.COM").expect("case is not identity");
        assert_eq!(upper["users"].as_array().expect("users").len(), 1);
    }

    #[test]
    fn an_actor_no_capture_holds_is_refused_rather_than_answered_with_zero() {
        let mut activities = json!({
            "users": [{ "name": "a@x", "short": "a" }],
            "changes": [], "timeline": [], "anomalies": [],
        });
        let failure = project_onto_user(&mut activities, "nobody@example.com")
            .expect_err("an unknown actor is a refusal");
        assert_eq!(failure.code(), super::super::UNKNOWN_ACTOR.code);
        assert!(failure.message().contains("a@x"), "{}", failure.message());
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
        project_onto_user(&mut activities, "a@x").expect("a@x is an actor");
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
    fn an_inadmissible_capture_is_a_named_row_in_the_human_answer() {
        let rendered = render(&json!({
            "captured": { "project_count": 1, "earliest_ms": 1_758_000_000_000i64,
                          "latest_ms": 1_758_000_000_000i64 },
            "totals": { "actors": 1, "transformers": 4, "designed": 2, "errors": 0 },
            "users": [], "anomalies": [], "changes": [], "more": {},
            "unreadable": [{ "ds_project": "p_two", "captured_at_ms": 1_758_000_000_000i64,
                             "code": "snapshot_invalid", "reason": "snapshot.digest" }],
            "not_claimed": super::not_claimed(),
        }));
        assert!(rendered.contains("unreadable p_two"), "{rendered}");
        assert!(rendered.contains("snapshot.digest"), "{rendered}");
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
