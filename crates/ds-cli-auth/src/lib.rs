//! Thin CLI host for ds-web's closed native client core.
//!
//! This crate owns terminal input, package resources, native HTTP, and private
//! per-user files. Identity, Firebase validation, refresh rotation, and the
//! project response contract remain exclusively in `ds-client-core`.

pub mod account;
mod context;
pub mod correspondence;
pub mod device;
pub mod link_approval;
mod profile;
mod state;
#[cfg(windows)]
mod state_windows;
pub mod sync;
#[cfg(test)]
mod test_support;
mod transport;
mod upload;

pub use account::{
    APPROVAL_INSTRUCTIONS, SIGNED_OUT_NEXT, SIGNED_OUT_REFUSAL, SIGNED_OUT_REMEDY, signed_out_next,
    signed_out_remedy,
};

/// The weak-network acceptance seam. Feature-gated, so it exists only for
/// `crates/ds-cli-auth/tests/weak_network.rs` and never in a release build;
/// see the module's own header for why an integration test needs it.
#[cfg(feature = "weak-network-harness")]
pub use upload::weak_network_harness;

#[cfg(unix)]
use std::io::Write;
use std::io::{self, BufRead, Read};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use context::CredentialProvider;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Domain, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{
    Client, ClientError, ErrorKind, Project, ProjectDirectory, ProjectFormSettingsEditor,
    ProjectFormsSnapshot, ProjectReportServiceCode, ProjectStatus, SolarSnapshot,
    SurveyEntriesChanges, SurveyEntriesChangesRequest, SurveyEntriesChangesServiceCode,
    SurveyEntriesSelectRequest, SurveyEntriesSelectServiceCode, SurveyEntriesSelection,
    SurveyEntryCreateReceipt, SurveyEntryCreateRequest, SurveyEntryCreateServiceCode,
    SurveyFormReadServiceCode, SurveyQueryRequest, SurveyQueryResult, SurveyQueryServiceCode,
    TransformerContext,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use state::{NativeRefreshStore, ProjectContextLease};
use transport::NativeTransport;
use zeroize::Zeroize;

pub use context::{
    AuthContext, CanonicalPrincipal, CredentialProviderKind, DeviceIdentity, MapProjectAddress,
    MapState, ProfileFence, ProviderIdentity, ProviderSelection, ProviderTarget, SelectedProject,
    SessionState, arbitrate_provider,
};
pub use ds_client_core::{
    BundleDownloadReceipt, ContourParameters, DataDistributionRequest, PrintContextKind,
};
pub use ds_client_core::{
    CompoundedArchive, CompoundedArchiveLayout, CompoundedReportReceipt, CompoundedReportRequest,
    CompoundedReportStatus, ExportReportOutcome, ExportReportResult, ExportReportsReceipt,
    LayerOrder, LayerOrderReceipt, LayerSnapshot, LayerVisibilityDefault,
    LayerVisibilityDefaultReceipt, PROJECT_REPORT_MAX_REASON_CHARS,
    PROJECT_REPORT_MAX_TRANSFORMER_CHARS, PROJECT_REPORT_MAX_TRANSFORMERS, ReportFileLevel,
    RetirementAction, RetirementReceipt, RetirementRecord, RetirementRefusal, RetirementRequest,
    RetirementResult, StyleEditReceipt, StyleInstruction, StyleSnapshot, TileCatalog, TileMutation,
    TileOperationResult, TileOperationStatus, TilePreflight, TilePreflightLayer,
    TilePreflightStatus, TileScope, TileType, TransformerInventory, TransformerInventoryRow,
    TransformerKind, TransformerLifecycle, TransformerSet, TransformerStatusList,
    TransformerStatusRow,
};
pub use ds_client_core::{SolarCalculationArtifactFinalize, SolarCalculationArtifactOpen};
pub use profile::Lane;

/// The remedy the transformer-context route's own rejection carries.
///
/// Attached by [`route_diagnostic`] only when the route itself answered, which
/// is the one case where re-authenticating is pointless. It is deliberately
/// *not* what the commands on that route declare: the same `auth_rejected`
/// code is also minted when the Firebase refresh ahead of the call is
/// rejected, and there the credential was never verified. A declaration is
/// read without knowing which happened, so it stays general and this stays the
/// runtime specialisation. Published so a caller can recognise the text.
pub const TRANSFORMER_CONTEXT_ROUTE_REMEDY: &str = "this lane's credential was verified for this call and the transformer-context route rejected it anyway; confirm the same credential still reads the project, then report a route-only rejection that no local change fixes";

/// Observe all durable headless providers without network or token output.
/// Two providers may coexist only when they name one exact canonical UID,
/// lane, audience and selected project.
pub fn probe_headless_identity(
    lane_token: &str,
) -> Result<Option<(ProviderIdentity, Option<String>)>, Failure> {
    probe_headless_providers(lane_token, Selection::Compared)
}

/// The same observation for an operation whose project the CALLER named: the
/// saved selection is not read at all, so nothing about it — not its absence,
/// not two providers disagreeing about it — can decide a call that never
/// wanted it.
pub fn probe_headless_identity_for_named_project(
    lane_token: &str,
) -> Result<Option<ProviderIdentity>, Failure> {
    Ok(probe_headless_providers(lane_token, Selection::Unread)?.map(|(identity, _)| identity))
}

/// Whether this observation is about the saved selection at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    /// Read it from both providers and require them to agree.
    Compared,
    /// Do not read it. The project is the caller's.
    Unread,
}

fn probe_headless_providers(
    lane_token: &str,
    selection: Selection,
) -> Result<Option<(ProviderIdentity, Option<String>)>, Failure> {
    let lane = Lane::parse(lane_token)?;
    let profile = profile::load(lane)?;
    let refresh = NativeRefreshStore::probe(&profile)?
        .map(|context| {
            let identity = ProviderIdentity::new(
                context.lane(),
                context.credential_audience_sha256(),
                context.uid(),
            )?;
            let project = match selection {
                Selection::Compared => {
                    ProjectContextLease::probe_selected(&profile, context.uid())?
                }
                Selection::Unread => None,
            };
            Ok::<_, Failure>((identity, project))
        })
        .transpose()?;
    let device = match selection {
        Selection::Compared => device::probe_identity(lane)?,
        Selection::Unread => {
            device::probe_identity_without_selection(lane)?.map(|identity| (identity, None))
        }
    };
    match (refresh, device) {
        (None, None) => Ok(None),
        (Some(provider), None) | (None, Some(provider)) => Ok(Some(provider)),
        (Some(refresh), Some(device)) => {
            let same_identity = refresh.0 == device.0;
            let same_project = refresh.1 == device.1;
            if !same_identity || !same_project {
                return Err(Failure::conflict(
                    "auth_context_mismatch",
                    "the protected Firebase and DS device providers disagree on canonical identity",
                )
                .remedy(
                    "do not attach to a map; explicitly sign out or revoke the unintended provider",
                ));
            }
            Ok(Some(device))
        }
    }
}

/// Observe the provider/device binding without exposing its credentials.
/// The caller separately verifies the account and refreshes native authority.
pub fn runtime_credential_binding(lane_value: &str) -> Result<String, Failure> {
    let lane = Lane::parse(lane_value)?;
    Ok(match device::probe_context(lane)? {
        Some(device) => format!("ds_device:{}:{}", device.device_id(), device.fingerprint()),
        None => {
            let profile = profile::load(lane)?;
            let context = NativeRefreshStore::probe(&profile)?.ok_or_else(|| {
                Failure::conflict("headless_signed_out", "the server has no native identity")
                    .remedy("sign in under the server's Linux account")
            })?;
            format!("firebase:{}", context.credential_instance_sha256())
        }
    })
}

/// Refresh native authority while preserving its canonical account identity.
pub fn refresh_runtime_identity(lane_value: &str) -> Result<ProviderIdentity, Failure> {
    let lane = Lane::parse(lane_value)?;
    let before = probe_headless_identity(lane_value)?
        .ok_or_else(|| {
            Failure::conflict("headless_signed_out", "the server has no native identity")
                .remedy(signed_out_remedy(lane_value))
                .next(signed_out_next(lane_value))
        })?
        .0;
    if device::restore_session(lane)?.is_none() {
        let profile = profile::load(lane)?;
        let store = NativeRefreshStore::open()?;
        let mut client = Client::new(profile, NativeTransport, store);
        let user = require_restore_before_context(&mut client)?;
        if user.uid() != before.uid() {
            return Err(Failure::conflict(
                "auth_identity_mismatch",
                "native identity changed while restoring the server",
            ));
        }
    }
    let after = probe_headless_identity(lane_value)?
        .ok_or_else(|| Failure::conflict("headless_signed_out", "native identity was removed"))?
        .0;
    if before != after {
        return Err(Failure::conflict(
            "auth_context_mismatch",
            "native identity changed during server authorization",
        ));
    }
    Ok(after)
}

/// The non-secret native context a durable local host needs to enter the
/// shared Sync Center store. It is deliberately not a gateway credential:
/// callers may fence local rows and leases, but cannot turn it into a general
/// authenticated HTTP client.
pub struct HeadlessSyncContext {
    account_uid: String,
    deployment: String,
    project_id: String,
}

impl HeadlessSyncContext {
    pub fn account_uid(&self) -> &str {
        &self.account_uid
    }

    pub fn deployment(&self) -> &str {
        &self.deployment
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

/// Restore the selected native project after refreshing the exact provider
/// identity. The returned context is fenced to the caller's lane and profile;
/// a server still has no bearer or arbitrary gateway route from this API.
pub fn headless_sync_context(lane_value: &str) -> Result<HeadlessSyncContext, Failure> {
    let lane = Lane::parse(lane_value)?;
    let identity = refresh_runtime_identity(lane.token())?;
    let (_, selected_project) = probe_headless_identity(lane.token())?.ok_or_else(|| {
        Failure::conflict("headless_signed_out", "the server has no native identity")
            .remedy("sign in under the server's Linux account")
    })?;
    let project_id = selected_project.ok_or_else(|| {
        Failure::conflict(
            "headless_project_not_selected",
            "no project is selected for this native user, lane, and credential audience",
        )
        .remedy("run ds auth project use --project <exact-id>")
    })?;
    let profile = profile::load(lane)?;
    Ok(HeadlessSyncContext {
        account_uid: identity.uid().to_owned(),
        deployment: profile.gateway_origin().to_owned(),
        project_id,
    })
}

/// The connection identity a durable local host has **without** a selected
/// project: the account, the deployment it is bound to, and the registered
/// install. A host that admits a project per operation needs exactly this and
/// must not be made to pick one at startup to obtain it. Read from the
/// protected native state on this machine — no refresh, no network, and no
/// saved selection, not even to compare one — so a host can stand up with no
/// upstream present and on an account that has never selected a project.
pub struct HeadlessPrincipal {
    account_uid: String,
    deployment: String,
    install_id: String,
}

impl HeadlessPrincipal {
    pub fn account_uid(&self) -> &str {
        &self.account_uid
    }
    pub fn deployment(&self) -> &str {
        &self.deployment
    }
    pub fn install_id(&self) -> &str {
        &self.install_id
    }
}

pub fn headless_principal(lane_value: &str) -> Result<HeadlessPrincipal, Failure> {
    let lane = Lane::parse(lane_value)?;
    // Deliberately the observation that does NOT read the saved selection.
    // `probe_headless_identity` acquires the project-context lease and makes
    // the two providers agree on a selected project, so a host that admits a
    // project per operation would have been refused (`auth_context_mismatch`,
    // `native_state_conflict`) over a value it never uses — which is the
    // saved selection deciding whether a Server may start. It decides nothing
    // here, so it is not read here.
    let identity = probe_headless_identity_for_named_project(lane.token())?.ok_or_else(|| {
        Failure::conflict("headless_signed_out", "the server has no native identity")
            .remedy("sign in under the server's Linux account")
    })?;
    let profile = profile::load(lane)?;
    let install_id = ds_edge_authority::load_or_create_install_id(
        &state::edge_authority_dir(lane.token())?.join("install-id"),
    )
    .map_err(|error| {
        Failure::failed("headless_install_unavailable", error)
            .remedy("check the server user's protected DS state root")
    })?;
    Ok(HeadlessPrincipal {
        account_uid: identity.uid().to_owned(),
        deployment: profile.gateway_origin().to_owned(),
        install_id,
    })
}

pub static DOMAIN: Domain = Domain {
    id: "auth",
    summary: "Sign in headlessly and select a visible project.",
    commands: &[
        &STATUS_COMMAND,
        &LOGIN_COMMAND,
        &LOGOUT_COMMAND,
        &device::BEGIN_COMMAND,
        &device::STATUS_COMMAND,
        &device::COMPLETE_COMMAND,
        &link_approval::COMMAND,
        &device::LIST_COMMAND,
        &device::READ_COMMAND,
        &device::REVOKE_COMMAND,
        &PROJECT_LIST_COMMAND,
        &PROJECT_USE_COMMAND,
        &PROJECT_STATUS_COMMAND,
        &PROJECT_CREATE_COMMAND,
        &PROJECT_UPDATE_COMMAND,
    ],
};

const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);
const EMAIL: Arg = Arg::value("email", "<address>", "Firebase account email.").required();
const PASSWORD_STDIN: Arg = Arg::switch(
    "password-stdin",
    "Read one bounded password line from stdin instead of a hidden TTY prompt.",
);
const PROJECT_ID: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact ds_project id from auth project list.",
)
.required();
const LIST_LIMIT: Arg = Arg::value(
    "limit",
    "<1-1000>",
    "Maximum number of project rows emitted; default 100.",
)
.default("100");
const DISPLAY_NAME_REQUIRED: Arg = Arg::value(
    "display-name",
    "<text>",
    "The project's name as people read it; its id slug is derived from it.",
)
.required();
const DISPLAY_NAME: Arg = Arg::value(
    "display-name",
    "<text>",
    "The project's name as people read it.",
);
const COUNTRY: Arg = Arg::value("country", "<name>", "Country the project is in.");
const CLIENT: Arg = Arg::value("client", "<name>", "Client the project is for.");
const DESCRIPTION: Arg = Arg::value("description", "<text>", "What the project is.");
const LOCATION: Arg = Arg::value("location", "<text>", "Where the project is.");
const NETWORK_TEMPLATE: Arg = Arg::value(
    "network-template",
    "<id>",
    "Network template; ds-brain defaults to master.",
);
const STYLING_TEMPLATE: Arg = Arg::value("styling-template", "<id>", "Styling template id.");

pub(crate) const PROFILE_REFUSAL: Refusal = Refusal {
    code: "native_profile_not_configured",
    when: "this build lacks its exact digest-pinned two-lane native client catalog",
    remedy: "install one complete ds release containing ds-client-profiles/catalog.json",
};
pub(crate) const STATE_REFUSAL: Refusal = Refusal {
    code: "native_state_unsafe",
    when: "protected native state is unsafe, malformed, or cannot be accessed atomically",
    remedy: "repair the owner-only DS config directory and retry",
};
const PROFILE_DIGEST_REFUSAL: Refusal = Refusal {
    code: "native_profile_digest_mismatch",
    when: "the packaged catalog bytes differ from the build pin",
    remedy: "reinstall one complete ds release",
};
const PROFILE_UNSAFE_REFUSAL: Refusal = Refusal {
    code: "native_profile_unsafe",
    when: "the packaged catalog is unsafe, oversized, or malformed",
    remedy: "reinstall one complete ds release",
};
const STATE_UNAVAILABLE_REFUSAL: Refusal = Refusal {
    code: "native_state_unavailable",
    when: "protected local state cannot be accessed",
    remedy: "repair the owner-only DS config directory",
};
const STATE_PROTECTION_REFUSAL: Refusal = Refusal {
    code: "native_state_protection_unavailable",
    when: "this platform build has no protected-state adapter",
    remedy: "install a build with the native protected-state adapter",
};
const STATE_ROOT_REFUSAL: Refusal = Refusal {
    code: "native_state_root_invalid",
    when: "a configured native state root is not absolute",
    remedy: "unset it or provide an absolute path",
};
const STATE_CONFLICT_REFUSAL: Refusal = Refusal {
    code: "native_state_conflict",
    when: "another native client holds the bounded state lease",
    remedy: "retry after the other native operation finishes",
};
const CLEANUP_REFUSAL: Refusal = Refusal {
    code: "native_cleanup_required",
    when: "credential mutation succeeded but context cleanup did not",
    remedy: "repair protected state and run logout again",
};
const AUTH_INPUT_REFUSAL: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "the email or password is outside the bounded contract",
    remedy: "correct the bounded input and retry",
};
const AUTH_REJECTED_REFUSAL: Refusal = Refusal {
    code: "auth_rejected",
    when: "the fixed authentication or project service rejects the request",
    remedy: "verify the account and its project access",
};
const INVALID_CREDENTIALS_REFUSAL: Refusal = Refusal {
    code: "auth_invalid_credentials",
    when: "Firebase does not accept the protected terminal sign-in",
    remedy: SIGNED_OUT_REMEDY,
};
const PASSWORD_SIGN_IN_UNAVAILABLE_REFUSAL: Refusal = Refusal {
    code: "auth_password_sign_in_unavailable",
    when: "terminal sign-in is disabled for the Firebase project or unavailable for this account",
    remedy: SIGNED_OUT_REMEDY,
};
const ACCOUNT_DISABLED_REFUSAL: Refusal = Refusal {
    code: "auth_account_disabled",
    when: "Firebase reports that this account is disabled",
    remedy: "restore the account with an administrator, then retry",
};
const AUTH_REVOKED_REFUSAL: Refusal = Refusal {
    code: "auth_revoked",
    when: "Firebase permanently revokes the native session",
    remedy: SIGNED_OUT_REMEDY,
};
const IDENTITY_REFUSAL: Refusal = Refusal {
    code: "auth_identity_mismatch",
    when: "Firebase returns an identity outside the bound session",
    remedy: "sign in again and report a repeated mismatch",
};
const TRANSIENT_REFUSAL: Refusal = Refusal {
    code: "auth_transient",
    when: "the fixed native transport is temporarily unavailable",
    remedy: "retry without changing local state",
};
const UNREADABLE_REFUSAL: Refusal = Refusal {
    code: "auth_response_unreadable",
    when: "a fixed auth or project reply violates its bounded contract",
    remedy: "retry once, then update ds if it persists",
};
const AUTH_CONTEXT_REFUSAL: Refusal = Refusal {
    code: "auth_context_unreadable",
    when: "the bounded non-secret authenticated context cannot be projected safely",
    remedy: "update ds and retry without changing protected state",
};
const AUTH_CONTEXT_MISMATCH_REFUSAL: Refusal = Refusal {
    code: "auth_context_mismatch",
    when: "protected or paired providers disagree on principal, lane, audience, or project",
    remedy: "use matching identity context or explicitly remove the unintended provider",
};
const PASSWORD_INPUT_REFUSAL: Refusal = Refusal {
    code: "password_input_invalid",
    when: "password stdin is empty, multiline, or oversized",
    remedy: "provide exactly one bounded line",
};
const PASSWORD_PROMPT_REFUSAL: Refusal = Refusal {
    code: "password_prompt_forbidden",
    when: "a non-interactive child process attempts to open a password prompt",
    remedy: SIGNED_OUT_REMEDY,
};
const PASSWORD_TTY_REFUSAL: Refusal = Refusal {
    code: "password_tty_unavailable",
    when: "hidden input has no controlling TTY",
    remedy: "use a trusted terminal or explicit --password-stdin",
};
const CONTEXT_STALE_REFUSAL: Refusal = Refusal {
    code: "project_context_stale",
    when: "saved context belongs to another user, lane, or audience",
    remedy: "select an exact freshly visible project again",
};
const LIMIT_REFUSAL: Refusal = Refusal {
    code: "project_limit_invalid",
    when: "project list limit is outside 1 through 1000",
    remedy: "pass a limit from 1 through 1000",
};
const NOT_VISIBLE_REFUSAL: Refusal = Refusal {
    code: "project_not_visible",
    when: "the exact project id is absent from all fresh buckets",
    remedy: "choose an exact id from auth project list",
};
// The codes below are the ones `POST /api/v1/projects` actually emits through
// the shared native client. What is this command's own is the WHEN and the
// REMEDY: a 403 here names the capability a project admin has to grant.
const PROJECT_CREATE_FORBIDDEN_REFUSAL: Refusal = Refusal {
    code: "auth_rejected",
    when: "the signed-in account lacks project.create",
    remedy: "ask a platform administrator to grant project.create",
};
const PROJECT_EDIT_FORBIDDEN_REFUSAL: Refusal = Refusal {
    code: "auth_rejected",
    when: "the account lacks project.properties.edit on this project, or the project is archived or expired",
    remedy: "ask a project admin for the admin role on this project, or unarchive it first",
};
const PROJECT_PROPERTIES_INVALID_REFUSAL: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "a property is empty, untrimmed, over 200 characters (description 2000), the server refused the payload, or an update names no property",
    remedy: "correct the named property, or pass at least one property to change",
};
const PROJECT_UNKNOWN_REFUSAL: Refusal = Refusal {
    code: "project_not_visible",
    when: "no project carries this id on this lane (HTTP 404)",
    remedy: "choose an exact id from auth project list",
};

const STATUS_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    AUTH_REVOKED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
    CONTEXT_STALE_REFUSAL,
    AUTH_CONTEXT_REFUSAL,
    AUTH_CONTEXT_MISMATCH_REFUSAL,
];
const LOGIN_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    AUTH_INPUT_REFUSAL,
    INVALID_CREDENTIALS_REFUSAL,
    PASSWORD_SIGN_IN_UNAVAILABLE_REFUSAL,
    ACCOUNT_DISABLED_REFUSAL,
    AUTH_REJECTED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
    PASSWORD_INPUT_REFUSAL,
    PASSWORD_PROMPT_REFUSAL,
    PASSWORD_TTY_REFUSAL,
    AUTH_CONTEXT_MISMATCH_REFUSAL,
];
const LOGOUT_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    AUTH_CONTEXT_MISMATCH_REFUSAL,
];
const PROJECT_LIST_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    SIGNED_OUT_REFUSAL,
    AUTH_REJECTED_REFUSAL,
    AUTH_REVOKED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
    LIMIT_REFUSAL,
];
const PROJECT_USE_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    SIGNED_OUT_REFUSAL,
    AUTH_REJECTED_REFUSAL,
    AUTH_REVOKED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
    NOT_VISIBLE_REFUSAL,
];
const PROJECT_STATUS_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    SIGNED_OUT_REFUSAL,
    AUTH_REVOKED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
    CONTEXT_STALE_REFUSAL,
];
const PROJECT_CREATE_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    SIGNED_OUT_REFUSAL,
    PROJECT_CREATE_FORBIDDEN_REFUSAL,
    PROJECT_PROPERTIES_INVALID_REFUSAL,
    AUTH_REVOKED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
];
const PROJECT_UPDATE_REFUSALS: &[Refusal] = &[
    PROFILE_REFUSAL,
    PROFILE_DIGEST_REFUSAL,
    PROFILE_UNSAFE_REFUSAL,
    STATE_REFUSAL,
    STATE_UNAVAILABLE_REFUSAL,
    STATE_PROTECTION_REFUSAL,
    STATE_ROOT_REFUSAL,
    STATE_CONFLICT_REFUSAL,
    CLEANUP_REFUSAL,
    SIGNED_OUT_REFUSAL,
    PROJECT_EDIT_FORBIDDEN_REFUSAL,
    PROJECT_PROPERTIES_INVALID_REFUSAL,
    PROJECT_UNKNOWN_REFUSAL,
    AUTH_REVOKED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
];

pub static STATUS_COMMAND: Command = Command {
    id: "auth.status",
    path: &["auth", "status"],
    contract: 2,
    chapter: Chapter::Project,
    summary: "Show the native signed-in user for one lane.",
    purpose: "Restores the refresh-only native session and reports its bounded account identity. It never reads or launches the paired desktop.",
    effect: Effect::LocalAuthState,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[LANE],
    output: "Lane, signed-in status, canonical account email/UID, and a non-secret provider-independent auth context; never credential material.",
    examples: &[Example {
        command: "ds auth status",
        note: "Stable is explicit by default.",
        runnable: false,
    }],
    refusals: STATUS_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_availability,
};

pub static LOGIN_COMMAND: Command = Command {
    id: "auth.login",
    path: &["auth", "login"],
    contract: 2,
    chapter: Chapter::Project,
    summary: "Sign in with protected terminal password input.",
    purpose: "Exchanges an email and hidden TTY password for a native Firebase session. Only the rotating refresh credential is stored; no password, ID token, argv token, environment token, or Desktop session is used.",
    effect: Effect::LocalAuthState,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[EMAIL, LANE, PASSWORD_STDIN],
    output: "The canonical signed-in account identity and lane; no credential material.",
    examples: &[Example {
        command: "ds auth login --email operator@example.com",
        note: "Prompts on the controlling TTY.",
        runnable: false,
    }],
    refusals: LOGIN_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_availability,
};

