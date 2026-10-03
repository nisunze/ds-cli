//! `ds report project archives` — the published Compounded Report ZIPs.

use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use ds_command_kernel::report::ArchiveLayout;

use super::{LANE_ARG, PROJECT_ARG};

pub static COMMAND: Command = Command {
    id: "report.project.archives",
    path: &["report", "project", "archives"],
    contract: 3,
    summary: "List the named project's published Compounded Report ZIPs.",
    purpose: "\
Read the audience-fenced archive registry, newest first, with the server's JSON \
authoring template/schema. Recorded composition is null on older archives. \
Rows confirm achieved foldering. Project touch may first publish queued local \
reports and pull remote heads. No URL or action override.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG, PROJECT_ARG],
    output: "\
Lane/project, count and archives: identity, cloud locator, actor/time, status, \
transformer scope, group_count/groups (first-level labels) and grouping (kind \
plan|flat|recorded|legacy_district|unrecorded, key, plan identity), artifact \
coverage, errors, layout (recorded generic and legacy knobs plus kernel level) \
and composition. layout_collapsed reports unresolved foldering; download_url_expires_at, \
download_url_seconds_remaining and download_url_expired report URL validity. \
composition_template/schema are server-owned authoring objects, null when absent.",
    examples: &[Example {
        command: "ds report project archives --output json --project <exact-id>",
        note: "Check `.data.archives[0].download_url_seconds_remaining` before fetching it.",
        runnable: false,
    }],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let headless = ds_cli_auth::compounded_report_list_for_project(
        inputs.require("lane")?,
        inputs.require("project")?,
    )?;
    let now = unix_now();
    let archives = headless
        .result()
        .iter()
        .map(|archive| {
            let groups = archive.groups();
            let mut row = json!({
                "stem": archive.stem(),
                "filename": archive.filename(),
                "gcs_path": archive.gcs_path(),
                "download_url": archive.download_url(),
                "created_at": archive.created_at(),
                "created_by": archive.created_by(),
                "status": archive.status(),
                "transformer_count": archive.transformer_count(),
                "transformers": archive.transformers(),
                "individual_artifact_transformer_count": archive.individual_artifact_transformer_count(),
                "missing_individual_artifact_count": archive.missing_individual_artifact_count(),
                "errors": archive.errors(),
                "artifact_index_state": archive.artifact_index_state(),
                "archive_members": archive.archive_members(),
                "archive_layout": archive
                    .archive_layout()
                    .map(|layout| layout_fields(&layout.report_layout())),
                "composition": archive.composition(),
                "layout_collapsed": archive.archive_layout().map(|layout| layout_collapsed(
                    &layout.report_layout(),
                    groups.filed_no_group(),
                    archive.transformer_count(),
                )),
            });
            let object = row.as_object_mut().expect("row is an object");
            object.extend(
                grouping_fields(groups)
                    .as_object()
                    .expect("grouping is an object")
                    .clone(),
            );
            object.extend(
                    download_validity(archive.download_url(), now)
                        .as_object()
                        .expect("validity is an object")
                        .clone(),
                );
            row
        })
        .collect::<Vec<_>>();
    let mut output = super::project_receipt(&headless);
    output["count"] = json!(archives.len());
    output["archives"] = Value::Array(archives);
    output["composition_template"] = json!(headless.result().composition_template());
    output["composition_schema"] = json!(headless.result().composition_schema());
    Ok(output)
}

/// The kernel's reading of the archive's grouping: ds-brain's generic groups
/// and plan identity, or the pre-plan district pair on an older archive —
/// never both, never re-decided here.
fn grouping_fields(groups: &ds_cli_auth::ArchiveGroups) -> Value {
    json!({
        "group_count": groups.count,
        "groups": groups.labels,
        "grouping": {
            "kind": groups.kind,
            "key": groups.key,
            "plan": groups.plan,
            "detail": groups.detail,
        },
    })
}

/// The one thing a row can say about the tree that was actually built: a run
/// that asked for group folders and resolved no group filed every artifact
/// under `_unassigned/`. The recorded layout is the request; this is its
/// outcome, and the two are not the same claim. A flat archive (no plan
/// applied) asked for no groups, which the kernel's `filed_no_group` already
/// tells apart.
fn layout_collapsed(layout: &ArchiveLayout, filed_no_group: bool, transformer_count: u64) -> bool {
    let combine =
        layout.combine_per_group == Some(true) || layout.combine_per_district == Some(true);
    let foldering_requested = combine || !matches!(layout.level(), "root" | "flat");
    foldering_requested && filed_no_group && transformer_count > 0
}

