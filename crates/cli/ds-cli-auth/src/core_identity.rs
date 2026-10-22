//! Offline projections of Core's existing protected identity, never another owner.
use crate::{context::ProviderIdentity, profile::Lane, state::NativeRefreshStore};
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Execution, Refusal, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

const INSTALL: Refusal = Refusal {
    code: "headless_install_unavailable",
    when: "Core has no valid retained installation identity in the selected lane",
    remedy: "authenticate and start Core in this lane once; the shell never creates its own identity",
};
const READ_REFUSALS: &[Refusal] = &[
    crate::PROFILE_REFUSAL,
    crate::PROFILE_DIGEST_REFUSAL,
    crate::PROFILE_UNSAFE_REFUSAL,
    crate::STATE_REFUSAL,
    crate::STATE_UNAVAILABLE_REFUSAL,
    crate::STATE_PROTECTION_REFUSAL,
    crate::STATE_ROOT_REFUSAL,
    crate::STATE_CONFLICT_REFUSAL,
    crate::AUTH_CONTEXT_REFUSAL,
    crate::AUTH_CONTEXT_MISMATCH_REFUSAL,
    INSTALL,
];

pub static IDENTITY: Command = Command {
    id: "core.identity",
    path: &["core", "identity"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Read this lane's existing Core installation identity offline.",
    purpose: "Read Core's protected installation UUID and this executable's runtime declaration. Does not create an identity, restore credentials, refresh, acquire a lock, contact a gateway or read a Desktop store. A missing or malformed identity refuses; Desktop must never replace it with its own UUID. Core appVersion is distinct from the optional shell version.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::LANE],
    output: "installId, device, arch, appVersion, channel and osVersion; no private paths or credentials.",
    examples: &[],
    refusals: READ_REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &[],
    requires: Requires::Server,
    availability: crate::state::availability,
};
pub static AUTH_STATUS: Command = Command {
    id: "core.auth.status",
    path: &["core", "auth", "status"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Observe Core's retained authentication identity without refresh.",
    purpose: "Inspect the selected lane's protected Firebase and device providers without refreshing credentials, writing state, selecting a project or contacting the network. Providers must agree on UID, lane and credential audience. Signed-out returns signedIn false. Project is null because this read carries no project context; individual operations still require their explicit governed project.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::LANE],
    output: "signedIn, uid, lane, audienceSha256 and null project; never tokens, keys or credential-instance identifiers.",
    examples: &[],
    refusals: READ_REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["offline"],
    requires: Requires::Server,
    availability: crate::native_availability,
};

pub fn identity(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let install_id = crate::state::read_only_install_file(lane.token())?
        .map(ds_edge_authority::read_install_id)
        .transpose()
        .map_err(|error| Failure::unavailable(INSTALL.code, error).remedy(INSTALL.remedy))?
        .ok_or_else(|| {
            Failure::unavailable(INSTALL.code, "Core installation has not been initialized")
                .remedy(INSTALL.remedy)
        })?;
    Ok(identity_projection(&install_id, lane.token()))
}
fn identity_projection(install_id: &str, lane: &str) -> Value {
    json!({"installId": install_id, "device": std::env::consts::OS,
        "arch": std::env::consts::ARCH, "appVersion": env!("DS_NATIVE_APP_VERSION"),
        "channel": lane, "osVersion": std::env::consts::OS})
}
pub fn auth_status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let profile = crate::profile::load(lane)?;
    let refresh = NativeRefreshStore::probe_read_only(&profile)?
        .map(|context| {
            ProviderIdentity::new(
                context.lane(),
                context.credential_audience_sha256(),
                context.uid(),
            )
        })
        .transpose()?;
    auth_projection(
        lane.token(),
        refresh,
        crate::device::probe_identity_read_only(lane)?,
    )
}
fn auth_projection(
    lane: &str,
    refresh: Option<ProviderIdentity>,
    device: Option<ProviderIdentity>,
) -> Result<Value, Failure> {
    let identity = match (refresh, device) {
        (None, None) => None,
        (Some(identity), None) | (None, Some(identity)) => Some(identity),
        (Some(refresh), Some(device)) if refresh == device => Some(refresh),
        _ => {
            return Err(Failure::conflict(
                crate::AUTH_CONTEXT_MISMATCH_REFUSAL.code,
                "Core's retained providers disagree on principal, lane or audience",
            )
            .remedy(crate::AUTH_CONTEXT_MISMATCH_REFUSAL.remedy));
        }
    };
    if identity
        .as_ref()
        .is_some_and(|identity| identity.lane() != lane)
    {
        return Err(Failure::conflict(
            crate::AUTH_CONTEXT_MISMATCH_REFUSAL.code,
            "Core's retained provider belongs to a different lane",
        )
        .remedy(crate::AUTH_CONTEXT_MISMATCH_REFUSAL.remedy));
    }
    Ok(
        json!({"signedIn": identity.is_some(), "uid": identity.as_ref().map(ProviderIdentity::uid),
        "lane": lane, "audienceSha256": identity.as_ref().map(ProviderIdentity::credential_audience_sha256), "project": null}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provider(uid: &str, lane: &str, audience: char) -> ProviderIdentity {
        ProviderIdentity::new(lane, &audience.to_string().repeat(64), uid).unwrap()
    }
    #[test]
    fn retained_identity_projection_has_no_credential_or_shell_state() {
        let identity = identity_projection("550e8400-e29b-41d4-a716-446655440000", "canary");
        assert_eq!(
            identity["installId"],
            "550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(identity["channel"], "canary");
        assert_eq!(identity.as_object().unwrap().len(), 6);
        assert_eq!(IDENTITY.effect, Effect::ReadOnly);
        assert_eq!(AUTH_STATUS.effect, Effect::ReadOnly);
    }
    #[test]
    fn signed_out_and_matching_providers_have_one_bounded_projection() {
        assert_eq!(
            auth_projection("canary", None, None).unwrap(),
            json!({"signedIn": false, "uid": null, "lane": "canary", "audienceSha256": null, "project": null})
        );
        let value = auth_projection(
            "canary",
            Some(provider("owner", "canary", 'a')),
            Some(provider("owner", "canary", 'a')),
        )
        .unwrap();
        assert_eq!(value["uid"], "owner");
        assert_eq!(value["signedIn"], true);
        assert_eq!(value.as_object().unwrap().len(), 5);
    }
    #[test]
    fn crossed_uid_lane_and_audience_refuse_without_arbitrating_a_winner() {
        for other in [
            provider("other", "canary", 'a'),
            provider("owner", "stable", 'a'),
            provider("owner", "canary", 'b'),
        ] {
            assert_eq!(
                auth_projection(
                    "canary",
                    Some(provider("owner", "canary", 'a')),
                    Some(other)
                )
                .unwrap_err()
                .code(),
                "auth_context_mismatch"
            );
        }
        assert_eq!(
            auth_projection("canary", Some(provider("owner", "stable", 'a')), None)
                .unwrap_err()
                .code(),
            "auth_context_mismatch"
        );
    }
}
