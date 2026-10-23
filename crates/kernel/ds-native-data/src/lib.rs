//! Shared native application operations for local source and elevation work.
//!
//! These operations address files owned by the invoking OS user. They do not
//! acquire project data, publish, or consult a selected DS project. Local-file
//! scope is explicit and captured before work starts; it cannot be promoted
//! into project authority by a path, request id or session token.

pub mod bridge;
mod columnar;
mod native_source;
pub mod compute;
pub mod elevation;
mod engine_host;
pub mod point_cloud;
pub mod rwanda_dem;

pub use columnar::NativeColumnarConversion;
pub use native_source::{NativeLinkAccess, NativeSourceDisposition, NativeSourcePrepareResult};

/// Application admission DTO. The engine's IO request is created only after
/// this closed caller contract and the captured local scope have been checked.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSourcePrepareRequest {
    pub source_path: std::path::PathBuf,
    pub disposition: NativeSourceDisposition,
    #[serde(default)]
    pub link_access: Option<NativeLinkAccess>,
    #[serde(default)]
    pub destination: Option<std::path::PathBuf>,
}

/// Host effect checked immediately before each intentional network read.
pub type OnlineCheck = fn() -> Result<(), String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestScope {
    LocalFiles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionHost {
    Desktop,
    Headless,
}

/// Immutable invocation context retained by long-lived local sessions.
#[derive(Clone, Debug)]
pub struct RequestContext {
    request_id: String,
    scope: RequestScope,
    host: ExecutionHost,
    pub(crate) check_online: OnlineCheck,
}

impl RequestContext {
    pub fn local_files(host: ExecutionHost, check_online: OnlineCheck) -> Self {
        Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            scope: RequestScope::LocalFiles,
            host,
            check_online,
        }
    }

    pub fn capture_local_files(
        request_id: String,
        host: ExecutionHost,
        check_online: OnlineCheck,
    ) -> Result<Self, String> {
        validate_request_id(&request_id)?;
        Ok(Self {
            request_id,
            scope: RequestScope::LocalFiles,
            host,
            check_online,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn execution_host(&self) -> &'static str {
        match self.host {
            ExecutionHost::Desktop => "desktop",
            ExecutionHost::Headless => "headless",
        }
    }

    pub fn scope(&self) -> RequestScope {
        self.scope
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        validate_request_id(&self.request_id)?;
        match self.scope {
            RequestScope::LocalFiles => Ok(()),
        }
    }
}

pub fn online() -> Result<(), String> {
    Ok(())
}

fn validate_request_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(
            "local request id must be 1-200 ASCII letters, digits, hyphens or underscores".into(),
        );
    }
    Ok(())
}

pub(crate) fn bounded_text(value: &str, name: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!(
            "{name} must be 1-{max} characters without control characters"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_scope_is_explicit_and_ids_cannot_be_paths() {
        for bad in ["", "../project", "a/b", "a\\b", "a b"] {
            assert!(RequestContext::capture_local_files(
                bad.into(),
                ExecutionHost::Headless,
                online
            )
            .is_err());
        }
        let a = RequestContext::local_files(ExecutionHost::Headless, online);
        let b = RequestContext::local_files(ExecutionHost::Headless, online);
        assert_eq!(a.scope(), RequestScope::LocalFiles);
        assert_ne!(a.request_id(), b.request_id());
        assert_eq!(a.clone().request_id(), a.request_id());
    }
}
