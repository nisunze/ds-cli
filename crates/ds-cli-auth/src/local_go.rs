//! Explicit local development transport. The protected profile and its
//! credential audience remain unchanged; only closed Go API calls move.

use std::ffi::OsStr;
use std::sync::OnceLock;

use ds_cli_contract::Failure;
use ds_client_core::TransportError;

const SELECTOR: &str = "DS_NATIVE_LOCAL_GO";
const LOCAL_GO_ORIGIN: &str = "http://127.0.0.1:8080";
static SELECTION: OnceLock<Result<Selection, SelectorError>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selection {
    Packaged,
    LocalGo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectorError {
    Invalid,
    Release,
    Fixture,
}

impl Selection {
    fn parse(value: Option<&OsStr>, debug: bool, fixture: bool) -> Result<Self, SelectorError> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(Self::Packaged);
        };
        if value != OsStr::new("1") {
            return Err(SelectorError::Invalid);
        }
        if !debug {
            return Err(SelectorError::Release);
        }
        if fixture {
            return Err(SelectorError::Fixture);
        }
        Ok(Self::LocalGo)
    }

    fn origin(self, packaged_origin: &str) -> &str {
        match self {
            Self::Packaged => packaged_origin,
            Self::LocalGo => LOCAL_GO_ORIGIN,
        }
    }

    fn agent(self) -> ureq::Agent {
        let config = ureq::Agent::config_builder().max_redirects(0);
        // A process-level HTTP proxy must never receive a local user bearer.
        let config = match self {
            Self::Packaged => config,
            Self::LocalGo => config.proxy(None),
        };
        config.build().new_agent()
    }

    fn url(self, packaged_origin: &str, declared_url: &str) -> Result<String, TransportError> {
        let suffix = declared_url
            .strip_prefix(packaged_origin)
            .filter(|suffix| suffix.starts_with('/') && !suffix.starts_with("//"))
            .ok_or(TransportError::Unreachable)?;
        Ok(format!("{}{suffix}", self.origin(packaged_origin)))
    }
}

fn selection() -> Result<Selection, SelectorError> {
    *SELECTION.get_or_init(|| {
        Selection::parse(
            std::env::var_os(SELECTOR).as_deref(),
            cfg!(debug_assertions),
            cfg!(ds_messaging_emulator),
        )
    })
}

/// Freeze and validate the process's transport before loading any native
/// profile or restoring protected credentials. No later call rereads the env.
pub(crate) fn capture_for_profile() -> Result<(), Failure> {
    selection().map(|_| ()).map_err(|error| {
        let message = match error {
            SelectorError::Invalid => "DS_NATIVE_LOCAL_GO must be absent, empty, or exactly 1",
            SelectorError::Release => "DS_NATIVE_LOCAL_GO=1 requires a debug executable",
            SelectorError::Fixture => "local Go transport cannot use the messaging fixture build",
        };
        Failure::invalid("native_profile_unsafe", message)
            .remedy("unset DS_NATIVE_LOCAL_GO, or use exactly 1 in an ordinary debug build")
    })
}

pub(crate) fn is_local() -> Result<bool, TransportError> {
    selection()
        .map(|selection| selection == Selection::LocalGo)
        .map_err(|_| TransportError::Unreachable)
}

pub(crate) fn api_origin(packaged_origin: &str) -> Result<&str, TransportError> {
    selection()
        .map(|selection| selection.origin(packaged_origin))
        .map_err(|_| TransportError::Unreachable)
}

/// Preserve the exact typed call's path and query. Neither a caller URL nor a
/// storage URL enters this adapter; callers pass only core-issued API calls.
pub(crate) fn api_url(packaged_origin: &str, declared_url: &str) -> Result<String, TransportError> {
    selection()
        .map_err(|_| TransportError::Unreachable)?
        .url(packaged_origin, declared_url)
}

pub(crate) fn api_agent() -> Result<ureq::Agent, TransportError> {
    selection()
        .map(Selection::agent)
        .map_err(|_| TransportError::Unreachable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_go_selector_is_explicit_debug_only_and_never_a_url() {
        assert_eq!(Selection::parse(None, true, false), Ok(Selection::Packaged));
        assert_eq!(
            Selection::parse(Some(OsStr::new("")), false, false),
            Ok(Selection::Packaged)
        );
        assert_eq!(
            Selection::parse(Some(OsStr::new("1")), true, false),
            Ok(Selection::LocalGo)
        );
        assert_eq!(
            Selection::parse(Some(OsStr::new("1")), false, false),
            Err(SelectorError::Release)
        );
        assert_eq!(
            Selection::parse(Some(OsStr::new("1")), true, true),
            Err(SelectorError::Fixture)
        );
        for value in [
            "0",
            "true",
            " 1",
            "1\n",
            "http://127.0.0.1:8080",
            "http://localhost:8080",
            "8081",
        ] {
            assert_eq!(
                Selection::parse(Some(OsStr::new(value)), true, false),
                Err(SelectorError::Invalid)
            );
        }
    }

    #[test]
    fn local_go_selection_freezes_once_even_when_later_configuration_differs() {
        let captured = OnceLock::new();
        assert_eq!(
            *captured.get_or_init(|| Selection::parse(Some(OsStr::new("1")), true, false)),
            Ok(Selection::LocalGo)
        );
        assert_eq!(
            *captured.get_or_init(|| Selection::parse(None, true, false)),
            Ok(Selection::LocalGo)
        );
    }

    #[test]
    fn local_go_destination_preserves_the_profile_origin_and_disables_proxy_redirects() {
        let profile = crate::test_support::profile();
        let gateway = profile.gateway_origin().to_owned();
        let profile_digest = profile.profile_sha256().to_owned();
        let audience = profile.credential_audience_sha256().to_owned();
        assert_eq!(Selection::Packaged.origin(&gateway), gateway);
        assert_eq!(Selection::LocalGo.origin(&gateway), LOCAL_GO_ORIGIN);
        let agent = Selection::LocalGo.agent();
        assert!(agent.config().proxy().is_none());
        assert_eq!(agent.config().max_redirects(), 0);
        assert_eq!(profile.gateway_origin(), gateway);
        assert_eq!(profile.profile_sha256(), profile_digest);
        assert_eq!(profile.credential_audience_sha256(), audience);
    }

    #[test]
    fn local_go_only_moves_closed_api_urls_and_preserves_exact_query() {
        let origin = "https://fixture.ue.gateway.dev";
        let url = format!("{origin}/api/v1/user/projects?status=template");
        assert_eq!(Selection::Packaged.url(origin, &url).unwrap(), url);
        assert_eq!(
            Selection::LocalGo.url(origin, &url).unwrap(),
            "http://127.0.0.1:8080/api/v1/user/projects?status=template"
        );
        for invalid in [
            "https://storage.googleapis.com/example/object",
            "https://identitytoolkit.googleapis.com/v1/accounts",
            "https://fixture.ue.gateway.dev.evil.test/api/v1/styles",
            "https://fixture.ue.gateway.dev//evil.test/styles",
        ] {
            assert!(Selection::LocalGo.url(origin, invalid).is_err());
        }
    }
}
