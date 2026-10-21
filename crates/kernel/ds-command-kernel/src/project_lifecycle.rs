//! Portable lifecycle projection exposed by the native client and WASM.
//! Contact information is project data; there are no embedded support people.
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
const DAY_MS: i64 = 86_400_000;
const THRESHOLDS: [i64; 5] = [1, 3, 7, 14, 30];
fn text<'a>(p: &'a Value, key: &str) -> &'a str {
    p[key].as_str().unwrap_or("").trim()
}
pub fn project(p: &Value, now: i64) -> Value {
    let expires = text(p, "expires_at");
    let ms = OffsetDateTime::parse(expires, &Rfc3339)
        .ok()
        .map(|date| (date.unix_timestamp_nanos() / 1_000_000) as i64);
    let reason = text(p, "expiration_reason");
    let email = text(p, "expiration_support_email");
    let phone = text(p, "expiration_support_phone");
    let contact = [email, phone]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    let archived = p["is_archived"] == true
        || text(p, "lifecycle_state") == "archived"
        || text(p, "read_only_reason") == "PROJECT_ARCHIVED";
    let expired = p["is_expired"] == true
        || text(p, "read_only_reason") == "PROJECT_EXPIRED"
        || ms.is_some_and(|ms| ms <= now);
    let days = ms
        .map(|ms| ((ms as i128 - now as i128 + DAY_MS as i128 - 1) / DAY_MS as i128).max(0) as i64)
        .unwrap_or(0);
    let label = ["display_name", "project_name", "ds_project", "project_id"]
        .iter()
        .map(|key| text(p, key))
        .find(|s| !s.is_empty())
        .unwrap_or("Project");
    let id = ["ds_project", "project_id"]
        .iter()
        .map(|key| text(p, key))
        .find(|s| !s.is_empty())
        .unwrap_or("");
    let threshold = THRESHOLDS.iter().find(|t| days <= **t).copied();
    let severity = if expired {
        "expired"
    } else {
        match threshold {
            Some(1) => "critical",
            Some(3) => "urgent",
            Some(7) => "warning",
            _ => "notice",
        }
    };
    let day_label = if days == 1 {
        "1 day".into()
    } else {
        format!("{days} days")
    };
    let title = if expired {
        format!("{label} has expired")
    } else {
        format!("{label} expires in {day_label}")
    };
    let badge = if expired {
        "Expired".into()
    } else {
        format!("Expires in {day_label}")
    };
    let notice = if ms.is_some() && (expired || threshold.is_some()) {
        json!({
            "projectId":id,"severity":severity,"expiresAt":expires,"daysRemaining":if expired {0} else {days},
            "thresholdDays":if expired {0} else {threshold.unwrap_or(30)},"title":title,"badgeLabel":badge,
            "detail":if expired { "Reads, downloads, and access management remain available during the grace window. Writes are locked until the expiration is extended or cleared." }
                else { "This is a non-blocking notice. The project will become read-only at expiration; reads, downloads, and access management will continue during the grace window." },
            "reason":reason,"supportEmail":email,"supportPhone":phone,"contactLabel":contact
        })
    } else {
        Value::Null
    };
    let chip = if ms.is_some() || expired {
        json!({
            "tone":if expired {"expired"} else if threshold.is_some() {severity} else {"far"},
            "full":badge,"compact":if expired {"Expired".into()} else {format!("{days}d")},"daysRemaining":if expired {0} else {days},
            "redHint":expired || days <= 5,
            "title":if expired { "This project has expired and is read-only until the expiration is extended or cleared.".into() }
                else if reason.is_empty() {title.clone()} else {format!("{title} — {reason}")}
        })
    } else {
        Value::Null
    };
    let banner = if archived {
        json!({"kind":"archived","title":"Project archived",
        "detail":"This project is read-only. Downloads and access management remain available.","reason":reason,
        "supportEmail":email,"supportPhone":phone,"contactLabel":contact})
    } else {
        Value::Null
    };
    json!({"state":if archived {"archived"} else if expired {"expired"} else {"active"},
        "notice":notice,"chip":chip,"banner":banner,"reason":reason,"supportEmail":email,"supportPhone":phone,"contactLabel":contact})
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 64 * 1024 {
        return Err("project_lifecycle_input_too_large".into());
    }
    let request: Value = serde_json::from_slice(input).map_err(|_| "project_lifecycle_invalid")?;
    let now = request["now"]
        .as_i64()
        .filter(|n| n.unsigned_abs() <= 9_007_199_254_740_991)
        .ok_or("project_lifecycle_clock_invalid")?;
    serde_json::to_string(&project(&request["project"], now)).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thresholds_and_contacts_are_one_projection() {
        let mut p = json!({"ds_project":"p","expires_at":"2026-10-02T00:00:00Z","expiration_support_email":" help@example.com "});
        let expires = OffsetDateTime::parse(p["expires_at"].as_str().unwrap(), &Rfc3339)
            .unwrap()
            .unix_timestamp()
            * 1000;
        for (days, severity) in [
            (1, "critical"),
            (3, "urgent"),
            (7, "warning"),
            (14, "notice"),
            (30, "notice"),
        ] {
            let result = project(&p, expires - days * DAY_MS);
            assert_eq!(result["notice"]["severity"], severity);
            assert_eq!(result["chip"]["redHint"], days <= 5);
            assert_eq!(result["contactLabel"], "help@example.com");
        }
        assert!(project(&p, expires - 31 * DAY_MS)["notice"].is_null());
        assert_eq!(project(&p, expires)["state"], "expired");
        p["is_archived"] = json!(true);
        assert_eq!(project(&p, expires)["banner"]["kind"], "archived");
        assert!(project(&json!({}), expires)["chip"].is_null());
        assert_eq!(project(&json!({}), expires)["supportEmail"], "");
    }
}
