//! `ds report project` — the project's compounded deliverable, produced by
//! the governed report service in the background.
//!
//! These commands need no map, no room, no Desktop and no local reporter
//! engine. They restore the native user and act only on that user's
//! audience-fenced selected project through the fixed `/report` contract:
//! ds-brain resolves the exact scope (every active saved transformer, or the
//! explicit names given), reuses fresh individual artifacts, regenerates the
//! missing or stale ones through the cloud reporter, composes the scope-correct
//! combined set, and publishes one ZIP with a registry row. The CLI never
//! replays those phases and never holds a token.
//!
//! ```text
//!   scope → compounded → archives
//! ```
//!
//! `scope` is the plan: it reads the lifecycle inventory and shows which
//! transformers participate and which are excluded (retired, deleted, missing)
//! before any artifact is produced.

pub mod archives;
pub mod compounded;
pub mod scope;

use ds_cli_auth::{
    HeadlessProjectReport, PROJECT_REPORT_MAX_TRANSFORMERS, TransformerInventory,
    TransformerLifecycle, TransformerSet,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use serde_json::{Value, json};

pub const TRANSFORMER_ARG: Arg = Arg::repeated(
    "transformer",
    "<name>",
    "Explicit transformer scope; repeat per name. Omit for every active saved transformer.",
);
pub const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

macro_rules! refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        pub const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

refusal!(
    NATIVE_PROFILE,
    "native_profile_not_configured",
    "the exact packaged native profile is unavailable",
    "install one complete ds release"
);
refusal!(
    NATIVE_PROFILE_DIGEST,
    "native_profile_digest_mismatch",
    "the packaged catalogue differs from the build pin",
    "reinstall one complete ds release"
);
refusal!(
    NATIVE_PROFILE_UNSAFE,
    "native_profile_unsafe",
    "the packaged native catalogue is unsafe or malformed",
    "reinstall one complete ds release"
);
refusal!(
    HEADLESS_SIGNED_OUT,
    "headless_signed_out",
    "the selected lane has no restorable native user",
    "run ds auth login --email <address>"
);
refusal!(
    HEADLESS_NO_PROJECT,
    "headless_project_not_selected",
    "the user has no audience-fenced selected project",
    "run ds auth project use --project <exact-id>"
);
refusal!(
    PROJECT_CONTEXT_STALE,
    "project_context_stale",
    "the saved project belongs to another identity, lane, or audience",
    "select the project again with ds auth project use"
);
refusal!(
    NATIVE_STATE_UNSAFE,
    "native_state_unsafe",
    "protected native state is unsafe or unreadable",
    "repair the owner-only DS config directory"
);
refusal!(
    NATIVE_STATE_UNAVAILABLE,
    "native_state_unavailable",
    "protected native state cannot be accessed",
    "repair the owner-only DS config directory"
);
refusal!(
    NATIVE_STATE_PROTECTION,
    "native_state_protection_unavailable",
    "this build has no protected-state adapter",
    "install a supported native ds build"
);
refusal!(
    NATIVE_STATE_ROOT,
    "native_state_root_invalid",
    "the configured state root is not absolute",
    "unset it or provide an absolute path"
);
refusal!(
    NATIVE_STATE_CONFLICT,
    "native_state_conflict",
    "another native operation holds the state lease",
    "retry after that operation finishes"
);
refusal!(
    NATIVE_CLEANUP,
    "native_cleanup_required",
    "revoked identity cleanup could not clear context",
    "repair protected state and run auth logout"
);
refusal!(
    AUTH_CONTEXT_MISMATCH,
    "auth_context_mismatch",
    "the protected native providers disagree on identity or selected project",
    "sign out or revoke the unintended provider before retrying"
);
refusal!(
    AUTH_INPUT,
    "auth_input_invalid",
    "the report service refused the bounded request: an unknown transformer, a blocked collision policy, or a malformed scope",
    "run `ds report project scope` and correct the named transformers"
);
refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the fixed gateway rejects the verified request, the user lacks design.compounded_report, or the project is archived or expired",
    "verify the account, its project access and capabilities, and the project lifecycle state"
);
refusal!(
    AUTH_REVOKED,
    "auth_revoked",
    "the native session was permanently revoked",
    "sign in again interactively"
);
refusal!(
    AUTH_IDENTITY_MISMATCH,
    "auth_identity_mismatch",
    "the restored identity differs from the bound native session",
    "sign in again and report a repeated mismatch"
);
refusal!(
    AUTH_TRANSIENT,
    "auth_transient",
    "the governed report service is temporarily unavailable, or transformer freshness could not be verified",
    "retry without changing local state; no artifact was regenerated"
);
refusal!(
    AUTH_UNREADABLE,
    "auth_response_unreadable",
    "the response violates its closed bounded contract, including an archive advertised for zero individual artifacts",
    "retry once, then update ds if it persists"
);
refusal!(
    NOT_FOUND,
    "transformer_not_found",
    "the service found no such project",
    "select the project again with ds auth project use"
);
refusal!(
    INVALID_SCOPE,
    "invalid_transformer_scope",
    "a --transformer name is blank, untrimmed, repeated, or over 200 characters, or more than 500 were named",
    "pass --transformer once per exact transformer name, or omit it for every active transformer"
);
refusal!(
    CONFIRMATION_REQUIRED,
    "confirmation_required",
    "--yes was not given for a command that publishes a durable report archive",
    "run `ds report project scope` first, then re-run with --yes"
);

