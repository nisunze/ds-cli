//! Thin CLI host for ds-web's closed native client core.
//!
//! This crate owns terminal input, package resources, native HTTP, and private
//! per-user files. Identity, Firebase validation, refresh rotation, and the
//! project response contract remain exclusively in `ds-client-core`.

mod context;
mod profile;
mod state;
mod transport;

use std::io::{self, BufRead, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use context::CredentialProvider;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Domain, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{
    Client, ClientError, ErrorKind, Project, ProjectFormSettingsEditor, ProjectFormsSnapshot,
    ProjectStatus, SolarSnapshot, SurveyEntriesChanges, SurveyEntriesChangesRequest,
    SurveyEntriesChangesServiceCode, SurveyEntriesSelectRequest, SurveyEntriesSelectServiceCode,
    SurveyEntriesSelection, SurveyEntryCreateReceipt, SurveyEntryCreateRequest,
    SurveyEntryCreateServiceCode, SurveyQueryRequest, SurveyQueryResult, TransformerContext,
};
use profile::Lane;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use state::{NativeRefreshStore, ProjectContextLease};
use transport::NativeTransport;
use zeroize::Zeroize;

pub use context::{
    AuthContext, CanonicalPrincipal, CredentialProviderKind, DeviceIdentity, ProfileFence,
    SelectedProject, SessionState,
};

pub static DOMAIN: Domain = Domain {
    id: "auth",
    summary: "Sign in headlessly and select a visible project.",
    commands: &[
        &STATUS_COMMAND,
        &LOGIN_COMMAND,
        &LOGOUT_COMMAND,
        &PROJECT_LIST_COMMAND,
        &PROJECT_USE_COMMAND,
        &PROJECT_STATUS_COMMAND,
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

const PROFILE_REFUSAL: Refusal = Refusal {
    code: "native_profile_not_configured",
    when: "this build lacks its exact digest-pinned two-lane native client catalog",
    remedy: "install one complete ds release containing ds-client-profiles/catalog.json",
};
const SIGNED_OUT_REFUSAL: Refusal = Refusal {
    code: "headless_signed_out",
    when: "no restorable refresh credential exists for the selected lane and profile",
    remedy: "run ds auth login --email <address>",
};
const STATE_REFUSAL: Refusal = Refusal {
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
const AUTH_REVOKED_REFUSAL: Refusal = Refusal {
    code: "auth_revoked",
    when: "Firebase permanently revokes the native session",
    remedy: "sign in again interactively",
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
const PASSWORD_INPUT_REFUSAL: Refusal = Refusal {
    code: "password_input_invalid",
    when: "password stdin is empty, multiline, or oversized",
    remedy: "provide exactly one bounded line",
};
const PASSWORD_PROMPT_REFUSAL: Refusal = Refusal {
    code: "password_prompt_forbidden",
    when: "an MCP child attempts to open a password prompt",
    remedy: "run auth login directly in a trusted terminal",
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
    AUTH_REJECTED_REFUSAL,
    IDENTITY_REFUSAL,
    TRANSIENT_REFUSAL,
    UNREADABLE_REFUSAL,
    PASSWORD_INPUT_REFUSAL,
    PASSWORD_PROMPT_REFUSAL,
    PASSWORD_TTY_REFUSAL,
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
    availability: profile::availability,
};

pub static LOGIN_COMMAND: Command = Command {
    id: "auth.login",
    path: &["auth", "login"],
    contract: 1,
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
    availability: profile::availability,
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
    availability: profile::availability,
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
    output: "Fresh visible project identities, names, roles, and lifecycle states.",
    examples: &[Example {
        command: "ds auth project list",
        note: "Reads all three lifecycle buckets.",
        runnable: false,
    }],
    refusals: PROJECT_LIST_REFUSALS,
    reference: Some("docs/reference/auth.md"),
    availability: profile::availability,
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
    availability: profile::availability,
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
    availability: profile::availability,
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
    lane: &'static str,
    project_name: String,
    project_status: String,
    snapshot: TransformerContext,
}

impl HeadlessTransformerContext {
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

/// One settings editor fetched under the restored user and that user's
/// audience-fenced selected project.
pub struct HeadlessProjectFormEditor {
    lane: &'static str,
    project_name: String,
    project_status: String,
    snapshot: ProjectFormSettingsEditor,
}

/// One governed Solar snapshot fetched under the restored user and the
/// audience-fenced selected project. Signed download URLs remain only inside
/// the zeroizing owner-intake bytes and are never exposed as fields.
pub struct HeadlessSolarSnapshot {
    lane: &'static str,
    project_name: String,
    project_status: String,
    snapshot: SolarSnapshot,
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
    client: NativeClient,
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
        let result = self
            .client
            .survey_entry_create(&self.project_id, request, now());
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
                self.client.profile(),
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

impl HeadlessSolarSnapshot {
    pub const fn lane(&self) -> &'static str {
        self.lane
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn project_status(&self) -> &str {
        &self.project_status
    }
    pub const fn snapshot(&self) -> &SolarSnapshot {
        &self.snapshot
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

/// Availability of the exact packaged native profiles used by headless
/// project commands.
pub fn native_availability() -> ds_cli_contract::spec::Availability {
    profile::availability()
}

/// Restore one native user and fetch one transformer from that user's fenced
/// selected project. There is deliberately no project-id or URL override.
pub fn transformer_context(
    lane_value: &str,
    transformer: &str,
) -> Result<HeadlessTransformerContext, Failure> {
    let lane = Lane::parse(lane_value)?;
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
        lane: lane.token(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        snapshot,
    })
}

/// Restore one native user and activate project forms for only the saved,
/// audience-fenced selected project. The gateway rechecks membership.
pub fn project_forms(lane_value: &str) -> Result<HeadlessProjectForms, Failure> {
    let lane = Lane::parse(lane_value)?;
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
    let result = client.project_forms(selected.project_id(), now());
    let snapshot = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessProjectForms {
        lane: lane.token(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        snapshot,
    })
}

/// Restore one native user and read one backend-owned settings editor from
/// only the saved, audience-fenced selected project.
pub fn project_form_editor(
    lane_value: &str,
    form_slug: &str,
) -> Result<HeadlessProjectFormEditor, Failure> {
    let lane = Lane::parse(lane_value)?;
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
    let result = client.project_form_editor(selected.project_id(), form_slug, now());
    let snapshot = with_released_context_disposition(client.profile(), &selected, result)?;
    Ok(HeadlessProjectFormEditor {
        lane: lane.token(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        snapshot,
    })
}

/// Restore one native user and capture one governed Solar city snapshot from
/// only the saved, audience-fenced selected project. There is no project or
/// Solar-root override.
pub fn solar_snapshot(
    lane_value: &str,
    template_id: &str,
) -> Result<HeadlessSolarSnapshot, Failure> {
    let lane = Lane::parse(lane_value)?;
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    // Restore may refresh Firebase. The credential store already serializes
    // that rotation; do not also hold the selected-project filesystem lease
    // over a remote identity call.
    let user = match client.restore(now()) {
        Ok(Some(user)) => user,
        Ok(None) => {
            return Err(Failure::unauthorized(
                "headless_signed_out",
                "no native user is signed in for this lane and profile",
            )
            .remedy("run ds auth login --email <address>")
            .next("ds auth status"));
        }
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::PermanentlyRevoked | ErrorKind::IdentityMismatch
            ) =>
        {
            let context = ProjectContextLease::acquire(client.profile())?;
            context.clear().map_err(|_| cleanup_required())?;
            return Err(map_client(error));
        }
        Err(error) => return Err(map_client(error)),
    };
    let context = ProjectContextLease::acquire(client.profile())?;
    let selected = context
        .load(client.profile(), user.uid(), user.email())?
        .ok_or_else(|| {
            Failure::conflict(
                "headless_project_not_selected",
                "no project is selected for this native user, lane, and credential audience",
            )
            .remedy("run ds auth project use --project <exact-id>")
            .next("ds auth project status")
        })?;
    // The audience/identity-fenced selected value is now owned and the core
    // binds the response to it. Do not serialize unrelated headless work over
    // the network call merely to retain a filesystem lease.
    drop(context);
    let snapshot = match client.solar_snapshot(selected.project_id(), template_id, now()) {
        Ok(snapshot) => snapshot,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::PermanentlyRevoked | ErrorKind::IdentityMismatch
            ) =>
        {
            let context = ProjectContextLease::acquire(client.profile())?;
            context
                .clear_if_unchanged(&selected)
                .map_err(|_| cleanup_required())?;
            return Err(map_client(error));
        }
        Err(error) if error.kind() == ErrorKind::ResourceNotFound => {
            return Err(Failure::invalid(
                "solar_city_not_found",
                "the Solar city does not exist in the selected project",
            )
            .remedy("pass one exact live city id from the selected project"));
        }
        Err(error) => return Err(map_client(error)),
    };
    Ok(HeadlessSolarSnapshot {
        lane: lane.token(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        snapshot,
    })
}

/// Restore one native user and run one typed aggregate against only the saved,
/// audience-fenced selected project. There is no project or request-target
/// override, and the context lease is released before the network call.
pub fn survey_query(
    lane_value: &str,
    query: &SurveyQueryRequest,
) -> Result<HeadlessSurveyQuery, Failure> {
    let lane = Lane::parse(lane_value)?;
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
    let result = client.survey_query(selected.project_id(), query, now());
    let result = match result {
        Err(error) if error.kind() == ErrorKind::ResourceNotFound => {
            return Err(Failure::invalid(
                "survey_scope_not_found",
                "the selected project or governed form is unavailable to this verified user",
            )
            .remedy("verify the selected project and pass one exact available form slug"));
        }
        Err(error) if error.kind() == ErrorKind::InvalidInput => {
            return Err(Failure::conflict(
                "survey_query_refused",
                "the backend refused the already validated Survey question or reported a stale view",
            )
            .remedy("retry once, then verify the governed form and Survey view state"));
        }
        other => with_released_context_disposition(client.profile(), &selected, other)?,
    };
    Ok(HeadlessSurveyQuery {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        result,
    })
}

/// Restore one native user and select one bounded spatial receipt from only
/// the saved, audience-fenced project. The context lease is released before
/// the fixed core network call. The result is mutable live-mirror data, not a
/// datastore snapshot, and the core verifies its server-issued digest.
pub fn survey_entries_select(
    lane_value: &str,
    request: &SurveyEntriesSelectRequest,
) -> Result<HeadlessSurveyEntriesSelection, Failure> {
    let lane = Lane::parse(lane_value)?;
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
    let selection = client.survey_entries_select(selected.project_id(), request, now());
    let selection = match selection {
        Err(error) if error.survey_entries_select_service_code().is_some() => {
            return Err(map_survey_entries_service_code(
                error
                    .survey_entries_select_service_code()
                    .expect("the guarded Survey selection service code is present"),
            ));
        }
        Err(error) if error.kind() == ErrorKind::ResourceNotFound => {
            return Err(Failure::invalid(
                "survey_entries_scope_not_found",
                "the selected project or governed form is unavailable to this verified user",
            )
            .remedy("verify the selected project and pass one exact available form slug"));
        }
        Err(error) if error.kind() == ErrorKind::InvalidInput => {
            return Err(Failure::conflict(
                "survey_entries_refused",
                "the backend refused the already validated bounded Survey entry selection",
            )
            .remedy("narrow --bbox or lower --limit, then verify the governed form state"));
        }
        Err(error) if error.kind() == ErrorKind::AuthenticationRejected => {
            return Err(Failure::unauthorized(
                "survey_entries_auth_rejected",
                "the fixed selection route rejected the verified identity or form authority",
            )
            .remedy("verify account and form authority in the selected project"));
        }
        Err(error) if error.kind() == ErrorKind::Transient => {
            return Err(Failure::unavailable(
                "survey_entries_transient",
                "the fixed selection service or its required mirror sync is temporarily unavailable",
            )
            .remedy("retry without changing local state"));
        }
        Err(error) if error.kind() == ErrorKind::UnreadableResponse => {
            return Err(Failure::unavailable(
                "survey_entries_unreadable",
                "the selection response violated its closed identity, geometry, consistency, order, or digest contract",
            )
            .remedy("retry once, then update ds if it persists"));
        }
        other => with_released_context_disposition(client.profile(), &selected, other)?,
    };
    Ok(HeadlessSurveyEntriesSelection {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        selection,
    })
}

/// Restore one native user and read one immutable-fence page of coalesced
/// Survey mirror changes from only the saved, audience-fenced project. The
/// selected-project lease is released before the fixed core network call.
pub fn survey_entries_changes(
    lane_value: &str,
    request: &SurveyEntriesChangesRequest,
) -> Result<HeadlessSurveyEntriesChanges, Failure> {
    let lane = Lane::parse(lane_value)?;
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
    let changes = client.survey_entries_changes(selected.project_id(), request, now());
    let changes = match changes {
        Err(error) if error.survey_entries_changes_service_code().is_some() => {
            return Err(map_survey_entries_changes_service_code(
                error
                    .survey_entries_changes_service_code()
                    .expect("the guarded Survey changes service code is present"),
            ));
        }
        Err(error) if error.kind() == ErrorKind::ResourceNotFound => {
            return Err(Failure::invalid(
                "survey_entries_scope_not_found",
                "the selected project or governed form is unavailable to this verified user",
            )
            .remedy("verify the selected project and pass one exact available form slug"));
        }
        Err(error) if error.kind() == ErrorKind::InvalidInput => {
            return Err(survey_entries_changes_refused());
        }
        Err(error) if error.kind() == ErrorKind::AuthenticationRejected => {
            return Err(Failure::unauthorized(
                "survey_entries_changes_auth_rejected",
                "the fixed changes route rejected the verified identity or form authority",
            )
            .remedy("verify account and form authority in the selected project"));
        }
        Err(error) if error.kind() == ErrorKind::Transient => {
            return Err(Failure::unavailable(
                "survey_entries_changes_transient",
                "the fixed changes service is temporarily unavailable without a recognized service code",
            )
            .remedy("retry the identical page request without advancing its checkpoint"));
        }
        Err(error) if error.kind() == ErrorKind::UnreadableResponse => {
            return Err(Failure::unavailable(
                "survey_entries_changes_unreadable",
                "the changes response violated its closed identity, clocks, geometry, ordering, paging, or consistency contract",
            )
            .remedy("retry once without advancing the checkpoint, then update ds if it persists"));
        }
        other => with_released_context_disposition(client.profile(), &selected, other)?,
    };
    Ok(HeadlessSurveyEntriesChanges {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        changes,
    })
}

/// Restore one native user and create one governed Survey entry in only the
/// saved, audience-fenced project. Local callers construct the strict core
/// request before entering this adapter; the selected-project lease is
/// released before the fixed network call.
pub fn survey_entry_create(
    lane_value: &str,
    request: &SurveyEntryCreateRequest,
) -> Result<HeadlessSurveyEntryCreate, Failure> {
    let lane = Lane::parse(lane_value)?;
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
    let result = client.survey_entry_create(selected.project_id(), request, now());
    let receipt = match result {
        Err(error) if error.survey_entry_create_service_code().is_some() => {
            return Err(map_survey_entry_create_service_code(
                error
                    .survey_entry_create_service_code()
                    .expect("the guarded Survey create service code is present"),
            ));
        }
        Err(error) if error.kind() == ErrorKind::ResourceNotFound => {
            return Err(Failure::invalid(
                "survey_entry_create_scope_not_found",
                "the selected project, governed form, or context ancestor is unavailable",
            )
            .remedy("verify the selected project, form, and optional context key"));
        }
        Err(error) if error.kind() == ErrorKind::InvalidInput => {
            return Err(Failure::invalid(
                "survey_entry_create_refused",
                "the backend refused the already validated governed Survey create request",
            )
            .remedy("recheck the form, document identity, context, and document bounds"));
        }
        Err(error) if error.kind() == ErrorKind::AuthenticationRejected => {
            return Err(Failure::unauthorized(
                "survey_entry_create_auth_rejected",
                "the fixed create route rejected the verified identity or form authority",
            )
            .remedy("verify account and entries.create authority in the selected project"));
        }
        Err(error) if error.kind() == ErrorKind::Transient => {
            return Err(Failure::unavailable(
                "survey_entry_create_failed",
                "the governed Survey create service failed temporarily",
            )
            .remedy(
                "after service recovery, retry the exact document with the same idempotency key",
            ));
        }
        Err(error) if error.kind() == ErrorKind::UnreadableResponse => {
            return Err(Failure::unavailable(
                "survey_entry_create_unreadable",
                "the create response violated its closed identity, version, clock, or authority contract",
            )
            .remedy("verify the backend release and update ds before retrying"));
        }
        other => with_released_context_disposition(client.profile(), &selected, other)?,
    };
    Ok(HeadlessSurveyEntryCreate {
        lane: lane.token(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        receipt,
    })
}

/// Restore one native user exactly once and freeze one audience-fenced
/// selected project for a sequential Survey import. The context lease is
/// released before this function returns and before any create call begins.
pub fn survey_import_session(lane_value: &str) -> Result<HeadlessSurveyImportSession, Failure> {
    let lane = Lane::parse(lane_value)?;
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
    let principal_sha256 = principal_binding_sha256(user.uid(), user.email());
    Ok(HeadlessSurveyImportSession {
        lane: lane.token(),
        project_id: selected.project_id().to_owned(),
        project_name: selected.project_name().to_owned(),
        project_status: selected.status().to_owned(),
        principal_sha256,
        credential_audience_sha256: client.profile().credential_audience_sha256().to_owned(),
        selected,
        client,
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
    let email = inputs.require("email")?;
    let mut password = read_password(inputs.switch("password-stdin"))?;
    let result = client.sign_in(email, &password, now()).map_err(map_client);
    password.zeroize();
    let user = result?;
    context.clear().map_err(|_| cleanup_required())?;
    Ok(json!({ "lane": lane.token(), "signed_in": true, "uid": user.uid(), "email": user.email() }))
}

pub fn run_logout(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    client.sign_out().map_err(map_client)?;
    context.clear().map_err(|_| cleanup_required())?;
    Ok(json!({ "lane": lane.token(), "signed_in": false, "context_cleared": true }))
}

pub fn run_project_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = parse_limit(inputs.require("limit")?)?;
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    require_restore(&mut client, &context)?;
    let directory = with_disposition(client.list_projects(now()), &context)?;
    let total = directory.projects().len();
    let projects = directory
        .projects()
        .iter()
        .take(limit)
        .map(project_json)
        .collect::<Vec<_>>();
    let returned = projects.len();
    Ok(json!({
        "lane": lane.token(),
        "projects": projects,
        "returned": returned,
        "total": total,
        "more": total > returned,
    }))
}

pub fn run_project_use(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (lane, mut client) = client(inputs)?;
    let context = ProjectContextLease::acquire(client.profile())?;
    let user = require_restore(&mut client, &context)?;
    let directory = with_disposition(client.list_projects(now()), &context)?;
    let requested = inputs.require("project")?;
    let project = directory.exact(requested).ok_or_else(|| {
        Failure::invalid(
            "project_not_visible",
            "that exact project id is not present in the freshly fetched project directory",
        )
        .remedy("run ds auth project list and pass one exact ds_project value")
    })?;
    let saved = context.save(client.profile(), user.uid(), user.email(), project)?;
    Ok(json!({ "lane": lane.token(), "selected": true, "project": context_json(&saved) }))
}

pub fn run_project_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
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
fn require_restore_before_context(
    client: &mut NativeClient,
) -> Result<ds_client_core::AuthenticatedUser, Failure> {
    match client.restore(now()) {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(Failure::unauthorized(
            "headless_signed_out",
            "no native user is signed in for this lane and profile",
        )
        .remedy("run ds auth login --email <address>")
        .next("ds auth status")),
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
    with_disposition(client.restore(now()), context)?.ok_or_else(|| {
        Failure::unauthorized(
            "headless_signed_out",
            "no native user is signed in for this lane and profile",
        )
        .remedy("run ds auth login --email <address>")
        .next("ds auth status")
    })
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

fn project_json(project: &Project) -> Value {
    json!({
        "ds_project": project.ds_project(),
        "project_name": project.project_name(),
        "display_name": project.display_name(),
        "role": project.role(),
        "status": project_status(project.status()),
    })
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

fn map_client(error: ClientError) -> Failure {
    map_client_kind(error.kind(), error.to_string())
}

fn map_client_kind(kind: ErrorKind, message: String) -> Failure {
    match kind {
        ErrorKind::InvalidInput => Failure::invalid("auth_input_invalid", message),
        ErrorKind::SignedOut => Failure::unauthorized("headless_signed_out", message)
            .next("ds auth login --email <address>"),
        ErrorKind::AuthenticationRejected => Failure::unauthorized(
            "auth_rejected",
            "the native authentication or project service rejected this verified request",
        ),
        ErrorKind::PermanentlyRevoked => Failure::unauthorized(
            "auth_revoked",
            "Firebase permanently revoked the native session",
        )
        .next("ds auth login --email <address>"),
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
    }
}

fn cleanup_required() -> Failure {
    Failure::failed(
        "native_cleanup_required",
        "the credential changed but its previous project context still requires cleanup",
    )
    .remedy("repair the owner-only DS config directory, then run ds auth logout again")
}

fn read_password(from_stdin: bool) -> Result<String, Failure> {
    refuse_mcp_prompt(
        from_stdin,
        std::env::var_os("DS_MCP_CHILD").is_some_and(|value| value == "1"),
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

fn refuse_mcp_prompt(from_stdin: bool, mcp_child: bool) -> Result<(), Failure> {
    if !from_stdin && mcp_child {
        Err(Failure::unavailable(
            "password_prompt_forbidden",
            "an MCP child cannot open an interactive password prompt",
        )
        .remedy("run ds auth login directly in a trusted terminal"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_child_cannot_open_the_hidden_prompt() {
        assert_eq!(
            refuse_mcp_prompt(false, true).unwrap_err().code(),
            "password_prompt_forbidden"
        );
        assert!(refuse_mcp_prompt(false, false).is_ok());
        assert!(refuse_mcp_prompt(true, true).is_ok());
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
            assert_eq!(command.effect, Effect::LocalAuthState);
            for forbidden in [
                "password",
                "token",
                "refresh-token",
                "endpoint",
                "url",
                "header",
                "desktop-descriptor",
            ] {
                assert!(
                    command.arg(forbidden).is_none(),
                    "{} exposes {forbidden}",
                    command.id
                );
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
}