pub static LOGOUT_COMMAND: Command = Command {
    id: "auth.logout",
    path: &["auth", "logout"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Remove one lane's native credential and context.",
    purpose: "Atomically removes the selected profile's refresh credential, then removes its fenced local project context. It never signs out or probes the paired desktop.",
    effect: Effect::LocalAuthState,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[LANE],
    output: "Whether the exact native lane is signed out and its context cleared.",
    examples: &[Example {
        command: "ds auth logout",
        note: "Signs out only the stable native profile.",
        runnable: false,
    }],
    refusals: LOGOUT_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_availability,
};

pub static PROJECT_LIST_COMMAND: Command = Command {
    id: "auth.project.list",
    path: &["auth", "project", "list"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "List fresh visible projects across all lifecycle buckets.",
    purpose: "Restores the native user and fetches active, archived, and testing projects through the one closed gateway route. A returned ID is visibility, not authority.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE, LIST_LIMIT],
    output: "Fresh visible project identities, names, roles, and lifecycle states; `elevated` says the list came by governance elevation, and a roleless row then reads `elevated`.",
    examples: &[Example {
        command: "ds auth project list",
        note: "Reads all three lifecycle buckets.",
        runnable: false,
    }],
    refusals: PROJECT_LIST_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_availability,
};

pub static PROJECT_USE_COMMAND: Command = Command {
    id: "auth.project.use",
    path: &["auth", "project", "use"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Select one exact freshly visible project.",
    purpose: "Fetches all three fresh project buckets, requires an exact visible ds_project match, and atomically saves a UID/lane/client-audience-fenced local context. A project ID by itself is never accepted as authority.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[PROJECT_ID, LANE],
    output: "The selected project's bounded identity and lifecycle state.",
    examples: &[Example {
        command: "ds auth project use --project exact-id",
        note: "The ID must appear in a fresh list.",
        runnable: false,
    }],
    refusals: PROJECT_USE_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_availability,
};

pub static PROJECT_STATUS_COMMAND: Command = Command {
    id: "auth.project.status",
    path: &["auth", "project", "status"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Show the fenced native project context.",
    purpose: "Restores the native user, then reads only a context bound to the same UID, canonical email, lane, and credential audience. It never reads the Desktop project.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE],
    output: "Selected project identity and lifecycle state, or selected false.",
    examples: &[Example {
        command: "ds auth project status",
        note: "Reports stable native context.",
        runnable: false,
    }],
    refusals: PROJECT_STATUS_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_availability,
};

pub static PROJECT_CREATE_COMMAND: Command = Command {
    id: "auth.project.create",
    path: &["auth", "project", "create"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Create one project with its properties.",
    purpose: "Creates a project on the lane under the signed-in account. The id slug is derived from the display name exactly as the Projects page derives it; every other property is optional and, when absent, is the server's default. Needs project.create. It selects nothing.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        DISPLAY_NAME_REQUIRED,
        COUNTRY,
        CLIENT,
        DESCRIPTION,
        LOCATION,
        NETWORK_TEMPLATE,
        STYLING_TEMPLATE,
        LANE,
    ],
    output: "The project as created: id, slug, display name, template and the parameters the server defaulted.",
    examples: &[Example {
        command: "ds auth project create --display-name \"Gisagara LV\" --country Rwanda --client EDCL --yes",
        note: "The id slug becomes gisagara_lv.",
        runnable: false,
    }],
    refusals: PROJECT_CREATE_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &["new project", "add project", "register"],
    requires: Requires::Server,
    availability: native_availability,
};

pub static PROJECT_UPDATE_COMMAND: Command = Command {
    id: "auth.project.update",
    path: &["auth", "project", "update"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Edit one project's name, country, client, description or location.",
    purpose: "Changes the named properties of the project given by id, and nothing else: an absent property is untouched. Needs project.properties.edit on that project (project admin). It edits the project it names, never the selected one.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        PROJECT_ID,
        DISPLAY_NAME,
        COUNTRY,
        CLIENT,
        DESCRIPTION,
        LOCATION,
        LANE,
    ],
    output: "The project id and how many fields changed.",
    examples: &[Example {
        command: "ds auth project update --project it_rwanda --display-name \"Integration test — Rwanda\" --country Rwanda --yes",
        note: "Only the two named properties change.",
        runnable: false,
    }],
    refusals: PROJECT_UPDATE_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[
        "rename",
        "edit project",
        "project country",
        "project client",
    ],
    requires: Requires::Server,
    availability: native_availability,
};

type NativeClient = Client<NativeTransport, NativeRefreshStore>;

struct NativeContextProvider<'a> {
    lane: Lane,
    client: &'a mut NativeClient,
    context: &'a ProjectContextLease,
}

impl CredentialProvider for NativeContextProvider<'_> {
    fn kind(&self) -> CredentialProviderKind {
        CredentialProviderKind::NativeRefresh
    }

    fn resolve_context(&mut self) -> Result<AuthContext, Failure> {
        let user = with_disposition(self.client.restore(now()), self.context)?;
        let Some(user) = user else {
            return Ok(AuthContext::signed_out(
                self.lane.token(),
                self.client.profile(),
            ));
        };
        let selected = self
            .context
            .load(self.client.profile(), user.uid(), user.email())?
            .as_ref()
            .map(selected_project);
        Ok(AuthContext::restored(
            self.lane.token(),
            self.client.profile(),
            &user,
            selected,
            self.kind(),
            None,
        ))
    }
}

/// One transformer snapshot fetched under the restored native user and the
/// audience-fenced selected project. The server remains the membership and
/// resource authority; this adapter only binds local identity and context.
pub struct HeadlessTransformerContext {
    identity: ProviderIdentity,
    lane: &'static str,
    project_name: String,
    project_status: String,
    snapshot: TransformerContext,
}

/// A transformer snapshot read for the project named on this request.
pub struct HeadlessNamedTransformerContext {
    identity: ProviderIdentity,
    snapshot: TransformerContext,
}

impl HeadlessNamedTransformerContext {
    pub const fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    pub const fn snapshot(&self) -> &TransformerContext {
        &self.snapshot
    }
}

impl HeadlessTransformerContext {
    pub const fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    pub const fn lane(&self) -> &'static str {
        self.lane
    }

    pub fn project_name(&self) -> &str {
        &self.project_name
    }

    pub fn project_status(&self) -> &str {
        &self.project_status
    }

    pub const fn snapshot(&self) -> &TransformerContext {
        &self.snapshot
    }
}

/// One project-form catalogue fetched under the restored user and that
/// user's audience-fenced selected project.
pub struct HeadlessProjectForms {
    lane: &'static str,
    project_name: String,
    project_status: String,
    snapshot: ProjectFormsSnapshot,
}

/// One closed tile status or generation result produced under the restored
/// user and that user's audience-fenced selected project.
pub struct HeadlessTileCatalog {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: TileCatalog,
}
impl HeadlessTileCatalog {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &TileCatalog {
        &self.result
    }
}

pub struct HeadlessTileMutation {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: TileMutation,
}
impl HeadlessTileMutation {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &TileMutation {
        &self.result
    }
}

pub struct HeadlessStyleEdit {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: StyleEditReceipt,
}
impl HeadlessStyleEdit {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &StyleEditReceipt {
        &self.result
    }
}

pub struct HeadlessLayerSnapshot {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: LayerSnapshot,
}
impl HeadlessLayerSnapshot {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &LayerSnapshot {
        &self.result
    }
}

pub struct HeadlessStyleSnapshot {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: StyleSnapshot,
}
impl HeadlessStyleSnapshot {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &StyleSnapshot {
        &self.result
    }
}

pub struct HeadlessLayerOrderReceipt {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: LayerOrderReceipt,
}
impl HeadlessLayerOrderReceipt {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &LayerOrderReceipt {
        &self.result
    }
}

pub struct HeadlessLayerVisibilityDefaultReceipt {
    lane: &'static str,
    project_id: String,
    result: LayerVisibilityDefaultReceipt,
}
impl HeadlessLayerVisibilityDefaultReceipt {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub const fn result(&self) -> &LayerVisibilityDefaultReceipt {
        &self.result
    }
}

pub struct HeadlessTileOperation {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: TileOperationResult,
}

/// One bounded tile source preflight produced under the restored user and
/// that user's audience-fenced selected project.
pub struct HeadlessTilePreflight {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: TilePreflight,
}

/// One selected-project `/report` result produced under the restored user
/// and that user's audience-fenced selected project. `T` is one of the closed
/// core receipt types; the project identity travels beside it so a CLI
/// receipt can name what it acted on without a second read.
pub struct HeadlessProjectReport<T> {
    identity: ProviderIdentity,
    /// The signed-in account's email, as the credential that carried the call
    /// states it — what a command needs when the caller is the subject (a
    /// task proposed about one's own work).
    user_email: String,
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: T,
}

impl<T> HeadlessProjectReport<T> {
    pub const fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    pub fn user_email(&self) -> &str {
        &self.user_email
    }
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &T {
        &self.result
    }
    pub fn into_result(self) -> T {
        self.result
    }
}

/// One settings editor fetched under the restored user and that user's
/// audience-fenced selected project.
pub struct HeadlessProjectFormEditor {
    lane: &'static str,
    project_name: String,
    project_status: String,
    snapshot: ProjectFormSettingsEditor,
}

/// One bounded Survey aggregate fetched under the restored user and the
/// audience-fenced selected project.
pub struct HeadlessSurveyQuery {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    result: SurveyQueryResult,
}

/// One bounded, mutable Survey mirror selection fetched under the restored
/// user and the audience-fenced selected project.
pub struct HeadlessSurveyEntriesSelection {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    selection: SurveyEntriesSelection,
}

/// One immutable-fence page of coalesced Survey mirror changes fetched under
/// the restored user and the audience-fenced selected project.
pub struct HeadlessSurveyEntriesChanges {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    changes: SurveyEntriesChanges,
}

/// One governed Survey create receipt produced under the restored user and
/// audience-fenced selected project. Request payload and idempotency material
/// are deliberately not retained in this projection.
pub struct HeadlessSurveyEntryCreate {
    lane: &'static str,
    project_name: String,
    project_status: String,
    receipt: SurveyEntryCreateReceipt,
}

/// One restored native session and one immutable selected-project snapshot for
/// a sequential Survey import. The bearer token, refresh credential, canonical
/// email, and request payload never cross this boundary.
pub struct HeadlessSurveyImportSession {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    principal_sha256: String,
    credential_audience_sha256: String,
    selected: state::ProjectContext,
    provider: SurveyImportProvider,
}

enum SurveyImportProvider {
    Firebase(Box<NativeClient>),
    Device(Box<device::DeviceSession>),
}

impl SurveyImportProvider {
    fn create(
        &mut self,
        project: &str,
        request: &SurveyEntryCreateRequest,
    ) -> Result<SurveyEntryCreateReceipt, ClientError> {
        match self {
            Self::Firebase(client) => client.survey_entry_create(project, request, now()),
            Self::Device(device) => device.survey_entry_create(project, request),
        }
    }

    fn profile(&self) -> &ds_client_core::ClientProfile {
        match self {
            Self::Firebase(client) => client.profile(),
            Self::Device(device) => device.profile(),
        }
    }
}

impl HeadlessSurveyImportSession {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn project_name(&self) -> &str {
        &self.project_name
    }

    pub fn project_status(&self) -> &str {
        &self.project_status
    }

    /// Stable, non-reversible binding used only to refuse a checkpoint opened
    /// by another authenticated principal. It is not an authentication proof.
    pub fn principal_sha256(&self) -> &str {
        &self.principal_sha256
    }

    pub fn credential_audience_sha256(&self) -> &str {
        &self.credential_audience_sha256
    }

    /// Execute exactly one create in this session's frozen project. Callers
    /// serialize invocations; this type deliberately exposes no Clone or
    /// transport handle and therefore no worker/concurrency escape.
    pub fn create(
        &mut self,
        request: &SurveyEntryCreateRequest,
    ) -> Result<SurveyEntryCreateReceipt, Failure> {
        let result = self.provider.create(&self.project_id, request);
        match result {
            Err(error) if error.survey_entry_create_service_code().is_some() => {
                Err(map_survey_entry_create_service_code(
                    error
                        .survey_entry_create_service_code()
                        .expect("the guarded Survey create service code is present"),
                ))
            }
            Err(error) if error.kind() == ErrorKind::ResourceNotFound => Err(Failure::invalid(
                "survey_entry_create_scope_not_found",
                "the selected project, governed form, or context ancestor is unavailable",
            )
            .remedy("verify the selected project, form, and optional context key")),
            Err(error) if error.kind() == ErrorKind::InvalidInput => Err(Failure::invalid(
                "survey_entry_create_refused",
                "the backend refused the already validated governed Survey create request",
            )
            .remedy("recheck the form, document identity, context, and document bounds")),
            Err(error) if error.kind() == ErrorKind::AuthenticationRejected => {
                Err(Failure::unauthorized(
                    "survey_entry_create_auth_rejected",
                    "the fixed create route rejected the verified identity or form authority",
                )
                .remedy("verify account and entries.create authority in the selected project"))
            }
            Err(error) if error.kind() == ErrorKind::Transient => Err(Failure::unavailable(
                "survey_entry_create_failed",
                "the governed Survey create service failed temporarily",
            )
            .remedy(
                "after service recovery, resume the exact import with unchanged idempotency keys",
            )),
            Err(error) if error.kind() == ErrorKind::UnreadableResponse => {
                Err(Failure::unavailable(
                    "survey_entry_create_unreadable",
                    "the create response violated its closed identity, version, clock, or authority contract",
                )
                .remedy("verify the backend release and update ds before resuming"))
            }
            other => with_released_context_disposition(
                self.provider.profile(),
                &self.selected,
                other,
            ),
        }
    }
}

impl HeadlessSurveyEntryCreate {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn receipt(&self) -> &SurveyEntryCreateReceipt {
        &self.receipt
    }
}

impl HeadlessSurveyEntriesChanges {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn changes(&self) -> &SurveyEntriesChanges {
        &self.changes
    }
}

impl HeadlessSurveyEntriesSelection {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn selection(&self) -> &SurveyEntriesSelection {
        &self.selection
    }
}

impl HeadlessSurveyQuery {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &SurveyQueryResult {
        &self.result
    }
}

impl HeadlessProjectFormEditor {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn snapshot(&self) -> &ProjectFormSettingsEditor {
        &self.snapshot
    }
}

impl HeadlessProjectForms {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn snapshot(&self) -> &ProjectFormsSnapshot {
        &self.snapshot
    }
}

impl HeadlessTileOperation {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &TileOperationResult {
        &self.result
    }
}

impl HeadlessTilePreflight {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn result(&self) -> &TilePreflight {
        &self.result
    }
}

/// Availability of the exact packaged native profiles and the side-effect-free
/// protected-state adapter probe used by headless commands.
pub fn native_availability() -> ds_cli_contract::spec::Availability {
    match profile::availability() {
        ds_cli_contract::spec::Availability::Available => state::availability(),
        unavailable => unavailable,
    }
}

/// The device session alone, for a call whose project is its argument. The
/// saved selection is neither read nor required, so a Server or a `--project`
/// call works on a machine that has never selected anything.
fn restored_device_session(lane: Lane) -> Result<Option<device::DeviceSession>, Failure> {
    let _ = probe_headless_identity_for_named_project(lane.token())?;
    device::restore_session(lane)
}

fn restored_device_project(
    lane: Lane,
) -> Result<Option<(device::DeviceSession, state::ProjectContext)>, Failure> {
    let _ = probe_headless_identity(lane.token())?;
    let Some(session) = device::restore_session(lane)? else {
        return Ok(None);
    };
    let identity = session.context();
    let selected = ProjectContextLease::acquire(session.profile())?
        .load_snapshot(session.profile(), identity.uid(), identity.email())?
        .ok_or_else(|| {
            Failure::conflict(
                "headless_project_not_selected",
                "no project is selected for this device, lane, and credential audience",
            )
            .remedy("run ds auth project use --project <exact-id>")
            .next("ds auth project status")
        })?;
    Ok(Some((session, selected)))
}

fn load_selected_project(
    profile: &ds_client_core::ClientProfile,
    user: &ds_client_core::AuthenticatedUser,
) -> Result<state::ProjectContext, Failure> {
    ProjectContextLease::acquire(profile)?
        .load_snapshot(profile, user.uid(), user.email())?
        .ok_or_else(|| {
            Failure::conflict(
                "headless_project_not_selected",
                "no project is selected for this native user, lane, and credential audience",
            )
            .remedy("run ds auth project use --project <exact-id>")
            .next("ds auth project status")
        })
}

/// Restore one native user and fetch one transformer from that user's fenced
/// selected project. There is deliberately no project-id or URL override.
pub fn transformer_context(
    lane_value: &str,
    transformer: &str,
) -> Result<HeadlessTransformerContext, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        let snapshot = device
            .transformer_context(selected.project_id(), transformer)
            .map_err(map_client)?;
        return Ok(HeadlessTransformerContext {
            identity: ProviderIdentity::new(
                lane.token(),
                device.profile().credential_audience_sha256(),
                device.context().uid(),
            )?,
            lane: lane.token(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            snapshot,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = ProjectContextLease::acquire(client.profile())?
        .load_snapshot(client.profile(), user.uid(), user.email())?
        .ok_or_else(|| {
            Failure::conflict(
                "headless_project_not_selected",
                "no project is selected for this native user, lane, and credential audience",
            )
            .remedy("run ds auth project use --project <exact-id>")
            .next("ds auth project status")
        })?;
    let result = client.transformer_context(selected.project_id(), transformer, now());
    let snapshot = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessTransformerContext {
        identity: ProviderIdentity::new(
            lane.token(),
            client.profile().credential_audience_sha256(),
            user.uid(),
        )?,
        lane: lane.token(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        snapshot,
    })
}

pub fn transformer_context_for_project(
    lane_value: &str,
    project: &str,
    transformer: &str,
) -> Result<HeadlessNamedTransformerContext, Failure> {
    let report = headless_named_project(
        lane_value,
        project,
        |device, project| device.transformer_context(project, transformer),
        |client, project| client.transformer_context(project, transformer, now()),
    )?;
    Ok(HeadlessNamedTransformerContext {
        identity: report.identity().clone(),
        snapshot: report.into_result(),
    })
}

/// The same read as [`transformer_context`], for MANY transformers of the
/// selected project under ONE restored session. A device session mints one
/// short-lived access token per restore, so a loop that restores per
/// transformer pays a refresh round trip each time — a 114-transformer
/// project's seed spent fourteen minutes there (2026-09-20). The answers keep
/// the request order; the first refusal ends the read.
pub fn transformer_contexts(
    lane_value: &str,
    transformers: &[String],
) -> Result<Vec<HeadlessTransformerContext>, Failure> {
    let lane = Lane::parse(lane_value)?;
    let mut contexts = Vec::with_capacity(transformers.len());
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        let identity = ProviderIdentity::new(
            lane.token(),
            device.profile().credential_audience_sha256(),
            device.context().uid(),
        )?;
        for transformer in transformers {
            let snapshot = device
                .transformer_context(selected.project_id(), transformer)
                .map_err(map_client)?;
            contexts.push(HeadlessTransformerContext {
                identity: identity.clone(),
                lane: lane.token(),
                project_name: selected.project_name().to_owned(),
                project_status: selected.status().to_owned(),
                snapshot,
            });
        }
        return Ok(contexts);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    let identity = ProviderIdentity::new(
        lane.token(),
        client.profile().credential_audience_sha256(),
        user.uid(),
    )?;
    for transformer in transformers {
        let result = client.transformer_context(selected.project_id(), transformer, now());
        let snapshot = with_released_context_disposition(client.profile(), &selected, result)?;
        contexts.push(HeadlessTransformerContext {
            identity: identity.clone(),
            lane: lane.token(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            snapshot,
        });
    }
    Ok(contexts)
}

/// Read the managed tile state of the caller's explicit project. The saved
/// native selection is never read; there is no URL or action override.
pub fn tile_list(
    lane_value: &str,
    project: &str,
    include_global: bool,
) -> Result<HeadlessTileCatalog, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.tile_list(project, include_global),
        |client, project| client.tile_list(project, include_global, now()),
    )?;
    Ok(HeadlessTileCatalog {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

/// Add another project's published tiles to the caller's explicit project.
pub fn tile_add(
    lane_value: &str,
    project: &str,
    tile_type: TileType,
    source_project: &str,
) -> Result<HeadlessTileMutation, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.tile_add(project, tile_type, source_project),
        |client, project| client.tile_add(project, tile_type, source_project, now()),
    )?;
    Ok(HeadlessTileMutation {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

/// Remove one tile archive from the caller's explicit project catalogue.
pub fn tile_remove(
    lane_value: &str,
    project: &str,
    tile_id: &str,
    scope: crate::TileScope,
) -> Result<HeadlessTileMutation, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.tile_remove(project, tile_id, scope),
        |client, project| client.tile_remove(project, tile_id, scope, now()),
    )?;
    Ok(HeadlessTileMutation {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

pub use ds_client_core::{MAX_UPLOAD_BYTES, ProjectDataCommand};
/// One project GIS upload action against the project the CALLER named. The
/// saved selection is never read. The command owns a reader for uploads, so it
/// is handed to whichever of the two sessions restores.
pub fn project_data(
    lane_value: &str,
    project: &str,
    command: ProjectDataCommand<'_>,
) -> Result<Value, Failure> {
    let command = std::cell::Cell::new(Some(command));
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| {
            device.project_data(project, command.take().expect("one project-data call"))
        },
        |client, project| {
            client.project_data(
                project,
                command.take().expect("one project-data call"),
                now(),
            )
        },
    )?;
    let mut data = named.result.data().clone();
    data["lane"] = serde_json::json!(named.lane);
    Ok(data)
}

pub use ds_client_core::StatusUploadDomain;

/// Run the Rust-owned design-intake state machine against files on
/// this machine. The selected project is acquired once and remains frozen for
/// every upload and process effect in the job.
pub fn status_upload(
    lane_value: &str,
    paths: &[String],
    mode: StatusUploadDomain,
    settings: &serde_json::Map<String, Value>,
) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        let mut result =
            drive_status_upload(selected.project_id(), paths, mode, settings, |command| {
                device
                    .status_processing(selected.project_id(), command)
                    .map(|receipt| receipt.data().clone())
                    .map_err(map_client)
            })?;
        decorate_status_upload(&mut result, lane.token(), &selected);
        return Ok(result);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    let mut result =
        drive_status_upload(selected.project_id(), paths, mode, settings, |command| {
            let call = client.status_processing(selected.project_id(), command, now());
            with_released_context_disposition(client.profile(), &selected, call)
                .map(|receipt| receipt.data().clone())
        })?;
    decorate_status_upload(&mut result, lane.token(), &selected);
    Ok(result)
}

fn decorate_status_upload(result: &mut Value, lane: &str, selected: &state::ProjectContext) {
    result["project"] = json!(selected.project_id());
    result["project_name"] = json!(selected.project_name());
    result["project_status"] = json!(selected.status());
    result["lane"] = json!(lane);
}

fn status_upload_failure(message: impl Into<String>) -> Failure {
    Failure::invalid("status_upload_invalid", message)
        .remedy("Pass bounded regular files and keep the selected project unchanged for the run")
}

fn status_upload_domain(value: &str) -> Result<StatusUploadDomain, Failure> {
    match value {
        "lv_drafting" => Ok(StatusUploadDomain::LvDrafting),
        "sketch_lv" => Ok(StatusUploadDomain::SketchLv),
        "lv_process" => Ok(StatusUploadDomain::LvProcess),
        _ => Err(status_upload_failure(
            "Upload kernel emitted an unknown processing domain.",
        )),
    }
}

fn drive_status_upload<F>(
    project: &str,
    paths: &[String],
    mode: StatusUploadDomain,
    settings: &serde_json::Map<String, Value>,
    mut execute: F,
) -> Result<Value, Failure>
where
    F: for<'a> FnMut(ds_client_core::StatusProcessingCommand<'a>) -> Result<Value, Failure>,
{
    let declarations = paths
        .iter()
        .map(|source| {
            let path = Path::new(source);
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let size = std::fs::metadata(path)
                .ok()
                .filter(|metadata| metadata.is_file())
                .map_or(0, |metadata| metadata.len());
            json!({"name":name,"size":size,"content_type":"application/octet-stream"})
        })
        .collect::<Vec<_>>();
    let mut reply: Value = serde_json::from_str(
        &ds_command_kernel::status_upload::evaluate(
            json!({
                "op":"start", "project_id":project, "mode":mode.token(),
                "files":declarations, "parallel_limit":1,
                "upload_chain":[], "use_firestore_design_data":false,
                "process_settings":settings
            })
            .to_string()
            .as_bytes(),
        )
        .map_err(status_upload_failure)?,
    )
    .map_err(|_| status_upload_failure("Upload kernel returned invalid JSON."))?;

    loop {
        let phase = reply["state"]["phase"]
            .as_str()
            .ok_or_else(|| status_upload_failure("Upload kernel omitted its phase."))?;
        if matches!(phase, "complete" | "cancelled") {
            return Ok(json!({
                "schema":reply["schema"],
                "phase":phase,
                "progress":reply["progress"],
                "results":reply["state"]["results"]
            }));
        }
        let effects = reply["effects"]
            .as_array()
            .ok_or_else(|| status_upload_failure("Upload kernel omitted its effects."))?;
        if effects.len() != 1 {
            return Err(status_upload_failure(
                "Single-worker upload kernel did not emit exactly one effect.",
            ));
        }
        let effect = &effects[0];
        let index = effect["file_index"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| status_upload_failure("Upload kernel emitted an invalid file index."))?;
        let event = if effect["kind"] == "upload" {
            let outcome = (|| {
                if effect["project_id"].as_str() != Some(project) {
                    return Err(status_upload_failure(
                        "Upload kernel changed the selected project.",
                    ));
                }
                let source = paths.get(index).ok_or_else(|| {
                    status_upload_failure("Upload kernel named a missing source file.")
                })?;
                let mut file = std::fs::File::open(source).map_err(|error| {
                    status_upload_failure(format!("Cannot open {source}: {error}"))
                })?;
                let metadata = file
                    .metadata()
                    .map_err(|error| status_upload_failure(error.to_string()))?;
                let declared_size = effect["size"].as_u64().unwrap_or(0);
                if !metadata.is_file() || metadata.len() != declared_size {
                    return Err(status_upload_failure(
                        "Source file changed after upload admission.",
                    ));
                }
                let name = effect["file_name"]
                    .as_str()
                    .ok_or_else(|| status_upload_failure("Upload kernel omitted the file name."))?;
                let content_type = effect["content_type"].as_str().ok_or_else(|| {
                    status_upload_failure("Upload kernel omitted the content type.")
                })?;
                let domain = status_upload_domain(effect["upload_domain"].as_str().unwrap_or(""))?;
                execute(ds_client_core::StatusProcessingCommand::Upload {
                    domain,
                    file_name: name,
                    size: metadata.len(),
                    content_type,
                    reader: &mut file,
                })
            })();
            match outcome {
                Ok(receipt) => json!({
                    "kind":"upload_succeeded", "file_index":index,
                    "blob_path":receipt["blob_path"]
                }),
                Err(error) => json!({
                    "kind":"upload_failed", "file_index":index,
                    "error":error.to_string()
                }),
            }
        } else if effect["kind"] == "process" {
            let outcome = (|| {
                if effect["project_id"].as_str() != Some(project) {
                    return Err(status_upload_failure(
                        "Upload kernel changed the selected project.",
                    ));
                }
                let files = effect["files"]
                    .as_array()
                    .filter(|files| files.len() == 1)
                    .ok_or_else(|| {
                        status_upload_failure("Upload kernel emitted an invalid process file list.")
                    })?;
                let name = files[0]["file_name"].as_str().ok_or_else(|| {
                    status_upload_failure("Upload kernel omitted the process file name.")
                })?;
                let path = files[0]["file_path"].as_str().ok_or_else(|| {
                    status_upload_failure("Upload kernel omitted the process blob path.")
                })?;
                let action = status_upload_domain(effect["action"].as_str().unwrap_or(""))?;
                let chain: Vec<String> =
                    serde_json::from_value(effect["chain"].clone()).map_err(|_| {
                        status_upload_failure("Upload kernel emitted an invalid process chain.")
                    })?;
                let extra = effect["extra"].as_object().ok_or_else(|| {
                    status_upload_failure("Upload kernel emitted invalid process settings.")
                })?;
                execute(ds_client_core::StatusProcessingCommand::Process {
                    action,
                    file_name: name,
                    blob_path: path,
                    chain: &chain,
                    extra,
                })
            })();
            match outcome {
                Ok(receipt) => json!({
                    "kind":"process_succeeded", "file_index":index,
                    "response":receipt
                }),
                Err(error) => json!({
                    "kind":"process_failed", "file_index":index,
                    "error":error.to_string()
                }),
            }
        } else {
            return Err(status_upload_failure(
                "Upload kernel emitted an unknown effect.",
            ));
        };
        reply = serde_json::from_str(
            &ds_command_kernel::status_upload::evaluate(
                json!({"op":"advance","state":reply["state"],"event":event})
                    .to_string()
                    .as_bytes(),
            )
            .map_err(status_upload_failure)?,
        )
        .map_err(|_| status_upload_failure("Upload kernel returned invalid JSON."))?;
    }
}

pub fn style_edit(
    lane_value: &str,
    project: &str,
    reference: &str,
    instruction: &StyleInstruction,
    apply: bool,
) -> Result<HeadlessStyleEdit, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.style_edit(project, reference, instruction, apply),
        |client, project| client.style_edit(project, reference, instruction, apply, now()),
    )?;
    Ok(HeadlessStyleEdit {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

/// One governed action on the GLOBAL reference publications.
///
/// Global, like the library catalog: no project is selected and none is
/// fenced, because a reference publication belongs to the product. Publishing
/// needs the gateway's own `global_tiles.manage`; reading needs the lane's
/// credential — its device credential when it holds one, otherwise the
/// restored native user — and nothing else.
pub fn global_tiles(
    lane_value: &str,
    command: &ds_client_core::global_tiles::Command,
) -> Result<serde_json::Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.global_tiles(command).map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let _user = require_restore_before_context(&mut client)?;
    client.global_tiles(command, now()).map_err(map_client)
}

/// One exact read of a national administrative hierarchy.
///
/// National reference data: no project is selected and none is fenced, because
/// a country's boundaries belong to the country. The lane's credential — its
/// device credential when it holds one, otherwise the restored native user — is
/// the identity the gateway sees, exactly as it saw the paired desktop's.
pub fn admin_bounds(
    lane_value: &str,
    command: &ds_client_core::admin_bounds::Command,
) -> Result<ds_client_core::admin_bounds::Answer, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.admin_bounds(command).map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let _user = require_restore_before_context(&mut client)?;
    client.admin_bounds(command, now()).map_err(map_client)
}

/// One bounded read of platform reliability.
///
/// Global means there is no project: fleet health, SLO burn and the request
/// event window belong to the platform, so this helper acquires no project
/// context lease, loads no saved selection, and cannot leak one into the
/// request. The authority is the lane's credential — its device credential
/// when it holds one, otherwise the restored native user; ds-brain refuses
/// both routes without the `platform.admin` capability.
///
/// Until 2026-09-18 these two reads travelled through a paired desktop, which
/// held the same user session and made the same requests. The window was never
/// part of the contract, only of the route — and it was the one host least
/// likely to be running when platform health is what you need.
pub fn sre(
    lane_value: &str,
    command: &ds_client_core::sre::Command,
) -> Result<serde_json::Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.sre(command).map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client.sre(command, now()).map_err(map_client)
}

/// One governed action on the GLOBAL installation inventory.
///
/// Global means there is no project to select and none to fence: an
/// installation belongs to the product. The authority is the lane's
/// credential — its device credential when it holds one, otherwise the
/// restored native user; ds-brain refuses every action without
/// `platform.admin` or `app.admin`.
///
/// Until now this inventory had no `ds` surface at all — it was reachable only
/// from the Governance page in a browser, which is precisely the host least
/// likely to be running where the question is asked.
pub fn installs(
    lane_value: &str,
    command: &ds_client_core::installs::Command,
) -> Result<serde_json::Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.installs(command).map_err(map_install_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client.installs(command, now()).map_err(map_install_client)
}

/// One install-path error as the refusal `ds install …` declares for it.
///
/// Before 2026-09-19 this path fell through to the shared kind mapping, so an
/// unknown install id came back as `transformer_not_found` with a remedy
/// naming an input the command does not have. Only that kind is this path's
/// own; every other kind keeps the shared mapping's code, which is what the
/// install commands declare.
fn map_install_client(error: ClientError) -> Failure {
    install_kind(error.kind()).unwrap_or_else(|| map_client(error))
}

/// The install path's own refusal for one transport kind, or `None` when the
/// shared mapping already says the right thing. Split from
/// [`map_install_client`] so it can be exercised without a `ClientError`.
fn install_kind(kind: ErrorKind) -> Option<Failure> {
    match kind {
        ErrorKind::ResourceNotFound => Some(
            Failure::invalid("install_not_found", "no registered install has this id")
                .remedy("run ds install list")
                .next("ds install list --output json"),
        ),
        _ => None,
    }
}

/// One governed action on the GLOBAL DS Grid library and example catalog.
///
/// Global means there is no project to select and none to fence: a library
/// release belongs to the product, not to a project, so this helper
/// deliberately does not acquire a project context lease, does not load a
/// saved selection, and cannot leak one into the request. The authority is the
/// lane's credential — its device credential when it holds one, otherwise the
/// restored native user; the gateway refuses a write without its own publish
/// capability.
///
/// Until 2026-09-18 these actions travelled through the paired desktop, which
/// held the same user session and posted the same body to the same path. The
/// window was never part of the contract, only of the route.
pub fn grid_catalog(
    lane_value: &str,
    command: &ds_client_core::grid_catalog::Command,
) -> Result<serde_json::Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.grid_catalog(command).map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let _user = require_restore_before_context(&mut client)?;
    client.grid_catalog(command, now()).map_err(map_client)
}

pub fn layer_config(lane_value: &str, refresh: bool) -> Result<HeadlessLayerSnapshot, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        let result = device
            .layer_config(selected.project_id(), refresh)
            .map_err(map_client)?;
        return Ok(HeadlessLayerSnapshot {
            lane: lane.token(),
            project_id: selected.project_id().to_owned(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    let result = client.layer_config(selected.project_id(), refresh, now());
    let result = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessLayerSnapshot {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        result,
    })
}

/// Opaque identity, audience, project and credential binding captured for one
/// layer operation. It can fence fixed layer calls, never construct a client or
/// reveal a credential.
///
/// The project is either this machine's saved selection
/// ([`capture_layer_scope_fence`], the desktop-paired CLI path) or the one the
/// caller named ([`capture_layer_scope_fence_for_project`], every host that
/// serves more than one project at a time). Which one it is, is decided once
/// here and never re-decided further down.
#[derive(Clone, Debug)]
pub struct LayerScopeFence {
    uid: String,
    audience: String,
    project: String,
    credential: String,
}
impl LayerScopeFence {
    pub fn uid(&self) -> &str {
        &self.uid
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn audience(&self) -> &str {
        &self.audience
    }
}

fn verify_restored_layer_identity(
    fence: &LayerScopeFence,
    uid: &str,
    audience: &str,
    project: &str,
) -> Result<(), Failure> {
    if uid != fence.uid || audience != fence.audience || project != fence.project {
        return Err(Failure::conflict(
            "project_context_changed",
            "the restored native client differs from the fenced layer identity",
        )
        .remedy("repeat the layer request under the current native account and project"));
    }
    Ok(())
}

/// The kernel's rule on a project id, ASKED rather than restated, where a
/// named project first enters this crate.
///
/// It used to be restated here — "1..500 characters, unpadded, free of control
/// characters" — and a restatement of a rule is that rule right up until it
/// drifts. It had. The kernel's grammar is ONE PATH SEGMENT of at most
/// `MAX_PROJECT_CHARS` characters with no separator, traversal, drive,
/// trailing dot, whitespace, wildcard or device name, and this door was
/// admitting `a/b`, `..`, `C:`, `NUL`, `A B` and five hundred characters of
/// them. Two consequences, both real: `ds map layer … --project a/b` is
/// refused against a Server, which asks the kernel, and was admitted against
/// the desktop, so ONE command id meant two different things depending on
/// which host ran it — exactly what the host-transparency ruling ends; and
/// what gets through this door is what becomes a preference key and a request
/// path. One rule, one answer, at every door, so this asks for it.
fn bounded_named_project(value: &str) -> Result<String, Failure> {
    if !ds_command_kernel::execution_context::valid_project(value) {
        return Err(Failure::invalid(
            "context_corrupt",
            format!(
                "a project id is one path segment of 1..={} characters: no separator, no traversal, no whitespace",
                ds_command_kernel::execution_context::MAX_PROJECT_CHARS
            ),
        )
        .remedy("copy one exact ds_project value from ds auth project list"));
    }
    Ok(value.to_owned())
}

/// The identity fence for a layer operation whose project the CALLER named.
///
/// This machine's saved selection is not read: the project is the argument.
/// What the fence still holds steady for the length of the operation is the
/// account, its credential audience and the exact credential binding, so a
/// document read under one account can never have its preferences or its order
/// written under another.
pub fn capture_layer_scope_fence_for_project(
    lane_value: &str,
    project: &str,
) -> Result<LayerScopeFence, Failure> {
    let project = bounded_named_project(project)?;
    let lane = Lane::parse(lane_value)?;
    let identity = probe_headless_identity_for_named_project(lane_value)?
        .ok_or_else(|| signed_out_failure(lane))?;
    Ok(LayerScopeFence {
        uid: identity.uid().to_owned(),
        audience: identity.credential_audience_sha256().to_owned(),
        project,
        credential: runtime_credential_binding(lane_value)?,
    })
}

pub fn capture_layer_scope_fence(lane_value: &str) -> Result<LayerScopeFence, Failure> {
    let lane = Lane::parse(lane_value)?;
    let (identity, project) =
        probe_headless_identity(lane_value)?.ok_or_else(|| signed_out_failure(lane))?;
    let project = project.ok_or_else(|| {
        Failure::conflict(
            "project_context_changed",
            "no selected project is available for the native layer operation",
        )
        .remedy("select a project and repeat the layer request")
    })?;
    Ok(LayerScopeFence {
        uid: identity.uid().to_owned(),
        audience: identity.credential_audience_sha256().to_owned(),
        project,
        credential: runtime_credential_binding(lane_value)?,
    })
}

pub fn verify_layer_scope_fence(
    lane_value: &str,
    fence: &LayerScopeFence,
    expected_uid: &str,
    expected_project: &str,
) -> Result<(), Failure> {
    if fence.uid != expected_uid || fence.project != expected_project {
        return Err(Failure::conflict(
            "project_context_changed",
            "the layer operation no longer has its read scope",
        )
        .remedy("repeat the layer request"));
    }
    let current = capture_layer_scope_fence(lane_value)?;
    if current.uid != fence.uid
        || current.audience != fence.audience
        || current.project != fence.project
        || current.credential != fence.credential
    {
        return Err(Failure::conflict("project_context_changed", "the native account, credential, or selected project changed during the layer operation")
            .remedy("repeat the layer request under the current native account and project"));
    }
    Ok(())
}

/// The counterpart of [`verify_layer_scope_fence`] for a fence that holds a
/// named project: the account, its audience and its credential must be the
/// same ones the document was read under, and the project is not re-resolved
/// because it was never resolved — it was given.
pub fn verify_layer_scope_fence_for_project(
    lane_value: &str,
    fence: &LayerScopeFence,
    expected_uid: &str,
    expected_project: &str,
) -> Result<(), Failure> {
    if fence.uid != expected_uid || fence.project != expected_project {
        return Err(Failure::conflict(
            "project_context_changed",
            "the layer operation no longer has its read scope",
        )
        .remedy("repeat the layer request"));
    }
    let current = capture_layer_scope_fence_for_project(lane_value, &fence.project)?;
    if current.uid != fence.uid
        || current.audience != fence.audience
        || current.credential != fence.credential
    {
        return Err(Failure::conflict(
            "project_context_changed",
            "the native account or credential changed during the layer operation",
        )
        .remedy("repeat the layer request under the current native account"));
    }
    Ok(())
}

/// One named project's assembled layer document, with the saved selection
/// never consulted.
///
/// This is the read every host that serves more than one project at a time
/// makes: the Server's `/v1/layers*`, and `ds map layer …` with an explicit
/// `--project`. [`layer_config_fenced`] stays the desktop-paired CLI's read,
/// where the saved selection IS the subject.
///
/// `project_name` and `project_status` are empty on purpose: they come from a
/// selection snapshot, and this path reads none. A caller that named an id gets
/// that id back and nothing invented around it.
pub fn layer_config_for_project(
    lane_value: &str,
    project: &str,
    refresh: bool,
    fence: &LayerScopeFence,
) -> Result<HeadlessLayerSnapshot, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    if let Some(mut device) = restored_device_session(lane)? {
        verify_restored_layer_identity(
            fence,
            device.context().uid(),
            device.profile().credential_audience_sha256(),
            &project,
        )?;
        verify_layer_scope_fence_for_project(lane_value, fence, fence.uid(), &project)?;
        let result = device.layer_config(&project, refresh).map_err(map_client)?;
        return Ok(HeadlessLayerSnapshot {
            lane: lane.token(),
            project_id: project,
            project_name: String::new(),
            project_status: String::new(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    verify_restored_layer_identity(
        fence,
        user.uid(),
        client.profile().credential_audience_sha256(),
        &project,
    )?;
    verify_layer_scope_fence_for_project(lane_value, fence, fence.uid(), &project)?;
    let result = client
        .layer_config(&project, refresh, now())
        .map_err(map_client)?;
    Ok(HeadlessLayerSnapshot {
        lane: lane.token(),
        project_id: project,
        project_name: String::new(),
        project_status: String::new(),
        result,
    })
}

/// The governed order write for a named project. The same statement as
/// [`layer_config_for_project`], for the effect rather than the read.
pub fn layer_reorder_for_project(
    lane_value: &str,
    project: &str,
    orders: &[crate::LayerOrder],
    fence: &LayerScopeFence,
) -> Result<HeadlessLayerOrderReceipt, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    if let Some(mut device) = restored_device_session(lane)? {
        verify_restored_layer_identity(
            fence,
            device.context().uid(),
            device.profile().credential_audience_sha256(),
            &project,
        )?;
        verify_layer_scope_fence_for_project(lane_value, fence, fence.uid(), &project)?;
        let result = device.layer_reorder(&project, orders).map_err(map_client)?;
        return Ok(HeadlessLayerOrderReceipt {
            lane: lane.token(),
            project_id: project,
            project_name: String::new(),
            project_status: String::new(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    verify_restored_layer_identity(
        fence,
        user.uid(),
        client.profile().credential_audience_sha256(),
        &project,
    )?;
    verify_layer_scope_fence_for_project(lane_value, fence, fence.uid(), &project)?;
    let result = client
        .layer_reorder(&project, orders, now())
        .map_err(map_client)?;
    Ok(HeadlessLayerOrderReceipt {
        lane: lane.token(),
        project_id: project,
        project_name: String::new(),
        project_status: String::new(),
        result,
    })
}

pub fn layer_default_visibility_for_project(
    lane_value: &str,
    project: &str,
    defaults: &[crate::LayerVisibilityDefault],
    fence: &LayerScopeFence,
) -> Result<HeadlessLayerVisibilityDefaultReceipt, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    if let Some(mut device) = restored_device_session(lane)? {
        verify_restored_layer_identity(
            fence,
            device.context().uid(),
            device.profile().credential_audience_sha256(),
            &project,
        )?;
        verify_layer_scope_fence_for_project(lane_value, fence, fence.uid(), &project)?;
        let result = device
            .layer_default_visibility(&project, defaults)
            .map_err(map_client)?;
        return Ok(HeadlessLayerVisibilityDefaultReceipt {
            lane: lane.token(),
            project_id: project,
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    verify_restored_layer_identity(
        fence,
        user.uid(),
        client.profile().credential_audience_sha256(),
        &project,
    )?;
    verify_layer_scope_fence_for_project(lane_value, fence, fence.uid(), &project)?;
    let result = client
        .layer_default_visibility(&project, defaults, now())
        .map_err(map_client)?;
    Ok(HeadlessLayerVisibilityDefaultReceipt {
        lane: lane.token(),
        project_id: project,
        result,
    })
}

pub fn layer_config_fenced(
    lane_value: &str,
    refresh: bool,
    fence: &LayerScopeFence,
) -> Result<HeadlessLayerSnapshot, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        verify_restored_layer_identity(
            fence,
            device.context().uid(),
            device.profile().credential_audience_sha256(),
            selected.project_id(),
        )?;
        verify_layer_scope_fence(lane_value, fence, fence.uid(), fence.project())?;
        let result = device
            .layer_config(selected.project_id(), refresh)
            .map_err(map_client)?;
        return Ok(HeadlessLayerSnapshot {
            lane: lane.token(),
            project_id: selected.project_id().to_owned(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    verify_restored_layer_identity(
        fence,
        user.uid(),
        client.profile().credential_audience_sha256(),
        selected.project_id(),
    )?;
    verify_layer_scope_fence(lane_value, fence, fence.uid(), fence.project())?;
    let result = client.layer_config(selected.project_id(), refresh, now());
    let result = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessLayerSnapshot {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        result,
    })
}

pub fn style_catalog(lane_value: &str, project: &str) -> Result<HeadlessStyleSnapshot, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.style_catalog(project),
        |client, project| client.style_catalog(project, now()),
    )?;
    Ok(HeadlessStyleSnapshot {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

pub fn layer_reorder(
    lane_value: &str,
    orders: &[crate::LayerOrder],
) -> Result<HeadlessLayerOrderReceipt, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        let result = device
            .layer_reorder(selected.project_id(), orders)
            .map_err(map_client)?;
        return Ok(HeadlessLayerOrderReceipt {
            lane: lane.token(),
            project_id: selected.project_id().to_owned(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    let result = client.layer_reorder(selected.project_id(), orders, now());
    let result = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessLayerOrderReceipt {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        result,
    })
}

pub fn layer_reorder_fenced(
    lane_value: &str,
    orders: &[crate::LayerOrder],
    fence: &LayerScopeFence,
) -> Result<HeadlessLayerOrderReceipt, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        verify_restored_layer_identity(
            fence,
            device.context().uid(),
            device.profile().credential_audience_sha256(),
            selected.project_id(),
        )?;
        verify_layer_scope_fence(lane_value, fence, fence.uid(), fence.project())?;
        let result = device
            .layer_reorder(selected.project_id(), orders)
            .map_err(map_client)?;
        return Ok(HeadlessLayerOrderReceipt {
            lane: lane.token(),
            project_id: selected.project_id().to_owned(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    verify_restored_layer_identity(
        fence,
        user.uid(),
        client.profile().credential_audience_sha256(),
        selected.project_id(),
    )?;
    verify_layer_scope_fence(lane_value, fence, fence.uid(), fence.project())?;
    let result = client.layer_reorder(selected.project_id(), orders, now());
    let result = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessLayerOrderReceipt {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        result,
    })
}

pub fn layer_default_visibility_fenced(
    lane_value: &str,
    defaults: &[crate::LayerVisibilityDefault],
    fence: &LayerScopeFence,
) -> Result<HeadlessLayerVisibilityDefaultReceipt, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        verify_restored_layer_identity(
            fence,
            device.context().uid(),
            device.profile().credential_audience_sha256(),
            selected.project_id(),
        )?;
        verify_layer_scope_fence(lane_value, fence, fence.uid(), fence.project())?;
        let result = device
            .layer_default_visibility(selected.project_id(), defaults)
            .map_err(map_client)?;
        return Ok(HeadlessLayerVisibilityDefaultReceipt {
            lane: lane.token(),
            project_id: selected.project_id().to_owned(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    verify_restored_layer_identity(
        fence,
        user.uid(),
        client.profile().credential_audience_sha256(),
        selected.project_id(),
    )?;
    verify_layer_scope_fence(lane_value, fence, fence.uid(), fence.project())?;
    let result = client.layer_default_visibility(selected.project_id(), defaults, now());
    let result = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessLayerVisibilityDefaultReceipt {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        result,
    })
}

/// Read one tile output's published state for the caller's explicit project.
pub fn tile_status(
    lane_value: &str,
    project: &str,
    tile_type: TileType,
) -> Result<HeadlessTileOperation, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.tile_status(project, tile_type),
        |client, project| client.tile_status(project, tile_type, now()),
    )?;
    Ok(HeadlessTileOperation {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

/// Inspect the bounded tile source shape of the caller's explicit project.
pub fn tile_preflight(
    lane_value: &str,
    project: &str,
    tile_type: TileType,
) -> Result<HeadlessTilePreflight, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.tile_preflight(project, tile_type),
        |client, project| client.tile_preflight(project, tile_type, now()),
    )?;
    Ok(HeadlessTilePreflight {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

/// Start one closed managed tile generation for the caller's explicit project.
pub fn tile_generate(
    lane_value: &str,
    project: &str,
    tile_type: TileType,
    force: bool,
) -> Result<HeadlessTileOperation, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.tile_generate(project, tile_type, force),
        |client, project| client.tile_generate(project, tile_type, force, now()),
    )?;
    Ok(HeadlessTileOperation {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

/// Run one closed selected-project `/report` operation through whichever
/// native provider restores: the device credential first, then the Firebase
/// refresh session. There is no project, URL, lane-header, or action
/// override; the core owns the grammar and the caller only chooses the
/// typed operation.
pub fn feeder_configuration(
    lane: &str,
    bounds: Option<(f64, f64)>,
) -> Result<ds_client_core::FeederConfiguration, Failure> {
    feeder_configuration_receipt(lane, bounds).map(HeadlessProjectReport::into_result)
}

/// Preserve the identity and project that supplied a configuration so a
/// multi-call native workflow can reject account or project changes.
pub fn feeder_configuration_receipt(
    lane: &str,
    bounds: Option<(f64, f64)>,
) -> Result<HeadlessProjectReport<ds_client_core::FeederConfiguration>, Failure> {
    let change =
        bounds.map(
            |(minimum, maximum)| ds_client_core::ProjectConfigurationChange::FeederLimits {
                minimum,
                maximum,
            },
        );
    headless_project_report(
        lane,
        |device, project| device.feeder_configuration(project, change.as_ref()),
        |client, project| client.feeder_configuration(project, change.as_ref(), now()),
    )
}

/// Read print settings for a caller-named project. No selected-project state
/// participates in the report's source snapshot.
pub fn feeder_configuration_for_project(
    lane_value: &str,
    project: &str,
) -> Result<HeadlessNamedProject<ds_client_core::FeederConfiguration>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.feeder_configuration(project, None),
        |client, project| client.feeder_configuration(project, None, now()),
    )
}

/// Settings uses the same selected-project transport and fresh readback as
/// feeder edits; the core validates the closed mutation against fetched sheets.
pub fn settings_configuration(
    lane: &str,
    change: ds_client_core::ProjectConfigurationChange,
) -> Result<ds_client_core::FeederConfiguration, Failure> {
    settings_configuration_receipt(lane, change).map(HeadlessProjectReport::into_result)
}

/// Retain identity and project fences when configuration joins another snapshot.
pub fn settings_configuration_receipt(
    lane: &str,
    change: ds_client_core::ProjectConfigurationChange,
) -> Result<HeadlessProjectReport<ds_client_core::FeederConfiguration>, Failure> {
    headless_project_report(
        lane,
        |device, project| device.feeder_configuration(project, Some(&change)),
        |client, project| client.feeder_configuration(project, Some(&change), now()),
    )
}

pub fn ensure_meter_type(
    lane: &str,
    name: &str,
) -> Result<ds_client_core::FeederConfiguration, Failure> {
    let change = ds_client_core::ProjectConfigurationChange::EnsureMeterType { name: name.into() };
    headless_project_report(
        lane,
        |device, project| device.feeder_configuration(project, Some(&change)),
        |client, project| client.feeder_configuration(project, Some(&change), now()),
    )
    .map(|receipt| receipt.result)
}

/// Save the project's `project_settings` sheet carrying a design output
/// selection the kernel wrote. The rows are the kernel's answer, not this
/// caller's composition: `report_formats::apply_output_selection` decides
/// which row holds the selection and creates one when the sheet has none, and
/// the closed change verifies that a readable selection is what arrives.
pub fn design_output_rows(
    lane: &str,
    rows: Vec<serde_json::Value>,
) -> Result<ds_client_core::FeederConfiguration, Failure> {
    let change = ds_client_core::ProjectConfigurationChange::DesignOutputs { rows };
    headless_project_report(
        lane,
        |device, project| device.feeder_configuration(project, Some(&change)),
        |client, project| client.feeder_configuration(project, Some(&change), now()),
    )
    .map(|receipt| receipt.result)
}

pub fn design_output_rows_for_project(
    lane: &str,
    project: &str,
    rows: Vec<serde_json::Value>,
) -> Result<ds_client_core::FeederConfiguration, Failure> {
    let change = ds_client_core::ProjectConfigurationChange::DesignOutputs { rows };
    headless_named_project(
        lane,
        project,
        |device, project| device.feeder_configuration(project, Some(&change)),
        |client, project| client.feeder_configuration(project, Some(&change), now()),
    )
    .map(HeadlessNamedProject::into_result)
}

pub fn customer_category_alias(
    lane: &str,
    alias: &str,
    category: &str,
) -> Result<ds_client_core::FeederConfiguration, Failure> {
    let change = ds_client_core::ProjectConfigurationChange::CustomerCategoryAlias {
        alias: alias.into(),
        category: category.into(),
    };
    headless_project_report(
        lane,
        |device, project| device.feeder_configuration(project, Some(&change)),
        |client, project| client.feeder_configuration(project, Some(&change), now()),
    )
    .map(|receipt| receipt.result)
}

/// One vocabulary housekeeping edit over the same selected-project transport
/// as the seeding verbs: the core resolves it against the document it just
/// fetched and proves the save by reading the sheet back.
pub fn catalog_housekeeping(
    lane: &str,
    change: ds_client_core::ProjectConfigurationChange,
) -> Result<ds_client_core::FeederConfiguration, Failure> {
    headless_project_report(
        lane,
        |device, project| device.feeder_configuration(project, Some(&change)),
        |client, project| client.feeder_configuration(project, Some(&change), now()),
    )
    .map(|receipt| receipt.result)
}

fn headless_project_report<T>(
    lane_value: &str,
    device_call: impl FnOnce(&mut device::DeviceSession, &str) -> Result<T, ClientError>,
    session_call: impl FnOnce(
        &mut Client<NativeTransport, NativeRefreshStore>,
        &str,
    ) -> Result<T, ClientError>,
) -> Result<HeadlessProjectReport<T>, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        let result = device_call(&mut device, selected.project_id()).map_err(map_client)?;
        return Ok(HeadlessProjectReport {
            identity: ProviderIdentity::new(
                lane.token(),
                device.profile().credential_audience_sha256(),
                device.context().uid(),
            )?,
            user_email: device.context().email().to_owned(),
            lane: lane.token(),
            project_id: selected.project_id().to_owned(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    let result = session_call(&mut client, selected.project_id());
    let result = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessProjectReport {
        identity: ProviderIdentity::new(
            lane.token(),
            client.profile().credential_audience_sha256(),
            user.uid(),
        )?,
        user_email: user.email().to_owned(),
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        result,
    })
}

/// Request one Combined Report for only the saved,
/// audience-fenced selected project. ds-brain owns scope, freshness,
/// composition, and publication.
pub fn compounded_report(
    lane_value: &str,
    request: &CompoundedReportRequest,
) -> Result<HeadlessProjectReport<CompoundedReportReceipt>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.compounded_report(project, request),
        |client, project| client.compounded_report(project, request, now()),
    )
}

/// Publish a Combined Report for the project explicitly named on this call.
pub fn compounded_report_for_project(
    lane_value: &str,
    project: &str,
    request: &CompoundedReportRequest,
) -> Result<HeadlessNamedProject<CompoundedReportReceipt>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.compounded_report(project, request),
        |client, project| client.compounded_report(project, request, now()),
    )
}

/// Ask the cloud to compute the individual reports of exact transformers of
/// only the saved, audience-fenced selected project and publish them to it.
/// ds-brain owns write governance, freshness, the claim and the fan-out to
/// the cloud reporter; the receipt says per transformer what the cloud did.
pub fn export_reports(
    lane_value: &str,
    request: &TransformerSet,
) -> Result<HeadlessProjectReport<ExportReportsReceipt>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.export_reports(project, request),
        |client, project| client.export_reports(project, request, now()),
    )
}

pub fn export_reports_for_project(
    lane_value: &str,
    project: &str,
    request: &TransformerSet,
) -> Result<HeadlessNamedProject<ExportReportsReceipt>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.export_reports(project, request),
        |client, project| client.export_reports(project, request, now()),
    )
}

/// List the published Combined Report archives of only the saved,
/// audience-fenced selected project.
pub fn compounded_report_list(
    lane_value: &str,
) -> Result<HeadlessProjectReport<Vec<CompoundedArchive>>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.compounded_report_list(project),
        |client, project| client.compounded_report_list(project, now()),
    )
}

pub fn compounded_report_list_for_project(
    lane_value: &str,
    project: &str,
) -> Result<HeadlessNamedProject<Vec<CompoundedArchive>>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.compounded_report_list(project),
        |client, project| client.compounded_report_list(project, now()),
    )
}

/// Read the transformer lifecycle inventory of only the saved,
/// audience-fenced selected project, or of the exact requested names.
pub fn transformer_inventory(
    lane_value: &str,
    requested: &TransformerSet,
) -> Result<HeadlessProjectReport<TransformerInventory>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.transformer_inventory(project, requested),
        |client, project| client.transformer_inventory(project, requested, now()),
    )
}

/// Read the lifecycle inventory for the caller's explicit project. The saved
/// native project selection is neither read nor changed.
pub fn transformer_inventory_for_project(
    lane_value: &str,
    project: &str,
    requested: &TransformerSet,
) -> Result<HeadlessNamedProject<TransformerInventory>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.transformer_inventory(project, requested),
        |client, project| client.transformer_inventory(project, requested, now()),
    )
}

/// Read several design snapshots from one explicitly named project under one
/// restored identity. A second project cannot enter between members.
pub fn transformer_contexts_for_project(
    lane_value: &str,
    project: &str,
    transformers: &[String],
) -> Result<HeadlessNamedProject<Vec<TransformerContext>>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| {
            transformers
                .iter()
                .map(|name| device.transformer_context(project, name))
                .collect()
        },
        |client, project| {
            transformers
                .iter()
                .map(|name| client.transformer_context(project, name, now()))
                .collect()
        },
    )
}