pub const NATIVE_READ_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    NATIVE_STATE_UNSAFE,
    NATIVE_STATE_UNAVAILABLE,
    NATIVE_STATE_PROTECTION,
    NATIVE_STATE_ROOT,
    NATIVE_STATE_CONFLICT,
    NATIVE_CLEANUP,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    AUTH_REVOKED,
    AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
    NOT_FOUND,
    INVALID_SCOPE,
];

pub const NATIVE_WRITE_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    NATIVE_STATE_UNSAFE,
    NATIVE_STATE_UNAVAILABLE,
    NATIVE_STATE_PROTECTION,
    NATIVE_STATE_ROOT,
    NATIVE_STATE_CONFLICT,
    NATIVE_CLEANUP,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    AUTH_REVOKED,
    AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
    NOT_FOUND,
    INVALID_SCOPE,
    CONFIRMATION_REQUIRED,
];

pub fn transformer_set(inputs: &ds_cli_contract::Inputs) -> Result<TransformerSet, Failure> {
    let names = inputs.repeated("transformer");
    if names.len() > PROJECT_REPORT_MAX_TRANSFORMERS {
        return Err(Failure::invalid(
            "invalid_transformer_scope",
            format!(
                "{} transformers named; the bound is {}",
                names.len(),
                PROJECT_REPORT_MAX_TRANSFORMERS
            ),
        )
        .remedy(INVALID_SCOPE.remedy));
    }
    TransformerSet::new(names.iter().cloned()).map_err(|error| {
        Failure::invalid("invalid_transformer_scope", error.to_string())
            .remedy(INVALID_SCOPE.remedy)
    })
}

pub fn project_receipt<T>(headless: &HeadlessProjectReport<T>) -> Value {
    json!({
        "lane": headless.lane(),
        "project": {
            "ds_project": headless.project_id(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
    })
}

/// The exact scope a compounded run would use, derived from the lifecycle
/// inventory: active ordinary transformers participate; retired, deleted and
/// missing names are excluded with their state, and `mv_data` is reported as
/// the project-level input the service folds in on its own.
pub fn scope_json(requested: &TransformerSet, inventory: &TransformerInventory) -> Value {
    let mut participating = Vec::new();
    let mut excluded = Vec::new();
    let mut project_level = Vec::new();
    for row in inventory.rows() {
        if row.kind() == ds_cli_auth::TransformerKind::ProjectLevel {
            project_level.push(json!({"name": row.name(), "state": row.lifecycle().token()}));
            continue;
        }
        match row.lifecycle() {
            TransformerLifecycle::Active => participating.push(row.name().to_owned()),
            state => {
                let mut entry = json!({"name": row.name(), "state": state.token()});
                if let Some(record) = row.retirement() {
                    entry["reason"] = json!(record.reason());
                }
                excluded.push(entry);
            }
        }
    }
    json!({
        "mode": if requested.is_empty() { "all_active" } else { "explicit" },
        "requested": requested.names(),
        "participating": participating,
        "participating_count": participating.len(),
        "excluded": excluded,
        "excluded_count": excluded.len(),
        "project_level": project_level,
        "compounded_ready": participating.len() >= 2,
    })
}