/// The layout as the registry recorded it — the generic knobs ds-brain writes
/// (`group_depth`, `transformer_folders`, `combine_per_group`) and the legacy
/// spellings it keeps beside them — plus the report layer's own vocabulary
/// for it, resolved generic first. A legacy archive written under
/// `transformer_grouping` alone is described exactly as the application
/// describes it, instead of as no layout.
fn layout_fields(layout: &ArchiveLayout) -> Value {
    let mut fields = json!({
        "file_level": layout.file_level,
        "transformer_grouping": layout.transformer_grouping,
        "combine_per_district": layout.combine_per_district == Some(true),
        "group_depth": layout.group_depth,
        "transformer_folders": layout.transformer_folders,
        "combine_per_group": layout.combine_per_group,
    });
    fields.as_object_mut().expect("layout is an object").extend(
        layout
            .describe()
            .as_object()
            .expect("vocabulary is an object")
            .clone(),
    );
    fields
}

/// The three derived fields that let a caller judge a signed download before
/// spending a request on it. Every one is null when the URL carries no
/// readable expiry: a listing must not fail on a signature it cannot read,
/// and suppressing or rewriting the URL would be a policy this command does
/// not own.
fn download_validity(url: Option<&str>, now: u64) -> Value {
    let expires_at = url.and_then(signed_url_expiry);
    json!({
        "download_url_expires_at": expires_at.map(rfc3339_utc),
        "download_url_seconds_remaining": expires_at.map(|at| at.saturating_sub(now)),
        "download_url_expired": expires_at.map(|at| at <= now),
    })
}

/// The expiry a signed download carries in its own query string, in epoch
/// seconds: GCS V2 `Expires=<epoch>`, which is what the service mints, then
/// V4 `X-Goog-Date` + `X-Goog-Expires` as the fallback. `None` when neither
/// is present or parseable.
fn signed_url_expiry(url: &str) -> Option<u64> {
    let (_, query) = url.split_once('?')?;
    let mut expires = None;
    let mut signed_at = None;
    let mut lifetime = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "Expires" => expires = value.parse::<u64>().ok(),
            "X-Goog-Date" => signed_at = epoch_from_basic_utc(value),
            "X-Goog-Expires" => lifetime = value.parse::<u64>().ok(),
            _ => {}
        }
    }
    match (expires, signed_at, lifetime) {
        (Some(at), _, _) => Some(at),
        (None, Some(at), Some(seconds)) => Some(at.saturating_add(seconds)),
        _ => None,
    }
}

/// `YYYYMMDDTHHMMSSZ`, the only stamp V4 signing writes, to epoch seconds.
fn epoch_from_basic_utc(stamp: &str) -> Option<u64> {
    let bytes = stamp.as_bytes();
    if bytes.len() != 16 || !stamp.is_ascii() || bytes[8] != b'T' || bytes[15] != b'Z' {
        return None;
    }
    let year = stamp[0..4].parse::<i64>().ok()?;
    let month = stamp[4..6].parse::<i64>().ok()?;
    let day = stamp[6..8].parse::<i64>().ok()?;
    let hour = stamp[9..11].parse::<i64>().ok()?;
    let minute = stamp[11..13].parse::<i64>().ok()?;
    let second = stamp[13..15].parse::<i64>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Hinnant's days-from-civil
/// algorithm), so no date dependency enters this crate for one query string.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse, for printing one epoch second as a UTC instant an operator
/// can compare with `created_at`.
fn rfc3339_utc(epoch_seconds: u64) -> String {
    let days = (epoch_seconds / 86_400) as i64;
    let rest = epoch_seconds % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_position + 2) / 5 + 1;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest / 60) % 60,
        rest % 60,
    )
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · {} archive(s)\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["count"].as_u64().unwrap_or(0),
    );
    if let Some(archives) = data["archives"].as_array() {
        for archive in archives {
            // The level the kernel resolved, generic knobs first.
            let level = archive["archive_layout"]["level"]
                .as_str()
                .unwrap_or("unrecorded");
            let noun = group_noun(archive);
            out.push_str(&format!(
                "  {:<34} {:<8} {} transformer(s) · {} layout · {} {noun}(s){} · {}{}\n",
                archive["stem"].as_str().unwrap_or("?"),
                archive["status"].as_str().unwrap_or("?"),
                archive["transformer_count"].as_u64().unwrap_or(0),
                level,
                archive["group_count"].as_u64().unwrap_or(0),
                grouping_note(archive),
                archive["created_at"].as_str().unwrap_or("?"),
                download_note(archive),
            ));
            if archive["layout_collapsed"].as_bool().unwrap_or(false) {
                let requested = if matches!(level, "root" | "flat") {
                    "district"
                } else {
                    level
                };
                let folder = if matches!(requested, "sector" | "district_sector" | "transformer") {
                    "_unassigned/_unassigned/"
                } else {
                    "_unassigned/"
                };
                out.push_str(&format!(
                    "    {requested} foldering requested · 0 {noun}(s) — every artifact filed under {folder}\n",
                ));
            }
        }
    }
    out
}

