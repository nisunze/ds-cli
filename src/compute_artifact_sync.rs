//! Pure retry decisions for retained compute-artifact publications.
//! Hosts own durable storage and transport; this module only assesses one
//! immutable failure snapshot before the host performs an owner-fenced update.

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "ds.compute-artifact.retry-assessment/v1";
pub const MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    engine: String,
    status: String,
    phase: String,
    engine_version: String,
    engine_build_manifest_sha256: Option<String>,
    failure_code: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Assessment {
    pub schema: &'static str,
    pub failure_code: Option<&'static str>,
    pub retry_eligible: bool,
    pub retry_remedy: Option<&'static str>,
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn admission_failure(failure_code: Option<&str>, error: Option<&str>) -> bool {
    match failure_code {
        Some(code) => code == "ENGINE_BUILD_NOT_ADMITTED",
        None => matches!(
            error.map(str::trim),
            Some("The producing engine build is not admitted")
                | Some("The producing engine build is not admitted.")
        ),
    }
}

pub fn assess(input: &[u8]) -> Result<Assessment, String> {
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err("Compute artifact retry assessment exceeds its byte bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|_| "Compute artifact retry assessment is invalid.".to_string())?;
    if request.schema != SCHEMA {
        return Err("Unsupported compute artifact retry assessment schema.".into());
    }

    let is_admission_failure =
        admission_failure(request.failure_code.as_deref(), request.error.as_deref());
    // A former Brain comparator compared presentation pointer addresses after
    // independent storage reads. Explicitly retry that exact retained failure;
    // the repaired server still verifies declarations, bytes and head fences.
    let is_storage_comparison_failure = request.failure_code.is_none()
        && request.error.as_deref().map(str::trim)
            == Some("compute artifact conflict: stored work changed before finalize");
    let retry_eligible = request.engine == "network_reporter"
        && matches!(request.status.as_str(), "failed" | "conflict")
        && request.phase == "pending"
        && crate::report::valid_network_reporter_engine_identity(&request.engine_version)
        && request
            .engine_build_manifest_sha256
            .as_deref()
            .is_some_and(|sha| lower_hex(sha, 64))
        && (is_admission_failure || is_storage_comparison_failure);

    Ok(Assessment {
        schema: SCHEMA,
        failure_code: is_admission_failure.then_some("ENGINE_BUILD_NOT_ADMITTED"),
        retry_eligible,
        retry_remedy: if is_storage_comparison_failure {
            Some(
            "After the server storage-comparison fix is installed, retry this retained row. Immutable bytes and current publication authority will be verified again.",
        )
        } else {
            is_admission_failure.then_some(
            "Verify that the server admits this exact engine_version and engine_build_manifest_sha256, then retry this same row.",
        )
        },
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    serde_json::to_string(&assess(input)?).map_err(|_| "Retry assessment encoding failed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> serde_json::Value {
        json!({
            "schema": SCHEMA,
            "engine": "network_reporter",
            "status": "failed",
            "phase": "pending",
            "engine_version": format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            "engine_build_manifest_sha256": "b".repeat(64),
            "failure_code": "ENGINE_BUILD_NOT_ADMITTED",
            "error": "The producing engine build is not admitted."
        })
    }

    #[test]
    fn admits_only_exact_retained_reporter_admission_failures() {
        let eligible = assess(&serde_json::to_vec(&request()).unwrap()).unwrap();
        assert!(eligible.retry_eligible);
        assert_eq!(eligible.failure_code, Some("ENGINE_BUILD_NOT_ADMITTED"));

        for (field, value) in [
            ("engine", json!("solar")),
            ("status", json!("pending")),
            ("phase", json!("opening")),
            (
                "engine_version",
                json!("ds-network-reporter@0.1.0+development"),
            ),
            ("engine_build_manifest_sha256", json!("ABC")),
            ("failure_code", json!("OTHER_FAILURE")),
        ] {
            let mut candidate = request();
            candidate[field] = value;
            let assessed = assess(&serde_json::to_vec(&candidate).unwrap()).unwrap();
            assert!(!assessed.retry_eligible, "{field} must refuse retry");
        }
        let mut future_release = request();
        future_release["engine_version"] =
            json!(format!("ds-network-reporter@0.2.1+{}", "c".repeat(40)));
        assert!(
            assess(&serde_json::to_vec(&future_release).unwrap())
                .unwrap()
                .retry_eligible
        );
        let mut unrelated = request();
        unrelated["failure_code"] = serde_json::Value::Null;
        unrelated["error"] = json!("prefix producing engine build is not admitted suffix");
        assert!(
            !assess(&serde_json::to_vec(&unrelated).unwrap())
                .unwrap()
                .retry_eligible
        );
    }

    #[test]
    fn retries_only_the_exact_storage_comparison_failure() {
        let mut candidate = request();
        candidate["status"] = json!("conflict");
        candidate["failure_code"] = serde_json::Value::Null;
        candidate["error"] =
            json!("compute artifact conflict: stored work changed before finalize");
        assert!(
            assess(&serde_json::to_vec(&candidate).unwrap())
                .unwrap()
                .retry_eligible
        );
        for (field, value) in [
            (
                "error",
                json!("compute artifact conflict: network reporter input receipt is stale"),
            ),
            (
                "error",
                json!("compute artifact conflict: output bytes changed"),
            ),
            ("phase", json!("stored_stale")),
            ("status", json!("syncing")),
            ("engine", json!("solar")),
            ("failure_code", json!("OTHER_FAILURE")),
        ] {
            let mut changed = candidate.clone();
            changed[field] = value;
            assert!(
                !assess(&serde_json::to_vec(&changed).unwrap())
                    .unwrap()
                    .retry_eligible
            );
        }
    }

    #[test]
    fn rejects_unknown_fields_and_oversized_requests() {
        let mut unknown = request();
        unknown["force"] = json!(true);
        assert!(assess(&serde_json::to_vec(&unknown).unwrap()).is_err());
        assert!(assess(&vec![b'x'; MAX_BYTES + 1]).is_err());
    }
}