/// Read the transformer status rows of only the saved, audience-fenced
/// selected project, or of the exact requested names.
///
/// This is the read every headless Design answer is built from. The rows are
/// returned as ds-brain sent them: ds-client-core bounds the envelope and the
/// reference names and reshapes nothing, because what a status member means
/// belongs to the module that reads it.
pub fn transformer_status(
    lane_value: &str,
    requested: &TransformerSet,
) -> Result<HeadlessProjectReport<TransformerStatusList>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.transformer_status(project, requested),
        |client, project| client.transformer_status(project, requested, now()),
    )
}

/// One `/report` result taken under the restored user against the project the
/// CALLER named.
///
/// The saved selection is neither read nor written, so the answer does not
/// depend on which project this machine happens to be pointed at. The account
/// travels beside the result because a capture has to be able to say whose
/// credential took it — ds-brain still decides membership, and naming the
/// project here is the end of guessing, not a second authority.
pub struct HeadlessNamedProject<T> {
    identity: ProviderIdentity,
    user_email: String,
    lane: &'static str,
    project_id: String,
    result: T,
}

impl<T> HeadlessNamedProject<T> {
    pub const fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    pub fn user_email(&self) -> &str {
        &self.user_email
    }
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub const fn result(&self) -> &T {
        &self.result
    }
    pub fn into_result(self) -> T {
        self.result
    }
}

