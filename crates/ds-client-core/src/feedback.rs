//! Fixed authenticated product backlog. No project selection or second store.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind, FeedbackCall,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    List {
        view: String,
        #[serde(default)]
        component: Option<String>,
        #[serde(default)]
        query: Option<String>,
        limit: usize,
        #[serde(default)]
        detail: bool,
        #[serde(default)]
        since: Option<String>,
    },
    Submit {
        title: String,
        detail: String,
        component: String,
        kind: String,
        severity: String,
        agent: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        client: Option<String>,
        #[serde(default)]
        evidence: Vec<String>,
        #[serde(default)]
        context: BTreeMap<String, String>,
    },
    Close {
        id: String,
        status: String,
        resolution: String,
        #[serde(default)]
        expected_version: Option<u64>,
    },
}
fn invalid() -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, "Invalid bounded feedback request")
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "Feedback response violates the product backlog contract",
    )
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.trim() == s
        && s.chars().count() <= max
        && !s
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
}
fn date(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}
impl Command {
    pub fn validate(&self) -> Result<(), ClientError> {
        let optional = |s: &Option<String>, max| s.as_ref().is_none_or(|v| text(v, max));
        let valid = match self {
            Self::List {
                view,
                component,
                query,
                limit,
                since,
                ..
            } => {
                ["not_addressed", "addressed", "all"].contains(&view.as_str())
                    && optional(component, 200)
                    && optional(query, 200)
                    && (1..=50).contains(limit)
                    && since.as_ref().is_none_or(|s| date(s).is_some())
            }
            Self::Submit {
                title,
                detail,
                component,
                kind,
                severity,
                agent,
                model,
                client,
                evidence,
                context,
            } => {
                text(title, 200)
                    && text(detail, 16384)
                    && detail.len() <= 16384
                    && text(component, 200)
                    && ["obstacle", "bug", "friction", "idea", "question"].contains(&kind.as_str())
                    && ["blocker", "major", "minor", "trivial"].contains(&severity.as_str())
                    && text(agent, 200)
                    && optional(model, 200)
                    && optional(client, 200)
                    && evidence.len() <= 20
                    && evidence.iter().all(|s| text(s, 500))
                    && context.len() <= 24
                    && context.iter().all(|(k, v)| text(k, 500) && text(v, 1000))
            }
            Self::Close {
                id,
                status,
                resolution,
                expected_version,
            } => {
                text(id, 200)
                    && ["resolved", "wont_fix"].contains(&status.as_str())
                    && text(resolution, 1000)
                    && expected_version.is_none_or(|v| v > 0 && v <= i64::MAX as u64)
            }
        };
        if valid {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
#[derive(Deserialize, Serialize)]
struct Report {
    id: String,
    status: String,
    kind: String,
    severity: String,
    component: String,
    surface: String,
    title: String,
    detail: String,
    occurrences: u64,
    reporters: Vec<String>,
    reporter_kind: String,
    version: u64,
    last_seen_at: String,
    updated_at: String,
    updated_by: String,
    #[serde(default)]
    resolution: String,
}
impl Report {
    fn validate(&self) -> Result<(), ClientError> {
        if !text(&self.id, 200)
            || !text(&self.title, 200)
            || self.detail.len() > 16384
            || self.version == 0
            || self.occurrences == 0
            || self.reporters.len() > 50
            || date(&self.last_seen_at).is_none()
            || date(&self.updated_at).is_none()
            || !["open", "triaged", "in_progress", "resolved", "wont_fix"]
                .contains(&self.status.as_str())
        {
            return Err(unreadable());
        }
        Ok(())
    }
    fn row(mut self, full: bool) -> Value {
        let clipped = !full && self.detail.chars().count() > 1200;
        if clipped {
            self.detail = self.detail.chars().take(1200).collect();
        }
        let mut row = serde_json::to_value(self).expect("feedback row");
        row["detail_truncated"] = json!(clipped);
        row
    }
}
fn report(v: Value) -> Result<Report, ClientError> {
    let r: Report = serde_json::from_value(v).map_err(|_| unreadable())?;
    r.validate()?;
    Ok(r)
}
fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    payload: Value,
) -> Result<Value, ClientError> {
    let response = transport.feedback(FeedbackCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    match response.status {
        200 => {}
        403 => {
            return Err(ClientError::new(
                ErrorKind::AuthenticationRejected,
                "feedback_not_permitted",
            ))
        }
        404 => {
            return Err(ClientError::new(
                ErrorKind::ResourceNotFound,
                "feedback_not_found",
            ))
        }
        409 => {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "feedback_conflict",
            ))
        }
        other => return Err(crate::authorized::classify_tile_error(other)),
    }
    if response.body.len() > 4 * 1024 * 1024 {
        return Err(unreadable());
    }
    let envelope: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if envelope["success"] != true || !envelope["data"].is_object() {
        return Err(unreadable());
    }
    Ok(envelope["data"].clone())
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate()?;
    match command {
        Command::List {
            view,
            component,
            query,
            limit,
            detail,
            since,
        } => {
            let data = call(
                profile,
                transport,
                authority,
                json!({"action":"list","limit":200}),
            )?;
            let raw = data["reports"]
                .as_array()
                .filter(|r| r.len() <= 200)
                .ok_or_else(unreadable)?;
            let cutoff = since.as_deref().and_then(date);
            let component = component.as_ref().map(|s| s.to_lowercase());
            let query = query.as_ref().map(|s| s.to_lowercase());
            let mut rows = Vec::new();
            for raw in raw {
                let r = report(raw.clone())?;
                let closed = ["resolved", "wont_fix"].contains(&r.status.as_str());
                if (view == "addressed" && !closed)
                    || (view == "not_addressed" && closed)
                    || component
                        .as_ref()
                        .is_some_and(|c| !r.component.to_lowercase().contains(c))
                    || query.as_ref().is_some_and(|q| {
                        ![&r.title, &r.detail, &r.component]
                            .iter()
                            .any(|s| s.to_lowercase().contains(q))
                    })
                    || cutoff.is_some_and(|c| {
                        date(&r.last_seen_at)
                            .unwrap()
                            .max(date(&r.updated_at).unwrap())
                            < c
                    })
                {
                    continue;
                }
                rows.push(r.row(*detail));
            }
            let total = rows.len();
            rows.truncate(*limit);
            Ok(
                json!({"view":view,"total":total,"truncated":total>*limit,"reports":rows,"scan_limit":200,"scanned":raw.len(),"scan_incomplete":raw.len()==200,"since":since}),
            )
        }
        Command::Submit {
            title,
            detail,
            component,
            kind,
            severity,
            agent,
            model,
            client,
            evidence,
            context,
        } => {
            let mut a = json!({"name":agent});
            if let Some(m) = model {
                a["model"] = json!(m)
            }
            if let Some(c) = client {
                a["client"] = json!(c)
            }
            let data = call(
                profile,
                transport,
                authority,
                json!({"action":"submit","report":{"title":title,"detail":detail,"component":component,"kind":kind,"severity":severity,"surface":"cli","reporter_kind":"agent","agent":a,"evidence":evidence,"context":context}}),
            )?;
            let r = report(data["report"].clone())?;
            let deduplicated = data["deduplicated"].as_bool().ok_or_else(unreadable)?;
            Ok(json!({"report":r.row(true),"deduplicated":deduplicated}))
        }
        Command::Close {
            id,
            status,
            resolution,
            expected_version,
        } => {
            let current = report(
                call(
                    profile,
                    transport,
                    authority,
                    json!({"action":"get","id":id}),
                )?["report"]
                    .clone(),
            )?;
            if current.id != *id {
                return Err(unreadable());
            }
            let version = expected_version.unwrap_or(current.version);
            let changed=report(call(profile,transport,authority,json!({"action":"update_status","id":id,"status":status,"resolution":resolution,"expected_version":version}))?["report"].clone())?;
            if changed.id != *id
                || changed.status != *status
                || changed.resolution != *resolution
                || changed.version < version
            {
                return Err(unreadable());
            }
            Ok(json!({"report":changed.row(true),"previous_status":current.status}))
        }
    }
}
