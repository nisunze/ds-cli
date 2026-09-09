//! Failure classification and retry timing are engine policy, not host policy.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Integrity,
    Terminal,
    Authority,
    Api,
    Transport,
    Engine,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorFacts {
    kind: Kind,
    message: String,
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    retryable: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    engine_state: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    owner: String,
    now: u64,
    row: Value,
    error: ErrorFacts,
}
pub fn evaluate(input: &[u8]) -> std::result::Result<String, String> {
    if input.len() > 2 * 1024 * 1024 {
        return Err("Failure transition exceeds its byte bound.".into());
    }
    let mut input: Input = serde_json::from_slice(input).map_err(|_| "Invalid failure facts.")?;
    if input.owner.is_empty() || text(&input.row, "owner_uid") != input.owner {
        return Err("Failure belongs to another account.".into());
    }
    let status = match input.error.kind {
        Kind::Integrity => "conflict",
        Kind::Terminal => "failed",
        Kind::Authority => {
            if input.error.retryable {
                "pending"
            } else {
                "failed"
            }
        }
        Kind::Api if input.error.status == Some(409) => "conflict",
        Kind::Api
            if !input.error.retryable
                && input.error.status.is_some_and(|v| (400..500).contains(&v)) =>
        {
            "failed"
        }
        Kind::Engine => match input.error.engine_state.as_deref() {
            Some("pending") => "pending",
            Some("conflict") => "conflict",
            Some("failed") => "failed",
            _ => return Err("Invalid engine failure state.".into()),
        },
        _ => "pending",
    };
    let message = regex::Regex::new(r"https://[^\s]+")
        .expect("constant regex")
        .replace_all(&input.error.message, "[resumable session]")
        .chars()
        .take(500)
        .collect::<String>();
    input.row["status"] = json!(status);
    input.row["phase"] = json!("pending");
    input.row["updated_at"] = json!(input.now);
    input.row["error"] = json!(message);
    remove(&mut input.row, "failure_code");
    remove(&mut input.row, "sync_lease_id");
    remove(&mut input.row, "sync_lease_until");
    if let Some(code) = input.error.code.filter(|v| {
        !v.is_empty()
            && v.len() <= 96
            && v.as_bytes()[0].is_ascii_uppercase()
            && v.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    }) {
        input.row["failure_code"] = json!(code);
    }
    let delay = retry_delay(input.row["attempts"].as_u64().unwrap_or(0));
    serde_json::to_string(&json!({"row":input.row,"status":status,"retry_at":if status=="pending" {Some(input.now.saturating_add(delay))} else {None}})).map_err(|e|e.to_string())
}
pub fn retry_delay(attempts: u64) -> u64 {
    (2000u64 << attempts.saturating_sub(1).min(16)).min(300_000)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_conflicts_terminal_and_transports_retryable_without_exposing_sessions() {
        for (kind, status, expected) in [
            ("api", 409, "conflict"),
            ("api", 403, "failed"),
            ("api", 503, "pending"),
            ("integrity", 0, "conflict"),
            ("transport", 0, "pending"),
        ] {
            let value = json!({"owner":"u","now":1000,"row":{"owner_uid":"u","attempts":1},"error":{"kind":kind,"status":status,"message":"failure https://private.invalid/session"}});
            let result: Value =
                serde_json::from_str(&evaluate(&serde_json::to_vec(&value).unwrap()).unwrap())
                    .unwrap();
            assert_eq!(result["status"], expected);
            assert!(!result.to_string().contains("private.invalid"));
            assert_eq!(result["retry_at"].is_number(), expected == "pending");
        }
        assert_eq!(retry_delay(u64::MAX), 300_000);
    }
}