fn headless_named_project<T>(
    lane_value: &str,
    project: &str,
    device_call: impl FnOnce(&mut device::DeviceSession, &str) -> Result<T, ClientError>,
    session_call: impl FnOnce(
        &mut Client<NativeTransport, NativeRefreshStore>,
        &str,
    ) -> Result<T, ClientError>,
) -> Result<HeadlessNamedProject<T>, Failure> {
    headless_named_project_with(lane_value, project, map_client, device_call, session_call)
}

/// One explicit-project call whose service keeps its own error vocabulary.
/// The saved native selection is never read; both providers map a failure
/// through the same `map_err`, so which one restored cannot change the answer.
fn headless_named_project_with<T>(
    lane_value: &str,
    project: &str,
    map_err: impl Fn(ClientError) -> Failure,
    device_call: impl FnOnce(&mut device::DeviceSession, &str) -> Result<T, ClientError>,
    session_call: impl FnOnce(
        &mut Client<NativeTransport, NativeRefreshStore>,
        &str,
    ) -> Result<T, ClientError>,
) -> Result<HeadlessNamedProject<T>, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    if let Some(mut device) = restored_device_session(lane)? {
        let result = device_call(&mut device, &project).map_err(&map_err)?;
        return Ok(HeadlessNamedProject {
            identity: ProviderIdentity::new(
                lane.token(),
                device.profile().credential_audience_sha256(),
                device.context().uid(),
            )?,
            user_email: device.context().email().to_owned(),
            lane: lane.token(),
            project_id: project,
            result,
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let result = session_call(&mut client, &project).map_err(&map_err)?;
    Ok(HeadlessNamedProject {
        identity: ProviderIdentity::new(
            lane.token(),
            client.profile().credential_audience_sha256(),
            user.uid(),
        )?,
        user_email: user.email().to_owned(),
        lane: lane.token(),
        project_id: project,
        result,
    })
}

/// Read the transformer status rows of the project the CALLER named.
///
/// The same fixed status call [`transformer_status`] makes, asked about a
/// named project instead of the saved one. Nothing about this machine's
/// selection is read, so the same question asked twice from two terminals
/// about the same project gets the same answer.
pub fn transformer_status_for_project(
    lane_value: &str,
    project: &str,
    requested: &TransformerSet,
) -> Result<HeadlessNamedProject<TransformerStatusList>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.transformer_status(project, requested),
        |client, project| client.transformer_status(project, requested, now()),
    )
}

/// Every project this account can currently reach, as the gateway lists it.
///
/// The directory a sweep plans over. It is the same read `ds auth project
/// list` performs, exposed as a library answer so a caller that is about to
/// visit many projects does not have to shell out to itself.
pub struct HeadlessDirectory {
    identity: ProviderIdentity,
    lane: &'static str,
    elevated: bool,
    projects: Vec<Value>,
}

impl HeadlessDirectory {
    pub const fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    /// Whether ds-brain listed these projects by elevation rather than by
    /// membership; see [`directory_rows`].
    pub const fn elevated(&self) -> bool {
        self.elevated
    }
    /// One entry per visible project: `ds_project`, `project_name`,
    /// `display_name`, `role`, `status` — exactly the members
    /// `ds auth project list` prints.
    pub fn projects(&self) -> &[Value] {
        &self.projects
    }
}

/// List every project the restored native user can reach on this lane.
pub fn project_directory(lane_value: &str) -> Result<HeadlessDirectory, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        let identity = ProviderIdentity::new(
            lane.token(),
            device.profile().credential_audience_sha256(),
            device.context().uid(),
        )?;
        let directory = device.list_projects().map_err(map_client)?;
        return Ok(HeadlessDirectory {
            identity,
            lane: lane.token(),
            elevated: directory.elevated(),
            projects: directory_rows(&directory, usize::MAX),
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let identity = ProviderIdentity::new(
        lane.token(),
        client.profile().credential_audience_sha256(),
        user.uid(),
    )?;
    let directory = client.list_projects(now()).map_err(map_client)?;
    Ok(HeadlessDirectory {
        identity,
        lane: lane.token(),
        elevated: directory.elevated(),
        projects: directory_rows(&directory, usize::MAX),
    })
}

/// One saved-selection operation in only the saved, audience-fenced selected
/// project.
///
/// Membership is evaluated by ds-brain and nothing in `ds` re-derives it: this
/// is the residency fix the survey called for, not a second authority. A read
/// returns the same evaluated member set and digest the register shows, and a
/// promotion echoes that digest back.
pub fn design_selections(
    lane_value: &str,
    request: &ds_client_core::DesignSelectionRequest,
) -> Result<HeadlessProjectReport<ds_client_core::DesignSelectionAnswer>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.design_selections(project, request),
        |client, project| client.design_selections(project, request, now()),
    )
}

/// The `pm` route's refusal, as `ds pm` documents it: the server declined
/// the command by name. See [`map_project_management_refusal`].
pub const PM_REFUSED_REFUSAL: Refusal = Refusal {
    code: "pm_refused",
    when: "ds-brain refused the command by name — a malformed intent, a duplicate id, a member who is not on the project, or a rule the engine enforces",
    remedy: "read detail.service_message; correct the flag it names and retry",
};

/// One governed project-management action — a read of the graph or the
/// context, or one committed command or draft — for only the saved,
/// audience-fenced selected project.
///
/// Until 2026-09-19 every `ds pm` command declared `Requires::Window` and
/// relayed through the paired desktop to the page's own adapters, so a server
/// — the host most likely to be asked what a plan says — could not read one at
/// all. `POST /api/v1/pm` was already published on both gateway lanes and
/// ds-brain already authenticated from the bearer: the window was habit, never
/// contract. On 2026-09-20 the writes followed the reads through this same
/// door.
pub fn project_management(
    lane_value: &str,
    command: &ds_client_core::project_management::Command,
) -> Result<HeadlessProjectReport<serde_json::Value>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.project_management(project, command),
        |client, project| client.project_management(project, command, now()),
    )
}

/// One project-management command whose project is named by this request.
/// The saved native selection is never observed or changed.
pub fn project_management_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::project_management::Command,
) -> Result<HeadlessNamedProject<serde_json::Value>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.project_management(project, command),
        |client, project| client.project_management(project, command, now()),
    )
}

/// One governed catalogue action on the project's assets, for only the
/// saved, audience-fenced selected project. `reader` carries the bytes of
/// an ingest and nothing else; the door streams them once.
///
/// Until 2026-09-20 nine `ds assets` commands relayed through the paired
/// desktop; `POST /api/v1/assets` authenticates from the bearer and needs no
/// window, exactly as `shared_assets` beside this already proved.
pub fn project_assets(
    lane_value: &str,
    command: &ds_client_core::project_assets::Command,
    reader: Option<&mut dyn std::io::Read>,
) -> Result<HeadlessProjectReport<Value>, Failure> {
    // Exactly one of the two closures runs, and the reader is consumed by
    // whichever does: a shared cell hands it to that one without the borrow
    // checker having to know which.
    let reader = std::cell::RefCell::new(reader);
    headless_project_report(
        lane_value,
        |device, project| device.project_assets(project, command, reader.borrow_mut().take()),
        |client, project| {
            client.project_assets(project, command, reader.borrow_mut().take(), now())
        },
    )
}

/// One asset catalogue action scoped to the project named on this request.
pub fn project_assets_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::project_assets::Command,
    reader: Option<&mut dyn std::io::Read>,
) -> Result<HeadlessNamedProject<Value>, Failure> {
    let reader = std::cell::RefCell::new(reader);
    headless_named_project(
        lane_value,
        project,
        |device, project| device.project_assets(project, command, reader.borrow_mut().take()),
        |client, project| {
            client.project_assets(project, command, reader.borrow_mut().take(), now())
        },
    )
}

/// One governed design annotation action — a tag, a group, a consumer
/// grouping, an administrative enrichment or a comment thread — for only the
/// saved, audience-fenced selected project.
///
/// Until 2026-09-20 twenty `ds design` commands relayed these through the
/// paired desktop; `POST /api/v1/design/annotations` authenticates from the
/// bearer and ds-brain is the only authority, so the window was never the
/// contract.
pub fn design_annotations(
    lane_value: &str,
    command: &ds_client_core::design_annotations::Command,
) -> Result<HeadlessProjectReport<Value>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.design_annotations(project, command),
        |client, project| client.design_annotations(project, command, now()),
    )
}

/// The selected project's known-column visibility: read it, or set one
/// field against the revision it was read at.
pub fn known_columns(
    lane_value: &str,
    command: &ds_client_core::known_columns::Command,
) -> Result<HeadlessProjectReport<Value>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.known_columns(project, command),
        |client, project| client.known_columns(project, command, now()),
    )
}

/// One preview-pinned pole-material catalogue repair through the report gate,
/// sourced from the selected project; the receipt is returned whole, with
/// the request the kernel built for that project, for the kernel to judge.
///
/// `build` is handed the selected project id — the request's
/// `source_project` — once the credential and its selection are restored,
/// so the request is built exactly once and for the right project.
pub fn material_propagation(
    lane_value: &str,
    build: impl FnOnce(&str) -> Result<Value, Failure>,
) -> Result<(String, Value, Value), Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((mut device, selected)) = restored_device_project(lane)? {
        let request = build(selected.project_id())?;
        let receipt = device
            .material_propagation(selected.project_id(), &request)
            .map_err(map_client)?;
        return Ok((selected.project_id().to_owned(), request, receipt));
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = load_selected_project(client.profile(), &user)?;
    let request = build(selected.project_id())?;
    let result = client.material_propagation(selected.project_id(), &request, now());
    let receipt = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok((selected.project_id().to_owned(), request, receipt))
}

/// One asset's catalogue row and its verified bytes, through the signed read
/// the catalogue mints for this caller, for only the selected project.
pub fn read_asset_bytes(
    lane_value: &str,
    asset_id: &str,
) -> Result<HeadlessProjectReport<(Value, Vec<u8>)>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.read_asset_bytes(project, asset_id),
        |client, project| client.read_asset_bytes(project, asset_id, now()),
    )
}

/// Read verified asset bytes for the project named on this request.
pub fn read_asset_bytes_for_project(
    lane_value: &str,
    project: &str,
    asset_id: &str,
) -> Result<HeadlessNamedProject<(Value, Vec<u8>)>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.read_asset_bytes(project, asset_id),
        |client, project| client.read_asset_bytes(project, asset_id, now()),
    )
}

/// Retire or restore exact transformers in only the saved, audience-fenced
/// selected project. ds-brain decides governance, ownership, and lifecycle
/// per name and answers every name in order.
pub fn transformer_retirement(
    lane_value: &str,
    action: RetirementAction,
    request: &RetirementRequest,
) -> Result<HeadlessProjectReport<RetirementReceipt>, Failure> {
    headless_project_report(
        lane_value,
        |device, project| device.transformer_retirement(project, action, request),
        |client, project| client.transformer_retirement(project, action, request, now()),
    )
}

/// Retire or restore exact transformers in the caller's explicit project. The
/// saved native project selection is neither read nor changed; ds-brain still
/// decides governance, ownership, and lifecycle per name.
pub fn transformer_retirement_for_project(
    lane_value: &str,
    project: &str,
    action: RetirementAction,
    request: &RetirementRequest,
) -> Result<HeadlessNamedProject<RetirementReceipt>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.transformer_retirement(project, action, request),
        |client, project| client.transformer_retirement(project, action, request, now()),
    )
}

/// Read the form bindings of the caller's explicit project. The gateway
/// rechecks membership; the saved native selection is never read.
pub fn project_forms(lane_value: &str, project: &str) -> Result<HeadlessProjectForms, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.project_forms(project),
        |client, project| client.project_forms(project, now()),
    )?;
    Ok(HeadlessProjectForms {
        lane: named.lane,
        project_name: String::new(),
        project_status: String::new(),
        snapshot: named.result,
    })
}

/// Read the settings editor of one form in the caller's explicit project.
pub fn project_form_editor(
    lane_value: &str,
    project: &str,
    form_slug: &str,
) -> Result<HeadlessProjectFormEditor, Failure> {
    let named = headless_named_project(
        lane_value,
        project,
        |device, project| device.project_form_editor(project, form_slug),
        |client, project| client.project_form_editor(project, form_slug, now()),
    )?;
    Ok(HeadlessProjectFormEditor {
        lane: named.lane,
        project_name: String::new(),
        project_status: String::new(),
        snapshot: named.result,
    })
}

/// Capture one governed city under immutable named-project authority.
/// Signed media URLs remain zeroizing owner-intake bytes, never public fields.
pub fn solar_snapshot_for_project(
    lane_value: &str,
    project: &str,
    template_id: &str,
) -> Result<SolarSnapshot, Failure> {
    let mut session = solar_project_session_for_project(lane_value, project)?;
    session.verify_authority()?;
    let result = match &mut session.provider {
        SolarProjectProvider::Firebase(client) => {
            client.solar_snapshot(&session.project, template_id, now())
        }
        SolarProjectProvider::Device(device) => {
            device.solar_snapshot(&session.project, template_id)
        }
    }
    .map_err(|error| {
        if error.kind() == ErrorKind::ResourceNotFound {
            Failure::invalid(
                "solar_city_not_found",
                "the city does not exist in the explicit project",
            )
            .remedy("pass one exact city id from ds solar cities --project <id>")
        } else {
            map_client(error)
        }
    });
    session.verify_authority()?;
    result
}

/// Ask one bounded Survey aggregate question of the caller's explicit project.
pub fn survey_query(
    lane_value: &str,
    project: &str,
    query: &SurveyQueryRequest,
) -> Result<HeadlessSurveyQuery, Failure> {
    let named = headless_named_project_with(
        lane_value,
        project,
        map_survey_query_error,
        |device, project| device.survey_query(project, query),
        |client, project| client.survey_query(project, query, now()),
    )?;
    Ok(HeadlessSurveyQuery {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        result: named.result,
    })
}

/// The Survey query vocabulary, identical whichever provider restored.
fn map_survey_query_error(error: ClientError) -> Failure {
    if let Some(code) = error.survey_query_service_code() {
        return map_survey_query_service_code(code);
    }
    if let Some(code) = error.survey_form_read_service_code() {
        return map_survey_form_read_service_code(code);
    }
    match error.kind() {
        ErrorKind::RouteUnavailable => route_unavailable(),
        ErrorKind::ResourceNotFound => Failure::invalid(
            "survey_scope_not_found",
            "the project or governed form is unavailable to this verified user",
        )
        .remedy("verify --project and pass one exact available form slug"),
        ErrorKind::InvalidInput => Failure::conflict(
            "survey_query_refused",
            "the backend refused the already validated Survey question or reported a stale view",
        )
        .remedy("retry once, then verify the governed form and Survey view state"),
        _ => map_client(error),
    }
}

/// Select Survey entries by a bounded box in the caller's explicit project.
pub fn survey_entries_select(
    lane_value: &str,
    project: &str,
    request: &SurveyEntriesSelectRequest,
) -> Result<HeadlessSurveyEntriesSelection, Failure> {
    let named = headless_named_project_with(
        lane_value,
        project,
        map_survey_entries_select_error,
        |device, project| device.survey_entries_select(project, request),
        |client, project| client.survey_entries_select(project, request, now()),
    )?;
    Ok(HeadlessSurveyEntriesSelection {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        selection: named.result,
    })
}

/// Read Survey entry changes since a clock in the caller's explicit project.
pub fn survey_entries_changes(
    lane_value: &str,
    project: &str,
    request: &SurveyEntriesChangesRequest,
) -> Result<HeadlessSurveyEntriesChanges, Failure> {
    let named = headless_named_project_with(
        lane_value,
        project,
        map_survey_entries_changes_error,
        |device, project| device.survey_entries_changes(project, request),
        |client, project| client.survey_entries_changes(project, request, now()),
    )?;
    Ok(HeadlessSurveyEntriesChanges {
        lane: named.lane,
        project_id: named.project_id,
        project_name: String::new(),
        project_status: String::new(),
        changes: named.result,
    })
}

/// Create one governed Survey entry in the caller's explicit project.
pub fn survey_entry_create(
    lane_value: &str,
    project: &str,
    request: &SurveyEntryCreateRequest,
) -> Result<HeadlessSurveyEntryCreate, Failure> {
    let named = headless_named_project_with(
        lane_value,
        project,
        map_survey_entry_create_error,
        |device, project| device.survey_entry_create(project, request),
        |client, project| client.survey_entry_create(project, request, now()),
    )?;
    Ok(HeadlessSurveyEntryCreate {
        lane: named.lane,
        project_name: String::new(),
        project_status: String::new(),
        receipt: named.result,
    })
}

/// The Survey create vocabulary, identical whichever provider restored.
fn map_survey_entry_create_error(error: ClientError) -> Failure {
    if let Some(code) = error.survey_entry_create_service_code() {
        return map_survey_entry_create_service_code(code);
    }
    match error.kind() {
        ErrorKind::ResourceNotFound => Failure::invalid(
            "survey_entry_create_scope_not_found",
            "the project, governed form, or context ancestor is unavailable",
        )
        .remedy("verify --project, the form, and the optional context key"),
        ErrorKind::InvalidInput => Failure::invalid(
            "survey_entry_create_refused",
            "the backend refused the already validated governed Survey create request",
        )
        .remedy("recheck the form, document identity, context, and document bounds"),
        ErrorKind::AuthenticationRejected => Failure::unauthorized(
            "survey_entry_create_auth_rejected",
            "the fixed create route rejected the verified identity or form authority",
        )
        .remedy("verify account and entries.create authority in the project"),
        ErrorKind::Transient => Failure::unavailable(
            "survey_entry_create_failed",
            "the governed Survey create service failed temporarily",
        )
        .remedy("after service recovery, retry the exact document with the same idempotency key"),
        ErrorKind::UnreadableResponse => Failure::unavailable(
            "survey_entry_create_unreadable",
            "the create response violated its closed identity, version, clock, or authority contract",
        )
        .remedy("verify the backend release and update ds before retrying"),
        _ => map_client(error),
    }
}

/// Open one Survey import session for the caller's explicit project. The
/// session's context is named, never the saved selection.
pub fn survey_import_session(
    lane_value: &str,
    project: &str,
) -> Result<HeadlessSurveyImportSession, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    if let Some(device) = restored_device_session(lane)? {
        let identity = device.context();
        let principal_sha256 = principal_binding_sha256(identity.uid(), identity.email());
        let credential_audience_sha256 = device.profile().credential_audience_sha256().to_owned();
        let selected = state::ProjectContext::named(
            device.profile(),
            identity.uid(),
            identity.email(),
            &project,
        );
        return Ok(HeadlessSurveyImportSession {
            lane: lane.token(),
            project_id: project,
            project_name: String::new(),
            project_status: String::new(),
            principal_sha256,
            credential_audience_sha256,
            selected,
            provider: SurveyImportProvider::Device(Box::new(device)),
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let principal_sha256 = principal_binding_sha256(user.uid(), user.email());
    let selected =
        state::ProjectContext::named(client.profile(), user.uid(), user.email(), &project);
    Ok(HeadlessSurveyImportSession {
        lane: lane.token(),
        project_id: project,
        project_name: String::new(),
        project_status: String::new(),
        principal_sha256,
        credential_audience_sha256: client.profile().credential_audience_sha256().to_owned(),
        selected,
        provider: SurveyImportProvider::Firebase(Box::new(client)),
    })
}

fn principal_binding_sha256(uid: &str, email: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"ds.survey.import.principal/v1\0");
    digest.update(uid.as_bytes());
    digest.update(b"\0");
    digest.update(email.as_bytes());
    format!("{:x}", digest.finalize())
}

fn map_survey_entry_create_service_code(code: SurveyEntryCreateServiceCode) -> Failure {
    match code {
        SurveyEntryCreateServiceCode::Invalid => Failure::invalid(
            "survey_entry_create_invalid",
            "the fixed service rejected the bounded Survey create request",
        )
        .remedy("recheck the exact form, document id, timestamp, context, and JSON document"),
        SurveyEntryCreateServiceCode::Unauthorized => Failure::unauthorized(
            "survey_entry_create_auth_rejected",
            "the governed Survey create service rejected the current native session",
        )
        .remedy("sign in again and verify the selected project"),
        SurveyEntryCreateServiceCode::PermissionDenied => Failure::unauthorized(
            "survey_entry_create_permission_denied",
            "the verified user lacks entries.create authority for this Survey form",
        )
        .remedy("request entries.create authority for the selected project and form"),
        SurveyEntryCreateServiceCode::ScopeNotFound => Failure::invalid(
            "survey_entry_create_scope_not_found",
            "the selected project, governed form, or context ancestor is unavailable",
        )
        .remedy("verify the selected project, form, and optional context key"),
        SurveyEntryCreateServiceCode::FormDisabled => Failure::invalid(
            "survey_entry_create_form_disabled",
            "the Survey form is not enabled for entry creation in the selected project",
        )
        .remedy("enable the project form before creating entries"),
        SurveyEntryCreateServiceCode::ProjectReadOnly => Failure::conflict(
            "survey_entry_create_project_read_only",
            "the selected project lifecycle does not permit Survey entry creation",
        )
        .remedy("select an active writable project"),
        SurveyEntryCreateServiceCode::IdempotencyConflict => Failure::conflict(
            "survey_entry_create_idempotency_conflict",
            "the idempotency key is already bound to a different Survey mutation",
        )
        .remedy("use the original exact request for replay, or a fresh key for a distinct create"),
        SurveyEntryCreateServiceCode::AlreadyExists => Failure::conflict(
            "survey_entry_create_already_exists",
            "the Survey document already exists and this request is not its exact replay",
        )
        .remedy("choose a new document id, or replay the original exact request and key"),
        SurveyEntryCreateServiceCode::Failed => Failure::unavailable(
            "survey_entry_create_failed",
            "the governed Survey create service failed temporarily",
        )
        .remedy("after service recovery, retry the exact document with the same idempotency key"),
    }
}

/// One error vocabulary for the coalesced Survey changes page, whichever lane
/// answered. The paired branch fell through to [`map_client`] for exactly the
/// same reason the selection branch did; see
/// [`map_survey_entries_select_error`].
fn map_survey_entries_changes_error(error: ClientError) -> Failure {
    if let Some(code) = error.survey_form_read_service_code() {
        return map_survey_form_read_service_code(code);
    }
    if let Some(code) = error.survey_entries_changes_service_code() {
        return map_survey_entries_changes_service_code(code);
    }
    survey_entries_changes_kind(error.kind()).unwrap_or_else(|| map_client(error))
}

/// The changes route's own refusal for one transport kind, or `None` when it
/// has nothing to add.
fn survey_entries_changes_kind(kind: ErrorKind) -> Option<Failure> {
    Some(match kind {
        ErrorKind::ResourceNotFound => Failure::invalid(
            "survey_entries_scope_not_found",
            "the selected project or governed form is unavailable to this verified user",
        )
        .remedy("verify the selected project and pass one exact available form slug"),
        ErrorKind::InvalidInput => survey_entries_changes_refused(),
        ErrorKind::AuthenticationRejected => Failure::unauthorized(
            "survey_entries_changes_auth_rejected",
            "the fixed changes route rejected the verified identity or form authority",
        )
        .remedy("verify account and form authority in the selected project"),
        ErrorKind::Transient => Failure::unavailable(
            "survey_entries_changes_transient",
            "the fixed changes service is temporarily unavailable without a recognized service code",
        )
        .remedy("retry the identical page request without advancing its checkpoint"),
        ErrorKind::UnreadableResponse => Failure::unavailable(
            "survey_entries_changes_unreadable",
            "the changes response violated its closed identity, clocks, geometry, ordering, paging, or consistency contract",
        )
        .remedy("retry once without advancing the checkpoint, then update ds if it persists"),
        _ => return None,
    })
}

fn survey_entries_changes_refused() -> Failure {
    Failure::invalid(
        "survey_entries_changes_refused",
        "the backend refused the already validated Survey changes request without a recognized service code",
    )
    .remedy("verify the form and restart from the last completed checkpoint")
}

fn map_survey_entries_changes_service_code(code: SurveyEntriesChangesServiceCode) -> Failure {
    match code {
        SurveyEntriesChangesServiceCode::Invalid => Failure::invalid(
            "survey_entries_changes_invalid",
            "the fixed service rejected the bounded Survey changes request",
        )
        .remedy("recheck the exact form, updated-after clock, limit, and cursor"),
        SurveyEntriesChangesServiceCode::CursorInvalid => Failure::invalid(
            "survey_entries_changes_cursor_invalid",
            "the opaque Survey changes cursor is invalid for this request or authority",
        )
        .remedy("reuse the exact next_cursor with identical --updated-after and --limit, or restart from the last completed checkpoint"),
        SurveyEntriesChangesServiceCode::FenceExpired => Failure::invalid(
            "survey_entries_changes_fence_expired",
            "the immutable page fence carried by this incomplete cursor has expired",
        )
        .remedy("discard the incomplete cursor and restart from the last previously completed checkpoint, never this expired feed's upper_fence"),
        SurveyEntriesChangesServiceCode::TooExpensive => Failure::failed(
            "survey_entries_changes_too_expensive",
            "the bounded Survey changes query exceeded its query budget",
        )
        .remedy("keep the last completed checkpoint unchanged; repair partitioning or indexing, or raise the governed backend query budget, then restart there"),
        SurveyEntriesChangesServiceCode::TooLarge => Failure::invalid(
            "survey_entries_changes_too_large",
            "the bounded Survey changes page exceeded its response limit",
        )
        .remedy("lower --limit and restart from the last completed checkpoint"),
        SurveyEntriesChangesServiceCode::MirrorInvalid => Failure::failed(
            "survey_entries_changes_mirror_invalid",
            "the Survey mirror could not represent valid change evidence",
        )
        .remedy("repair or update the governed Survey mirror; an unchanged retry is not a remedy"),
        SurveyEntriesChangesServiceCode::SnapshotUnavailable => Failure::unavailable(
            "survey_entries_changes_snapshot_unavailable",
            "the immutable BigQuery table version for this changes cursor is temporarily unavailable",
        )
        .remedy("retry the identical page request with the exact same cursor"),
        SurveyEntriesChangesServiceCode::Unavailable => Failure::failed(
            "survey_entries_changes_unavailable",
            "the governed Survey changes service or its durable cursor signing key is unavailable on this deployment",
        )
        .remedy("configure the governed deployment and durable changes cursor signing key, then retry from the last completed checkpoint"),
        SurveyEntriesChangesServiceCode::SyncFailed => Failure::unavailable(
            "survey_entries_changes_sync_failed",
            "Survey data could not be synchronized before reading changes",
        )
        .remedy("retry without changing the page request; report repeated sync failures"),
        SurveyEntriesChangesServiceCode::Failed => Failure::unavailable(
            "survey_entries_changes_failed",
            "the governed Survey changes service failed temporarily",
        )
        .remedy("retry without changing the page request; report repeated failures"),
        SurveyEntriesChangesServiceCode::ScopeNotFound => Failure::invalid(
            "survey_entries_scope_not_found",
            "the selected project or governed form is unavailable to this verified user",
        )
        .remedy("verify the selected project and pass one exact available form slug"),
    }
}

/// One error vocabulary for the bounded Survey selection, whichever lane
/// answered.
///
/// The paired-device branch used to fall through to [`map_client`], whose
/// `ResourceNotFound` arm names a transformer — a word this route never says.
/// The same absent form therefore refused as `survey_entries_scope_not_found`
/// headlessly and as `transformer_not_found` beside a running application,
/// which is one operation with two vocabularies. Both branches now call this.
fn map_survey_entries_select_error(error: ClientError) -> Failure {
    if let Some(code) = error.survey_form_read_service_code() {
        return map_survey_form_read_service_code(code);
    }
    if let Some(code) = error.survey_entries_select_service_code() {
        return map_survey_entries_service_code(code);
    }
    // Only the kinds this route has a word of its own for; anything else is
    // the shared native mapping, unchanged.
    survey_entries_select_kind(error.kind()).unwrap_or_else(|| map_client(error))
}

fn map_survey_query_service_code(code: SurveyQueryServiceCode) -> Failure {
    match code {
        SurveyQueryServiceCode::FormUnknown => Failure::conflict("survey_view_not_found",
            "the authorized survey form has no queryable survey view")
            .remedy("verify the project binding with ds survey project-forms read, then synchronize the project's Survey data to prepare its view"),
        SurveyQueryServiceCode::FieldUnknown => Failure::invalid("survey_field_unknown",
            "the requested field is unavailable to the survey aggregate")
            .remedy("read the governed form schema and use a queryable, non-restricted field"),
        SurveyQueryServiceCode::ViewStale => Failure::conflict("survey_view_stale",
            "the survey view is out of date with its governed schema")
            .remedy("refresh the project's Survey data to rebuild the view before retrying"),
        SurveyQueryServiceCode::TooExpensive => Failure::invalid("survey_query_too_expensive",
            "the survey aggregate exceeds the server scan limit")
            .remedy("narrow the filters or date range before retrying"),
        SurveyQueryServiceCode::SyncFailed => Failure::unavailable("survey_query_sync_failed",
            "Survey synchronization failed before the aggregate could run")
            .remedy("retry without changing project or form; report a persistent synchronization failure"),
        SurveyQueryServiceCode::Unavailable => Failure::unavailable("survey_query_unavailable",
            "Survey queries are unavailable on this deployment")
            .remedy("use a deployment that provides the governed Survey query service"),
        SurveyQueryServiceCode::ScopeNotFound => Failure::invalid("survey_scope_not_found",
            "the service refused the project or survey form scope")
            .remedy("verify the selected project and its bound forms with ds survey project-forms read"),
    }
}

fn map_survey_form_read_service_code(code: SurveyFormReadServiceCode) -> Failure {
    match code {
        SurveyFormReadServiceCode::ProjectAccessDenied => Failure::unauthorized(
            "survey_project_access_denied",
            "the verified user does not have access to the selected project",
        )
        .remedy("select a project whose mirrored membership grants this account"),
        SurveyFormReadServiceCode::FormAccessDenied => Failure::unauthorized(
            "survey_form_access_denied",
            "the governed form is outside this user's project grant",
        )
        .remedy("ask a project manager to add the exact form to this user's grant"),
        SurveyFormReadServiceCode::FormBindingNotFound => Failure::invalid(
            "survey_form_binding_not_found",
            "the governed form is not bound to the selected project",
        )
        .remedy("pass an exact bound slug from `ds survey project-forms read`"),
        SurveyFormReadServiceCode::FormNotParticipating => Failure::conflict(
            "survey_form_not_participating",
            "the bound project form is disabled, hidden, or withdrawn from Survey reads",
        )
        .remedy("enable the project form for Survey participation before retrying"),
    }
}

/// The selection route's own refusal for one transport kind, or `None` when
/// it has nothing to add. Split out from [`map_survey_entries_select_error`]
/// so it can be exercised without a `ClientError`, which no caller of this
/// crate can construct.
fn survey_entries_select_kind(kind: ErrorKind) -> Option<Failure> {
    Some(match kind {
        ErrorKind::ResourceNotFound => Failure::invalid(
            "survey_entries_scope_not_found",
            "the selected project or governed form is unavailable to this verified user",
        )
        .remedy("verify the selected project and pass one exact available form slug"),
        ErrorKind::InvalidInput => Failure::conflict(
            "survey_entries_refused",
            "the backend refused the already validated bounded Survey entry selection",
        )
        .remedy("narrow --bbox or lower --limit, then verify the governed form state"),
        ErrorKind::AuthenticationRejected => Failure::unauthorized(
            "survey_entries_auth_rejected",
            "the fixed selection route rejected the verified identity or form authority",
        )
        .remedy("verify account and form authority in the selected project"),
        ErrorKind::Transient => Failure::unavailable(
            "survey_entries_transient",
            "the fixed selection service or its required mirror sync is temporarily unavailable",
        )
        .remedy("retry without changing local state"),
        ErrorKind::UnreadableResponse => Failure::unavailable(
            "survey_entries_unreadable",
            "the selection response violated its closed identity, geometry, consistency, order, or digest contract",
        )
        .remedy("retry once, then update ds if it persists"),
        _ => return None,
    })
}

fn map_survey_entries_service_code(code: SurveyEntriesSelectServiceCode) -> Failure {
    match code {
        SurveyEntriesSelectServiceCode::TooExpensive => Failure::invalid(
            "survey_entries_too_expensive",
            "the bounded Survey entry selection exceeded its query budget",
        )
        .remedy("narrow --bbox before retrying"),
        SurveyEntriesSelectServiceCode::TooLarge => Failure::invalid(
            "survey_entries_too_large",
            "the bounded Survey entry selection exceeded its response limit",
        )
        .remedy("narrow --bbox or lower --limit before retrying"),
        SurveyEntriesSelectServiceCode::SyncFailed => Failure::unavailable(
            "survey_entries_sync_failed",
            "Survey data could not be synchronized before entry selection",
        )
        .remedy("retry without changing the selection; report repeated sync failures"),
        SurveyEntriesSelectServiceCode::MirrorInvalid => Failure::failed(
            "survey_entries_mirror_invalid",
            "the Survey mirror could not represent the entry selection safely",
        )
        .remedy("repair or update the governed Survey mirror; an unchanged retry is not a remedy"),
        SurveyEntriesSelectServiceCode::Invalid => Failure::invalid(
            "survey_entries_invalid",
            "the fixed service rejected the bounded Survey entry selection",
        )
        .remedy("recheck the exact form, bbox, and limit before retrying"),
        SurveyEntriesSelectServiceCode::Unavailable => Failure::unavailable(
            "survey_entries_unavailable",
            "the governed Survey entry selection service is unavailable on this deployment",
        )
        .remedy("retry later without changing the selection"),
        SurveyEntriesSelectServiceCode::Failed => Failure::unavailable(
            "survey_entries_failed",
            "the governed Survey entry selection service failed temporarily",
        )
        .remedy("retry without changing the selection; report repeated failures"),
        SurveyEntriesSelectServiceCode::ScopeNotFound => Failure::invalid(
            "survey_entries_scope_not_found",
            "the selected project or governed form is unavailable to this verified user",
        )
        .remedy("verify the selected project and pass one exact available form slug"),
    }
}

fn client(inputs: &Inputs) -> Result<(Lane, NativeClient), Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    Ok((lane, Client::new(profile, NativeTransport, store)))
}

