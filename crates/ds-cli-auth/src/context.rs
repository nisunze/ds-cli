//! Provider-independent authenticated identity projection.
//!
//! This module contains identity and authority only. Credential bytes remain
//! inside the provider and the closed native client core.

use ds_cli_contract::{AuthorityCapability, Failure};
use ds_client_core::{AuthenticatedUser, ClientProfile, DeviceAuthContext};
use serde::Serialize;
use serde_json::json;

pub const AUTH_CONTEXT_SCHEMA: &str = "ds.auth-context/v1";

/// Complete key for deciding whether paired Desktop may supply the same
/// principal authority as a headless provider. Canonical email is deliberately
/// absent: it is display metadata, not the stable principal key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIdentity {
    lane: String,
    credential_audience_sha256: String,
    uid: String,
}

impl ProviderIdentity {
    pub fn new(lane: &str, credential_audience_sha256: &str, uid: &str) -> Result<Self, Failure> {
        if !bounded_identity(lane, 32)
            || !valid_digest(credential_audience_sha256)
            || !bounded_identity(uid, 256)
        {
            return Err(Failure::invalid(
                "auth_context_unreadable",
                "the provider identity fence is malformed",
            )
            .remedy("update ds and retry without changing protected state"));
        }
        Ok(Self {
            lane: lane.to_owned(),
            credential_audience_sha256: credential_audience_sha256.to_owned(),
            uid: uid.to_owned(),
        })
    }

    pub fn lane(&self) -> &str {
        &self.lane
    }

    pub fn credential_audience_sha256(&self) -> &str {
        &self.credential_audience_sha256
    }

