//! Shared saved portfolio analysis projection for native and WASM consumers.
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Binding {
    portfolio_id: String,
    portfolio_name: String,
    city_ids: Vec<String>,
    membership_revision: Option<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    binding: Binding,
    outcome: Outcome,
    #[serde(default)]
    require_binding: bool,
}
#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Outcome {
    Error { error: String },
    Data { data: Option<Value> },
}
fn text(value: &Value) -> Value {
    match value {
        Value::String(s) if !s.trim().is_empty() => json!(s.trim()),
        Value::Number(n) => json!(n.to_string()),
        _ => Value::Null,
    }
}
fn count(value: &Value) -> Value {
    value
        .as_f64()
        .filter(|n| n.fract() == 0. && (0. ..=200.).contains(n))
        .map_or(Value::Null, |n| json!(n as u64))
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err("Saved Solar analysis exceeds the 8 MiB bound".into());
    }
    let input: Input = serde_json::from_slice(bytes)
        .map_err(|_| "Invalid saved Solar analysis projection request")?;
    let binding = input.binding;
    if !identifier(&binding.portfolio_id)
        || binding.portfolio_name.trim().is_empty()
        || binding.portfolio_name.len() > 1024
        || binding.city_ids.len() > 200
        || binding.city_ids.iter().any(|s| !identifier(s))
        || binding
            .city_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != binding.city_ids.len()
    {
        return Err("Invalid saved Solar portfolio binding".into());
    }
    let revision = text(binding.membership_revision.as_ref().unwrap_or(&Value::Null));
    let mut value = json!({
        "portfolio_id":binding.portfolio_id,
        "portfolio_name":binding.portfolio_name,
        "membership_revision":revision,
        "city_ids":binding.city_ids,
        "city_count":binding.city_ids.len(),
        "saved_analysis":"none", "analysis":null, "error":null
    });
    match input.outcome {
        Outcome::Error { error } => {
            if error.len() > 16 * 1024 {
                return Err("Saved Solar analysis refusal exceeds 16 KiB".into());
            }
            value["saved_analysis"] = json!("failed");
            value["error"] = json!(error);
        }
        Outcome::Data { data: Some(data) } if data["meta"].is_object() => {
            let meta = &data["meta"];
            // A saved response cannot become current by borrowing the selected
            // membership. Preserve absence, but reject declared foreign identity.
            let mismatch = (input.require_binding
                && (meta["portfolio_id"] != value["portfolio_id"]
                    || meta["city_ids"] != value["city_ids"]
                    || (!revision.is_null() && text(&meta["membership_revision"]) != revision)))
                || (meta.get("portfolio_id").is_some()
                    && meta["portfolio_id"] != value["portfolio_id"])
                || (meta.get("city_ids").is_some() && meta["city_ids"] != value["city_ids"])
                || (meta.get("membership_revision").is_some()
                    && !revision.is_null()
                    && text(&meta["membership_revision"]) != revision)
                || (meta.get("city_count").is_some()
                    && count(&meta["city_count"]) != value["city_count"]);
            if mismatch {
                value["saved_analysis"] = json!("failed");
                value["error"] = json!(if meta["portfolio_id"] != value["portfolio_id"]
                    && (input.require_binding || meta.get("portfolio_id").is_some())
                {
                    "The saved portfolio result belongs to a different portfolio."
                } else {
                    "The saved portfolio result belongs to a different membership revision."
                });
            } else {
                let native = &data["native_portfolio"];
                value["saved_analysis"] = json!("ready");
                value["analysis"] = json!({
                    "generated_at":text(&meta["generated_at"]),
                    "city_count":count(&meta["city_count"]),
                    "graph_strategy":text(&meta["graph_strategy"]),
                    "representative_city_id":text(&meta["representative_city_id"]),
                    "portfolio_digest":text(&native["portfolio_digest"]),
                    "run_id":text(&native["run_id"])
                });
            }
        }
        Outcome::Data { .. } => (),
    }
    serde_json::to_string(&value).map_err(|_| "Cannot encode saved Solar analysis".into())
}

/// Validate a hosted document before it can enter a cache or render pipeline.
pub fn bind_hosted(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err("Saved Solar analysis exceeds the 8 MiB bound".into());
    }
    let mut wire: Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid saved Solar analysis document")?;
    wire["require_binding"] = json!(true);
    let projection: Value = serde_json::from_str(&evaluate(
        &serde_json::to_vec(&wire).map_err(|_| "Cannot encode saved Solar analysis")?,
    )?)
    .map_err(|_| "Cannot read saved Solar analysis projection")?;
    if projection["saved_analysis"] != "ready" {
        return Err(projection["error"]
            .as_str()
            .unwrap_or("The saved portfolio analysis has no metadata.")
            .into());
    }
    let mut data = wire["outcome"]["data"].clone();
    data["meta"]["portfolio_name"] = projection["portfolio_name"].clone();
    serde_json::to_string(&data).map_err(|_| "Cannot encode bound Solar analysis".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project(outcome: Value) -> Value {
        serde_json::from_str(&evaluate(&serde_json::to_vec(&json!({
            "binding":{"portfolioId":"pf_1","portfolioName":"Two cities","cityIds":["a","b"],"membershipRevision":7},
            "outcome":outcome
        })).unwrap()).unwrap()).unwrap()
    }
    #[test]
    fn absence_failure_and_ready_preserve_exact_identity_and_missing_values() {
        let none = project(json!({"data":null}));
        let failed = project(json!({"error":"  refusal verbatim\n"}));
        let ready = project(json!({"data":{"meta":{"city_count":2,"generated_at":"now"}}}));
        assert_eq!(none["saved_analysis"], "none");
        assert_eq!(failed["saved_analysis"], "failed");
        assert_eq!(failed["error"], "  refusal verbatim\n");
        assert_eq!(ready["saved_analysis"], "ready");
        assert!(ready["analysis"]["run_id"].is_null());
        assert_eq!(ready["membership_revision"], "7");
        for value in [&failed, &ready] {
            assert_eq!(value["city_ids"], none["city_ids"]);
            assert_eq!(value["portfolio_id"], none["portfolio_id"]);
        }
    }
    #[test]
    fn foreign_saved_identity_does_not_become_ready() {
        for meta in [
            json!({"portfolio_id":"other"}),
            json!({"city_ids":["b","a"]}),
            json!({"membership_revision":8}),
            json!({"city_count":4}),
        ] {
            assert_eq!(
                project(json!({"data":{"meta":meta}}))["saved_analysis"],
                "failed"
            );
        }
    }
}