pub fn run_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let _ = probe_headless_identity(lane.token())?;
    if let Some(status) = device_status(lane)? {
        return Ok(status);
    }
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    let auth_context = NativeContextProvider {
        lane,
        client: &mut client,
        context: &context,
    }
    .resolve_context()?;
    let mut output = match auth_context.principal() {
        Some(principal) => json!({
            "lane": lane.token(),
            "signed_in": true,
            "uid": principal.uid(),
            "email": principal.email(),
        }),
        None => json!({ "lane": lane.token(), "signed_in": false }),
    };
    output["auth_context"] = serde_json::to_value(auth_context).map_err(|_| {
        Failure::failed(
            "auth_context_unreadable",
            "the bounded authenticated context could not be projected safely",
        )
    })?;
    Ok(output)
}

/// The `auth status` answer for a lane connected by device credential, or
/// `None` when the lane holds no device credential. `ds account connect`
/// answers with exactly this shape, so a caller reads one status whichever
/// command it asked.
pub(crate) fn device_status(lane: Lane) -> Result<Option<Value>, Failure> {
    let profile = profile::load(lane)?;
    let Some(device) = device::probe_context(lane)? else {
        return Ok(None);
    };
    let selected = ProjectContextLease::acquire(&profile)?
        .load_snapshot(&profile, device.uid(), device.email())?
        .as_ref()
        .map(selected_project);
    let auth_context = AuthContext::restored_device(&profile, &device, selected);
    Ok(Some(json!({
        "lane": lane.token(), "signed_in": true,
        "uid": device.uid(), "email": device.email(),
        "credential_provider": "ds_device", "device_id": device.device_id(),
        "auth_context": serde_json::to_value(auth_context).map_err(|_| Failure::failed(
            "auth_context_unreadable", "the bounded authenticated context could not be projected safely"
        ))?,
    })))
}

fn selected_project(context: &state::ProjectContext) -> SelectedProject {
    SelectedProject::new(
        context.project_id(),
        context.project_name(),
        context.display_name(),
        context.role(),
        context.status(),
    )
}

pub fn run_login(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    let durable_device = device::probe_context(lane)?;
    let email = inputs.require("email")?;
    let mut password = read_password(inputs.switch("password-stdin"))?;
    let result = client.sign_in(email, &password, now()).map_err(map_client);
    password.zeroize();
    let user = result?;
    if let Some(device) = durable_device
        && (device.lane() != lane.token()
            || device.credential_audience_sha256() != client.profile().credential_audience_sha256()
            || device.uid() != user.uid())
    {
        client.sign_out().map_err(|_| cleanup_required())?;
        return Err(Failure::conflict(
            "auth_context_mismatch",
            "the signed-in Firebase principal conflicts with the protected DS device principal",
        )
        .remedy("revoke the unintended device or sign in with its exact canonical account"));
    }
    context.clear().map_err(|_| cleanup_required())?;
    Ok(json!({ "lane": lane.token(), "signed_in": true, "uid": user.uid(), "email": user.email() }))
}

pub fn run_logout(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let _ = probe_headless_identity(lane.token())?;
    let remaining_device = device::probe_context(lane)?;
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    client.sign_out().map_err(map_client)?;
    if let Some(device) = remaining_device {
        return Ok(json!({
            "lane": lane.token(),
            "signed_in": true,
            "firebase_signed_out": true,
            "credential_provider": "ds_device",
            "device_id": device.device_id(),
            "context_cleared": false,
        }));
    }
    context.clear().map_err(|_| cleanup_required())?;
    Ok(
        json!({ "lane": lane.token(), "signed_in": false, "firebase_signed_out": true, "context_cleared": true }),
    )
}

pub fn run_project_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = parse_limit(inputs.require("limit")?)?;
    let lane = Lane::parse(inputs.require("lane")?)?;
    let _ = probe_headless_identity(lane.token())?;
    if let Some(mut device) = device::restore_session(lane)? {
        let directory = device.list_projects().map_err(map_client)?;
        let mut answer = directory_answer(lane, &directory, limit);
        answer["credential_provider"] = json!("ds_device");
        return Ok(answer);
    }
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    require_restore(&mut client, &context)?;
    let directory = with_disposition(client.list_projects(now()), &context)?;
    Ok(directory_answer(lane, &directory, limit))
}

/// The `ds auth project list` answer for one directory read.
fn directory_answer(lane: Lane, directory: &ProjectDirectory, limit: usize) -> Value {
    let total = directory.projects().len();
    let projects = directory_rows(directory, limit);
    let returned = projects.len();
    json!({
        "lane": lane.token(),
        "elevated": directory.elevated(),
        "projects": projects,
        "returned": returned,
        "total": total,
        "more": total > returned,
    })
}

pub fn run_project_use(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let _ = probe_headless_identity(lane.token())?;
    if let Some(mut device) = device::restore_session(lane)? {
        let identity = device.context();
        let directory = device.list_projects().map_err(map_client)?;
        let requested = inputs.require("project")?;
        let project = directory.exact(requested).ok_or_else(|| {
            Failure::invalid(
                "project_not_visible",
                "that exact project id is not present in the project directory the gateway returned",
            )
            .remedy("run ds auth project list and pass one exact ds_project value")
        })?;
        let context = ProjectContextLease::acquire(device.profile())?;
        let saved = context.save(device.profile(), identity.uid(), identity.email(), project)?;
        return Ok(
            json!({ "lane": lane.token(), "credential_provider": "ds_device", "selected": true,
            "project": context_json(&saved) }),
        );
    }
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    let user = require_restore(&mut client, &context)?;
    let directory = with_disposition(client.list_projects(now()), &context)?;
    let requested = inputs.require("project")?;
    let project = directory.exact(requested).ok_or_else(|| {
        Failure::invalid(
            "project_not_visible",
            "that exact project id is not present in the project directory the gateway returned",
        )
        .remedy("run ds auth project list and pass one exact ds_project value")
    })?;
    let saved = context.save(client.profile(), user.uid(), user.email(), project)?;
    Ok(json!({ "lane": lane.token(), "selected": true, "project": context_json(&saved) }))
}

pub fn run_project_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let _ = probe_headless_identity(lane.token())?;
    if let Some(device) = device::probe_context(lane)? {
        let profile = profile::load(lane)?;
        let saved =
            ProjectContextLease::acquire(&profile)?.load(&profile, device.uid(), device.email())?;
        return Ok(match saved {
            Some(context) => json!({ "lane": lane.token(), "credential_provider": "ds_device",
                "selected": true, "project": context_json(&context) }),
            None => {
                json!({ "lane": lane.token(), "credential_provider": "ds_device", "selected": false })
            }
        });
    }
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    let user = require_restore(&mut client, &context)?;
    let saved = context.load(client.profile(), user.uid(), user.email())?;
    Ok(match saved {
        Some(context) => {
            json!({ "lane": lane.token(), "selected": true, "project": context_json(&context) })
        }
        None => json!({ "lane": lane.token(), "selected": false }),
    })
}

/// Restore before taking the project-context lease. Refresh-token rotation has
/// its own durable store lease, so a remote Firebase refresh must not serialize
/// unrelated selected-project reads.
/// One property flag as the kernel wants it: absent when not given.
fn property(inputs: &Inputs, name: &str) -> Option<String> {
    inputs.value(name).map(str::to_owned)
}

fn properties(
    inputs: &Inputs,
    with_display_name: bool,
) -> ds_client_core::project_properties::Properties {
    ds_client_core::project_properties::Properties {
        display_name: if with_display_name {
            property(inputs, "display-name")
        } else {
            None
        },
        location: property(inputs, "location"),
        country: property(inputs, "country"),
        client: property(inputs, "client"),
        description: property(inputs, "description"),
    }
}

pub fn run_project_create(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let command = ds_client_core::project_properties::Command::Create {
        display_name: inputs.require("display-name")?.to_owned(),
        properties: properties(inputs, false),
        network_template: property(inputs, "network-template"),
        styling_template: property(inputs, "styling-template"),
    };
    project_properties(inputs.require("lane")?, &command)
}

pub fn run_project_update(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let command = ds_client_core::project_properties::Command::Update {
        project_id: inputs.require("project")?.to_owned(),
        properties: properties(inputs, true),
    };
    project_properties(inputs.require("lane")?, &command)
}

/// Create a project or edit its properties, through the lane's credential —
/// its device credential when it holds one, otherwise the restored native
/// user. ds-brain admits both on `/api/v1/projects` and decides by the
/// principal's capabilities; no window and no project selection are involved.
pub fn project_properties(
    lane_value: &str,
    command: &ds_client_core::project_properties::Command,
) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    let receipt = if let Some(mut device) = restored_device_session(lane)? {
        device
            .project_properties(command)
            .map_err(map_project_properties_client)?
    } else {
        let profile = profile::load(lane)?;
        let store = NativeRefreshStore::open()?;
        let mut client = Client::new(profile, NativeTransport, store);
        require_restore_before_context(&mut client)?;
        client
            .project_properties(command, now())
            .map_err(map_project_properties_client)?
    };
    let mut answer = serde_json::to_value(&receipt)
        .map_err(|error| Failure::unavailable(UNREADABLE_REFUSAL.code, error.to_string()))?;
    answer["lane"] = json!(lane.token());
    answer["action"] = json!(command.action());
    Ok(answer)
}

/// One project-properties error as the refusal the two commands declare for
/// it. The route's own sentence is the actionable half of a 403 — WHICH
/// capability a project admin has to grant — so it is carried on the message
/// and the remedy names who can grant it. A 404 is the project-directory
/// refusal `auth project use` already declares, not the shared mapping's
/// transformer one.
fn map_project_properties_client(error: ClientError) -> Failure {
    use ds_client_core::ProjectPropertiesServiceCode as Code;
    let Some(code) = error.project_properties_service_code() else {
        return map_client(error);
    };
    let owner = error.to_string();
    let (status, sentence) = match error.service_refusal() {
        Some(refusal) => (Some(refusal.status()), refusal.message().map(str::to_owned)),
        None => (None, None),
    };
    let message = match (&status, &sentence) {
        (Some(status), Some(sentence)) => format!("{owner} (HTTP {status}): {sentence}"),
        (Some(status), None) => format!("{owner} (HTTP {status})"),
        _ => owner,
    };
    let detail = json!({ "http_status": status, "service_message": sentence });
    match code {
        Code::Invalid => Failure::invalid(PROJECT_PROPERTIES_INVALID_REFUSAL.code, message)
            .detail(detail)
            .remedy(PROJECT_PROPERTIES_INVALID_REFUSAL.remedy),
        Code::CapabilityMissing => {
            let remedy = if sentence
                .as_deref()
                .is_some_and(|s| s.contains("project.create"))
            {
                PROJECT_CREATE_FORBIDDEN_REFUSAL.remedy
            } else {
                PROJECT_EDIT_FORBIDDEN_REFUSAL.remedy
            };
            Failure::unauthorized(PROJECT_EDIT_FORBIDDEN_REFUSAL.code, message)
                .detail(detail)
                .remedy(remedy)
        }
        Code::ProjectReadOnly => {
            Failure::unauthorized(PROJECT_EDIT_FORBIDDEN_REFUSAL.code, message)
                .detail(detail)
                .remedy("unarchive the project, or extend its expiry, before editing it")
        }
        Code::ProjectNotFound => Failure::invalid(PROJECT_UNKNOWN_REFUSAL.code, message)
            .detail(detail)
            .remedy(PROJECT_UNKNOWN_REFUSAL.remedy)
            .next("ds auth project list --output json"),
    }
}

pub fn render_project_properties(data: &Value) -> String {
    match data["action"].as_str() {
        Some("create") => format!(
            "created  {}  {}  ({})\n",
            data["project_id"].as_str().unwrap_or(""),
            data["display_name"].as_str().unwrap_or(""),
            data["project_name"].as_str().unwrap_or(""),
        ),
        _ => format!(
            "updated  {}  {} field(s) changed\n",
            data["project_id"].as_str().unwrap_or(""),
            data["fields_updated"].as_u64().unwrap_or(0),
        ),
    }
}

fn require_restore_before_context(
    client: &mut NativeClient,
) -> Result<ds_client_core::AuthenticatedUser, Failure> {
    match client.restore(now()) {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(signed_out_failure(lane_of(client.profile()))),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::PermanentlyRevoked | ErrorKind::IdentityMismatch
            ) =>
        {
            let context = ProjectContextLease::acquire(client.profile())?;
            context.clear().map_err(|_| cleanup_required())?;
            Err(map_client(error))
        }
        Err(error) => Err(map_client(error)),
    }
}

/// Apply identity disposition after a request whose selected-project snapshot
/// no longer holds the lease. Conditional cleanup cannot erase a concurrent
/// project replacement.
fn with_released_context_disposition<T>(
    profile: &ds_client_core::ClientProfile,
    selected: &state::ProjectContext,
    result: Result<T, ClientError>,
) -> Result<T, Failure> {
    match result {
        Ok(value) => Ok(value),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::PermanentlyRevoked | ErrorKind::IdentityMismatch
            ) =>
        {
            let context = ProjectContextLease::acquire(profile)?;
            context
                .clear_if_unchanged(selected)
                .map_err(|_| cleanup_required())?;
            Err(map_client(error))
        }
        Err(error) => Err(map_client(error)),
    }
}

fn require_restore(
    client: &mut NativeClient,
    context: &ProjectContextLease,
) -> Result<ds_client_core::AuthenticatedUser, Failure> {
    with_disposition(client.restore(now()), context)?
        .ok_or_else(|| signed_out_failure(lane_of(client.profile())))
}

/// The lane a loaded profile was built for, as this crate names it.
fn lane_of(profile: &ds_client_core::ClientProfile) -> Lane {
    match profile.lane() {
        ds_client_core::DeploymentLane::Stable => Lane::Stable,
        ds_client_core::DeploymentLane::Canary => Lane::Canary,
    }
}

/// The signed-out refusal for one lane, with the truth about that lane.
///
/// Two things went wrong with the old sentence on 2026-09-19. Its remedy and
/// next step named no lane, so an agent following `install list --lane
/// canary`'s refusal exactly ran `ds auth login` on the default lane and
/// reproduced the refusal. And on a lane that holds a device credential —
/// where `ds auth status` answers `signed_in: true` — it said nobody was
/// signed in. The code is unchanged: it is the one every caller declares.
/// What changes is that the sentence says which credential is missing, and
/// that every command it names carries the lane it was asked about.
fn signed_out_failure(lane: Lane) -> Failure {
    // A device-store fault must not replace the refusal it was consulted for.
    let device_linked = device::probe_context(lane).ok().flatten().is_some();
    signed_out_refusal(lane, device_linked)
}

fn signed_out_refusal(lane: Lane, device_linked: bool) -> Failure {
    let token = lane.token();
    if device_linked {
        // The lane IS connected. What this command lacks is a route that
        // accepts the device credential, and no sign-in a person can do
        // changes that — so the remedy is to report the gap, never to
        // send them to a terminal.
        return Failure::unauthorized(
            "headless_signed_out",
            format!(
                "lane {token} is connected by device credential, but this command has no \
                 device-credential route yet"
            ),
        )
        .remedy(
            "report this command as a device-link coverage gap with `ds feedback submit`; \
             the connected device credential is intact",
        )
        .next(format!("ds auth status --lane {token}"));
    }
    Failure::unauthorized(
        "headless_signed_out",
        format!("no credential is connected for lane {token}"),
    )
    .remedy(signed_out_remedy(token))
    .next(signed_out_next(token))
}

fn with_disposition<T>(
    result: Result<T, ClientError>,
    context: &ProjectContextLease,
) -> Result<T, Failure> {
    match result {
        Ok(value) => Ok(value),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::PermanentlyRevoked | ErrorKind::IdentityMismatch
            ) =>
        {
            context.clear().map_err(|_| cleanup_required())?;
            Err(map_client(error))
        }
        Err(error) => Err(map_client(error)),
    }
}

/// The first `limit` rows of one directory, each carrying the directory's
/// elevation verdict.
///
/// A `platform_admin` sees every project through ds-brain's elevated path
/// with no membership document behind any row, so the server's `role` is
/// empty on all of them — which reads as "no role" when the truth is "seen by
/// elevation". The directory carries the verdict once; a row whose role the
/// server left empty presents it as `elevated`. A role the server DID name is
/// never rewritten, and without the verdict an empty role stays what the
/// server said.
fn directory_rows(directory: &ProjectDirectory, limit: usize) -> Vec<Value> {
    directory
        .projects()
        .iter()
        .take(limit)
        .map(|project| project_json(project, directory.elevated()))
        .collect()
}

fn project_json(project: &Project, elevated: bool) -> Value {
    json!({
        "ds_project": project.ds_project(),
        "project_name": project.project_name(),
        "display_name": project.display_name(),
        "role": presented_role(project.role(), elevated),
        "status": project_status(project.status()),
    })
}

/// The role a directory row presents; see [`directory_rows`].
fn presented_role(role: Option<&str>, elevated: bool) -> Value {
    match role {
        Some("") | None if elevated => json!("elevated"),
        other => json!(other),
    }
}

fn context_json(context: &state::ProjectContext) -> Value {
    json!({ "ds_project": context.project_id(), "project_name": context.project_name(), "status": context.status() })
}

fn project_status(status: ProjectStatus) -> &'static str {
    match status {
        ProjectStatus::Active => "active",
        ProjectStatus::Archived => "archived",
        ProjectStatus::Testing => "testing",
    }
}

fn parse_limit(value: &str) -> Result<usize, Failure> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| (1..=1000).contains(value))
        .ok_or_else(|| {
            Failure::invalid(
                "project_limit_invalid",
                "--limit must be an integer from 1 through 1000",
            )
            .remedy("pass --limit 100, or another value from 1 through 1000")
        })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// The governed report route's own refusal, when it named one.
///
/// The route is the only place these causes exist, and the status code cannot
/// separate them: `409` carries two unrelated causes and `424` is the one a
/// whole combined run ends on. Before this they all reached the caller as
/// `auth_input_invalid` with no detail and no remedy, so a blocked reporting
/// session was unreadable from the headless lane while the desktop lane showed
/// the cause plainly.
fn map_project_report_service_code(code: ProjectReportServiceCode) -> Failure {
    match code {
        ProjectReportServiceCode::NoIndividualArtifacts => Failure::conflict(
            "report_no_individual_artifacts",
            "No selected transformer had an individual report to package, so no archive was published.",
        )
        .remedy("generate the individual reports first, then retry this command")
        .next("ds report project scope"),
        ProjectReportServiceCode::GroupingStale => Failure::conflict(
            "report_grouping_stale",
            "The applied report grouping changed after this request pinned its digest.",
        )
        .remedy("re-read the applied grouping and retry with the digest it reports now")
        .next("ds design consumer-grouping read"),
        ProjectReportServiceCode::GroupingIncomplete => Failure::conflict(
            "report_grouping_incomplete",
            "The applied report grouping does not cover every requested transformer.",
        )
        .remedy("apply a grouping that covers the requested transformers, or narrow the scope")
        .next("ds design consumer-grouping apply"),
    }
}

fn map_client(error: ClientError) -> Failure {
    if let Some(code) = error.project_report_service_code() {
        return map_project_report_service_code(code);
    }
    let message = error.to_string();
    // These are closed, static Core diagnostics, not backend response text.
    // Preserve the publication step instead of naming authentication as its cause.
    if error.kind() == ErrorKind::Transient && message.starts_with("Solar publication ") {
        return Failure::unavailable("auth_transient", message)
            .remedy("retry the exact sealed publication without changing local state");
    }
    // A governed route that authored its own bounded refusal has said
    // something the coarse class cannot: which status it used and what is
    // wrong. Collapsing that into `auth_response_unreadable` is what made a
    // print layout the deployed validator could not decode read as an
    // authentication failure on 2026-09-10.
    let refusal = error.service_refusal();
    // A route the lane's gateway never published is its own fact, and the
    // known-columns route is not a Survey one: name it for what it is.
    if error.kind() == ErrorKind::RouteUnavailable && message.contains("known-columns") {
        return Failure::failed(DESIGN_ROUTE_UNAVAILABLE_REFUSAL.code, message)
            .remedy(DESIGN_ROUTE_UNAVAILABLE_REFUSAL.remedy);
    }
    let failure = match refusal {
        Some(refusal) => map_service_refusal(error.kind(), refusal, &message),
        None => map_client_kind(error.kind(), message.clone()),
    };
    // ClientError messages are static, owner-authored text, never response
    // bodies. Preserve these bounded route diagnostics for data/config calls.
    if message.starts_with("the transformer context route")
        || message.starts_with("project configuration")
        || message.starts_with("feeder settings were saved")
    {
        let diagnostic = route_diagnostic(&message);
        let mut detail = serde_json::json!({"owner_message":message});
        // The status and the service's own sentence are the difference
        // between "retry" and "this deployment cannot answer": carry both
        // where the route authored them.
        if let Some(refusal) = refusal {
            detail["http_status"] = serde_json::json!(refusal.status());
            if let Some(code) = refusal.code() {
                detail["service_code"] = serde_json::json!(code);
            }
            if let Some(sentence) = refusal.message() {
                detail["service_message"] = serde_json::json!(sentence);
            }
        }
        let mut failure = failure.detail(detail);
        // The shared kind mapping has nothing to say about a route that
        // rejects a credential this lane just verified, and a named refusal
        // with no way out is a dead end. Only fill the silence: a code that
        // brought its own remedy — `transformer_not_found` on the same route,
        // for one — keeps it.
        if failure.remedy_text().is_none()
            && let Some((remedy, next)) = diagnostic
        {
            failure = failure.remedy(remedy).next(next);
        }
        failure
    } else {
        failure
    }
}

