//! `ds report project archives` — the published compounded archives.

use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::LANE_ARG;

pub static COMMAND: Command = Command {
    id: "report.project.archives",
    path: &["report", "project", "archives"],
    contract: 1,
    summary: "List the selected project's published compounded archives.",
    purpose: "\
Restores the native user and reads only its audience-fenced selected \
project's compounded archive registry, newest first, through the fixed list \
call. Each row is the durable record of one `compounded` run and the only \
place its achieved foldering is confirmed. No project, URL, body or action \
override is accepted.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG],
    output: "\
Lane and selected-project identity/status, `count`, and `archives`: stem, \
filename, cloud locator, creation actor and time, status, transformer and \
district counts and names, artifact coverage, bounded errors, the requested \
layout, and two derivations — `layout_collapsed` (folders asked for, no \
district resolved, everything filed under `_unassigned/`) and the signed \
`download_url`'s own expiry, since it is short-lived and can arrive nearly \
spent: `download_url_expires_at`, `download_url_seconds_remaining`, \
`download_url_expired`.",
    examples: &[Example {
        command: "ds report project archives --output json",
        note: "Check `.data.archives[0].download_url_seconds_remaining` before fetching it.",
        runnable: false,
    }],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let headless = ds_cli_auth::compounded_report_list(inputs.require("lane")?)?;
    let now = unix_now();
    let archives = headless
        .result()
        .iter()
        .map(|archive| {
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
                "district_count": archive.district_count(),
                "districts": archive.districts(),
                "individual_artifact_transformer_count": archive.individual_artifact_transformer_count(),
                "missing_individual_artifact_count": archive.missing_individual_artifact_count(),
                "errors": archive.errors(),
                "archive_layout": archive.archive_layout().map(|layout| json!({
                    "file_level": layout.file_level(),
                    "combine_per_district": layout.combine_per_district(),
                })),
                "layout_collapsed": archive.archive_layout().map(|layout| layout_collapsed(
                    layout.file_level(),
                    layout.combine_per_district(),
                    archive.district_count(),
                    archive.transformer_count(),
                )),
            });
            row.as_object_mut()
                .expect("row is an object")
                .extend(
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
    Ok(output)
}

/// The one thing a row can say about the tree that was actually built: a run
/// that asked for district or sector folders and resolved no district filed
/// every artifact under `_unassigned/`. The recorded layout is the request;
/// this is its outcome, and the two are not the same claim.
fn layout_collapsed(
    file_level: Option<&str>,
    combine_per_district: bool,
    district_count: u64,
    transformer_count: u64,
) -> bool {
    let foldering_requested =
        combine_per_district || matches!(file_level, Some(level) if level != "root");
    foldering_requested && district_count == 0 && transformer_count > 0
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
            let file_level = archive["archive_layout"]["file_level"]
                .as_str()
                .unwrap_or("unrecorded");
            out.push_str(&format!(
                "  {:<34} {:<8} {} transformer(s) · {} layout · {} district(s) · {}{}\n",
                archive["stem"].as_str().unwrap_or("?"),
                archive["status"].as_str().unwrap_or("?"),
                archive["transformer_count"].as_u64().unwrap_or(0),
                file_level,
                archive["district_count"].as_u64().unwrap_or(0),
                archive["created_at"].as_str().unwrap_or("?"),
                download_note(archive),
            ));
            if archive["layout_collapsed"].as_bool().unwrap_or(false) {
                let requested = if file_level == "root" {
                    "district"
                } else {
                    file_level
                };
                let folder = if matches!(requested, "sector" | "transformer") {
                    "_unassigned/_unassigned/"
                } else {
                    "_unassigned/"
                };
                out.push_str(&format!(
                    "    {requested} foldering requested · 0 district(s) — every artifact filed under {folder}\n",
                ));
            }
        }
    }
    out
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

    #[test]
    fn requested_foldering_with_no_district_is_collapsed() {
        // The D4 archive: `--file-level sector` over 195 transformers, and a
        // manifest whose whole tree is `_unassigned/_unassigned/`.
        assert!(layout_collapsed(Some("sector"), false, 0, 195));
        assert!(layout_collapsed(Some("district"), false, 0, 195));
        assert!(layout_collapsed(Some("root"), true, 0, 195));
        // Districts resolved, nothing was asked for, or nothing was filed.
        assert!(!layout_collapsed(Some("sector"), false, 7, 195));
        assert!(!layout_collapsed(Some("root"), false, 0, 195));
        assert!(!layout_collapsed(None, false, 0, 195));
        assert!(!layout_collapsed(Some("sector"), false, 0, 0));
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
                "district_count": 0,
                "created_at": "2026-09-06T11:00:00Z",
                "download_url": "https://storage.googleapis.com/b/o.zip?Expires=1788696000",
                "download_url_seconds_remaining": 20,
                "download_url_expired": false,
                "archive_layout": {"file_level": "sector", "combine_per_district": false},
                "layout_collapsed": true,
            }],
        }));
        assert!(
            rendered.contains("sector layout · 0 district(s)"),
            "{rendered}"
        );
        assert!(rendered.contains("download expires in 20s"), "{rendered}");
        assert!(
            rendered.contains(
                "sector foldering requested · 0 district(s) — every artifact filed under _unassigned/_unassigned/"
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
                    "district_count": 3,
                    "created_at": "2026-09-06T11:00:00Z",
                    "download_url": "https://storage.googleapis.com/b/o.zip?Expires=1788696000",
                    "download_url_expired": true,
                    "download_url_seconds_remaining": 0,
                    "archive_layout": {"file_level": "district", "combine_per_district": false},
                    "layout_collapsed": false,
                },
                {
                    "stem": "legacy",
                    "status": "success",
                    "transformer_count": 4,
                    "district_count": 0,
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
