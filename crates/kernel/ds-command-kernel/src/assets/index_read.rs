//! Assets retained-page admission. The host's existing store holds bytes and
//! promises; this policy owns scope, stable query identity and completion fences.
//! No network, storage, clock, authorization refresh or notification acquisition.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const FRESH_MS: i64 = 300_000;
const MAX_EPOCH: u64 = 9_007_199_254_740_991;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub uid: String,
    pub email: String,
    pub lane: String,
    pub audience: String,
    pub project: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub scope_key: String,
    pub key: String,
    pub epoch: u64,
    pub refresh: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Held {
    pub key: String,
    pub saved_at_ms: i64,
    pub invalidated: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Identity {
        scope: Scope,
        query: Value,
    },
    Plan {
        scope: Scope,
        query: Value,
        epoch: u64,
        held: Option<Held>,
        pending: Option<Ticket>,
        online: bool,
        now_ms: i64,
    },
    Accept {
        scope: Scope,
        ticket: Ticket,
        epoch: u64,
        outcome: Outcome,
        held: Option<Held>,
        now_ms: i64,
    },
    Invalidate {
        scope: Scope,
        notice: Notice,
        epoch: u64,
    },
    Mutation {
        scope: Scope,
        captured_scope: Scope,
        expected_asset_id: String,
        expected_version: u64,
        receipt: MutationReceipt,
        epoch: u64,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    AccessRefused,
    TemporaryFailure,
    Unavailable,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub resource: String,
    pub scope: Scope,
    pub source_revision: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MutationReceipt {
    pub project_id: String,
    pub asset_id: String,
    pub version: u64,
    pub audit_id: String,
    pub changed: Vec<String>,
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.trim() == s && !s.chars().any(char::is_control)
}
fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| "assets_index_scope_invalid")?)
    ))
}
fn scope_key(scope: &Scope) -> Result<String, String> {
    if !text(&scope.uid, 256)
        || !text(&scope.email, 320)
        || !text(&scope.audience, 2048)
        || !text(&scope.project, 256)
        || !["canary", "production", "stable"].contains(&scope.lane.as_str())
        || scope.project.chars().any(|c| matches!(c, '/' | '\\'))
    {
        return Err("assets_index_scope_invalid".into());
    }
    digest(&(
        "ds.assets.read-scope/v1",
        &scope.uid,
        scope.email.to_ascii_lowercase(),
        &scope.lane,
        &scope.audience,
        &scope.project,
    ))
}
fn identity(scope: &Scope, query: &Value) -> Result<(String, String, bool), String> {
    let raw = query.as_object().ok_or("assets_index_query_invalid")?;
    let mut canonical: BTreeMap<&str, Value> = BTreeMap::new();
    let mut refresh = false;
    for (key, value) in raw {
        match key.as_str() {
            "refresh" => {
                refresh = value.as_bool().ok_or("assets_index_query_invalid")?;
            }
            "limit" => {
                if value.as_u64().is_none_or(|n| n == 0 || n > 200) {
                    return Err("assets_index_query_invalid".into());
                };
                canonical.insert(key.as_str(), value.clone());
            }
            "order" | "folder" | "kind" | "status" | "sensitivity" | "since" | "cursor" => {
                if value.as_str().is_none_or(|s| !text(s, 2048)) {
                    return Err("assets_index_query_invalid".into());
                };
                canonical.insert(key.as_str(), value.clone());
            }
            _ => return Err("assets_index_query_invalid".into()),
        }
    }
    canonical.entry("order").or_insert(json!("recent"));
    canonical.entry("limit").or_insert(json!(50));
    let scope_key = scope_key(scope)?;
    let key = digest(&("ds.assets.read-page/v1", &scope_key, canonical))?;
    Ok((scope_key, key, refresh))
}
fn fence(ticket: &Ticket, scope: &Scope, epoch: u64) -> Result<bool, String> {
    if epoch > MAX_EPOCH
        || ticket.epoch > MAX_EPOCH
        || ticket.key.len() != 64
        || !ticket
            .key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("assets_index_epoch_invalid".into());
    }
    Ok(ticket.epoch == epoch && ticket.scope_key == scope_key(scope)?)
}
pub fn evaluate(request: Request) -> Result<Value, String> {
    match request {
        Request::Identity { scope, query } => {
            let (scope_key, key, refresh) = identity(&scope, &query)?;
            Ok(json!({"scope_key":scope_key,"key":key,"refresh":refresh,"query":query}))
        }
        Request::Plan {
            scope,
            query,
            epoch,
            held,
            pending,
            online,
            now_ms,
        } => {
            if epoch > MAX_EPOCH || now_ms < 0 {
                return Err("assets_index_epoch_invalid".into());
            }
            let (scope_key, key, refresh) = identity(&scope, &query)?;
            let force_supersedes = refresh
                && pending.as_ref().is_some_and(|p| {
                    p.scope_key == scope_key && p.key == key && p.epoch == epoch && !p.refresh
                });
            let ticket = Ticket {
                scope_key,
                key,
                epoch: if force_supersedes {
                    epoch
                        .checked_add(1)
                        .filter(|n| *n <= MAX_EPOCH)
                        .ok_or("assets_index_epoch_invalid")?
                } else {
                    epoch
                },
                refresh,
            };
            let held = held
                .filter(|h| h.key == ticket.key && h.saved_at_ms >= 0 && h.saved_at_ms <= now_ms);
            let fresh = held
                .as_ref()
                .is_some_and(|h| !h.invalidated && now_ms - h.saved_at_ms < FRESH_MS);
            let decision = if held.is_some() && (!online || (!refresh && fresh)) {
                "retained"
            } else if !online {
                "unavailable"
            } else if pending.as_ref().is_some_and(|p| {
                p.scope_key == ticket.scope_key
                    && p.key == ticket.key
                    && p.epoch == ticket.epoch
                    && (!refresh || p.refresh)
            }) {
                "pending"
            } else {
                "acquire"
            };
            Ok(
                json!({"decision":decision,"ticket":ticket,"stale":held.as_ref().is_some_and(|h|h.invalidated || !fresh),
    "show_retained":held.is_some() && !refresh && query.get("cursor").is_none()}),
            )
        }
        Request::Accept {
            scope,
            ticket,
            epoch,
            outcome,
            held,
            now_ms,
        } => {
            if now_ms < 0 {
                return Err("assets_index_epoch_invalid".into());
            }
            let retained_usable = held.is_some_and(|h| {
                h.key == ticket.key
                    && !h.invalidated
                    && h.saved_at_ms >= 0
                    && h.saved_at_ms <= now_ms
            });
            let decision = if !fence(&ticket, &scope, epoch)? {
                "superseded"
            } else {
                match outcome {
                    Outcome::Success => "retain",
                    Outcome::AccessRefused => "drop",
                    Outcome::TemporaryFailure if retained_usable => "retained",
                    _ => "refuse",
                }
            };
            Ok(
                json!({"decision":decision,"epoch":if decision=="drop" {epoch.checked_add(1).filter(|n| *n<=MAX_EPOCH).ok_or("assets_index_epoch_invalid")?} else {epoch}}),
            )
        }
        Request::Mutation {
            scope,
            captured_scope,
            expected_asset_id,
            expected_version,
            receipt,
            epoch,
        } => {
            if epoch >= MAX_EPOCH {
                return Err("assets_index_epoch_invalid".into());
            }
            let admitted = scope_key(&scope)? == scope_key(&captured_scope)?
                && receipt.project_id == scope.project
                && receipt.asset_id == expected_asset_id
                && text(&receipt.asset_id, 256)
                && text(&receipt.audit_id, 256)
                && expected_version > 0
                && expected_version.checked_add(1) == Some(receipt.version)
                && !receipt.changed.is_empty();
            Ok(
                json!({"admitted":admitted,"epoch":if admitted {epoch+1}else{epoch},"acquire":false}),
            )
        }
        Request::Invalidate {
            scope,
            notice,
            epoch,
        } => {
            if epoch >= MAX_EPOCH {
                return Err("assets_index_epoch_invalid".into());
            }
            let admitted = notice.resource == "assets_index"
                && text(&notice.source_revision, 256)
                && scope_key(&scope)? == scope_key(&notice.scope)?;
            Ok(
                json!({"admitted":admitted,"epoch":if admitted {epoch+1}else{epoch},"acquire":false}),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            uid: "u1".into(),
            email: "owner@example.test".into(),
            lane: "canary".into(),
            audience: "https://gateway.example.test".into(),
            project: "p1".into(),
        }
    }
    fn keys(s: Scope, q: Value) -> Value {
        evaluate(Request::Identity { scope: s, query: q }).unwrap()
    }
    fn ticket() -> Ticket {
        serde_json::from_value(
            evaluate(Request::Plan {
                scope: scope(),
                query: json!({}),
                epoch: 0,
                held: None,
                pending: None,
                online: true,
                now_ms: 1000,
            })
            .unwrap()["ticket"]
                .clone(),
        )
        .unwrap()
    }
    #[test]
    fn every_authority_dimension_and_query_are_fenced() {
        let base = keys(scope(), json!({"limit":80,"kind":"doc"}));
        for field in ["uid", "email", "lane", "audience", "project"] {
            let mut v = serde_json::to_value(scope()).unwrap();
            v[field] = json!(if field == "lane" {
                "production"
            } else {
                "other"
            });
            let changed = keys(
                serde_json::from_value(v).unwrap(),
                json!({"limit":80,"kind":"doc"}),
            );
            assert_ne!(base["key"], changed["key"])
        }
        assert_eq!(
            base["key"],
            keys(scope(), json!({"kind":"doc","refresh":true,"limit":80}))["key"]
        );
        assert_eq!(
            keys(scope(), json!({}))["key"],
            keys(scope(), json!({"order":"recent","limit":50}))["key"]
        );
        assert_ne!(
            base["key"],
            keys(scope(), json!({"limit":80,"kind":"sheet"}))["key"]
        );
    }
    #[test]
    fn concurrent_demand_reuses_pending_and_warm_offline_never_acquires() {
        let ticket = ticket();
        let held = Held {
            key: ticket.key.clone(),
            saved_at_ms: 1000,
            invalidated: false,
        };
        let plan = |held, pending, online, refresh| {
            evaluate(Request::Plan {
                scope: scope(),
                query: json!({"refresh":refresh}),
                epoch: 0,
                held,
                pending,
                online,
                now_ms: 2000,
            })
            .unwrap()
        };
        assert_eq!(plan(None, None, true, false)["decision"], "acquire");
        assert_eq!(
            plan(None, Some(ticket.clone()), true, false)["decision"],
            "pending"
        );
        assert_eq!(
            plan(Some(held.clone()), None, true, false)["decision"],
            "retained"
        );
        assert_eq!(plan(Some(held), None, false, true)["decision"], "retained");
        assert_eq!(plan(None, None, false, false)["decision"], "unavailable");
    }
    #[test]
    fn explicit_refresh_does_not_join_a_nonrefresh_request() {
        let ordinary = ticket();
        let plan = evaluate(Request::Plan {
            scope: scope(),
            query: json!({"refresh":true}),
            epoch: 0,
            held: None,
            pending: Some(ordinary.clone()),
            online: true,
            now_ms: 1000,
        })
        .unwrap();
        assert_eq!(plan["decision"], "acquire");
        assert_eq!(plan["ticket"]["epoch"], 1);
        assert_eq!(
            evaluate(Request::Accept {
                scope: scope(),
                ticket: ordinary,
                epoch: 1,
                outcome: Outcome::Success,
                held: None,
                now_ms: 1000
            })
            .unwrap()["decision"],
            "superseded"
        );
        let refresh: Ticket = serde_json::from_value(plan["ticket"].clone()).unwrap();
        assert_eq!(
            evaluate(Request::Plan {
                scope: scope(),
                query: json!({"refresh":true}),
                epoch: 1,
                held: None,
                pending: Some(refresh),
                online: true,
                now_ms: 1000
            })
            .unwrap()["decision"],
            "pending"
        );
    }
    #[test]
    fn completion_after_account_lane_or_generation_change_cannot_publish() {
        let mut other = scope();
        other.lane = "production".into();
        for (scope, epoch) in [(other, 0), (scope(), 1)] {
            assert_eq!(
                evaluate(Request::Accept {
                    scope,
                    ticket: ticket(),
                    epoch,
                    outcome: Outcome::Success,
                    held: None,
                    now_ms: 1000
                })
                .unwrap()["decision"],
                "superseded"
            )
        }
        assert_eq!(
            evaluate(Request::Accept {
                scope: scope(),
                ticket: ticket(),
                epoch: 0,
                outcome: Outcome::AccessRefused,
                held: None,
                now_ms: 1000
            })
            .unwrap()["decision"],
            "drop"
        );
    }
    #[test]
    fn classification_requires_exact_changed_receipt_and_context() {
        let receipt = MutationReceipt {
            project_id: "p1".into(),
            asset_id: "a_123".into(),
            version: 8,
            audit_id: "audit-1".into(),
            changed: vec!["kind".into()],
        };
        let check = |receipt, captured_scope| {
            evaluate(Request::Mutation {
                scope: scope(),
                captured_scope,
                expected_asset_id: "a_123".into(),
                expected_version: 7,
                receipt,
                epoch: 0,
            })
            .unwrap()
        };
        assert_eq!(check(receipt.clone(), scope())["admitted"], true);
        let mut no_op = receipt.clone();
        no_op.changed.clear();
        no_op.version = 7;
        assert_eq!(check(no_op, scope())["admitted"], false);
        let mut wrong = receipt.clone();
        wrong.project_id = "p2".into();
        assert_eq!(check(wrong, scope())["admitted"], false);
        let mut other = scope();
        other.email = "changed@example.test".into();
        assert_eq!(check(receipt, other)["admitted"], false);
    }
    #[test]
    fn invalidation_is_scoped_lazy_and_revoked_or_changed_data_never_fallback() {
        let notice = Notice {
            resource: "assets_index".into(),
            scope: scope(),
            source_revision: "revision-7".into(),
        };
        let valid = evaluate(Request::Invalidate {
            scope: scope(),
            notice: notice.clone(),
            epoch: 0,
        })
        .unwrap();
        assert_eq!(valid["epoch"], 1);
        assert_eq!(valid["acquire"], false);
        let mut unscoped = notice;
        unscoped.scope.project = "p2".into();
        assert_eq!(
            evaluate(Request::Invalidate {
                scope: scope(),
                notice: unscoped,
                epoch: 0
            })
            .unwrap()["admitted"],
            false
        );
        assert_eq!(
            evaluate(Request::Accept {
                scope: scope(),
                ticket: ticket(),
                epoch: 0,
                outcome: Outcome::TemporaryFailure,
                held: None,
                now_ms: 1000
            })
            .unwrap()["decision"],
            "refuse"
        );
    }
}