/// One governed route's own refusal, rendered as a CLI failure.
///
/// The code a caller plans against is this crate's, not the server's string:
/// only the two codes the printing contract declares are adopted by name, and
/// every other refusal keeps the shared kind mapping. What always crosses is
/// the exact status and the service's bounded sentence, because a receipt that
/// says only "refused" is a receipt nobody can act on.
fn map_service_refusal(
    kind: ErrorKind,
    refusal: &ds_client_core::ServiceRefusal,
    owner_message: &str,
) -> Failure {
    let message = match refusal.message() {
        Some(sentence) => format!("{owner_message} (HTTP {}): {sentence}", refusal.status()),
        None => format!("{owner_message} (HTTP {})", refusal.status()),
    };
    // Project management authored this refusal (`ds-client-core::
    // project_management::refusal` names the route in every sentence). Its
    // three conditions each have a different next step — an admin, a re-read,
    // a corrected flag — which is why `ds pm` documents them by name rather
    // than as one `auth_rejected`.
    if owner_message.starts_with("project management") {
        return map_project_management_refusal(kind, refusal, message);
    }
    if owner_message.starts_with("project assets") {
        return map_project_assets_refusal(kind, refusal, message);
    }
    if owner_message.starts_with("design annotations")
        || owner_message.starts_with("known columns")
        || owner_message.starts_with("the material propagation route")
    {
        return map_design_collaboration_refusal(kind, refusal, message);
    }
    match refusal.code() {
        Some("version_not_found") => Failure::invalid("version_not_found", message)
            .detail(serde_json::json!({
                "http_status": refusal.status(),
                "service_code": "version_not_found",
                "service_message": refusal.message(),
            }))
            .remedy("list published versions and choose one exact version_id that exists")
            .next("ds design version list --transformer <name> --output json"),
        Some("print_layout_invalid") => Failure::invalid("print_layout_invalid", message)
            .remedy(
                "correct the layout against ds report layout schema; if the refused field is \
                 valid, the deployed print validator is older than this client and has to be \
                 redeployed",
            )
            .next("ds report layout schema --output json"),
        Some("print_setup_not_found") => Failure::invalid("print_setup_not_found", message)
            .remedy("name a setup this scope holds, or copy one into it")
            .next("ds report layout list --scope project --output json"),
        Some("print_validator_unavailable") => {
            Failure::unavailable("print_validator_unavailable", message).remedy(
                "retry without changing the layout; a repeated refusal means the deployed print \
                 validator is unavailable, not that the layout is wrong",
            )
        }
        // The foundation reads (docs/contracts/foundation-datasets.md) refuse
        // by name at ds-brain; each name is a distinct next step for the
        // operator, so it crosses as its own code with the route's sentence.
        Some("upi_not_found") => Failure::invalid("upi_not_found", message).detail(json!({
            "http_status": refusal.status(),
            "service_code": "upi_not_found",
            "service_message": refusal.message(),
        })),
        Some("upi_invalid") => Failure::invalid("upi_invalid", message).detail(json!({
            "http_status": refusal.status(),
            "service_code": "upi_invalid",
            "service_message": refusal.message(),
        })),
        Some("invalid_admin_scope") => {
            Failure::invalid("invalid_admin_scope", message).detail(json!({
                "http_status": refusal.status(),
                "service_code": "invalid_admin_scope",
                "service_message": refusal.message(),
            }))
        }
        Some("bound_exceeded") => Failure::invalid("bound_exceeded", message).detail(json!({
            "http_status": refusal.status(),
            "service_code": "bound_exceeded",
            "service_message": refusal.message(),
        })),
        Some("dataset_ambiguous") => {
            Failure::conflict("dataset_ambiguous", message).detail(json!({
                "http_status": refusal.status(),
                "service_code": "dataset_ambiguous",
                "service_message": refusal.message(),
            }))
        }
        Some("dataset_cloud_only") => {
            Failure::conflict("dataset_cloud_only", message).detail(json!({
                "http_status": refusal.status(),
                "service_code": "dataset_cloud_only",
                "service_message": refusal.message(),
            }))
        }
        // Every other refusal keeps the shared kind mapping's class, code,
        // remedy and next step — several of those arms answer with their own
        // static sentence, so the status and the service's words are carried
        // back over it rather than lost.
        _ => {
            let failure = map_client_kind(kind, message.clone()).with_message(message);
            // A route that named its refusal hands the token on in `detail`,
            // so a domain (`ds pm`, `ds solar seed`) can give it a code and a
            // remedy of its own without this crate learning every vocabulary.
            match refusal.code() {
                Some(code) => {
                    let mut detail = json!({
                        "http_status": refusal.status(),
                        "service_code": code,
                    });
                    if let Some(sentence) = refusal.message() {
                        detail["service_message"] = json!(sentence);
                    }
                    failure.detail(detail)
                }
                None => failure,
            }
        }
    }
}

/// The assets route's own refusal, rendered as the failure `ds assets`
/// documents — the same seven codes the paired adapter minted from ds-brain's
/// answer (`assetsRefusal`, ds-web `cli-assets.ts`), so a caller who planned
/// for them on the desktop has planned for them headless: a 404 is
/// `asset_not_found` (also a confidential row the caller may not know exists,
/// by design), 401/403 `asset_class_forbidden`, 409 `asset_version_conflict`,
/// a 400 the catalogue refused by rule `asset_refused`, any other 400
/// `asset_request_invalid`, 501 `assets_not_implemented` and 5xx
/// `assets_service_failed`. The server's sentence, code and the rule it
/// names travel in `detail` and in the message.
pub fn map_project_assets_refusal(
    kind: ErrorKind,
    refusal: &ds_client_core::ServiceRefusal,
    message: String,
) -> Failure {
    let detail = json!({
        "http_status": refusal.status(),
        "service_code": refusal.code(),
        "service_message": refusal.message(),
    });
    let sentence = refusal
        .message()
        .map(str::to_owned)
        .unwrap_or_else(|| message.clone());
    match (refusal.status(), refusal.code()) {
        (404, _) => Failure::invalid(ASSET_NOT_FOUND_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(ASSET_NOT_FOUND_REFUSAL.remedy)
            .next("ds assets list --output json"),
        (401 | 403, _) => Failure::unauthorized(ASSET_CLASS_FORBIDDEN_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(ASSET_CLASS_FORBIDDEN_REFUSAL.remedy),
        (409, _) => Failure::conflict(ASSET_VERSION_CONFLICT_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(ASSET_VERSION_CONFLICT_REFUSAL.remedy),
        (400 | 422, Some("asset_refused")) => {
            Failure::invalid(ASSET_REFUSED_REFUSAL.code, sentence)
                .detail(detail)
                .remedy(ASSET_REFUSED_REFUSAL.remedy)
        }
        (400 | 422, _) => Failure::invalid(ASSET_REQUEST_INVALID_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(ASSET_REQUEST_INVALID_REFUSAL.remedy),
        (501, _) => Failure::unavailable(ASSETS_NOT_IMPLEMENTED_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(ASSETS_NOT_IMPLEMENTED_REFUSAL.remedy),
        (500..=599, _) => Failure::unavailable(ASSETS_SERVICE_FAILED_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(ASSETS_SERVICE_FAILED_REFUSAL.remedy),
        _ => map_client_kind(kind, message.clone())
            .with_message(message)
            .detail(detail),
    }
}

/// A design collaboration route's own refusal (annotations, known columns,
/// material propagation), rendered as the failure `ds design` documents:
/// a 404 is `design_record_not_found` (no such thread, definition, group or
/// transformer), 401/403 `design_not_permitted` (the capability the message
/// names), 409/412 `design_version_conflict` (the record moved; re-read),
/// any other 4xx `design_request_invalid` (the route's own sentence names the
/// bound or the field), 5xx `design_service_failed`.
pub fn map_design_collaboration_refusal(
    kind: ErrorKind,
    refusal: &ds_client_core::ServiceRefusal,
    message: String,
) -> Failure {
    let detail = json!({
        "http_status": refusal.status(),
        "service_code": refusal.code(),
        "service_message": refusal.message(),
    });
    let sentence = refusal
        .message()
        .map(str::to_owned)
        .unwrap_or_else(|| message.clone());
    match refusal.status() {
        404 => Failure::invalid(DESIGN_RECORD_NOT_FOUND_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(DESIGN_RECORD_NOT_FOUND_REFUSAL.remedy),
        401 | 403 => Failure::unauthorized(DESIGN_NOT_PERMITTED_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(DESIGN_NOT_PERMITTED_REFUSAL.remedy),
        409 | 412 => Failure::conflict(DESIGN_VERSION_CONFLICT_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(DESIGN_VERSION_CONFLICT_REFUSAL.remedy),
        400..=499 => Failure::invalid(DESIGN_REQUEST_INVALID_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(DESIGN_REQUEST_INVALID_REFUSAL.remedy),
        500..=599 => Failure::unavailable(DESIGN_SERVICE_FAILED_REFUSAL.code, sentence)
            .detail(detail)
            .remedy(DESIGN_SERVICE_FAILED_REFUSAL.remedy),
        _ => map_client_kind(kind, message.clone())
            .with_message(message)
            .detail(detail),
    }
}

pub const DESIGN_RECORD_NOT_FOUND_REFUSAL: Refusal = Refusal {
    code: "design_record_not_found",
    when: "no thread, definition, group, grouping or transformer carries this id in the selected project",
    remedy: "check the id with the matching `list` command",
};
pub const DESIGN_NOT_PERMITTED_REFUSAL: Refusal = Refusal {
    code: "design_not_permitted",
    when: "the signed-in user lacks the capability this write needs; the message names it",
    remedy: "ask a project admin for the capability the message names",
};
pub const DESIGN_VERSION_CONFLICT_REFUSAL: Refusal = Refusal {
    code: "design_version_conflict",
    when: "the record or plan moved while the write was in flight (a stale version or plan digest)",
    remedy: "re-read or re-preview and issue the command again",
};
pub const DESIGN_REQUEST_INVALID_REFUSAL: Refusal = Refusal {
    code: "design_request_invalid",
    when: "ds-brain refused the request's own shape, or a bound it exceeded",
    remedy: "the message names the bound or the field; change the request rather than repeating it",
};
/// The lane's API Gateway does not publish `/config/{project}/known-columns`
/// (2026-09-20: neither lane does — `ds-apis-tf/api_ds_system.tf` publishes
/// `/config/{eds_project_id}` GET only, so the browser's known-columns
/// feature only works against a local dev stack too). Not an authority
/// refusal and not a missing record.
pub const DESIGN_ROUTE_UNAVAILABLE_REFUSAL: Refusal = Refusal {
    code: "design_route_unavailable",
    when: "this lane's API Gateway does not publish the known-columns route",
    remedy: "publish GET and PATCH /config/{eds_project_id}/known-columns on the gateway (ds-apis-tf api_ds_system.tf); until then the route answers only on a local dev stack",
};
pub const DESIGN_SERVICE_FAILED_REFUSAL: Refusal = Refusal {
    code: "design_service_failed",
    when: "ds-brain faulted while serving the request",
    remedy: "retry once; nothing in the request changes the outcome while the service faults",
};

/// The catalogue's refusals as `ds assets` documents them — one table, so
/// the mapping above and every command's `REFUSALS` section read the same
/// words.
pub const ASSET_NOT_FOUND_REFUSAL: Refusal = Refusal {
    code: "asset_not_found",
    when: "no asset or folder has this id or path, or it is confidential and the caller may not know it exists",
    remedy: "read the available ids and paths with `ds assets list` or `ds assets tree`",
};
pub const ASSET_CLASS_FORBIDDEN_REFUSAL: Refusal = Refusal {
    code: "asset_class_forbidden",
    when: "the signed-in user lacks the assets capability this sensitivity class or this write requires",
    remedy: "ask a project admin for the assets capability the message names",
};
pub const ASSET_VERSION_CONFLICT_REFUSAL: Refusal = Refusal {
    code: "asset_version_conflict",
    when: "the catalogue row or folder moved while the write was in flight",
    remedy: "re-read it with `ds assets list` or `ds assets tree` and issue the command again",
};
pub const ASSET_REQUEST_INVALID_REFUSAL: Refusal = Refusal {
    code: "asset_request_invalid",
    when: "the catalogue declined the request as malformed, or over a bound it names with the number",
    remedy: "change the request as the message says rather than repeating it",
};
pub const ASSET_REFUSED_REFUSAL: Refusal = Refusal {
    code: "asset_refused",
    when: "the catalogue refused the action by one of its rules: a loosening the caller may not make, a reserved root, a write onto a system row; the message names the rule in brackets",
    remedy: "the message names the rule; act on what it names rather than retrying",
};
pub const ASSETS_NOT_IMPLEMENTED_REFUSAL: Refusal = Refusal {
    code: "assets_not_implemented",
    when: "this lane's ds-brain does not serve this action yet",
    remedy: "the action lands with the next ds-brain deployment on this lane; retry then",
};
pub const ASSETS_SERVICE_FAILED_REFUSAL: Refusal = Refusal {
    code: "assets_service_failed",
    when: "the catalogue service faulted while serving the request",
    remedy: "retry once; nothing in the request changes the outcome while the service faults",
};

/// The `pm` route's own refusal, rendered as the failure `ds pm` documents.
///
/// The codes are `ds pm`'s, held to ds-brain's envelope: `PM_REVISION_CONFLICT`
/// is the plan moving under a command (`work_revision_conflict`, re-read and
/// decide again); a 403 is the permission gate (`work_not_permitted`, ask a
/// project admin); a 404 is a project the caller is not a member of
/// (`project_not_visible`); everything else the route refused by name
/// (`VALIDATION_FAILED`, `PM_REFUSED`, `CONFLICT`) is `pm_refused`, carrying
/// the server's sentence and code in `detail` so the operator reads exactly
/// what was declined.
pub fn map_project_management_refusal(
    kind: ErrorKind,
    refusal: &ds_client_core::ServiceRefusal,
    message: String,
) -> Failure {
    let detail = json!({
        "http_status": refusal.status(),
        "service_code": refusal.code(),
        "service_message": refusal.message(),
    });
    match (refusal.status(), refusal.code()) {
        (409, Some("pm_revision_conflict")) => Failure::conflict(
            "work_revision_conflict",
            "the plan moved while the command was in flight",
        )
        .detail(detail)
        .remedy("re-read with `ds pm task read` and issue the command again")
        .next("ds pm task read --task <task-id>"),
        (401 | 403, _) => Failure::unauthorized(
            "work_not_permitted",
            refusal.message().map(str::to_owned).unwrap_or_else(|| {
                "the signed-in user may not perform this Project Management command".into()
            }),
        )
        .detail(detail)
        .remedy("ask a project admin for schedule-editor access")
        .next("ds pm plan"),
        (404, _) => Failure::unauthorized(
            "project_not_visible",
            "the selected project is not a project this account is a member of",
        )
        .detail(detail)
        .remedy("choose an exact id from auth project list")
        .next("ds auth project list"),
        (400 | 409 | 422, _) => Failure::invalid("pm_refused", message)
            .detail(detail)
            .remedy("read detail.service_message; correct the flag it names and retry")
            .next("ds pm task read --task <task-id>"),
        _ => map_client_kind(kind, message.clone())
            .with_message(message)
            .detail(detail),
    }
}

/// What a caller can do about one preserved route diagnostic, as
/// `(remedy, next)`.
///
/// Only the transformer-context route has an answer today, and it is not the
/// obvious one: the credential was verified for this very call, so
/// re-authenticating changes nothing. Taking the message as the input keeps
/// this testable — `ClientError` is `pub(crate)` to construct.
fn route_diagnostic(message: &str) -> Option<(&'static str, &'static str)> {
    message
        .starts_with("the transformer context route")
        .then_some((TRANSFORMER_CONTEXT_ROUTE_REMEDY, "ds auth project status"))
}

fn map_client_kind(kind: ErrorKind, message: String) -> Failure {
    match kind {
        ErrorKind::InvalidInput => Failure::invalid("auth_input_invalid", message),
        // No lane reaches this mapping (a `ClientError` carries none), so the
        // next step says that one has to be named rather than naming the
        // default by omission.
        ErrorKind::SignedOut => Failure::unauthorized("headless_signed_out", message)
            .remedy(SIGNED_OUT_REMEDY)
            .next("ds account connect --lane <stable|canary>"),
        ErrorKind::InvalidCredentials => Failure::unauthorized(
            "auth_invalid_credentials",
            "Firebase did not accept this terminal sign-in",
        )
        .remedy(SIGNED_OUT_REMEDY)
        .next(SIGNED_OUT_NEXT),
        ErrorKind::PasswordSignInUnavailable => Failure::unauthorized(
            "auth_password_sign_in_unavailable",
            "terminal sign-in is unavailable for this Firebase account or project",
        )
        .remedy(SIGNED_OUT_REMEDY)
        .next(SIGNED_OUT_NEXT),
        ErrorKind::AccountDisabled => Failure::unauthorized(
            "auth_account_disabled",
            "Firebase reports that this account is disabled",
        )
        .remedy("restore the account with an administrator, then retry"),
        ErrorKind::AuthenticationRejected => Failure::unauthorized(
            "auth_rejected",
            "the native authentication or project service rejected this verified request",
        ),
        ErrorKind::PermanentlyRevoked => Failure::unauthorized(
            "auth_revoked",
            "Firebase permanently revoked the native session",
        )
        .remedy(SIGNED_OUT_REMEDY)
        .next(SIGNED_OUT_NEXT),
        ErrorKind::IdentityMismatch => Failure::unauthorized(
            "auth_identity_mismatch",
            "Firebase returned an identity outside the bound native session",
        ),
        ErrorKind::Transient => Failure::unavailable(
            "auth_transient",
            "the native authentication service is temporarily unavailable",
        )
        .remedy("retry without changing local state"),
        ErrorKind::UnreadableResponse => Failure::unavailable(
            "auth_response_unreadable",
            "the authentication or project response did not match its closed contract",
        ),
        ErrorKind::DurableState => Failure::unavailable(
            "native_state_unsafe",
            "the protected native auth state is unsafe, stale, or unreadable",
        ),
        ErrorKind::ResourceNotFound => Failure::invalid(
            "transformer_not_found",
            "the selected transformer does not exist in the selected project",
        )
        .remedy("pass one exact transformer name from the selected project"),
        ErrorKind::RouteUnavailable => route_unavailable(),
    }
}

/// The deployment answered 404 without ever reaching DS.
///
/// This is not an authority refusal and not a missing object: the lane's API
/// Gateway was never taught the path, so no account and no project selection
/// could have made the call succeed. Naming it as its own code is what stops an
/// operator reading "you may not see this" off a route that is simply absent.
/// `ds survey query`, `ds survey entries select` and `ds survey entries
/// changes` declare this same constant, so the contract a caller reads
/// beforehand and the receipt they get cannot drift apart.
pub const SURVEY_ROUTE_UNAVAILABLE_REFUSAL: Refusal = Refusal {
    code: "survey_route_unavailable",
    when: "this lane's API Gateway does not publish this Survey read route",
    remedy: "update ds; if it persists, the route is unpublished on this lane",
};

fn route_unavailable() -> Failure {
    Failure::failed(
        SURVEY_ROUTE_UNAVAILABLE_REFUSAL.code,
        "this deployment does not serve that governed Survey read route",
    )
    .remedy(SURVEY_ROUTE_UNAVAILABLE_REFUSAL.remedy)
}

fn cleanup_required() -> Failure {
    Failure::failed(
        "native_cleanup_required",
        "the credential changed but its previous project context still requires cleanup",
    )
    .remedy("repair the owner-only DS config directory, then run ds auth logout again")
}

fn read_password(from_stdin: bool) -> Result<String, Failure> {
    // The fact this domain needs is "nobody is at a terminal", which is not a
    // fact about any protocol. `ds mcp serve` sets this variable when it
    // re-invokes the executable, and so would any other machine host; naming
    // it for one of them would make every later host either impersonate that
    // one or edit this file.
    refuse_noninteractive_prompt(
        from_stdin,
        std::env::var_os("DS_CLI_NONINTERACTIVE").is_some_and(|value| value == "1"),
    )?;
    if from_stdin {
        let mut input = String::new();
        let read = io::stdin().lock().take(4098).read_line(&mut input);
        if read.is_err() {
            input.zeroize();
            return Err(password_failure());
        }
        if input.ends_with('\n') {
            input.pop();
            if input.ends_with('\r') {
                input.pop();
            }
        }
        if input.is_empty() || input.len() > 4096 || input.contains(['\r', '\n']) {
            input.zeroize();
            return Err(password_failure());
        }
        return Ok(input);
    }
    hidden_tty_password()
}

fn refuse_noninteractive_prompt(from_stdin: bool, noninteractive: bool) -> Result<(), Failure> {
    if !from_stdin && noninteractive {
        Err(Failure::unavailable(
            "password_prompt_forbidden",
            "a non-interactive child process cannot open an interactive password prompt",
        )
        .remedy(SIGNED_OUT_REMEDY))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn hidden_tty_password() -> Result<String, Failure> {
    use std::os::fd::AsRawFd;
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| password_tty_failure())?;
    let fd = tty.as_raw_fd();
    let mut previous = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, previous.as_mut_ptr()) } != 0 {
        return Err(password_tty_failure());
    }
    let previous = unsafe { previous.assume_init() };
    let mut hidden = previous;
    hidden.c_lflag &= !libc::ECHO;
    tty.write_all(b"Password: ")
        .map_err(|_| password_tty_failure())?;
    tty.flush().map_err(|_| password_tty_failure())?;
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &hidden) } != 0 {
        return Err(password_tty_failure());
    }
    let mut input = String::new();
    let read = io::BufReader::new(&tty).take(4098).read_line(&mut input);
    let restored = unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &previous) } == 0;
    let _ = tty.write_all(b"\n");
    if read.is_err() || !restored {
        input.zeroize();
        return Err(password_tty_failure());
    }
    if input.ends_with('\n') {
        input.pop();
        if input.ends_with('\r') {
            input.pop();
        }
    }
    if input.is_empty() || input.len() > 4096 || input.contains(['\r', '\n']) {
        input.zeroize();
        return Err(password_failure());
    }
    Ok(input)
}

#[cfg(windows)]
fn hidden_tty_password() -> Result<String, Failure> {
    Err(Failure::unavailable(
        "password_tty_unavailable",
        "hidden TTY password input is unavailable in this Windows build",
    )
    .remedy("pipe one bounded line and pass --password-stdin"))
}

fn password_failure() -> Failure {
    Failure::invalid(
        "password_input_invalid",
        "password input is empty, multiline, or exceeds 4096 bytes",
    )
}

#[cfg(unix)]
fn password_tty_failure() -> Failure {
    Failure::unavailable(
        "password_tty_unavailable",
        "a controlling TTY is required for hidden password input",
    )
    .remedy("run from an interactive terminal or explicitly pipe one line with --password-stdin")
}

pub fn render_status(data: &Value) -> String {
    if data["signed_in"].as_bool() == Some(true) {
        format!(
            "signed in ({})  {}\n",
            data["lane"].as_str().unwrap_or(""),
            data["email"].as_str().unwrap_or("")
        )
    } else {
        format!("signed out ({})\n", data["lane"].as_str().unwrap_or(""))
    }
}
pub fn render_login(data: &Value) -> String {
    render_status(data)
}
pub fn render_logout(data: &Value) -> String {
    render_status(data)
}
pub fn render_project_list(data: &Value) -> String {
    let mut out = String::new();
    if let Some(projects) = data["projects"].as_array() {
        for project in projects {
            out.push_str(&format!(
                "{}  {}  {}  {}\n",
                project["ds_project"].as_str().unwrap_or(""),
                project["status"].as_str().unwrap_or(""),
                project["role"].as_str().unwrap_or("-"),
                project["project_name"].as_str().unwrap_or("")
            ));
        }
    }
    let returned = data["returned"].as_u64().unwrap_or(0);
    let total = data["total"].as_u64().unwrap_or(returned);
    if total == 0 {
        out.push_str(&format!(
            "no visible projects ({})\n",
            data["lane"].as_str().unwrap_or("")
        ));
    } else if data["more"].as_bool() == Some(true) {
        out.push_str(&format!(
            "showing {returned} of {total}; increase --limit to view more\n"
        ));
    }
    out
}
pub fn render_project(data: &Value) -> String {
    if data["selected"].as_bool() == Some(true) {
        format!(
            "selected  {}  {}\n",
            data["project"]["ds_project"].as_str().unwrap_or(""),
            data["project"]["project_name"].as_str().unwrap_or("")
        )
    } else {
        format!(
            "no project selected ({})\n",
            data["lane"].as_str().unwrap_or("")
        )
    }
}

pub use ds_client_core::SurveyControlCommand;
/// Run one Survey control command. A command that acts on a project names it
/// explicitly; the saved native selection is never read.
pub fn survey_control(
    lane_value: &str,
    project: Option<&str>,
    command: &SurveyControlCommand,
) -> Result<Value, Failure> {
    command.validate().map_err(map_client)?;
    let lane = Lane::parse(lane_value)?;
    let _ = probe_headless_identity(lane.token())?;
    let project = match (command.project_hint().is_some(), project) {
        (true, Some(project)) => Some(bounded_named_project(project)?),
        (true, None) => {
            return Err(Failure::invalid(
                "project_required",
                "this Survey control acts on a project; pass --project <exact-id>",
            )
            .remedy("pass one exact ds_project value from ds auth project list"));
        }
        (false, _) => None,
    };
    if let Some(mut device) = device::restore_session(lane)? {
        return device
            .survey_control(project.as_deref(), command)
            .map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .survey_control(project.as_deref(), command, now())
        .map_err(map_client)
}

pub fn solar_project_session(lane_value: &str) -> Result<HeadlessSolarProjectSession, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some((device, selected)) = restored_device_project(lane)? {
        let identity = device.context();
        let principal_sha256 = solar_principal_binding_sha256(identity.uid(), identity.email());
        let credential_audience_sha256 = device.profile().credential_audience_sha256().to_owned();
        return Ok(HeadlessSolarProjectSession {
            lane: lane.token(),
            project_id: selected.project_id().to_owned(),
            project_name: selected.project_name().to_owned(),
            project_status: selected.status().to_owned(),
            principal_sha256,
            credential_audience_sha256,
            selected,
            provider: SolarProjectProvider::Device(Box::new(device)),
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    let selected = ProjectContextLease::acquire(client.profile())?
        .load_snapshot(client.profile(), user.uid(), user.email())?
        .ok_or_else(|| {
            Failure::conflict(
                "headless_project_not_selected",
                "no project is selected for this native user, lane, and credential audience",
            )
            .remedy("run ds auth project use --project <exact-id>")
            .next("ds auth project status")
        })?;
    let principal_sha256 = solar_principal_binding_sha256(user.uid(), user.email());
    Ok(HeadlessSolarProjectSession {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        principal_sha256,
        credential_audience_sha256: client.profile().credential_audience_sha256().to_owned(),
        selected,
        provider: SolarProjectProvider::Firebase(Box::new(client)),
    })
}

fn solar_principal_binding_sha256(uid: &str, email: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ds.solar.project.principal/v1\0");
    hash.update(uid.as_bytes());
    hash.update(b"\0");
    hash.update(email.as_bytes());
    format!("{:x}", hash.finalize())
}
pub use ds_client_core::solar_project::{
    Command as SolarProjectCommand, Output as SolarProjectOutput,
};
pub struct HeadlessSolarProjectSession {
    lane: &'static str,
    project_id: String,
    project_name: String,
    project_status: String,
    principal_sha256: String,
    credential_audience_sha256: String,
    selected: state::ProjectContext,
    provider: SolarProjectProvider,
}
enum SolarProjectProvider {
    Firebase(Box<NativeClient>),
    Device(Box<device::DeviceSession>),
}
impl HeadlessSolarProjectSession {
    pub fn binding(&self) -> Value {
        json!({"project":self.project_id,"lane":self.lane,"principal":self.principal_sha256,"audience":self.credential_audience_sha256,"project_name":self.project_name,"project_status":self.project_status})
    }
    pub fn execute(&mut self, command: &SolarProjectCommand) -> Result<Value, Failure> {
        command.validate(&self.project_id).map_err(map_client)?;
        let (profile, result) = match &mut self.provider {
            SolarProjectProvider::Firebase(client) => {
                let result = client.solar_project(&self.project_id, command, now());
                (client.profile(), result)
            }
            SolarProjectProvider::Device(device) => {
                let result = device.solar_project(&self.project_id, command);
                (device.profile(), result)
            }
        };
        with_released_context_disposition(profile, &self.selected, result)
    }
}

/// The seeding door's own refusals, published beside the hosts that emit them.
///
/// No command declares these yet: the headless `ds data project-cache
/// status|seed` commands of the context-seeding design land in a later slice
/// and will declare them by reference. Until then `refusal_coverage.rs` names
/// both codes as unreachable, and that entry is the reminder.
pub const DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL: Refusal = Refusal {
    code: "data_distribution_unavailable",
    when: "ds-brain's geographic data-distribution route could not answer: unreachable, past the gateway deadline, a server fault, or an answer outside its closed contract",
    remedy: "retry without changing local state; a repeated refusal is a deployment or provider outage, not a request defect",
};
pub const REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL: Refusal = Refusal {
    code: "reference_bundle_download_failed",
    when: "the published reference bundle could not be fetched from storage.googleapis.com, could not be written, or its bytes did not carry the catalogue row's size and SHA-256",
    remedy: "retry the download; if the digest keeps failing the catalogue row is stale, so read the catalogue again before installing",
};

/// One data-distribution action against the project the CALLER named: the
/// reference catalogue, or one bounded derived print-context acquisition.
/// The saved selection is never read. There is deliberately no URL, action or
/// credential override, and the billed `query_dataset` page read is not in the
/// vocabulary at all.
pub fn data_distribution(
    lane_value: &str,
    project: &str,
    request: &DataDistributionRequest,
) -> Result<Value, Failure> {
    request.validate().map_err(map_client)?;
    headless_named_project_with(
        lane_value,
        project,
        map_data_distribution,
        |device, project| device.data_distribution(project, request),
        |client, project| client.data_distribution(project, request, now()),
    )
    .map(HeadlessNamedProject::into_result)
}

/// The route itself could not answer — as opposed to the credential refresh
/// ahead of it, whose failures keep their auth codes. The core's messages for
/// this route all name it, which is what tells the two apart.
fn is_data_distribution_outage(error: &ClientError) -> bool {
    matches!(
        error.kind(),
        ErrorKind::Transient | ErrorKind::UnreadableResponse
    ) && error
        .to_string()
        .to_ascii_lowercase()
        .contains("data distribution")
}

fn map_data_distribution(error: ClientError) -> Failure {
    if !is_data_distribution_outage(&error) {
        return map_client(error);
    }
    let owner_message = error.to_string();
    let mut message = owner_message.clone();
    let mut detail = json!({ "owner_message": owner_message });
    if let Some(refusal) = error.service_refusal() {
        message = match refusal.message() {
            Some(sentence) => format!("{message} (HTTP {}): {sentence}", refusal.status()),
            None => format!("{message} (HTTP {})", refusal.status()),
        };
        detail["http_status"] = json!(refusal.status());
        if let Some(code) = refusal.code() {
            detail["service_code"] = json!(code);
        }
        if let Some(sentence) = refusal.message() {
            detail["service_message"] = json!(sentence);
        }
    }
    Failure::unavailable(DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL.code, message)
        .remedy(DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL.remedy)
        .detail(detail)
}

/// Fetch one published reference bundle to `dest`, admitted only when its
/// bytes carry the catalogue row's exact size and SHA-256.
///
/// The URL is pinned by the core to one unsigned object on
/// `storage.googleapis.com` and the transport attaches no DS credential; the
/// lane is required so the fetch runs only for a machine that holds a valid
/// packaged profile and a signed-in native identity — the same fence the
/// catalogue read that named the bundle went through. A refused or failed
/// fetch leaves nothing at `dest`.
pub fn download_reference_bundle(
    lane_value: &str,
    url: &str,
    expected_sha256: &str,
    expected_bytes: u64,
    dest: &Path,
) -> Result<(), Failure> {
    let call = ds_client_core::BundleDownloadCall::new(url, expected_sha256, expected_bytes)
        .map_err(map_client)?;
    let lane = Lane::parse(lane_value)?;
    let _ = profile::load(lane)?;
    probe_headless_identity(lane.token())?.ok_or_else(|| signed_out_failure(lane))?;
    let failed = |message: String| {
        Failure::unavailable(REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL.code, message)
            .remedy(REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL.remedy)
    };
    let mut file = open_bundle_destination(dest)
        .map_err(|_| failed("the reference bundle destination could not be created".into()))?;
    let outcome = ds_client_core::download_bundle(&mut NativeTransport, call, &mut file)
        .map_err(|error| failed(error.to_string()))
        .and_then(|receipt| {
            file.sync_all()
                .map(|_| receipt)
                .map_err(|_| failed("the reference bundle could not be committed to disk".into()))
        });
    drop(file);
    match outcome {
        Ok(_) => Ok(()),
        Err(failure) => {
            let _ = std::fs::remove_file(dest);
            Err(failure)
        }
    }
}

fn open_bundle_destination(dest: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(dest)
}

pub use ds_client_core::PrintingRequest;
/// Global templates need identity but no selected project. Project requests
/// can only address the principal's held, verified project context.
pub fn printing(
    lane_value: &str,
    global: bool,
    request: &PrintingRequest,
) -> Result<serde_json::Value, Failure> {
    request.validate().map_err(map_client)?;
    let lane = Lane::parse(lane_value)?;
    let needs_project = request.needs_project(global);
    if needs_project {
        if let Some((mut device, selected)) = restored_device_project(lane)? {
            return device
                .printing(selected.project_id(), request)
                .map_err(map_client);
        }
    } else {
        let _ = probe_headless_identity(lane.token())?;
        if let Some(mut device) = device::restore_session(lane)? {
            return device.printing("", request).map_err(map_client);
        }
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    if !needs_project {
        return client.printing("", request, now()).map_err(map_client);
    }
    let selected = load_selected_project(client.profile(), &user)?;
    let result = client.printing(selected.project_id(), request, now());
    with_released_context_disposition(client.profile(), &selected, result)
}

/// Resolve models only inside the restored user's selected project.
pub fn report_artifact(
    lane: &str,
    command: &ds_client_core::report_artifact::Command,
) -> Result<HeadlessProjectReport<serde_json::Value>, Failure> {
    headless_project_report(
        lane,
        |device, project| device.report_artifact(project, command),
        |client, project| client.report_artifact(project, command, now()),
    )
}
pub fn remove_report_artifact(
    lane: &str,
    project: &str,
    command: &ds_client_core::report_artifact::RemoveCommand,
) -> Result<HeadlessNamedProject<serde_json::Value>, Failure> {
    headless_named_project(
        lane,
        project,
        |device, project| device.remove_report_artifact(project, command),
        |client, project| client.remove_report_artifact(project, command, now()),
    )
}
/// One model operation against the project the CALLER named, keeping the
/// credential's identity for the host's own scoping (this machine's working
/// copies). The saved selection is never read.
pub fn grid_models(
    lane: &str,
    project: &str,
    command: &ds_client_core::grid_models::Command,
) -> Result<HeadlessNamedProject<ds_client_core::grid_models::Receipt>, Failure> {
    headless_named_project_with(
        lane,
        project,
        map_grid_publication,
        |device, project| {
            command.validate(project)?;
            device.grid_models(project, command)
        },
        |client, project| {
            command.validate(project)?;
            client.grid_models(project, command, now())
        },
    )
}
/// One explicit-project model operation. The gateway authorizes the project
/// on every request; no saved CLI selection is consulted or changed.
pub fn grid_models_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::grid_models::Command,
) -> Result<ds_client_core::grid_models::Receipt, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    command.validate(&project).map_err(map_grid_publication)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device
            .grid_models(&project, command)
            .map_err(map_grid_publication);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .grid_models(&project, command, now())
        .map_err(map_grid_publication)
}

fn map_grid_publication(error: ClientError) -> Failure {
    if error
        .service_refusal()
        .is_some_and(|r| r.status() == 409 && r.code() == Some("grid_publication_conflict"))
    {
        return Failure::conflict("publish_conflict", "the model publication conflicts with stored state; the request was not changed or retried")
            .remedy("read the exact model head and stored revision, review the conflict, then publish deliberately");
    }
    map_client(error)
}

pub use ds_client_core::grid_models::Command as GridModelsCommand;
pub use ds_client_core::report_artifact::{
    Command as ReportArtifactCommand, RemoveCommand as RemoveReportArtifactCommand,
};

pub use ds_client_core::{
    TransformerSaveBatch, TransformerSaveItem, TransformerSaveReceipt, TransformerSaved,
};
pub fn save_transformers(
    lane: &str,
    batch: &TransformerSaveBatch,
) -> Result<HeadlessProjectReport<TransformerSaveReceipt>, Failure> {
    headless_project_report(
        lane,
        |device, project| device.save_transformers(project, batch),
        |client, project| client.save_transformers(project, batch, now()),
    )
}

pub fn shared_assets(
    lane: &str,
    command: &ds_client_core::shared_assets::Command,
) -> Result<HeadlessProjectReport<Value>, Failure> {
    headless_project_report(
        lane,
        |device, project| device.shared_assets(project, command),
        |client, project| client.shared_assets(project, command, now()),
    )
}
/// Execute a shared-asset operation using only the caller's explicit project.
pub fn shared_assets_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::shared_assets::Command,
) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    command.validate(&project).map_err(map_client)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.shared_assets(&project, command).map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .shared_assets(&project, command, now())
        .map_err(map_client)
}

pub fn design_versions(
    lane: &str,
    command: &ds_client_core::design_versions::Command,
) -> Result<HeadlessProjectReport<Value>, Failure> {
    headless_project_report(
        lane,
        |device, project| device.design_versions(project, command),
        |client, project| client.design_versions(project, command, now()),
    )
}
/// Explicit request context: saved selection is neither read nor changed.
pub fn design_versions_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::design_versions::Command,
) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    command.validate(&project).map_err(|error| {
        Failure::invalid("invalid_input", error).remedy(
            "Choose one exact kind/object and assigned vN versions; MV restore is unavailable",
        )
    })?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device
            .design_versions(&project, command)
            .map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .design_versions(&project, command, now())
        .map_err(map_client)
}
pub fn design_attachments_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::design_attachments::Command,
) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    command.validate(&project).map_err(map_client)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device
            .design_attachments(&project, command)
            .map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .design_attachments(&project, command, now())
        .map_err(map_client)
}
pub fn design_tags(
    lane: &str,
    command: &ds_client_core::design_tags::Command,
) -> Result<HeadlessProjectReport<Value>, Failure> {
    headless_project_report(
        lane,
        |device, project| device.design_tags(project, command),
        |client, project| client.design_tags(project, command, now()),
    )
}
pub use ds_client_core::design_tags::Command as DesignTagsCommand;

/// Cross-project design migration — ONE endpoint, `kind` transformer|dsgrid.
/// Migration is stateless: a source project INTO an explicit destination. The
/// destination is an operand the caller names, never the saved selection, so
/// the same call means the same thing on every machine and in every session.
pub fn design_migration_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::design_migration::Command,
) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    command.validate(&project).map_err(map_client)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device
            .design_migration(&project, command)
            .map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .design_migration(&project, command, now())
        .map_err(map_client)
}
pub use ds_client_core::design_migration::Command as DesignMigrationCommand;