/// What one row's groups are called: districts on a pre-plan archive, groups
/// otherwise.
fn group_noun(archive: &Value) -> &'static str {
    if archive["grouping"]["kind"] == "legacy_district" {
        "district"
    } else {
        "group"
    }
}

/// The grouping key, or that no plan was applied — so a flat archive never
/// reads as a collapsed one.
fn grouping_note(archive: &Value) -> String {
    let grouping = &archive["grouping"];
    match grouping["kind"].as_str() {
        Some("flat") => " (no plan applied, flat)".to_owned(),
        Some("plan") => grouping["key"]
            .as_str()
            .map(|key| format!(" by {key}"))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// What is left of one signed download, for the operator who never asks for
/// `--output json`.
fn download_note(archive: &Value) -> String {
    if archive["download_url"].is_null() {
        return String::new();
    }
    if archive["download_url_expired"].as_bool().unwrap_or(false) {
        return " · download expired".to_owned();
    }
    match archive["download_url_seconds_remaining"].as_u64() {
        Some(seconds) => format!(" · download expires in {seconds}s"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_district_projection_uses_the_native_group_contract() {
        use ds_command_kernel::report::archive_groups::{ArchiveGroupingFields, decode};
        let legacy = decode(
            &serde_json::from_value::<ArchiveGroupingFields>(json!({
                "district_count":1,"districts":["Nyamagabe"]
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            district_scope(&legacy),
            (1, ["Nyamagabe".to_string()].as_slice())
        );
        let generic = decode(
            &serde_json::from_value::<ArchiveGroupingFields>(json!({
                "group_count":1,"groups":["Sector A"],
                "district_count":1,"districts":["stale legacy value"]
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(district_scope(&generic), (0, [].as_slice()));
    }

    const HOUR: u64 = 3_600;

    #[test]
    fn v2_expires_is_the_url_s_own_expiry() {
        let url = "https://storage.googleapis.com/b/o.zip?GoogleAccessId=a&Expires=1788696000&Signature=x";
        assert_eq!(signed_url_expiry(url), Some(1_788_696_000));
        let validity = download_validity(Some(url), 1_788_696_000 - 20);
        assert_eq!(
            validity["download_url_expires_at"],
            json!("2026-09-06T12:00:00Z")
        );
        assert_eq!(validity["download_url_seconds_remaining"], json!(20));
        assert_eq!(validity["download_url_expired"], json!(false));
    }

    #[test]
    fn v4_signing_date_and_lifetime_are_the_fallback() {
        let url = "https://storage.googleapis.com/b/o.zip?X-Goog-Algorithm=GOOG4-RSA-SHA256\
                   &X-Goog-Date=20260906T110000Z&X-Goog-Expires=3600&X-Goog-Signature=x";
        let expires_at = signed_url_expiry(url).expect("a V4 URL carries its own expiry");
        assert_eq!(rfc3339_utc(expires_at), "2026-09-06T12:00:00Z");
        let validity = download_validity(Some(url), expires_at - HOUR);
        assert_eq!(validity["download_url_seconds_remaining"], json!(HOUR));
        assert_eq!(validity["download_url_expired"], json!(false));
    }

    #[test]
    fn a_spent_signature_reports_zero_and_expired() {
        let url = "https://storage.googleapis.com/b/o.zip?Expires=1788696000&Signature=x";
        let validity = download_validity(Some(url), 1_788_696_000 + 5);
        assert_eq!(validity["download_url_seconds_remaining"], json!(0));
        assert_eq!(validity["download_url_expired"], json!(true));
    }

    #[test]
    fn a_url_without_expiry_params_derives_nothing() {
        for url in [
            "https://storage.googleapis.com/b/o.zip",
            "https://storage.googleapis.com/b/o.zip?alt=media",
            "https://storage.googleapis.com/b/o.zip?X-Goog-Date=20260906T110000Z",
        ] {
            assert_eq!(signed_url_expiry(url), None, "{url}");
            let validity = download_validity(Some(url), 1_788_696_000);
            assert_eq!(validity["download_url_expires_at"], Value::Null, "{url}");
            assert_eq!(
                validity["download_url_seconds_remaining"],
                Value::Null,
                "{url}"
            );
            assert_eq!(validity["download_url_expired"], Value::Null, "{url}");
        }
        let absent = download_validity(None, 1_788_696_000);
        assert_eq!(absent["download_url_expires_at"], Value::Null);
    }

    #[test]
    fn a_garbage_expiry_is_unreadable_not_fatal() {
        for url in [
            "https://storage.googleapis.com/b/o.zip?Expires=soon",
            "https://storage.googleapis.com/b/o.zip?Expires=-1",
            "https://storage.googleapis.com/b/o.zip?Expires=99999999999999999999999",
            "https://storage.googleapis.com/b/o.zip?X-Goog-Date=yesterday&X-Goog-Expires=3600",
            "https://storage.googleapis.com/b/o.zip?X-Goog-Date=20261306T110000Z&X-Goog-Expires=3600",
            "https://storage.googleapis.com/b/o.zip?Expires",
        ] {
            assert_eq!(signed_url_expiry(url), None, "{url}");
        }
    }

    fn layout(value: Value) -> ArchiveLayout {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn requested_foldering_with_no_group_is_collapsed() {
        let sector = layout(json!({"file_level": "sector"}));
        // The D4 archive: `--file-level sector` over 195 transformers, and a
        // manifest whose whole tree is `_unassigned/_unassigned/`.
        assert!(layout_collapsed(&sector, true, 195));
        assert!(layout_collapsed(
            &layout(json!({"file_level": "district"})),
            true,
            195
        ));
        assert!(layout_collapsed(
            &layout(json!({"file_level": "root", "combine_per_district": true})),
            true,
            195
        ));
        // The generic knobs ask for group folders the same way.
        assert!(layout_collapsed(
            &layout(json!({"group_depth": 1, "transformer_folders": false})),
            true,
            195
        ));
        assert!(layout_collapsed(
            &layout(json!({"group_depth": 0, "combine_per_group": true})),
            true,
            195
        ));
        // Groups resolved, nothing was asked for, or nothing was filed.
        assert!(!layout_collapsed(&sector, false, 195));
        assert!(!layout_collapsed(
            &layout(json!({"file_level": "root"})),
            true,
            195
        ));
        assert!(!layout_collapsed(
            &layout(json!({"group_depth": 0, "transformer_folders": true})),
            true,
            195
        ));
        assert!(!layout_collapsed(&sector, true, 0));
    }

    /// The layout knobs ds-brain writes are recorded on the row and resolve
    /// the level before the legacy words kept beside them.
    #[test]
    fn the_row_records_and_describes_the_generic_layout() {
        let fields = layout_fields(&layout(json!({
            "group_depth": 1, "transformer_folders": false,
            "combine_per_group": true, "combine_per_district": true,
            "file_level": "transformer", "transformer_grouping": "district_sector",
        })));
        assert_eq!(fields["group_depth"], 1);
        assert_eq!(fields["transformer_folders"], false);
        assert_eq!(fields["combine_per_group"], true);
        assert_eq!(fields["file_level"], "transformer");
        assert_eq!(fields["level"], "district");
        assert_eq!(fields["level_key"], "pctl_layout_district");
        assert_eq!(fields["label_key"], "pctl_layout_per_district");
        // A legacy-only layout records no generic knob and keeps its reading.
        let legacy = layout_fields(&layout(json!({"transformer_grouping": "district_sector"})));
        assert_eq!(legacy["group_depth"], Value::Null);
        assert_eq!(legacy["transformer_folders"], Value::Null);
        assert_eq!(legacy["level"], "district_sector");
    }

    /// The registry rows ds-brain writes (a plan-grouped and a flat archive)
    /// and a pre-plan one, decoded by the kernel exactly as the native client
    /// does, then shaped and rendered by this command.
    #[test]
    fn brain_generic_groups_and_legacy_districts_reach_the_row_and_render() {
        let doc: Value = serde_json::from_str(include_str!(
            "../../../../../ds-command-kernel/tests/fixtures/compounded-archive-groups.json"
        ))
        .unwrap();
        let rows: Vec<Value> = doc["compounded_reports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let groups = ds_command_kernel::report::archive_groups::decode(
                    &serde_json::from_value(entry.clone()).unwrap(),
                )
                .unwrap();
                let mut row = grouping_fields(&groups);
                for key in ["stem", "status", "transformer_count", "created_at"] {
                    row[key] = entry[key].clone();
                }
                let recorded = layout(entry["archive_layout"].clone());
                row["archive_layout"] = layout_fields(&recorded);
                row["download_url"] = Value::Null;
                row["layout_collapsed"] = json!(layout_collapsed(
                    &recorded,
                    groups.filed_no_group(),
                    entry["transformer_count"].as_u64().unwrap(),
                ));
                row
            })
            .collect();
        assert_eq!(rows[0]["group_count"], 2);
        assert_eq!(rows[0]["groups"], json!(["Huye", "Nyamagabe"]));
        assert_eq!(rows[0]["grouping"]["kind"], "plan");
        assert_eq!(rows[0]["grouping"]["key"], "loc_admin_level_2");
        assert_eq!(rows[0]["grouping"]["plan"]["revision"], 3);
        assert_eq!(rows[1]["grouping"]["kind"], "flat");
        // A flat archive asked for no groups: it is not a collapsed one.
        assert_eq!(rows[1]["layout_collapsed"], false);
        assert_eq!(rows[2]["grouping"]["kind"], "legacy_district");
        assert_eq!(rows[2]["group_count"], 3);
        assert!(rows.iter().all(|row| row.get("district_count").is_none()));

        let rendered = render(&json!({
            "lane": "stable",
            "project": {"project_name": "Huye 2", "ds_project": "huye-2"},
            "count": 3,
            "archives": rows,
        }));
        assert!(
            rendered.contains("transformer layout · 2 group(s) by loc_admin_level_2"),
            "{rendered}"
        );
        assert!(
            rendered.contains("0 group(s) (no plan applied, flat)"),
            "{rendered}"
        );
        assert!(rendered.contains("3 district(s) ·"), "{rendered}");
        assert!(!rendered.contains("foldering requested"), "{rendered}");
    }

    #[test]
    fn render_shows_the_achieved_foldering_and_what_is_left_of_the_download() {
        let rendered = render(&json!({
            "lane": "stable",
            "project": {"project_name": "Aderm", "ds_project": "aderm"},
            "count": 1,
            "archives": [{
                "stem": "aderm-2026-09-06",
                "status": "success",
                "transformer_count": 195,
                "group_count": 0,
                "grouping": {"kind": "plan", "key": "loc_admin_level_2"},
                "created_at": "2026-09-06T11:00:00Z",
                "download_url": "https://storage.googleapis.com/b/o.zip?Expires=1788696000",
                "download_url_seconds_remaining": 20,
                "download_url_expired": false,
                "archive_layout": {"file_level": "sector", "level": "sector", "combine_per_district": false},
                "layout_collapsed": true,
            }],
        }));
        assert!(
            rendered.contains("sector layout · 0 group(s) by loc_admin_level_2"),
            "{rendered}"
        );
        assert!(rendered.contains("download expires in 20s"), "{rendered}");
        assert!(
            rendered.contains(
                "sector foldering requested · 0 group(s) — every artifact filed under _unassigned/_unassigned/"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn render_stays_quiet_when_the_layout_held_and_the_row_is_legacy() {
        let rendered = render(&json!({
            "lane": "stable",
            "project": {"project_name": "Aderm", "ds_project": "aderm"},
            "count": 2,
            "archives": [
                {
                    "stem": "held",
                    "status": "success",
                    "transformer_count": 12,
                    "group_count": 3,
                    "grouping": {"kind": "legacy_district", "key": "district"},
                    "created_at": "2026-09-06T11:00:00Z",
                    "download_url": "https://storage.googleapis.com/b/o.zip?Expires=1788696000",
                    "download_url_expired": true,
                    "download_url_seconds_remaining": 0,
                    "archive_layout": {"file_level": "district", "level": "district", "combine_per_district": false},
                    "layout_collapsed": false,
                },
                {
                    "stem": "legacy",
                    "status": "success",
                    "transformer_count": 4,
                    "group_count": 0,
                    "grouping": {"kind": "unrecorded"},
                    "created_at": "2026-08-01T09:00:00Z",
                    "download_url": Value::Null,
                    "archive_layout": Value::Null,
                    "layout_collapsed": Value::Null,
                },
            ],
        }));
        assert!(!rendered.contains("foldering requested"), "{rendered}");
        assert!(rendered.contains("download expired"), "{rendered}");
        assert!(rendered.contains("unrecorded layout"), "{rendered}");
    }
}