    pub fn uid(&self) -> &str {
        &self.uid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTarget {
    /// An explicit user/project operation that remains valid without a map.
    Headless,
    /// An operation whose result is injected into or depends on the paired
    /// map/shared projection.
    MapAttached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSelection {
    PairedDesktop,
    HeadlessDevice,
}

/// Select Desktop only for one exact canonical provider identity.
///
/// A mismatch never changes either provider's account, lane, audience, or
/// project. Explicit headless work stays map-independent; a map-attached path
/// fails closed because injecting state across identities is forbidden.
pub fn arbitrate_provider(
    headless: &ProviderIdentity,
    desktop: &ProviderIdentity,
    target: ProviderTarget,
    headless_project: Option<&str>,
    desktop_project: Option<&str>,
) -> Result<ProviderSelection, Failure> {
    let same_lane = headless.lane == desktop.lane;
    let same_audience = headless.credential_audience_sha256 == desktop.credential_audience_sha256;
    let same_uid = headless.uid == desktop.uid;
    let same_project = match (headless_project, desktop_project) {
        (None, None) => true,
        (None, Some(_)) if target == ProviderTarget::MapAttached => true,
        (Some(headless), Some(desktop)) => headless == desktop,
        _ => false,
    };
    if same_lane && same_audience && same_uid && same_project {
        return Ok(ProviderSelection::PairedDesktop);
    }
    if target == ProviderTarget::Headless {
        return Ok(ProviderSelection::HeadlessDevice);
    }
    Err(Failure::conflict(
        "auth_context_mismatch",
        "the paired map and headless provider do not represent the same canonical identity",
    )
    .remedy("use the explicit headless command without map attachment, or align the intended Desktop account and lane")
    .detail(json!({
        "same_lane": same_lane,
        "same_credential_audience": same_audience,
        "same_uid": same_uid,
        "same_project": same_project,
    })))
}

fn bounded_identity(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The narrow seam a credential implementation exposes to command code.
///
/// Resolving may rotate protected provider state. It may never return raw
/// credential material. Project mutation, device approval, and revocation
/// remain typed provider operations rather than fields on this projection.
pub(crate) trait CredentialProvider {
    fn kind(&self) -> CredentialProviderKind;
    fn resolve_context(&mut self) -> Result<AuthContext, Failure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialProviderKind {
    None,
    NativeRefresh,
    DsDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    SignedOut,
    Active,
}

/// Optional paired-map observation, independent of the operation target.
///
/// A native/headless provider reports `Unobserved`: resolving identity must
/// not probe or launch Desktop. A composed Desktop provider may populate the
/// other states without changing the selected project used by headless work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MapState {
    Unobserved,
    Unavailable,
    Paired {
        active_project: Option<MapProjectAddress>,
    },
    Active {
        active_project: Option<MapProjectAddress>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapProjectAddress {
    ds_project: String,
    project_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CanonicalPrincipal {
    uid: String,
    email: String,
}

impl CanonicalPrincipal {
    pub fn uid(&self) -> &str {
        &self.uid
    }

    pub fn email(&self) -> &str {
        &self.email
    }
}

/// Non-secret immutable profile and audience fence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileFence {
    client_id: String,
    source_revision: String,
    catalog_entry_sha256: String,
    profile_sha256: String,
    credential_audience_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelectedProject {
    ds_project: String,
    project_name: String,
    display_name: Option<String>,
    role: Option<String>,
    status: String,
}

impl SelectedProject {
    pub(crate) fn new(
        ds_project: &str,
        project_name: &str,
        display_name: Option<&str>,
        role: Option<&str>,
        status: &str,
    ) -> Self {
        Self {
            ds_project: ds_project.to_owned(),
            project_name: project_name.to_owned(),
            display_name: display_name.map(str::to_owned),
            role: role.map(str::to_owned),
            status: status.to_owned(),
        }
    }

    pub fn project_id(&self) -> &str {
        &self.ds_project
    }
}

/// Public device attribution only. Private device keys never enter this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceIdentity {
    device_id: String,
    name: String,
    public_key_fingerprint: String,
}

/// One canonical, non-secret authorization projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthContext {
    schema: &'static str,
    principal: Option<CanonicalPrincipal>,
    lane: String,
    profile: ProfileFence,
    authority_capabilities: Vec<AuthorityCapability>,
    selected_project: Option<SelectedProject>,
    credential_provider: CredentialProviderKind,
    device_identity: Option<DeviceIdentity>,
    map_state: MapState,
    session_state: SessionState,
}

impl AuthContext {
    pub(crate) fn signed_out(lane: &str, profile: &ClientProfile) -> Self {
        Self::build(
            lane,
            profile_fence(profile),
            None,
            None,
            CredentialProviderKind::None,
            None,
            SessionState::SignedOut,
        )
    }

    pub(crate) fn restored(
        lane: &str,
        profile: &ClientProfile,
        user: &AuthenticatedUser,
        selected_project: Option<SelectedProject>,
        provider: CredentialProviderKind,
        device_identity: Option<DeviceIdentity>,
    ) -> Self {
        Self::build(
            lane,
            profile_fence(profile),
            Some(CanonicalPrincipal {
                uid: user.uid().to_owned(),
                email: user.email().to_owned(),
            }),
            selected_project,
            provider,
            device_identity,
            SessionState::Active,
        )
    }

    pub(crate) fn restored_device(
        profile: &ClientProfile,
        device: &DeviceAuthContext,
        selected_project: Option<SelectedProject>,
    ) -> Self {
        Self::build(
            device.lane(),
            profile_fence(profile),
            Some(CanonicalPrincipal {
                uid: device.uid().to_owned(),
                email: device.email().to_owned(),
            }),
            selected_project,
            CredentialProviderKind::DsDevice,
            Some(DeviceIdentity {
                device_id: device.device_id().to_owned(),
                name: device.device_name().to_owned(),
                public_key_fingerprint: device.fingerprint().to_owned(),
            }),
            SessionState::Active,
        )
    }

    fn build(
        lane: &str,
        profile: ProfileFence,
        principal: Option<CanonicalPrincipal>,
        selected_project: Option<SelectedProject>,
        credential_provider: CredentialProviderKind,
        device_identity: Option<DeviceIdentity>,
        session_state: SessionState,
    ) -> Self {
        let authority_capabilities = if principal.is_none() {
            vec![AuthorityCapability::None]
        } else if selected_project.is_some() {
            vec![
                AuthorityCapability::None,
                AuthorityCapability::User,
                AuthorityCapability::Project,
            ]
        } else {
            vec![AuthorityCapability::None, AuthorityCapability::User]
        };
        Self {
            schema: AUTH_CONTEXT_SCHEMA,
            principal,
            lane: lane.to_owned(),
            profile,
            authority_capabilities,
            selected_project,
            credential_provider,
            device_identity,
            map_state: MapState::Unobserved,
            session_state,
        }
    }

    pub fn principal(&self) -> Option<&CanonicalPrincipal> {
        self.principal.as_ref()
    }

    pub fn selected_project(&self) -> Option<&SelectedProject> {
        self.selected_project.as_ref()
    }

    pub fn session_state(&self) -> SessionState {
        self.session_state
    }

    pub fn has_capability(&self, capability: AuthorityCapability) -> bool {
        self.authority_capabilities.contains(&capability)
    }
}

fn profile_fence(profile: &ClientProfile) -> ProfileFence {
    ProfileFence {
        client_id: profile.client_id().to_owned(),
        source_revision: profile.source_revision().to_owned(),
        catalog_entry_sha256: profile.descriptor_sha256().to_owned(),
        profile_sha256: profile.profile_sha256().to_owned(),
        credential_audience_sha256: profile.credential_audience_sha256().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ProfileFence {
        ProfileFence {
            client_id: "ds-native-client".to_owned(),
            source_revision: "revision-1".to_owned(),
            catalog_entry_sha256: "a".repeat(64),
            profile_sha256: "b".repeat(64),
            credential_audience_sha256: "c".repeat(64),
        }
    }

    #[test]
    fn signed_out_context_has_only_none_capability() {
        let context = AuthContext::build(
            "stable",
            profile(),
            None,
            None,
            CredentialProviderKind::None,
            None,
            SessionState::SignedOut,
        );
        assert!(context.has_capability(AuthorityCapability::None));
        assert!(!context.has_capability(AuthorityCapability::User));
        assert_eq!(context.session_state(), SessionState::SignedOut);
    }

    #[test]
    fn selected_project_adds_project_without_desktop_or_map() {
        let context = AuthContext::build(
            "stable",
            profile(),
            Some(CanonicalPrincipal {
                uid: "uid-1".to_owned(),
                email: "operator@example.com".to_owned(),
            }),
            Some(SelectedProject::new(
                "project-1",
                "Project One",
                None,
                Some("editor"),
                "active",
            )),
            CredentialProviderKind::NativeRefresh,
            None,
            SessionState::Active,
        );
        assert!(context.has_capability(AuthorityCapability::User));
        assert!(context.has_capability(AuthorityCapability::Project));
        assert!(!context.has_capability(AuthorityCapability::Desktop));
        assert!(!context.has_capability(AuthorityCapability::Map));
    }

    #[test]
    fn serialized_context_has_no_credential_fields_or_material() {
        let marker = "secret-refresh-marker";
        let context = AuthContext::build(
            "stable",
            profile(),
            Some(CanonicalPrincipal {
                uid: "uid-1".to_owned(),
                email: "operator@example.com".to_owned(),
            }),
            None,
            CredentialProviderKind::NativeRefresh,
            None,
            SessionState::Active,
        );
        let value = serde_json::to_value(context).expect("serializable context");
        let encoded = serde_json::to_string(&value).expect("encoded context");
        assert!(!encoded.contains(marker));
        for forbidden in [
            "password",
            "id_token",
            "refresh_token",
            "authorization",
            "bearer",
            "session_secret",
            "private_key",
        ] {
            assert!(value.get(forbidden).is_none(), "exposed `{forbidden}`");
            assert!(!encoded.contains(&format!("\"{forbidden}\"")));
        }
        assert_eq!(value["map_state"]["state"], "unobserved");
    }

    #[test]
    fn provider_arbitration_uses_lane_stable_audience_and_uid_only() {
        let audience = "a".repeat(64);
        let headless = ProviderIdentity::new("stable", &audience, "uid-a").unwrap();
        let exact = ProviderIdentity::new("stable", &audience, "uid-a").unwrap();
        assert_eq!(
            arbitrate_provider(&headless, &exact, ProviderTarget::Headless, None, None).unwrap(),
            ProviderSelection::PairedDesktop
        );

        let other_uid = ProviderIdentity::new("stable", &audience, "uid-b").unwrap();
        assert_eq!(
            arbitrate_provider(&headless, &other_uid, ProviderTarget::Headless, None, None,)
                .unwrap(),
            ProviderSelection::HeadlessDevice,
            "a mismatched map must not limit explicit headless work"
        );
        assert_eq!(
            arbitrate_provider(
                &headless,
                &other_uid,
                ProviderTarget::MapAttached,
                None,
                None,
            )
            .unwrap_err()
            .code(),
            "auth_context_mismatch"
        );
        assert_eq!(
            arbitrate_provider(
                &headless,
                &exact,
                ProviderTarget::MapAttached,
                None,
                Some("map-project"),
            )
            .unwrap(),
            ProviderSelection::PairedDesktop,
            "an authorized map supplies its own project when headless has no selection"
        );

        for separate in [
            ProviderIdentity::new("canary", &audience, "uid-a").unwrap(),
            ProviderIdentity::new("stable", &"b".repeat(64), "uid-a").unwrap(),
        ] {
            assert_eq!(
                arbitrate_provider(&headless, &separate, ProviderTarget::Headless, None, None,)
                    .unwrap(),
                ProviderSelection::HeadlessDevice
            );
            assert_eq!(
                arbitrate_provider(
                    &headless,
                    &separate,
                    ProviderTarget::MapAttached,
                    None,
                    None,
                )
                .unwrap_err()
                .code(),
                "auth_context_mismatch"
            );
        }

        assert_eq!(
            arbitrate_provider(
                &headless,
                &exact,
                ProviderTarget::Headless,
                Some("project-a"),
                Some("project-b"),
            )
            .unwrap(),
            ProviderSelection::HeadlessDevice,
            "an open map on another project must not steal a project-bound headless operation"
        );
        assert_eq!(
            arbitrate_provider(
                &headless,
                &exact,
                ProviderTarget::MapAttached,
                Some("project-a"),
                Some("project-b"),
            )
            .unwrap_err()
            .code(),
            "auth_context_mismatch"
        );
    }
}