/// The pipeline route refused a survey-data migration by its own rule.
pub const SURVEY_MIGRATION_REFUSED_REFUSAL: Refusal = Refusal {
    code: "migration_refused",
    when: "the service refused: no access or no pipeline.migrate capability on either project, or an archived destination",
    remedy: "read detail.service_message; it names the refusal",
};
/// The service answered, but not with a receipt for the mode that was asked.
pub const SURVEY_MIGRATION_UNVERIFIED_REFUSAL: Refusal = Refusal {
    code: "unverified_receipt",
    when: "the receipt is not for the plan or apply that was asked for",
    remedy: "re-run the plan; a receipt for the other mode is never reported as a success",
};

/// Survey-data migration: every entry of the command's source INTO the
/// explicit destination `project`. Stateless, like design's: neither project
/// is the saved selection. The door's two own outcomes cross under the codes
/// `ds survey migrate` declares; everything else keeps the shared mapping.
pub fn survey_migration_for_project(
    lane_value: &str,
    project: &str,
    command: &ds_client_core::survey_migration::Command,
) -> Result<Value, Failure> {
    let convert = |error: ds_client_core::ClientError| {
        let message = error.to_string();
        match error.service_refusal() {
            // 401 is the credential, 5xx/429 a retry: the shared mapping says
            // those. Anything else is the route's own rule.
            Some(refusal)
                if message == ds_client_core::survey_migration::REFUSED
                    && error.kind() != ErrorKind::Transient
                    && refusal.status() != 401 =>
            {
                let sentence = refusal.message().unwrap_or("no reason given");
                Failure::failed(
                    SURVEY_MIGRATION_REFUSED_REFUSAL.code,
                    format!("{message} (HTTP {}): {sentence}", refusal.status()),
                )
                .detail(json!({
                    "http_status": refusal.status(),
                    "service_message": refusal.message(),
                }))
                .remedy(SURVEY_MIGRATION_REFUSED_REFUSAL.remedy)
            }
            None if message == ds_client_core::survey_migration::UNVERIFIED => {
                Failure::failed(SURVEY_MIGRATION_UNVERIFIED_REFUSAL.code, message)
                    .remedy(SURVEY_MIGRATION_UNVERIFIED_REFUSAL.remedy)
            }
            _ => map_client(error),
        }
    };
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    command.validate(&project).map_err(map_client)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.survey_migration(&project, command).map_err(convert);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .survey_migration(&project, command, now())
        .map_err(convert)
}
pub use ds_client_core::survey_migration::Command as SurveyMigrationCommand;

pub use ds_client_core::shared_assets::Command as SharedAssetsCommand;

