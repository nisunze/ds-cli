//! A held authenticated project CRS capture; never a second project setting.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.print-project-crs/v1";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema: String,
    pub project: String,
    pub lane: String,
    pub principal: String,
    pub credential_audience_sha256: String,
    /// Exact normalized ProjectListItem.project_params from authenticated discovery.
    pub project_params: Value,
    pub source_sha256: String,
}
impl Capture {
    pub fn new(
        project: &str,
        lane: &str,
        principal: &str,
        audience: &str,
        project_params: Value,
    ) -> Result<Self, String> {
        let bytes = serde_json::to_vec(&project_params).map_err(|e| e.to_string())?;
        let capture = Self {
            schema: SCHEMA.into(),
            project: project.into(),
            lane: lane.into(),
            principal: principal.into(),
            credential_audience_sha256: audience.into(),
            source_sha256: format!("{:x}", Sha256::digest(&bytes)),
            project_params,
        };
        capture.validate_scope(project, lane, principal, audience, None)?;
        Ok(capture)
    }
    /// Identity is supplied by the restored provider, never taken from this capsule.
    /// A fresh directory read additionally fences the held capture to its current source.
    pub fn validate_scope(
        &self,
        project: &str,
        lane: &str,
        principal: &str,
        audience: &str,
        current_source_sha256: Option<&str>,
    ) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.project != project
            || self.lane != lane
            || self.principal != principal
            || self.credential_audience_sha256 != audience
        {
            return Err(
                "print_project_crs_context_mismatch: capture identity differs from this print"
                    .into(),
            );
        }
        let bytes = serde_json::to_vec(&self.project_params).map_err(|e| e.to_string())?;
        if bytes.len() > 8192
            || !self.project_params["crs"].is_object()
            || self.project_params["crs"]["mode"].as_str().is_none()
        {
            return Err(
                "print_project_crs_invalid: capture requires bounded normalized project_params.crs"
                    .into(),
            );
        }
        if format!("{:x}", Sha256::digest(&bytes)) != self.source_sha256
            || current_source_sha256.is_some_and(|sha| sha != self.source_sha256)
        {
            return Err(
                "print_project_crs_stale: captured project CRS source digest differs".into(),
            );
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn captured_scope_and_current_source_are_required_without_mutating_project_values() {
        let source = json!({"project_type":"new_build","crs":{"mode":"utm","utm_zone":36,"utm_hemisphere":"south"}});
        let capture = Capture::new("p", "stable", "u", "a", source.clone()).unwrap();
        assert_eq!(capture.project_params, source);
        assert!(capture
            .validate_scope("p", "stable", "u", "a", Some(&capture.source_sha256))
            .is_ok());
        for (p, l, u, a) in [
            ("q", "stable", "u", "a"),
            ("p", "canary", "u", "a"),
            ("p", "stable", "v", "a"),
            ("p", "stable", "u", "b"),
        ] {
            assert!(capture
                .validate_scope(p, l, u, a, None)
                .unwrap_err()
                .starts_with("print_project_crs_context_mismatch"));
        }
        assert!(capture
            .validate_scope("p", "stable", "u", "a", Some("changed"))
            .unwrap_err()
            .starts_with("print_project_crs_stale"));
        let mut tampered = capture.clone();
        tampered.project_params["crs"]["utm_zone"] = json!(35);
        assert!(tampered
            .validate_scope("p", "stable", "u", "a", None)
            .is_err());
        assert_eq!(capture.project_params, source);
    }
}