/// Shared product feedback is user scoped, independent of project selection.
pub fn feedback(
    lane_value: &str,
    command: &ds_client_core::feedback::Command,
) -> Result<serde_json::Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    let convert = |error: ds_client_core::ClientError| match error.to_string().as_str() {
        "feedback_not_permitted" => {
            Failure::unauthorized("feedback_not_permitted", "This user cannot triage feedback")
                .remedy("Use a user with platform triage capability")
        }
        "feedback_not_found" => {
            Failure::invalid("feedback_not_found", "No feedback report carries this id")
                .remedy("List the backlog again")
        }
        "feedback_conflict" => {
            Failure::conflict("feedback_conflict", "Feedback changed since it was read")
                .remedy("Read the current version before closing it")
        }
        // A settled report and a version conflict are both 409 on the wire and
        // want opposite things: one wants a NEW report, the other a re-read.
        "feedback_settled" => Failure::conflict(
            "feedback_settled",
            "That report is settled; a settled report is never revived",
        )
        .remedy("Submit a new report that names this id, rather than reopening it"),
        "feedback_note_limit" => Failure::invalid(
            "feedback_note_limit",
            "That report already carries the maximum number of notes",
        )
        .remedy("Close it with its resolution, or file a new report that references it"),
        "feedback_cursor_rejected" => Failure::invalid(
            "feedback_cursor_rejected",
            "That is not a cursor the backlog issued for this query",
        )
        .remedy("Drop --cursor to read from where this account left off, or pass --all"),
        "feedback_backlog_too_large" => Failure::invalid(
            "feedback_backlog_too_large",
            "The backlog is too large to enumerate completely in one answer",
        )
        .remedy("Close reports; the backlog refuses rather than answering partially"),
        _ => map_client(error),
    };
    command.validate().map_err(convert)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.feedback(command).map_err(convert);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client.feedback(command, now()).map_err(convert)
}
pub use ds_client_core::feedback::Command as FeedbackCommand;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_layer_identity_requires_exact_uid_audience_and_project() {
        let fence = LayerScopeFence {
            uid: "u1".into(),
            audience: "a1".into(),
            project: "p1".into(),
            credential: "c1".into(),
        };
        assert!(verify_restored_layer_identity(&fence, "u1", "a1", "p1").is_ok());
        for (uid, audience, project) in [("u2", "a1", "p1"), ("u1", "a2", "p1"), ("u1", "a1", "p2")]
        {
            assert_eq!(
                verify_restored_layer_identity(&fence, uid, audience, project)
                    .unwrap_err()
                    .code(),
                "project_context_changed"
            );
        }
    }

    /// The desktop door and the Server door read ONE grammar, because there is
    /// one: whatever `execution_context::valid_project` says, said here with
    /// the same code and a sentence that states the rule rather than a bound.
    /// A test that only listed the ids this refuses would go stale the day the
    /// kernel's grammar moves, so it asks the kernel about every case.
    #[test]
    fn a_named_project_is_bounded_by_the_kernels_grammar_and_nothing_else() {
        for candidate in [
            "proj-kigali",
            "a/b",
            "a\\b",
            "..",
            ".",
            "a/../b",
            "C:",
            "a:b",
            "NUL",
            "con.json",
            "A B",
            " padded",
            "padded ",
            "trailing.",
            "wild*card",
            "",
            &"p".repeat(ds_command_kernel::execution_context::MAX_PROJECT_CHARS),
            &"p".repeat(ds_command_kernel::execution_context::MAX_PROJECT_CHARS + 1),
        ] {
            let admitted = bounded_named_project(candidate);
            assert_eq!(
                admitted.is_ok(),
                ds_command_kernel::execution_context::valid_project(candidate),
                "the door and the kernel disagree about {candidate:?}"
            );
            match admitted {
                Ok(project) => assert_eq!(project, candidate, "an id is never repaired"),
                Err(refusal) => {
                    assert_eq!(refusal.code(), "context_corrupt");
                    // The sentence states the RULE — an operator who reads it
                    // knows which character was refused without guessing.
                    let message = refusal.message();
                    assert!(message.contains("one path segment"), "{message}");
                    assert!(message.contains("no separator"), "{message}");
                    assert!(
                        refusal.remedy_text().is_some(),
                        "a refusal with nothing to do next"
                    );
                }
            }
        }
    }

    #[test]
    fn project_report_adapter_exposes_only_lane_and_typed_requests() {
        let _: fn(
            &str,
            &CompoundedReportRequest,
        ) -> Result<HeadlessProjectReport<CompoundedReportReceipt>, Failure> = compounded_report;
        let _: fn(&str) -> Result<HeadlessProjectReport<Vec<CompoundedArchive>>, Failure> =
            compounded_report_list;
        let _: fn(
            &str,
            &TransformerSet,
        ) -> Result<HeadlessProjectReport<TransformerInventory>, Failure> = transformer_inventory;
        let _: fn(
            &str,
            &TransformerSet,
        ) -> Result<HeadlessProjectReport<TransformerStatusList>, Failure> = transformer_status;
        let _: fn(
            &str,
            RetirementAction,
            &RetirementRequest,
        ) -> Result<HeadlessProjectReport<RetirementReceipt>, Failure> = transformer_retirement;
        assert_eq!(ReportFileLevel::Sector.token(), "sector");
        assert_eq!(RetirementAction::Restore.token(), "restore");
    }

    #[test]
    fn tile_adapter_exposes_only_lane_project_kind_and_force_choices() {
        // The project is always named by the caller; no saved selection.
        let _: fn(&str, &str, TileType) -> Result<HeadlessTileOperation, Failure> = tile_status;
        let _: fn(&str, &str, TileType) -> Result<HeadlessTilePreflight, Failure> = tile_preflight;
        let _: fn(&str, &str, TileType, bool) -> Result<HeadlessTileOperation, Failure> =
            tile_generate;
        assert_eq!(TileType::Survey.token(), "survey");
        assert_eq!(TileType::Design.token(), "design");
    }

    #[test]
    fn every_native_state_auth_descriptor_uses_the_native_gate() {
        let expected = native_availability as fn() -> ds_cli_contract::spec::Availability;
        for command in [
            &STATUS_COMMAND,
            &LOGIN_COMMAND,
            &LOGOUT_COMMAND,
            &PROJECT_LIST_COMMAND,
            &PROJECT_USE_COMMAND,
            &PROJECT_STATUS_COMMAND,
            &device::BEGIN_COMMAND,
            &device::STATUS_COMMAND,
            &device::COMPLETE_COMMAND,
            &device::LIST_COMMAND,
            &device::READ_COMMAND,
            &device::REVOKE_COMMAND,
        ] {
            assert!(std::ptr::fn_addr_eq(command.availability, expected));
        }
    }

    #[test]
    fn a_noninteractive_child_cannot_open_the_hidden_prompt() {
        assert_eq!(
            refuse_noninteractive_prompt(false, true)
                .unwrap_err()
                .code(),
            "password_prompt_forbidden"
        );
        assert!(refuse_noninteractive_prompt(false, false).is_ok());
        assert!(refuse_noninteractive_prompt(true, true).is_ok());
    }

    #[test]
    fn list_projection_bound_is_validated_locally() {
        assert_eq!(parse_limit("1").unwrap(), 1);
        assert_eq!(parse_limit("1000").unwrap(), 1000);
        for invalid in ["0", "1001", "-1", "many"] {
            assert_eq!(
                parse_limit(invalid).unwrap_err().code(),
                "project_limit_invalid"
            );
        }
    }

    #[test]
    fn survey_entries_absence_never_borrows_the_transformer_vocabulary() {
        // Both lanes of both Survey entries reads now map through these, so a
        // missing form is one refusal whichever lane answered. Before this,
        // the paired branch fell through to the transformer mapper and the
        // same absent form came back as `transformer_not_found`.
        for failure in [
            survey_entries_select_kind(ErrorKind::ResourceNotFound)
                .expect("the selection route speaks for an absent scope"),
            survey_entries_changes_kind(ErrorKind::ResourceNotFound)
                .expect("the changes route speaks for an absent scope"),
        ] {
            assert_eq!(failure.code(), "survey_entries_scope_not_found");
            assert_eq!(
                failure.class(),
                ds_cli_contract::outcome::ExitClass::InvalidInput
            );
            let remedy = failure.remedy_text().expect("a way out");
            assert!(
                !remedy.contains("transformer"),
                "a Survey read must not send the caller after a transformer: {remedy}"
            );
            assert!(!failure.message().contains("transformer"));
        }
        // Kinds these routes have no word of their own for stay with the
        // shared mapping, which is what the disposition arm still handles.
        assert!(survey_entries_select_kind(ErrorKind::SignedOut).is_none());
        assert!(survey_entries_changes_kind(ErrorKind::SignedOut).is_none());
    }

    #[test]
    fn survey_query_view_failures_do_not_claim_missing_access() {
        for (service, code) in [
            (SurveyQueryServiceCode::FormUnknown, "survey_view_not_found"),
            (SurveyQueryServiceCode::FieldUnknown, "survey_field_unknown"),
            (SurveyQueryServiceCode::ViewStale, "survey_view_stale"),
            (
                SurveyQueryServiceCode::TooExpensive,
                "survey_query_too_expensive",
            ),
            (
                SurveyQueryServiceCode::SyncFailed,
                "survey_query_sync_failed",
            ),
            (
                SurveyQueryServiceCode::Unavailable,
                "survey_query_unavailable",
            ),
            (
                SurveyQueryServiceCode::ScopeNotFound,
                "survey_scope_not_found",
            ),
        ] {
            let failure = map_survey_query_service_code(service);
            assert_eq!(failure.code(), code);
            assert!(failure.remedy_text().is_some());
            assert_eq!(
                failure.class().retryable(),
                matches!(
                    service,
                    SurveyQueryServiceCode::SyncFailed
                        | SurveyQueryServiceCode::Unavailable
                        | SurveyQueryServiceCode::FormUnknown
                        | SurveyQueryServiceCode::ViewStale
                )
            );
        }
    }

    #[test]
    fn survey_form_authority_conditions_keep_distinct_cli_repairs() {
        use ds_cli_contract::outcome::ExitClass;
        let cases = [
            (
                SurveyFormReadServiceCode::ProjectAccessDenied,
                "survey_project_access_denied",
                ExitClass::Unauthorized,
            ),
            (
                SurveyFormReadServiceCode::FormAccessDenied,
                "survey_form_access_denied",
                ExitClass::Unauthorized,
            ),
            (
                SurveyFormReadServiceCode::FormBindingNotFound,
                "survey_form_binding_not_found",
                ExitClass::InvalidInput,
            ),
            (
                SurveyFormReadServiceCode::FormNotParticipating,
                "survey_form_not_participating",
                ExitClass::Conflict,
            ),
        ];
        for (service_code, code, class) in cases {
            let failure = map_survey_form_read_service_code(service_code);
            assert_eq!(failure.code(), code);
            assert_eq!(failure.class(), class);
            assert!(failure.remedy_text().is_some());
        }
    }

    #[test]
    fn a_route_only_rejection_says_so_and_names_the_check() {
        let (remedy, next) =
            route_diagnostic("the transformer context route rejected authentication (HTTP 401)")
                .expect("the transformer-context route carries its own way out");
        assert_eq!(remedy, TRANSFORMER_CONTEXT_ROUTE_REMEDY);
        assert_eq!(next, "ds auth project status");
        assert!(
            !remedy.contains("verify the account"),
            "the credential was already verified for this call: {remedy}"
        );
        // The other preserved diagnostics have no route-specific answer, and
        // must not be given this one.
        assert!(route_diagnostic("project configuration could not be read").is_none());
        assert!(route_diagnostic("feeder settings were saved").is_none());
    }

    #[test]
    fn transformer_absence_keeps_its_resource_specific_code() {
        let failure = map_client_kind(
            ErrorKind::ResourceNotFound,
            "fixture detail that must not become an auth rejection".to_owned(),
        );
        assert_eq!(failure.code(), "transformer_not_found");
        assert_eq!(
            failure.class(),
            ds_cli_contract::outcome::ExitClass::InvalidInput
        );
    }

    // ------------------------------------------------------------------
    // 2026-09-19 governance-headless findings: the signed-out refusal and
    // the install path.
    // ------------------------------------------------------------------

    /// `install list --lane canary` refused with a remedy and a next step
    /// that named no lane, so following them exactly reproduced the refusal
    /// on the default lane.
    #[test]
    fn signed_out_refusal_carries_the_lane_it_was_asked_about() {
        for lane in [Lane::Stable, Lane::Canary] {
            let failure = signed_out_refusal(lane, false);
            assert_eq!(failure.code(), "headless_signed_out");
            let flag = format!("--lane {}", lane.token());
            let remedy = failure.remedy_text().expect("a way out");
            assert!(remedy.contains(&flag), "{remedy}");
            assert!(remedy.contains("ds account connect"), "{remedy}");
            assert!(remedy.contains("ds account connect"), "{remedy}");
            assert!(
                failure
                    .next_commands()
                    .iter()
                    .any(|next| next.contains(&flag) && next.starts_with("ds account connect")),
                "{:?}",
                failure.next_commands()
            );
        }
    }

    /// The same lane answers `auth status` with `signed_in: true` on a device
    /// credential; a refusal that then says "no user is signed in" contradicts
    /// it. The code stays — it is the one every caller declares — and the
    /// sentence says which credential is missing.
    #[test]
    fn a_device_linked_lane_is_not_called_signed_out() {
        let failure = signed_out_refusal(Lane::Canary, true);
        assert_eq!(failure.code(), "headless_signed_out");
        assert!(
            failure.message().contains("device credential"),
            "{}",
            failure.message()
        );
        assert!(
            failure.message().contains("canary"),
            "{}",
            failure.message()
        );
        // The lane IS connected; no sign-in fixes a missing route, so the
        // remedy reports the gap and the next step names the lane.
        let remedy = failure.remedy_text().expect("a way out");
        assert!(remedy.contains("coverage gap"), "{remedy}");
        assert!(!remedy.contains("account connect"), "{remedy}");
        assert!(
            failure
                .next_commands()
                .iter()
                .any(|next| next == "ds auth status --lane canary"),
            "{:?}",
            failure.next_commands()
        );
        // Without a device credential the sentence must not mention one.
        assert!(
            !signed_out_refusal(Lane::Canary, false)
                .message()
                .contains("device credential")
        );
    }

    /// The shared kind mapping has no lane in hand; its next step at least
    /// says that one has to be named.
    #[test]
    fn signed_out_kind_tells_the_caller_to_name_a_lane() {
        let failure = map_client_kind(ErrorKind::SignedOut, "session gone".to_owned());
        assert_eq!(failure.code(), "headless_signed_out");
        assert!(
            failure
                .next_commands()
                .iter()
                .any(|next| next.contains("--lane")),
            "{:?}",
            failure.next_commands()
        );
    }

    /// An unknown install id surfaced as `transformer_not_found` with a remedy
    /// naming an input `ds install show` does not have.
    #[test]
    fn install_absence_is_an_install_refusal_not_a_transformer() {
        let failure = install_kind(ErrorKind::ResourceNotFound)
            .expect("the install path speaks for an absent installation");
        assert_eq!(failure.code(), "install_not_found");
        assert_eq!(
            failure.class(),
            ds_cli_contract::outcome::ExitClass::InvalidInput
        );
        let remedy = failure.remedy_text().expect("a way out");
        assert!(remedy.contains("ds install list"), "{remedy}");
        assert!(!remedy.contains("transformer"), "{remedy}");
        // Every other kind keeps the shared mapping, which is what the
        // install commands declare.
        for kind in [
            ErrorKind::SignedOut,
            ErrorKind::InvalidInput,
            ErrorKind::AuthenticationRejected,
            ErrorKind::UnreadableResponse,
            ErrorKind::Transient,
        ] {
            assert!(install_kind(kind).is_none(), "{kind:?}");
        }
    }

    #[test]
    fn rejected_password_points_to_the_passwordless_identity_link() {
        let failure = map_client_kind(
            ErrorKind::InvalidCredentials,
            "upstream detail must not escape".to_owned(),
        );
        assert_eq!(failure.code(), "auth_invalid_credentials");
        assert_eq!(failure.next_commands(), &["ds account connect"]);
        assert!(
            failure
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("ds account connect"))
        );
        assert!(!failure.message().contains("upstream detail"));
    }

    #[test]
    fn survey_entry_service_codes_have_exact_stable_cli_refusals() {
        use SurveyEntriesSelectServiceCode as ServiceCode;
        use ds_cli_contract::outcome::ExitClass;

        let cases = [
            (
                ServiceCode::TooExpensive,
                "survey_entries_too_expensive",
                ExitClass::InvalidInput,
                "narrow --bbox",
                false,
            ),
            (
                ServiceCode::TooLarge,
                "survey_entries_too_large",
                ExitClass::InvalidInput,
                "lower --limit",
                false,
            ),
            (
                ServiceCode::SyncFailed,
                "survey_entries_sync_failed",
                ExitClass::Unavailable,
                "retry without changing",
                true,
            ),
            (
                ServiceCode::MirrorInvalid,
                "survey_entries_mirror_invalid",
                ExitClass::Failed,
                "unchanged retry is not a remedy",
                false,
            ),
            (
                ServiceCode::Invalid,
                "survey_entries_invalid",
                ExitClass::InvalidInput,
                "recheck the exact form, bbox, and limit",
                false,
            ),
            (
                ServiceCode::Unavailable,
                "survey_entries_unavailable",
                ExitClass::Unavailable,
                "retry later",
                true,
            ),
            (
                ServiceCode::Failed,
                "survey_entries_failed",
                ExitClass::Unavailable,
                "retry without changing",
                true,
            ),
            (
                ServiceCode::ScopeNotFound,
                "survey_entries_scope_not_found",
                ExitClass::InvalidInput,
                "verify the selected project",
                false,
            ),
        ];
        for (service_code, cli_code, class, remedy, retryable) in cases {
            let failure = map_survey_entries_service_code(service_code);
            assert_eq!(failure.code(), cli_code);
            assert_eq!(failure.class(), class);
            assert_eq!(failure.class().retryable(), retryable);
            assert!(failure.remedy_text().unwrap().contains(remedy));
        }
        let scope = map_survey_entries_service_code(ServiceCode::ScopeNotFound);
        assert_eq!(
            scope.message(),
            "the selected project or governed form is unavailable to this verified user"
        );
    }

    #[test]
    fn survey_changes_service_codes_have_exact_stable_cli_refusals() {
        use SurveyEntriesChangesServiceCode as ServiceCode;
        use ds_cli_contract::outcome::ExitClass;

        let cases = [
            (
                ServiceCode::Invalid,
                "survey_entries_changes_invalid",
                ExitClass::InvalidInput,
                "recheck the exact form",
                false,
            ),
            (
                ServiceCode::CursorInvalid,
                "survey_entries_changes_cursor_invalid",
                ExitClass::InvalidInput,
                "exact next_cursor",
                false,
            ),
            (
                ServiceCode::FenceExpired,
                "survey_entries_changes_fence_expired",
                ExitClass::InvalidInput,
                "last previously completed checkpoint",
                false,
            ),
            (
                ServiceCode::TooExpensive,
                "survey_entries_changes_too_expensive",
                ExitClass::Failed,
                "query budget",
                false,
            ),
            (
                ServiceCode::TooLarge,
                "survey_entries_changes_too_large",
                ExitClass::InvalidInput,
                "lower --limit",
                false,
            ),
            (
                ServiceCode::MirrorInvalid,
                "survey_entries_changes_mirror_invalid",
                ExitClass::Failed,
                "unchanged retry is not a remedy",
                false,
            ),
            (
                ServiceCode::SnapshotUnavailable,
                "survey_entries_changes_snapshot_unavailable",
                ExitClass::Unavailable,
                "exact same cursor",
                true,
            ),
            (
                ServiceCode::Unavailable,
                "survey_entries_changes_unavailable",
                ExitClass::Failed,
                "durable changes cursor signing key",
                false,
            ),
            (
                ServiceCode::SyncFailed,
                "survey_entries_changes_sync_failed",
                ExitClass::Unavailable,
                "retry without changing",
                true,
            ),
            (
                ServiceCode::Failed,
                "survey_entries_changes_failed",
                ExitClass::Unavailable,
                "retry without changing",
                true,
            ),
            (
                ServiceCode::ScopeNotFound,
                "survey_entries_scope_not_found",
                ExitClass::InvalidInput,
                "verify the selected project",
                false,
            ),
        ];
        for (service_code, cli_code, class, remedy, retryable) in cases {
            let failure = map_survey_entries_changes_service_code(service_code);
            assert_eq!(failure.code(), cli_code);
            assert_eq!(failure.class(), class);
            assert_eq!(failure.class().retryable(), retryable);
            assert!(failure.remedy_text().unwrap().contains(remedy));
        }
        let scope = map_survey_entries_changes_service_code(ServiceCode::ScopeNotFound);
        assert_eq!(
            scope.message(),
            "the selected project or governed form is unavailable to this verified user"
        );

        let coarse = survey_entries_changes_refused();
        assert_eq!(coarse.code(), "survey_entries_changes_refused");
        assert_eq!(coarse.class(), ExitClass::InvalidInput);
        assert!(!coarse.class().retryable());
    }

    #[test]
    fn survey_create_service_codes_have_exact_stable_cli_refusals() {
        use SurveyEntryCreateServiceCode as ServiceCode;
        use ds_cli_contract::outcome::ExitClass;

        let cases = [
            (
                ServiceCode::Invalid,
                "survey_entry_create_invalid",
                ExitClass::InvalidInput,
                "recheck the exact form",
                false,
            ),
            (
                ServiceCode::Unauthorized,
                "survey_entry_create_auth_rejected",
                ExitClass::Unauthorized,
                "sign in again",
                false,
            ),
            (
                ServiceCode::PermissionDenied,
                "survey_entry_create_permission_denied",
                ExitClass::Unauthorized,
                "entries.create authority",
                false,
            ),
            (
                ServiceCode::ScopeNotFound,
                "survey_entry_create_scope_not_found",
                ExitClass::InvalidInput,
                "optional context key",
                false,
            ),
            (
                ServiceCode::FormDisabled,
                "survey_entry_create_form_disabled",
                ExitClass::InvalidInput,
                "enable the project form",
                false,
            ),
            (
                ServiceCode::ProjectReadOnly,
                "survey_entry_create_project_read_only",
                ExitClass::Conflict,
                "active writable project",
                true,
            ),
            (
                ServiceCode::IdempotencyConflict,
                "survey_entry_create_idempotency_conflict",
                ExitClass::Conflict,
                "fresh key",
                true,
            ),
            (
                ServiceCode::AlreadyExists,
                "survey_entry_create_already_exists",
                ExitClass::Conflict,
                "new document id",
                true,
            ),
            (
                ServiceCode::Failed,
                "survey_entry_create_failed",
                ExitClass::Unavailable,
                "same idempotency key",
                true,
            ),
        ];
        for (service_code, cli_code, class, remedy, retryable) in cases {
            let failure = map_survey_entry_create_service_code(service_code);
            assert_eq!(failure.code(), cli_code);
            assert_eq!(failure.class(), class);
            assert_eq!(failure.class().retryable(), retryable);
            assert!(failure.remedy_text().unwrap().contains(remedy));
        }
    }

    #[test]
    fn auth_contract_has_no_secret_or_generic_transport_inputs() {
        for command in DOMAIN.commands {
            for forbidden in [
                "password",
                "token",
                "refresh-token",
                "endpoint",
                "url",
                "header",
            ] {
                assert!(
                    command.arg(forbidden).is_none(),
                    "{} exposes {forbidden}",
                    command.id
                );
            }
            if command.id == "auth.link.approve" {
                assert_eq!(command.effect, Effect::GlobalWrite);
                // The native user approves; the descriptor names the paired
                // fallback for a lane with no native session.
                assert_eq!(command.authority, Authority::HeadlessUser);
                assert!(command.arg("desktop-descriptor").is_some());
            } else if matches!(
                command.id,
                "auth.link.status" | "auth.device.list" | "auth.device.read"
            ) {
                assert_eq!(command.effect, Effect::ReadOnly);
                assert!(command.arg("desktop-descriptor").is_none());
            } else if matches!(
                command.id,
                "auth.device.revoke" | "auth.project.create" | "auth.project.update"
            ) {
                // The confirmed global writes: a device revocation, a project
                // creation and a project edit. Each mutates governed shared
                // state on the gateway and none touches local auth state.
                assert_eq!(command.effect, Effect::GlobalWrite);
                assert_eq!(command.authority, Authority::HeadlessUser);
                assert!(command.arg("desktop-descriptor").is_none());
            } else {
                assert_eq!(command.effect, Effect::LocalAuthState);
                assert!(command.arg("desktop-descriptor").is_none());
            }
        }
        assert_eq!(LOGIN_COMMAND.authority, Authority::None);
        assert_eq!(LOGOUT_COMMAND.authority, Authority::None);
        assert_eq!(PROJECT_LIST_COMMAND.authority, Authority::HeadlessUser);
        assert_eq!(PROJECT_USE_COMMAND.authority, Authority::HeadlessUser);
        assert_eq!(PROJECT_STATUS_COMMAND.authority, Authority::HeadlessUser);
        assert_eq!(STATUS_COMMAND.contract, 2);
    }

    #[test]
    fn each_leaf_advertises_only_its_reachable_special_refusals() {
        let codes = |command: &Command| {
            command
                .refusals
                .iter()
                .map(|refusal| refusal.code)
                .collect::<std::collections::BTreeSet<_>>()
        };
        let status = codes(&STATUS_COMMAND);
        assert!(!status.contains("password_input_invalid"));
        assert!(!status.contains("project_limit_invalid"));
        assert!(!status.contains("headless_signed_out"));
        assert!(status.contains("project_context_stale"));
        assert!(status.contains("auth_context_unreadable"));

        let login = codes(&LOGIN_COMMAND);
        assert!(login.contains("password_input_invalid"));
        assert!(!login.contains("project_not_visible"));
        assert!(!login.contains("auth_revoked"));

        let list = codes(&PROJECT_LIST_COMMAND);
        assert!(list.contains("project_limit_invalid"));
        assert!(!list.contains("project_not_visible"));
        assert!(!list.contains("project_context_stale"));

        assert!(codes(&PROJECT_USE_COMMAND).contains("project_not_visible"));
        assert!(codes(&PROJECT_STATUS_COMMAND).contains("project_context_stale"));
    }

    #[test]
    fn renderers_do_not_invent_secret_fields() {
        let marker = "refresh-token-marker";
        let data = json!({
            "lane": "stable",
            "signed_in": true,
            "uid": "uid-1",
            "email": "operator@example.com",
            "password": marker,
            "id_token": marker,
        });
        let rendered = render_login(&data);
        assert!(!rendered.contains(marker));
        assert!(!rendered.contains("uid-1"));
        assert!(rendered.contains("operator@example.com"));
    }

    #[test]
    fn human_project_list_makes_empty_and_truncated_results_visible() {
        let empty = render_project_list(&json!({
            "lane": "stable", "projects": [], "returned": 0, "total": 0, "more": false
        }));
        assert_eq!(empty, "no visible projects (stable)\n");

        let truncated = render_project_list(&json!({
            "lane": "stable",
            "projects": [{
                "ds_project": "project-1", "status": "active", "role": "owner",
                "project_name": "Project One"
            }],
            "returned": 1, "total": 2, "more": true
        }));
        assert!(truncated.contains("project-1  active  owner  Project One"));
        assert!(truncated.contains("showing 1 of 2"));
    }

    /// A platform_admin's 63 projects arrive through ds-brain's elevated path
    /// with no membership document, so every row's `role` is `""` — which read
    /// as "no role" when the truth was "seen by elevation". The answer now
    /// carries the verdict, and a row the server left roleless says so; a role
    /// the server named is untouched, and without the verdict nothing changes.
    #[test]
    fn an_elevated_directory_names_its_roleless_rows_as_elevated() {
        use crate::test_support::{FixtureTransport, NOW, SIGN_IN, signed_in};

        fn bucket(id: &str, state: &str, role: &str, elevated: bool) -> Vec<u8> {
            serde_json::to_vec(&json!({
                "success": true,
                "data": {
                    "count": 1,
                    "projects": [{
                        "id": id, "eds_project_id": id, "project_name": id,
                        "role": role, "lifecycle_state": state
                    }],
                    "elevated": elevated,
                    "status": state
                }
            }))
            .unwrap()
        }

        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        // One elevated read: two roleless rows and one the server named.
        transport.push_projects(&bucket("a", "active", "", true));
        transport.push_projects(&bucket("b", "archived", "owner", true));
        transport.push_projects(&bucket("c", "testing", "", true));
        // One membership read with the same empty role.
        transport.push_projects(&bucket("a", "active", "", false));
        transport.push_projects(&bucket("b", "archived", "", false));
        transport.push_projects(&bucket("c", "testing", "", false));
        let mut client = signed_in(transport);

        let elevated = client.list_projects(NOW + 1).unwrap();
        let answer = directory_answer(Lane::Canary, &elevated, 2);
        assert_eq!(answer["elevated"], true);
        assert_eq!(answer["projects"][0]["role"], "elevated");
        assert_eq!(answer["projects"][1]["role"], "owner");
        assert_eq!(answer["returned"], 2);
        assert_eq!(answer["total"], 3);
        assert_eq!(answer["more"], true);
        assert!(
            render_project_list(&answer).contains("a  active  elevated  a"),
            "{}",
            render_project_list(&answer)
        );

        let membership = client.list_projects(NOW + 2).unwrap();
        let answer = directory_answer(Lane::Stable, &membership, 10);
        assert_eq!(answer["elevated"], false);
        for row in answer["projects"].as_array().unwrap() {
            assert_eq!(row["role"], "", "{row}");
        }

        // A row with no role at all is presented the same way as an empty one.
        assert_eq!(presented_role(None, true), json!("elevated"));
        assert_eq!(presented_role(None, false), Value::Null);
    }

    #[test]
    fn import_principal_binding_is_stable_non_identity_evidence() {
        let first = principal_binding_sha256("uid-1", "operator@example.com");
        let same = principal_binding_sha256("uid-1", "operator@example.com");
        let other = principal_binding_sha256("uid-2", "operator@example.com");
        assert_eq!(first, same);
        assert_ne!(first, other);
        assert_eq!(first.len(), 64);
        assert!(!first.contains("uid-1"));
        assert!(!first.contains("operator@example.com"));
    }

    /// `ds auth project update` on a project the account does not administer
    /// reads as the capability a project admin has to grant, not as a generic
    /// authentication failure; a 404 is the directory refusal `auth project
    /// use` already declares; a 400 carries the server's own sentence. Each
    /// code and remedy is the one the command's `--help` declares.
    #[test]
    fn project_properties_refusals_read_as_the_commands_declare_them() {
        use crate::test_support::{FixtureTransport, NOW, SIGN_IN, signed_in};
        use ds_client_core::project_properties::{Command, Properties, Receipt};

        fn envelope(code: &str, message: &str) -> Vec<u8> {
            serde_json::to_vec(&json!({
                "success": false,
                "error": {"code": code, "message": message, "timestamp": 1},
                "timestamp": "2026-09-20T10:00:00Z",
                "service": "data-solutions-backend",
                "version": "1.0.0"
            }))
            .unwrap()
        }
        let update = Command::Update {
            project_id: "it_rwanda".into(),
            properties: Properties {
                display_name: Some("Integration test — Rwanda".into()),
                ..Properties::default()
            },
        };
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_properties(
            403,
            &envelope(
                "INSUFFICIENT_PERMISSIONS",
                "Requires project.properties.edit capability (project admin)",
            ),
        );
        transport.push_project_properties(
            403,
            &envelope(
                "INSUFFICIENT_PERMISSIONS",
                "Requires project.create capability",
            ),
        );
        transport.push_project_properties(
            403,
            &envelope("PROJECT_ARCHIVED", "This project is archived."),
        );
        transport.push_project_properties(404, &envelope("PROJECT_NOT_FOUND", "Project not found"));
        transport.push_project_properties(
            400,
            &envelope("VALIDATION_FAILED", "unsupported project_type \"x\""),
        );
        transport.push_project_properties(
            200,
            &serde_json::to_vec(&json!({
                "success": true,
                "message": "Project updated successfully",
                "data": {"project_id": "it_rwanda", "resources_ensured": true, "dataset_exists": true, "fields_updated": 1},
                "timestamp": "2026-09-20T10:00:00Z", "service": "data-solutions-backend", "version": "1.0.0"
            }))
            .unwrap(),
        );
        let mut client = signed_in(transport);
        let mut refused = || {
            map_project_properties_client(client.project_properties(&update, NOW + 1).unwrap_err())
        };

        let edit = refused();
        assert_eq!(edit.code(), PROJECT_EDIT_FORBIDDEN_REFUSAL.code);
        assert_eq!(
            edit.remedy_text(),
            Some(PROJECT_EDIT_FORBIDDEN_REFUSAL.remedy)
        );
        assert!(
            edit.message().contains(
                "(HTTP 403): Requires project.properties.edit capability (project admin)"
            ),
            "{edit:?}"
        );
        let create = refused();
        assert_eq!(create.code(), PROJECT_CREATE_FORBIDDEN_REFUSAL.code);
        assert_eq!(
            create.remedy_text(),
            Some(PROJECT_CREATE_FORBIDDEN_REFUSAL.remedy)
        );
        let archived = refused();
        assert_eq!(archived.code(), "auth_rejected");
        assert!(archived.message().contains("archived"), "{archived:?}");
        let unknown = refused();
        assert_eq!(unknown.code(), PROJECT_UNKNOWN_REFUSAL.code);
        assert_eq!(unknown.remedy_text(), Some(PROJECT_UNKNOWN_REFUSAL.remedy));
        let invalid = refused();
        assert_eq!(invalid.code(), PROJECT_PROPERTIES_INVALID_REFUSAL.code);
        assert!(
            invalid.message().contains("unsupported project_type"),
            "{invalid:?}"
        );

        // A success is the receipt, and the rendered line names the project.
        let receipt = client.project_properties(&update, NOW + 1).unwrap();
        assert!(matches!(receipt, Receipt::Updated(ref r) if r.fields_updated == 1));
        let mut answer = serde_json::to_value(&receipt).unwrap();
        answer["action"] = json!("update");
        assert_eq!(
            render_project_properties(&answer),
            "updated  it_rwanda  1 field(s) changed\n"
        );

        // Nothing to change is refused before the transport is reached.
        let nothing = map_project_properties_client(
            client
                .project_properties(
                    &Command::Update {
                        project_id: "it_rwanda".into(),
                        properties: Properties::default(),
                    },
                    NOW + 1,
                )
                .unwrap_err(),
        );
        assert_eq!(nothing.code(), PROJECT_PROPERTIES_INVALID_REFUSAL.code);
    }

    /// The receipt an operator actually reads. On 2026-09-10 a print layout
    /// the deployed validator could not decode reached `ds` as
    /// `auth_response_unreadable` — an authentication code, with no field
    /// named and no way forward. Each governed refusal now keeps its status,
    /// its sentence and a code that says what kind of problem it is.
    #[test]
    fn a_governed_refusal_reads_as_what_it_is_and_never_as_an_auth_failure() {
        use ds_client_core::ServiceRefusal;

        let invalid = map_service_refusal(
            ErrorKind::InvalidInput,
            &ServiceRefusal::new(
                422,
                Some("print_layout_invalid"),
                Some("unknown field `composition`, expected one of `schema`, `id`, `name`"),
            ),
            "Printing request refused: the layout is not acceptable to this deployment",
        );
        assert_eq!(invalid.code(), "print_layout_invalid");
        assert!(invalid.message().contains("HTTP 422"), "{invalid:?}");
        assert!(
            invalid.message().contains("unknown field `composition`"),
            "{invalid:?}"
        );
        assert!(invalid.remedy_text().is_some());
        assert!(
            invalid
                .next_commands()
                .iter()
                .any(|command| command.contains("report layout schema")),
            "{invalid:?}"
        );

        let unavailable = map_service_refusal(
            ErrorKind::Transient,
            &ServiceRefusal::new(
                502,
                Some("print_validator_unavailable"),
                Some("The print layout validator answered 500"),
            ),
            "The printing service is temporarily unavailable",
        );
        assert_eq!(unavailable.code(), "print_validator_unavailable");
        assert!(
            unavailable.message().contains("HTTP 502"),
            "{unavailable:?}"
        );

        // A refusal the printing contract does not name keeps the shared kind
        // mapping — and still carries the status and the sentence, which is
        // what `report.project.outputs.set` had none of.
        let configuration = map_service_refusal(
            ErrorKind::Transient,
            &ServiceRefusal::new(502, Some("internal_error"), Some("store unavailable")),
            "project configuration request was refused; no cached configuration is substituted",
        );
        assert_eq!(configuration.code(), "auth_transient");
        assert!(
            configuration.message().contains("HTTP 502"),
            "{configuration:?}"
        );
        assert!(
            configuration.message().contains("store unavailable"),
            "{configuration:?}"
        );

        // A missing setup is a printing fact. The shared kind mapping's
        // `transformer_not_found` was the nearest thing it had, and it was
        // wrong about what was missing.
        let missing = map_service_refusal(
            ErrorKind::ResourceNotFound,
            &ServiceRefusal::new(
                404,
                Some("print_setup_not_found"),
                Some("Printing setup not found"),
            ),
            "The printing setup does not exist",
        );
        assert_eq!(missing.code(), "print_setup_not_found");
        assert!(missing.message().contains("HTTP 404"), "{missing:?}");

        // No code and no sentence is still better than a bare class: the
        // status alone tells a caller whether to retry.
        let bare = map_service_refusal(
            ErrorKind::AuthenticationRejected,
            &ServiceRefusal::new(403, None, None),
            "Printing request rejected for this identity",
        );
        assert_eq!(bare.code(), "auth_rejected");
        assert!(bare.message().ends_with("(HTTP 403)"), "{bare:?}");
    }

    #[test]
    fn native_seed_adapter_can_read_the_closed_service_code() {
        let failure = map_service_refusal(
            ErrorKind::InvalidInput,
            &ds_client_core::ServiceRefusal::new(409, Some("solar_seed_digest_mismatch"), None),
            "The governed Solar seed request was refused",
        );
        let detail = failure.detail_value().unwrap();
        assert_eq!(detail["service_code"], "solar_seed_digest_mismatch");
        assert_eq!(detail["http_status"], 409);
        assert!(detail.get("service_message").is_none());
    }
}
#[cfg(test)]
mod status_upload_tests {
    use super::*;

    #[test]
    fn native_upload_host_drives_kernel_effects_without_redeciding_the_job() {
        let path =
            std::env::temp_dir().join(format!("ds-status-upload-{}-a.zip", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let sources = vec![path.to_string_lossy().into_owned()];
        let settings = json!({"assign_earthing":true}).as_object().unwrap().clone();
        let mut effects = Vec::new();
        let result = drive_status_upload(
            "project_a",
            &sources,
            StatusUploadDomain::LvProcess,
            &settings,
            |command| match command {
                ds_client_core::StatusProcessingCommand::Upload {
                    domain,
                    file_name,
                    size,
                    reader,
                    ..
                } => {
                    let mut bytes = Vec::new();
                    reader.read_to_end(&mut bytes).unwrap();
                    effects.push("upload");
                    assert_eq!(domain, StatusUploadDomain::LvProcess);
                    assert_eq!(file_name, path.file_name().unwrap().to_str().unwrap());
                    assert_eq!(size, 3);
                    assert_eq!(bytes, b"abc");
                    Ok(json!({"blob_path":format!("projects/project_a/process_uploads/v2/lv_process/r/{file_name}")}))
                }
                ds_client_core::StatusProcessingCommand::Process {
                    action,
                    file_name,
                    chain,
                    extra,
                    ..
                } => {
                    effects.push("process");
                    assert_eq!(action, StatusUploadDomain::LvProcess);
                    assert!(chain.is_empty());
                    assert_eq!(extra["settings"]["assign_earthing"], true);
                    Ok(json!({"results":[{"file_name":file_name,"submitted":1}]}))
                }
            },
        )
        .unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(effects, ["upload", "process"]);
        assert_eq!(result["phase"], "complete");
        assert_eq!(result["results"][0]["ok"], true);
    }

    #[test]
    fn native_upload_host_records_a_missing_file_and_terminates() {
        let missing = std::env::temp_dir().join(format!(
            "ds-status-upload-{}-missing.zip",
            std::process::id()
        ));
        let result = drive_status_upload(
            "project_a",
            &[missing.to_string_lossy().into_owned()],
            StatusUploadDomain::LvDrafting,
            &serde_json::Map::new(),
            |_| panic!("a missing file must not reach a network effect"),
        )
        .unwrap();
        assert_eq!(result["phase"], "complete");
        assert_eq!(result["results"][0]["ok"], false);
        assert!(
            result["results"][0]["error"]
                .as_str()
                .unwrap()
                .contains("Cannot open")
        );
    }
}

/// Explicit-project image work never reads or changes the saved selection.
pub fn survey_photo(
    lane_value: &str,
    project: &str,
    command: ds_client_core::survey_photo::Command<'_>,
) -> Result<ds_client_core::survey_photo::Receipt, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.survey_photo(&project, command).map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .survey_photo(&project, command, now())
        .map_err(map_client)
}

/// Execute a closed Solar operation against an explicit project without changing selection.
pub fn solar_for_project(
    lane_value: &str,
    project: &str,
    command: &SolarProjectCommand,
) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    command.validate(&project).map_err(map_client)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.solar_project(&project, command).map_err(map_client);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client
        .solar_project(&project, command, now())
        .map_err(map_client)
}
pub use ds_client_core::solar_portfolio::Command as SolarPortfolioCommand;

/// Native Solar authority captured for an explicit project, independent of selection.
pub struct NamedSolarProjectSession {
    project: String,
    lane: &'static str,
    uid: String,
    audience: String,
    principal_sha256: String,
    provider: SolarProjectProvider,
}
pub fn solar_project_session_for_project(
    lane_value: &str,
    project: &str,
) -> Result<NamedSolarProjectSession, Failure> {
    let lane = Lane::parse(lane_value)?;
    let project = bounded_named_project(project)?;
    if let Some(device) = restored_device_session(lane)? {
        return Ok(NamedSolarProjectSession {
            project,
            lane: lane.token(),
            uid: device.context().uid().to_owned(),
            audience: device.profile().credential_audience_sha256().to_owned(),
            principal_sha256: solar_principal_binding_sha256(
                device.context().uid(),
                device.context().email(),
            ),
            provider: SolarProjectProvider::Device(Box::new(device)),
        });
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    let user = require_restore_before_context(&mut client)?;
    Ok(NamedSolarProjectSession {
        project,
        lane: lane.token(),
        uid: user.uid().to_owned(),
        principal_sha256: solar_principal_binding_sha256(user.uid(), user.email()),
        audience: client.profile().credential_audience_sha256().to_owned(),
        provider: SolarProjectProvider::Firebase(Box::new(client)),
    })
}
impl NamedSolarProjectSession {
    pub fn binding(&self) -> Value {
        json!({"project":self.project,"lane":self.lane,"uid":self.uid,"principal":self.principal_sha256,"audience":self.audience})
    }
    fn verify_authority(&self) -> Result<(), Failure> {
        let identity = probe_headless_identity_for_named_project(self.lane)?.ok_or_else(|| {
            Failure::unauthorized("headless_signed_out", "Solar authority signed out")
        })?;
        if identity.uid() != self.uid || identity.credential_audience_sha256() != self.audience {
            return Err(Failure::unauthorized(
                "auth_rejected",
                "Solar authority changed; capture a new explicit session",
            ));
        }
        Ok(())
    }
    pub fn execute(&mut self, command: &SolarProjectCommand) -> Result<Value, Failure> {
        command.validate(&self.project).map_err(map_client)?;
        self.verify_authority()?;
        let result = match &mut self.provider {
            SolarProjectProvider::Firebase(client) => {
                client.solar_project(&self.project, command, now())
            }
            SolarProjectProvider::Device(device) => device.solar_project(&self.project, command),
        }
        .map_err(map_client);
        self.verify_authority()?;
        result
    }
}
